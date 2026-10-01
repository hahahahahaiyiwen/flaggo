use std::{error::Error, sync::Arc};

use flaggo_otel_ingestion::{
    configured_database_url, configured_inbox_limits, configured_listen_address, router,
};
use flaggo_raw_otlp_inbox::SqliteRawOtlpInbox;
use serde_json::json;
use tokio::{net::TcpListener, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let inbox = Arc::new(
        SqliteRawOtlpInbox::connect(&configured_database_url()?, configured_inbox_limits()?)
            .await?,
    );
    let listener = TcpListener::bind(configured_listen_address()?).await?;
    let address = listener.local_addr()?;
    println!(
        "{}",
        json!({
            "address": format!("http://{address}"),
            "event": "server.listening"
        })
    );

    let result = axum::serve(listener, router(inbox.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    inbox.close().await;
    result?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = signal::ctrl_c().await {
        eprintln!("Failed to install Ctrl+C handler: {error}");
    }
}
