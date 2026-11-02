use std::sync::Arc;

use axum::Router;
use flaggo_contract::{DecisionContract, DecisionExecutable, digests, strict_json};
use flaggo_contract_store::{AcceptedContractVersion, SqliteContractVersionStore, timestamp};
use flaggo_decision_service::router;
use flaggo_executable_store::{ExecutableLifecycleState, SqliteExecutableStore, StoredExecutable};
use flaggo_expression::{ExecutableCompiler, ExpressionCompiler};
use serde_json::{Value, json};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use tempfile::TempDir;

#[path = "../../../../tests/support/otlp_capture.rs"]
pub mod capture;
pub mod process;

pub struct Fixture {
    pub directory: TempDir,
    pub contracts: SqliteContractVersionStore,
    pub executables: SqliteExecutableStore,
    pub pool: SqlitePool,
    pub accepted: AcceptedContractVersion,
    pub executable_digest: String,
}

impl Fixture {
    pub async fn new(predicate: &str, result: Value) -> Self {
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
        let mut document: Value = serde_json::from_slice(
            br#"{
            "authority":{"tenant":"t","application":"a","environment":"e"},
            "name":"http.parity","expression_syntax":"flaggo.cel/v1",
            "attributes":[{"name":"x","schema":{"type":"integer","minimum":-10,"maximum":10}}],
            "result":{"schema":{"type":"integer","minimum":0,"maximum":10},"default":0}
        }"#,
        )
        .unwrap();
        document["authoredExecutable"] = json!({
            "rules": [{"name": "choose", "when": {"expression": predicate}, "return": result}]
        });
        let model: DecisionContract =
            strict_json::deserialize(&serde_json::to_vec(&document).unwrap()).unwrap();
        let accepted = AcceptedContractVersion {
            contract_digest: digests::contract_digest(&model, None).unwrap(),
            accepted_at: timestamp::parse_exact("2026-03-03T10:00:00.1234567+00:00").unwrap(),
            contract: model,
        };
        contracts.put(&accepted, None).await.unwrap();
        let source: DecisionExecutable = serde_json::from_value(json!({
            "contractDigest": accepted.contract_digest,
            "rules": [{"name": "choose", "when": {"expression": predicate}, "return": result}],
        }))
        .unwrap();
        let compiled = ExecutableCompiler::default()
            .compile(&accepted.contract, &source)
            .unwrap();
        let executable_digest = compiled.executable_digest.clone();
        executables
            .put_candidate(
                &StoredExecutable {
                    executable_digest: compiled.executable_digest,
                    executable: compiled.executable,
                    checked_executable: compiled.checked_executable,
                    state: ExecutableLifecycleState::Candidate,
                    state_version: 0,
                    created_at: accepted.accepted_at,
                    activated_at: None,
                    provenance: None,
                },
                None,
            )
            .await
            .unwrap();
        executables
            .activate_if_none(&accepted.contract_digest, &executable_digest, None)
            .await
            .unwrap();
        let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(path))
            .await
            .unwrap();
        Self {
            directory,
            contracts,
            executables,
            pool,
            accepted,
            executable_digest,
        }
    }

    pub fn path(&self) -> String {
        format!(
            "/v3/decision-contracts/http.parity/versions/{}/decisions",
            self.accepted.contract_digest
        )
    }

    pub fn router(&self) -> Router {
        router(self.contracts.clone(), self.executables.clone())
    }

    pub async fn close(self) {
        self.contracts.close().await;
        self.executables.close().await;
        self.pool.close().await;
        self.directory.close().unwrap();
    }
}
