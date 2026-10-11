use std::{error::Error, io, sync::Arc};

use flaggo_async_analysis::{
    config::AnalysisServiceConfig,
    observability::{INSTRUMENTATION_SCOPE, SERVICE_NAME},
    service::{run_analysis_service, service_failure_category},
};
use flaggo_service_observability::ServiceObservability;
use flaggo_tetris_playground_analysis::TetrisPlaygroundAgentProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let telemetry = ServiceObservability::initialize(
        SERVICE_NAME,
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
    )?;
    let result = run().await;
    if let Err(error) = &result {
        tracing::event!(
            target: INSTRUMENTATION_SCOPE,
            tracing::Level::ERROR,
            {
                "event.name" = "flaggo.service.failed",
                "flaggo.operation.outcome" = "failure",
                "flaggo.failure.category" = service_failure_category(error.as_ref()),
                "error.type" = std::any::type_name_of_val(error.as_ref()),
            },
            "Tetris playground Async Analysis failed"
        );
    }
    telemetry.shutdown()?;
    result
}

async fn run() -> Result<(), Box<dyn Error>> {
    let config = AnalysisServiceConfig::from_environment()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    run_analysis_service(
        &config,
        Arc::new(TetrisPlaygroundAgentProvider),
        shutdown_signal(),
        async { Ok(()) },
    )
    .await
}

#[cfg(not(unix))]
async fn shutdown_signal() -> io::Result<()> {
    tokio::signal::ctrl_c().await
}

#[cfg(unix)]
async fn shutdown_signal() -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal as unix_signal};

    let mut terminate = unix_signal(SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}
