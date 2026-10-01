use std::error::Error;

use flaggo_otel_ingestion::{configured_listen_address, router};
use serde_json::json;
use tokio::{net::TcpListener, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(configured_listen_address()?).await?;
    let address = listener.local_addr()?;
    println!(
        "{}",
        json!({
            "address": format!("http://{address}"),
            "event": "server.listening"
        })
    );

    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = signal::ctrl_c().await {
        eprintln!("Failed to install Ctrl+C handler: {error}");
    }
}
