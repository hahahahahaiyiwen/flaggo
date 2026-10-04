use std::{cmp, error::Error, future::IntoFuture, io, sync::Arc, time::Duration};

use flaggo_otel_ingestion::{
    configured_database_url, configured_inbox_limits, configured_listen_address,
    configured_receiver, router,
};
use flaggo_raw_otlp_inbox::{
    RawOtlpInboxError, RawOtlpInboxRetention, RawOtlpInboxRetentionResult, SqliteRawOtlpInbox,
};
use serde_json::json;
use tokio::{
    net::TcpListener,
    signal,
    sync::watch,
    time::{MissedTickBehavior, interval},
};

const MAXIMUM_RETENTION_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = configured_database_url()?;
    let inbox_limits = configured_inbox_limits()?;
    let listen_address = configured_listen_address()?;
    let receiver = configured_receiver()?;
    let inbox = Arc::new(SqliteRawOtlpInbox::connect(&database_url, inbox_limits).await?);
    report_retention(inbox.enforce_retention().await?);
    let listener = TcpListener::bind(listen_address).await?;
    let address = listener.local_addr()?;
    println!(
        "{}",
        json!({
            "address": format!("http://{address}"),
            "event": "server.listening"
        })
    );

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let server = axum::serve(listener, router(inbox.clone(), receiver, inbox_limits))
        .with_graceful_shutdown(wait_for_shutdown(shutdown_receiver.clone()))
        .into_future();
    let retention_worker = run_retention_worker(
        inbox.clone(),
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
    inbox.close().await;
    result
}

async fn shutdown_signal() {
    if let Err(error) = signal::ctrl_c().await {
        eprintln!("Failed to install Ctrl+C handler: {error}");
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
    maintenance_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RawOtlpInboxError> {
    let mut maintenance = interval(maintenance_interval);
    maintenance.set_missed_tick_behavior(MissedTickBehavior::Delay);
    maintenance.tick().await;
    loop {
        tokio::select! {
            _ = maintenance.tick() => {
                report_retention(inbox.enforce_retention().await?);
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

fn report_retention(result: RawOtlpInboxRetentionResult) {
    if result.expired_batch_count == 0 {
        return;
    }
    println!(
        "{}",
        json!({
            "event": "inbox.retention_expired",
            "expiredBatchCount": result.expired_batch_count,
            "expiredPayloadBytes": result.expired_payload_bytes
        })
    );
}

enum ServiceExit {
    Server(Result<(), io::Error>),
    Retention(Result<(), RawOtlpInboxError>),
    Shutdown,
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

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
