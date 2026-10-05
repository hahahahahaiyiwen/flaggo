use std::{cmp, error::Error, future::IntoFuture, io, sync::Arc, time::Duration};

use flaggo_otel_ingestion::{
    INSTRUMENTATION_SCOPE, OtlpIngestionObservability, SERVICE_NAME, configured_database_url,
    configured_inbox_limits, configured_listen_address, configured_receiver, instrumented_router,
};
use flaggo_raw_otlp_inbox::{
    RawOtlpInbox, RawOtlpInboxError, RawOtlpInboxRetention, RawOtlpInboxRetentionResult,
    SqliteRawOtlpInbox,
};
use flaggo_service_observability::{ServiceObservability, reject_own_receiver};
use tokio::{
    net::TcpListener,
    signal,
    sync::watch,
    time::{MissedTickBehavior, interval},
};
use tracing::Instrument as _;

const MAXIMUM_RETENTION_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = configured_database_url()?;
    let inbox_limits = configured_inbox_limits()?;
    let listen_address = configured_listen_address()?;
    let receiver = configured_receiver()?;
    let listener = TcpListener::bind(listen_address).await?;
    let address = listener.local_addr()?;
    reject_own_receiver(address)?;
    let telemetry = ServiceObservability::initialize(
        SERVICE_NAME,
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
    )?;
    let result = run(
        database_url,
        inbox_limits,
        receiver,
        listener,
        address,
        OtlpIngestionObservability::new(),
    );
    let result = result.await;
    if let Err(error) = &result {
        tracing::event!(
            target: INSTRUMENTATION_SCOPE,
            tracing::Level::ERROR,
            {
                "event.name" = "flaggo.service.failed",
                "flaggo.operation.outcome" = "failure",
                "flaggo.failure.category" = failure_category(error.as_ref()),
                "error.type" = std::any::type_name_of_val(error.as_ref()),
            },
            "OTel Ingestion failed"
        );
    }
    telemetry.shutdown()?;
    result
}

async fn run(
    database_url: String,
    inbox_limits: flaggo_raw_otlp_inbox::RawOtlpInboxLimits,
    receiver: flaggo_otel_ingestion::OtlpReceiverConfig,
    listener: TcpListener,
    address: std::net::SocketAddr,
    observability: OtlpIngestionObservability,
) -> Result<(), Box<dyn Error>> {
    let inbox = Arc::new(SqliteRawOtlpInbox::connect(&database_url, inbox_limits).await?);
    enforce_retention(&inbox, &observability).await?;
    observability.record_health(&inbox.inspect().await?);
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.started",
            "flaggo.operation.outcome" = "success",
            "server.address" = %format!("http://{address}"),
        },
        "OTel Ingestion started"
    );

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let server = axum::serve(
        listener,
        instrumented_router(inbox.clone(), receiver, inbox_limits, observability.clone()),
    )
    .with_graceful_shutdown(wait_for_shutdown(shutdown_receiver.clone()))
    .into_future();
    let retention_worker = run_retention_worker(
        inbox.clone(),
        observability,
        cmp::min(
            inbox_limits.hard_retention(),
            MAXIMUM_RETENTION_MAINTENANCE_INTERVAL,
        ),
        shutdown_receiver,
    );
    tokio::pin!(server);
    tokio::pin!(retention_worker);

    let exit = tokio::select! {
        result = &mut server => ServiceExit::Server(result),
        result = &mut retention_worker => ServiceExit::Retention(result),
        () = shutdown_signal() => ServiceExit::Shutdown,
    };
    let _ = shutdown_sender.send(true);
    let result: Result<(), Box<dyn Error>> = async {
        match exit {
            ServiceExit::Server(server_result) => {
                let retention_result = retention_worker.await;
                server_result?;
                retention_result?;
            }
            ServiceExit::Retention(retention_result) => {
                let server_result = server.await;
                server_result?;
                retention_result?;
            }
            ServiceExit::Shutdown => {
                let (server_result, retention_result) = tokio::join!(server, retention_worker);
                server_result?;
                retention_result?;
            }
        }
        Ok(())
    }
    .await;
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopping",
            "flaggo.operation.outcome" = "success",
        },
        "OTel Ingestion stopping"
    );
    inbox.close().await;
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopped",
            "flaggo.operation.outcome" = "success",
        },
        "OTel Ingestion stopped"
    );
    result
}

async fn shutdown_signal() {
    if let Err(error) = signal::ctrl_c().await {
        tracing::event!(
            target: INSTRUMENTATION_SCOPE,
            tracing::Level::ERROR,
            {
                "error.type" = std::any::type_name_of_val(&error),
                "flaggo.failure.category" = "internal",
            },
            "Failed to install Ctrl+C handler"
        );
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

async fn run_retention_worker(
    inbox: Arc<SqliteRawOtlpInbox>,
    observability: OtlpIngestionObservability,
    maintenance_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RawOtlpInboxError> {
    let mut maintenance = interval(maintenance_interval);
    maintenance.set_missed_tick_behavior(MissedTickBehavior::Delay);
    maintenance.tick().await;
    loop {
        tokio::select! {
            _ = maintenance.tick() => {
                enforce_retention(&inbox, &observability).await?;
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn enforce_retention(
    inbox: &SqliteRawOtlpInbox,
    observability: &OtlpIngestionObservability,
) -> Result<(), RawOtlpInboxError> {
    let span = tracing::info_span!(
        target: INSTRUMENTATION_SCOPE,
        "flaggo.inbox.retention",
        flaggo.operation.name = "inbox.retention",
        flaggo.operation.outcome = tracing::field::Empty,
        flaggo.failure.category = tracing::field::Empty,
    );
    async {
        match inbox.enforce_retention().await {
            Ok(result) => {
                tracing::Span::current().record("flaggo.operation.outcome", "success");
                report_retention(result, observability);
                observability.record_health(&inbox.inspect().await?);
                Ok(())
            }
            Err(error) => {
                tracing::Span::current().record("flaggo.operation.outcome", "failure");
                tracing::Span::current().record("flaggo.failure.category", "dependency");
                Err(error)
            }
        }
    }
    .instrument(span)
    .await
}

fn report_retention(
    result: RawOtlpInboxRetentionResult,
    observability: &OtlpIngestionObservability,
) {
    observability.record_retention(result);
    if result.expired_batch_count == 0 {
        return;
    }
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.inbox.retention.completed",
            "flaggo.operation.outcome" = "success",
            "flaggo.inbox.expired.batch_count" = result.expired_batch_count,
            "flaggo.inbox.expired.payload_bytes" = result.expired_payload_bytes,
        },
        "Raw OTLP Inbox retention completed"
    );
}

fn failure_category(error: &(dyn Error + 'static)) -> &'static str {
    if error.is::<RawOtlpInboxError>() || error.is::<io::Error>() {
        "dependency"
    } else {
        "internal"
    }
}

enum ServiceExit {
    Server(Result<(), io::Error>),
    Retention(Result<(), RawOtlpInboxError>),
    Shutdown,
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use flaggo_otel_ingestion::OtlpIngestionObservability;
    use flaggo_raw_otlp_inbox::{
        DEFAULT_OTLP_PROFILE_VERSION, NewRawOtlpBatch, OtlpSignal, OtlpTransportCompression,
        OtlpWireEncoding, RawOtlpInbox, RawOtlpInboxLimits, SqliteRawOtlpInbox,
    };
    use tokio::{
        sync::watch,
        time::{sleep, timeout},
    };

    use super::run_retention_worker;

    #[tokio::test]
    async fn retention_worker_periodically_expires_and_stops_on_shutdown() {
        let directory = tempfile::tempdir().expect("temporary inbox directory");
        let database_url = format!(
            "sqlite://{}",
            directory
                .path()
                .join("inbox.db")
                .to_string_lossy()
                .replace('\\', "/")
        );
        let limits = RawOtlpInboxLimits::new(100, Duration::from_millis(10)).expect("valid limits");
        let inbox = Arc::new(
            SqliteRawOtlpInbox::connect(&database_url, limits)
                .await
                .expect("SQLite inbox"),
        );
        inbox
            .append(NewRawOtlpBatch::new(
                OtlpSignal::Logs,
                OtlpWireEncoding::Protobuf,
                OtlpTransportCompression::Identity,
                DEFAULT_OTLP_PROFILE_VERSION
                    .parse()
                    .expect("profile version"),
                vec![1, 2, 3],
            ))
            .await
            .expect("durable append");
        sleep(Duration::from_millis(25)).await;

        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker = tokio::spawn(run_retention_worker(
            inbox.clone(),
            OtlpIngestionObservability::new(),
            Duration::from_millis(10),
            shutdown_receiver,
        ));
        timeout(Duration::from_secs(1), async {
            loop {
                if inbox
                    .inspect()
                    .await
                    .expect("inbox health")
                    .expired_batch_count
                    == 1
                {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("periodic retention");

        shutdown_sender.send(true).expect("retention shutdown");
        worker
            .await
            .expect("retention worker task")
            .expect("retention worker");
        inbox.close().await;
    }
}
