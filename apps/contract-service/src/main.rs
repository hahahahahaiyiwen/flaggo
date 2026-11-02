use flaggo_contract_service::{
    ContractLifecycle, INSTRUMENTATION_SCOPE, SERVICE_NAME, router,
    worker::{ActivationWorker, POLL_INTERVAL_VARIABLE, poll_interval, scan_failed},
};
use flaggo_contract_store::SqliteContractVersionStore;
use flaggo_executable_store::SqliteExecutableStore;
use flaggo_expression::ExpressionCompiler;
use flaggo_service_observability::{ServiceObservability, reject_own_receiver};
use std::{
    env, error::Error, future::IntoFuture as _, net::SocketAddr, path::PathBuf, sync::Arc,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct ConfigurationError(String);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let telemetry = ServiceObservability::initialize(
        SERVICE_NAME,
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
    )?;
    let result = run().await;
    if let Err(error) = &result {
        tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::ERROR, { "event.name" = "flaggo.service.failed",
            flaggo.operation.outcome = "failure", flaggo.failure.category = failure_category(error.as_ref()),
            error = %error }, "Contract Service failed");
    }
    let shutdown = telemetry.shutdown();
    result?;
    shutdown?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn Error>> {
    flaggo_service_hosting::reject_legacy_core_configuration(
        "FLAGGO_CONTRACT_DATABASE_PATH",
        "FLAGGO_CONTRACT_LISTEN_ADDRESS",
    )?;
    let address = match env::var("FLAGGO_CONTRACT_LISTEN_ADDRESS") {
        Ok(value) => value.parse::<SocketAddr>()?,
        Err(env::VarError::NotPresent) => "127.0.0.1:8080".parse()?,
        Err(error) => return Err(error.into()),
    };
    let interval_value = match env::var(POLL_INTERVAL_VARIABLE) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let interval = poll_interval(interval_value.as_deref()).map_err(ConfigurationError)?;
    if tokio::time::Instant::now().checked_add(interval).is_none() {
        return Err(ConfigurationError(
            "Activation interval exceeds the platform timer range.".into(),
        )
        .into());
    }
    let database = env::var_os("FLAGGO_CONTRACT_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("flaggo.db"));
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    reject_own_receiver(address)?;
    let canonicalizer = Arc::new(ExpressionCompiler::default());
    let contracts =
        SqliteContractVersionStore::open(&database, Some(canonicalizer.clone())).await?;
    let executables = match SqliteExecutableStore::open(&database, Some(canonicalizer)).await {
        Ok(store) => store,
        Err(error) => {
            contracts.close().await;
            return Err(error.into());
        }
    };
    let result: Result<(), Box<dyn Error>> = async {
        contracts.initialize(None).await?;
        executables.initialize(None).await?;
        let lifecycle = ContractLifecycle::new(contracts.clone(), executables.clone());
        let mut worker = ActivationWorker::new(lifecycle.clone());
        if let Err(error) = worker.run_once().await { scan_failed(&error); }
        let (worker_stop, worker_stopping) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(worker.run(interval, worker_stopping));
        tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, { "event.name" = "flaggo.service.started",
            flaggo.operation.outcome = "success", server.address = %format!("http://{address}") }, "Contract Service started");
        let result: Result<(), Box<dyn Error>> = async {
            let (stop, requested) = tokio::sync::oneshot::channel::<()>();
            let server = axum::serve(listener, router(lifecycle))
                .with_graceful_shutdown(async { let _ = requested.await; }).into_future();
            tokio::pin!(server);
            tokio::select! {
                result = &mut server => { service_stopping(); result?; }
                signal = shutdown_signal() => {
                    service_stopping();
                    if stop.send(()).is_err() { tracing::debug!(target: INSTRUMENTATION_SCOPE, "HTTP server already stopped"); }
                    let drained = tokio::time::timeout(Duration::from_secs(30), server).await;
                    signal?;
                    drained??;
                }
            }
            Ok(())
        }.await;
        if worker_stop.send(true).is_err() {
            tracing::debug!(target: INSTRUMENTATION_SCOPE, "Activation worker already stopped");
        }
        let stopped = tokio::time::timeout(Duration::from_secs(30), worker).await;
        result?;
        stopped??;
        Ok(())
    }.await;
    contracts.close().await;
    executables.close().await;
    tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, { "event.name" = "flaggo.service.stopped",
        flaggo.operation.outcome = if result.is_ok() { "success" } else { "failure" } }, "Contract Service stopped");
    result
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

fn service_stopping() {
    tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, { "event.name" = "flaggo.service.stopping",
        flaggo.operation.outcome = "success" }, "Contract Service stopping");
}

fn failure_category(error: &(dyn Error + 'static)) -> &'static str {
    if error.is::<env::VarError>()
        || error.is::<ConfigurationError>()
        || error.is::<flaggo_service_hosting::LegacyConfigurationError>()
        || error.is::<std::net::AddrParseError>()
        || error.is::<flaggo_service_observability::ObservabilityError>()
    {
        "configuration"
    } else if error.is::<tokio::time::error::Elapsed>() {
        "timeout"
    } else if error.is::<flaggo_contract_store::ContractStoreError>()
        || error.is::<flaggo_executable_store::ExecutableStoreError>()
        || error.is::<std::io::Error>()
    {
        "dependency"
    } else {
        "internal"
    }
}
