use std::{error::Error, io, sync::Arc};

use flaggo_analysis_agent::{
    context::ANALYSIS_SYSTEM_PROMPT,
    copilot::{CopilotAgentProvider, CopilotRuntimeConfig},
};
use flaggo_async_analysis::{
    config::AnalysisServiceConfig,
    observability::{INSTRUMENTATION_SCOPE, SERVICE_NAME},
    service::{run_analysis_service, service_failure_category},
};
use flaggo_service_observability::ServiceObservability;

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
            "Async Analysis failed"
        );
    }
    telemetry.shutdown()?;
    result
}

async fn run() -> Result<(), Box<dyn Error>> {
    let config = AnalysisServiceConfig::from_environment()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let copilot_provider = Arc::new(
        CopilotAgentProvider::start(
            CopilotRuntimeConfig {
                base_directory: config.copilot_home.clone(),
                inference: config.inference.clone(),
            },
            config.skill_directory.clone(),
            ANALYSIS_SYSTEM_PROMPT.to_owned(),
            config.run_timeout,
        )
        .await?,
    );
    run_analysis_service(
        &config,
        copilot_provider.clone(),
        shutdown_signal(),
        async move {
            copilot_provider
                .stop()
                .await
                .map_err(|error| Box::new(error) as Box<dyn Error>)
        },
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
