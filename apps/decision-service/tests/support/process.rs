use std::{path::Path, process::Command};

#[path = "../../../../tests/support/service_process.rs"]
mod common;
pub use common::Server;
pub use common::assert_legacy_configuration_rejected;

impl Server {
    pub async fn rust(database: &Path, log: &Path, telemetry: Option<&str>) -> Self {
        let address = common::unused_address();
        let mut command = rust_command();
        command
            .env("FLAGGO_DECISION_DATABASE_PATH", database)
            .env("FLAGGO_DECISION_LISTEN_ADDRESS", address.to_string());
        if let Some(endpoint) = telemetry {
            command
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint)
                .env("OTEL_METRIC_EXPORT_INTERVAL", "50")
                .env("OTEL_BSP_SCHEDULE_DELAY", "50")
                .env("OTEL_BLRP_SCHEDULE_DELAY", "50");
        }
        Self::start(command, address, log).await
    }
}

pub fn rust_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flaggo-decision-service"));
    common::clear_telemetry(&mut command);
    command.env("RUST_LOG", "info");
    command
}
