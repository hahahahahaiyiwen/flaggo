use std::{error::Error, io, sync::Arc};

use flaggo_analysis_agent::{
    AgentPool,
    copilot::{
        ANALYSIS_SKILL_NAME, ANALYSIS_SKILL_VERSION, CopilotAgentProvider, CopilotRuntimeConfig,
    },
};
use flaggo_analysis_domain::AnalysisProfile;
use flaggo_analysis_workspace::LocalWorkspaceProvider;
use flaggo_async_analysis::{
    AnalysisCoordinator, CoordinatorConfig, CoordinatorRunOutcome, SystemClock,
    config::AnalysisServiceConfig, contract_client::ContractServiceClient,
    local_capabilities::LocalCapabilityFactory,
};
use flaggo_evidence_store::{EvidenceQueryLimits, SqliteEvidenceAnalysisStore};
use serde_json::json;
use tokio::{
    signal,
    sync::watch,
    task::JoinSet,
    time::{MissedTickBehavior, interval},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if let Err(error) = run().await {
        eprintln!(
            "{}",
            json!({
                "error": error.to_string(),
                "event": "async_analysis.failed"
            })
        );
        return Err(error);
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn Error>> {
    let config = AnalysisServiceConfig::from_environment()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let contract_service = Arc::new(ContractServiceClient::new(
        &config.contract_service_url,
        config.catalog_url.as_deref(),
        config.query_timeout,
    )?);
    let evidence_store =
        Arc::new(SqliteEvidenceAnalysisStore::connect(&config.database_url).await?);
    let workspace_provider = Arc::new(LocalWorkspaceProvider::new(&config.workspace_root)?);
    let clock = Arc::new(SystemClock);
    let capabilities = Arc::new(LocalCapabilityFactory::new(
        contract_service.clone(),
        evidence_store.clone(),
        EvidenceQueryLimits {
            max_rows: config.query_max_rows,
            max_bytes: config.query_max_bytes,
            timeout: config.query_timeout,
        },
        clock.clone(),
    ));
    let copilot_provider = Arc::new(
        CopilotAgentProvider::start(
            CopilotRuntimeConfig {
                base_directory: config.copilot_home.clone(),
                github_token: config.github_token.clone(),
                log_session_events: config.log_session_events,
            },
            config.model.clone(),
            config.skill_root.clone(),
            config.run_timeout,
        )
        .await?,
    );
    let coordinator = Arc::new(AnalysisCoordinator::new(
        contract_service,
        workspace_provider,
        capabilities,
        AgentPool::new(copilot_provider.clone(), config.max_agents),
        CoordinatorConfig {
            supersession_poll_interval: config.poll_interval,
            yield_grace_period: config.yield_grace_period,
            analysis_profile: AnalysisProfile {
                skill_name: ANALYSIS_SKILL_NAME.to_owned(),
                skill_version: ANALYSIS_SKILL_VERSION.to_owned(),
            },
        },
        clock,
    ));

    println!(
        "{}",
        json!({
            "event": "async_analysis.started",
            "maxAgents": config.max_agents.get(),
            "model": config.model.as_deref().unwrap_or("copilot-sdk-default"),
            "sessionEventLogging": config.log_session_events,
            "workspaceRoot": config.workspace_root
        })
    );

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut workers = JoinSet::new();
    for worker_id in 0..config.max_agents.get() {
        workers.spawn(run_worker(
            worker_id,
            Arc::clone(&coordinator),
            config.poll_interval,
            shutdown_receiver.clone(),
        ));
    }

    let shutdown_reason = tokio::select! {
        result = signal::ctrl_c() => {
            result?;
            "signal"
        }
        worker = workers.join_next() => {
            match worker {
                Some(Ok(())) => "worker-exited",
                Some(Err(error)) => {
                    eprintln!("{}", json!({
                        "error": error.to_string(),
                        "event": "async_analysis.worker_failed"
                    }));
                    "worker-failed"
                }
                None => "workers-empty",
            }
        }
    };
    println!(
        "{}",
        json!({
            "event": "async_analysis.stopping",
            "reason": shutdown_reason
        })
    );
    let _ = shutdown_sender.send(true);
    while let Some(worker) = workers.join_next().await {
        if let Err(error) = worker {
            eprintln!(
                "{}",
                json!({
                    "error": error.to_string(),
                    "event": "async_analysis.worker_failed"
                })
            );
        }
    }
    copilot_provider.stop().await?;
    evidence_store.close().await;
    println!("{}", json!({"event": "async_analysis.stopped"}));
    Ok(())
}

async fn run_worker(
    worker_id: usize,
    coordinator: Arc<AnalysisCoordinator>,
    poll_interval: std::time::Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut work = interval(poll_interval);
    work.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            _ = work.tick() => {
                match coordinator
                    .run_once_until(chrono::Utc::now(), shutdown.clone())
                    .await
                {
                    Ok(CoordinatorRunOutcome::Idle) => {}
                    Ok(outcome) => {
                        println!("{}", json!({
                            "event": "async_analysis.worker_outcome",
                            "outcome": outcome,
                            "workerId": worker_id
                        }));
                    }
                    Err(error) => {
                        eprintln!("{}", json!({
                            "error": error.to_string(),
                            "event": "async_analysis.worker_attempt_failed",
                            "workerId": worker_id
                        }));
                    }
                }
                if *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}
