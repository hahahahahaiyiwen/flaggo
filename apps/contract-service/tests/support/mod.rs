use flaggo_contract::{DecisionContract, strict_json};
use flaggo_contract_service::ContractLifecycle;
use flaggo_contract_store::SqliteContractVersionStore;
use flaggo_executable_store::SqliteExecutableStore;
use flaggo_expression::ExpressionCompiler;
use serde_json::{Value, json};
use std::{path::Path, process::Command, sync::Arc};

#[path = "../../../../tests/support/otlp_capture.rs"]
pub mod capture;
#[path = "../../../../tests/support/service_process.rs"]
mod process;
pub use process::Server;
pub use process::assert_legacy_configuration_rejected;

pub fn contract() -> Value {
    serde_json::from_slice::<Value>(include_bytes!(
        "../../../../contracts/fixtures/management/decision-contract/01-validate-valid.json"
    ))
    .unwrap()["request"]["body"]
        .clone()
}

pub fn submission(cycle: &str, expression: &str, result: i32) -> Value {
    json!({"rules": [{"name": "learned", "when": {"expression": expression}, "return": {"value": result}}],
        "provenance": {"workspaceId": format!("sha256:{}", "a".repeat(64)), "cycleId": cycle,
            "attemptId": "attempt-1", "analysisManifestDigest": format!("sha256:{}", "b".repeat(64))}})
}

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub lifecycle: ContractLifecycle,
    pub pool: sqlx::SqlitePool,
}

impl Fixture {
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("http.db");
        let canonicalizer = Arc::new(ExpressionCompiler::default());
        let contracts = SqliteContractVersionStore::open(&path, Some(canonicalizer.clone()))
            .await
            .unwrap();
        let executables = SqliteExecutableStore::open(&path, Some(canonicalizer))
            .await
            .unwrap();
        contracts.initialize(None).await.unwrap();
        executables.initialize(None).await.unwrap();
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        Self {
            directory,
            lifecycle: ContractLifecycle::new(contracts, executables),
            pool,
        }
    }

    pub async fn deploy(&self, document: &Value) -> Value {
        let contract: DecisionContract =
            strict_json::deserialize(&serde_json::to_vec(document).unwrap()).unwrap();
        let version = self
            .lifecycle
            .deploy(&contract.name, &contract)
            .await
            .unwrap()
            .0;
        serde_json::to_value(version).unwrap()
    }

    pub async fn close(self) {
        self.lifecycle.contracts.close().await;
        self.lifecycle.executables.close().await;
        self.pool.close().await;
        self.directory.close().unwrap();
    }
}

impl Server {
    pub async fn rust(database: &Path, log: &Path, telemetry: Option<&str>, poll: &str) -> Self {
        let address = process::unused_address();
        let mut command = rust_command();
        command
            .env("FLAGGO_CONTRACT_DATABASE_PATH", database)
            .env("FLAGGO_CONTRACT_LISTEN_ADDRESS", address.to_string())
            .env("FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS", poll);
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_flaggo-contract-service"));
    process::clear_telemetry(&mut command);
    command.env("RUST_LOG", "info");
    command
}
