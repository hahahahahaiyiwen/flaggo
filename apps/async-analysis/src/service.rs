use std::{error::Error, future::Future, io, sync::Arc};

use flaggo_analysis_agent::{AgentPool, AgentProvider};
use flaggo_analysis_workspace::LocalWorkspaceProvider;
use flaggo_evidence_store::{EvidenceQueryLimits, SqliteEvidenceAnalysisStore};
use tokio::{
    sync::watch,
    task::JoinSet,
    time::{MissedTickBehavior, interval},
};

use crate::{
    AnalysisCoordinator, CoordinatorConfig, CoordinatorRunOutcome, SystemClock,
    config::AnalysisServiceConfig,
    contract_client::ContractServiceClient,
    local_capabilities::LocalCapabilityFactory,
    observability::{INSTRUMENTATION_SCOPE, analysis_failure_category},
};

pub async fn run_analysis_service<F, P>(
    config: &AnalysisServiceConfig,
    provider: Arc<dyn AgentProvider>,
    shutdown_signal: F,
    provider_shutdown: P,
) -> Result<(), Box<dyn Error>>
where
    F: Future<Output = io::Result<()>>,
    P: Future<Output = Result<(), Box<dyn Error>>>,
{
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
    let coordinator = Arc::new(AnalysisCoordinator::new(
        contract_service,
        workspace_provider,
        capabilities,
        AgentPool::new(provider, config.max_agents),
        CoordinatorConfig {
            supersession_poll_interval: config.poll_interval,
            yield_grace_period: config.yield_grace_period,
            analysis_profile: flaggo_analysis_agent::context::analysis_profile(),
        },
        clock,
    ));

    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.started",
            "flaggo.operation.outcome" = "success",
        },
        "Async Analysis started"
    );

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut workers = JoinSet::new();
    for _ in 0..config.max_agents.get() {
        workers.spawn(run_worker(
            Arc::clone(&coordinator),
            config.poll_interval,
            shutdown_receiver.clone(),
        ));
    }

    tokio::pin!(shutdown_signal);
    let exit_failure: Option<Box<dyn Error>> = tokio::select! {
        result = &mut shutdown_signal => {
            result
                .err()
                .map(|error| Box::new(error) as Box<dyn Error>)
        }
        worker = workers.join_next() => {
            match worker {
                Some(Ok(())) => Some(Box::new(io::Error::other(
                    "an Async Analysis worker exited unexpectedly",
                )) as Box<dyn Error>),
                Some(Err(error)) => Some(Box::new(io::Error::other(format!(
                    "an Async Analysis worker failed: {error}",
                ))) as Box<dyn Error>),
                None => Some(Box::new(io::Error::other(
                    "the Async Analysis worker set became empty",
                )) as Box<dyn Error>),
            }
        }
    };

    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopping",
            "flaggo.operation.outcome" = "success",
        },
        "Async Analysis stopping"
    );
    let _ = shutdown_sender.send(true);
    while let Some(worker) = workers.join_next().await {
        if let Err(error) = worker {
            tracing::event!(
                target: INSTRUMENTATION_SCOPE,
                tracing::Level::ERROR,
                {
                    "flaggo.failure.category" = "internal",
                    "error.type" = std::any::type_name_of_val(&error),
                },
                "Async Analysis worker failed during shutdown"
            );
        }
    }
    let provider_result = provider_shutdown.await;
    evidence_store.close().await;
    provider_result?;
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopped",
            "flaggo.operation.outcome" = "success",
        },
        "Async Analysis stopped"
    );
    exit_failure.map_or(Ok(()), Err)
}

async fn run_worker(
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
                    Ok(_) => {}
                    Err(error) => {
                        tracing::event!(
                            target: INSTRUMENTATION_SCOPE,
                            tracing::Level::WARN,
                            {
                                "flaggo.failure.category" =
                                    analysis_failure_category(&error),
                                "error.type" = std::any::type_name_of_val(&error),
                            },
                            "Async Analysis polling attempt failed"
                        );
                    }
                }
                if *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

#[must_use]
pub fn service_failure_category(error: &(dyn Error + 'static)) -> &'static str {
    if let Some(error) = error.downcast_ref::<io::Error>() {
        return match error.kind() {
            io::ErrorKind::InvalidInput => "configuration",
            io::ErrorKind::Other => "internal",
            _ => "dependency",
        };
    }
    "dependency"
}
