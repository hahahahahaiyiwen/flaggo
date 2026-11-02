use std::{
    env, error::Error, future::IntoFuture as _, net::SocketAddr, path::PathBuf, sync::Arc,
    time::Duration,
};

use flaggo_contract_store::SqliteContractVersionStore;
use flaggo_decision_service::{INSTRUMENTATION_SCOPE, SERVICE_NAME, router};
use flaggo_executable_store::SqliteExecutableStore;
use flaggo_expression::ExpressionCompiler;
use flaggo_service_observability::{ServiceObservability, reject_own_receiver};

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
            "flaggo.failure.category" = failure_category(error.as_ref()),
            error = %error,
            },
            "Decision Service failed",
        );
    }
    let shutdown = telemetry.shutdown();
    result?;
    shutdown?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn Error>> {
    flaggo_service_hosting::reject_legacy_core_configuration(
        "FLAGGO_DECISION_DATABASE_PATH",
        "FLAGGO_DECISION_LISTEN_ADDRESS",
    )?;
    let address = match env::var("FLAGGO_DECISION_LISTEN_ADDRESS") {
        Ok(value) => value.parse::<SocketAddr>()?,
        Err(env::VarError::NotPresent) => "127.0.0.1:8081".parse()?,
        Err(error) => return Err(error.into()),
    };
    let database = env::var_os("FLAGGO_DECISION_DATABASE_PATH")
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
        tracing::event!(
            target: INSTRUMENTATION_SCOPE,
            tracing::Level::INFO,
            {
            "event.name" = "flaggo.service.started",
            "flaggo.operation.outcome" = "success",
            "server.address" = %format!("http://{address}"),
            },
            "Decision Service started",
        );
        let (stop, requested) = tokio::sync::oneshot::channel::<()>();
        let server = axum::serve(listener, router(contracts.clone(), executables.clone()))
            .with_graceful_shutdown(async {
                let _ = requested.await;
            })
            .into_future();
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => {
                service_stopping();
                result?;
            }
            signal = shutdown_signal() => {
                service_stopping();
                if stop.send(()).is_err() {
                    tracing::debug!(target: INSTRUMENTATION_SCOPE, "HTTP server already stopped");
                }
                let drained = tokio::time::timeout(Duration::from_secs(30), server).await;
                signal?;
                drained??;
            }
        }
        Ok(())
    }
    .await;
    contracts.close().await;
    executables.close().await;
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
        "event.name" = "flaggo.service.stopped",
        "flaggo.operation.outcome" = "success",
        },
        "Decision Service stopped",
    );
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
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
        "event.name" = "flaggo.service.stopping",
        "flaggo.operation.outcome" = "success",
        },
        "Decision Service stopping",
    );
}

fn failure_category(error: &(dyn Error + 'static)) -> &'static str {
    if error.is::<env::VarError>()
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
