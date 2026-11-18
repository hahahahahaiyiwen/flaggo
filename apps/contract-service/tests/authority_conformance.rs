use std::sync::Arc;

use flaggo_core_conformance::{
    AuthorityFixture, c03_atomic_failure, c03_publication, c04_admission, c05_activation,
    c07_namespace,
};
use flaggo_executable_store::{ContractAuthority, SqliteContractAuthority};
use flaggo_expression::ExpressionCompiler;
use sqlx::SqlSafeStr;

#[path = "support/ownership.rs"]
mod ownership;

struct Fixture {
    _directory: tempfile::TempDir,
    first: Arc<SqliteContractAuthority>,
    second: Arc<SqliteContractAuthority>,
    pool: sqlx::SqlitePool,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("authority.db");
        let first = Arc::new(
            SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
                .await
                .unwrap()
                .with_admission_fence(Arc::new(ownership::FixedOwnership)),
        );
        first.initialize().await.unwrap();
        let second = Arc::new(
            SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
                .await
                .unwrap()
                .with_admission_fence(Arc::new(ownership::FixedOwnership)),
        );
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(path),
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            first,
            second,
            pool,
        }
    }

    async fn close(self) {
        self.first.close().await;
        self.second.close().await;
        self.pool.close().await;
    }
}

#[async_trait::async_trait]
impl AuthorityFixture for Fixture {
    fn client(&self) -> Arc<dyn ContractAuthority> {
        self.first.clone()
    }
    fn independent_client(&self) -> Arc<dyn ContractAuthority> {
        self.second.clone()
    }
    async fn fail_publication(&self, enabled: bool) {
        sqlx::raw_sql(if enabled {
            "CREATE TRIGGER fail_publication BEFORE INSERT ON decision_contract_current
             BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;"
        } else {
            "DROP TRIGGER fail_publication;"
        })
        .execute(&self.pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn full_namespace_and_scoped_admission_paging_conform() {
    let fixture = Fixture::new().await;
    c07_namespace(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn publication_conforms() {
    let fixture = Fixture::new().await;
    c03_publication(&fixture).await;
    fixture.close().await;
}

async fn legacy_fixture(fixture: &Fixture) -> flaggo_contract::ContractResourceId {
    use flaggo_contract::{AnalysisCandidateSubmission, ContractResourceId, strict_json};
    use flaggo_contract_service::ContractLifecycle;
    let lifecycle = ContractLifecycle::new(fixture.first.clone());
    let contract = flaggo_core_conformance::contract();
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let version = lifecycle.deploy(&resource, &contract).await.unwrap().0;
    let submission: AnalysisCandidateSubmission = strict_json::deserialize(
        &serde_json::to_vec(&serde_json::json!({
            "rules": [{"name": "learned", "when": {"expression": "true"}, "return": {"value": 400}}],
            "ownership": {"attemptId": "attempt-1", "epoch": "1"},
            "provenance": {
                "workspaceId": format!("sha256:{}", "a".repeat(64)),
                "cycleId": "legacy-cycle", "attemptId": "legacy-attempt",
                "analysisManifestDigest": format!("sha256:{}", "b".repeat(64))
            }
        })).unwrap(),
    ).unwrap();
    lifecycle
        .submit(&resource, &version.contract_digest, submission)
        .await
        .unwrap();
    sqlx::raw_sql(
        "BEGIN IMMEDIATE;
         CREATE TABLE decision_contract_resources_legacy (
             contract_name TEXT PRIMARY KEY, tenant TEXT NOT NULL, application TEXT NOT NULL, environment TEXT NOT NULL);
         INSERT INTO decision_contract_resources_legacy SELECT contract_name, tenant, application, environment FROM decision_contract_resources;
         CREATE TABLE decision_contract_versions_legacy (
             contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL, accepted_at TEXT NOT NULL, contract_json BLOB NOT NULL,
             PRIMARY KEY(contract_name, contract_digest),
             FOREIGN KEY(contract_name) REFERENCES decision_contract_resources_legacy(contract_name));
         INSERT INTO decision_contract_versions_legacy
             SELECT resource.contract_name, version.contract_digest, version.accepted_at, version.contract_json
             FROM decision_contract_versions AS version JOIN decision_contract_resources AS resource USING(resource_key);
         CREATE TABLE decision_contract_current_legacy (
             contract_name TEXT PRIMARY KEY, contract_digest TEXT NOT NULL,
             FOREIGN KEY(contract_name, contract_digest) REFERENCES decision_contract_versions_legacy(contract_name, contract_digest));
         INSERT INTO decision_contract_current_legacy
             SELECT resource.contract_name, current.contract_digest FROM decision_contract_current AS current
             JOIN decision_contract_resources AS resource USING(resource_key);
         DROP TABLE decision_contract_current;
         DROP TABLE decision_contract_versions;
         DROP TABLE decision_contract_resources;
         ALTER TABLE decision_contract_resources_legacy RENAME TO decision_contract_resources;
         ALTER TABLE decision_contract_versions_legacy RENAME TO decision_contract_versions;
         ALTER TABLE decision_contract_current_legacy RENAME TO decision_contract_current;
         CREATE INDEX ix_contract_versions_history ON decision_contract_versions(contract_name, accepted_at DESC, contract_digest DESC);
         CREATE TABLE analysis_candidate_admissions_legacy (
             workspace_id TEXT NOT NULL, cycle_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
             contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL, executable_digest TEXT NOT NULL,
             analysis_manifest_digest TEXT NOT NULL, created_at TEXT NOT NULL,
             PRIMARY KEY(workspace_id, cycle_id),
             FOREIGN KEY(executable_digest) REFERENCES decision_executables(executable_digest));
         INSERT INTO analysis_candidate_admissions_legacy
             SELECT workspace_id, cycle_id, attempt_id, contract_name, contract_digest, executable_digest, analysis_manifest_digest, created_at
             FROM analysis_candidate_admissions;
         DROP TABLE analysis_candidate_admissions;
         ALTER TABLE analysis_candidate_admissions_legacy RENAME TO analysis_candidate_admissions;
         CREATE INDEX ix_analysis_candidates_contract ON analysis_candidate_admissions(contract_name, contract_digest, created_at);
         UPDATE flaggo_schema_versions SET version = 3 WHERE component = 'contract-store';
         UPDATE flaggo_schema_versions SET version = 5 WHERE component = 'executable-store';
         COMMIT;",
    ).execute(&fixture.pool).await.unwrap();
    resource
}

type ArtifactSnapshot = (
    String,
    Vec<u8>,
    Vec<u8>,
    Option<Vec<u8>>,
    String,
    i64,
    String,
    Option<String>,
);

#[tokio::test]
async fn explicit_migration_preserves_exact_artifacts_timestamps_authority_and_provenance() {
    let fixture = Fixture::new().await;
    let resource = legacy_fixture(&fixture).await;
    let versions: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT contract_digest, accepted_at, contract_json FROM decision_contract_versions ORDER BY contract_digest",
    ).fetch_all(&fixture.pool).await.unwrap();
    let snapshot = "SELECT executable_digest, executable_json, checked_executable_json, provenance_json, lifecycle_state, state_version, created_at, activated_at FROM decision_executables ORDER BY executable_digest";
    let artifacts: Vec<ArtifactSnapshot> =
        sqlx::query_as(sqlx::AssertSqlSafe(snapshot).into_sql_str())
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert!(
        fixture.first.initialize().await.is_err(),
        "startup cannot silently migrate legacy authority"
    );
    assert!(fixture.first.migrate_scoped_resources().await.unwrap());
    assert!(
        !fixture.second.migrate_scoped_resources().await.unwrap(),
        "migration is idempotent"
    );
    let migrated: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT contract_digest, accepted_at, contract_json FROM decision_contract_versions ORDER BY contract_digest",
    ).fetch_all(&fixture.pool).await.unwrap();
    let migrated_artifacts: Vec<ArtifactSnapshot> =
        sqlx::query_as(sqlx::AssertSqlSafe(snapshot).into_sql_str())
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(migrated, versions);
    assert_eq!(migrated_artifacts, artifacts);
    assert_eq!(
        fixture
            .second
            .get_current_contract(&resource)
            .await
            .unwrap()
            .unwrap()
            .contract_digest,
        versions[0].0
    );
    let pending = fixture.second.list_pending(10, None).await.unwrap();
    assert_eq!(pending.candidates.len(), 1);
    let admission = &pending.candidates[0].admission;
    assert_eq!(admission.resource, resource);
    assert_eq!(admission.workspace_id, format!("sha256:{}", "a".repeat(64)));
    assert_eq!(admission.cycle_id, "legacy-cycle");
    assert_eq!(admission.attempt_id, "legacy-attempt");
    assert_eq!(fixture.second.readiness().await.unwrap(), (true, true));
    fixture.close().await;
}

#[tokio::test]
async fn failed_executable_migration_rolls_back_the_contract_migration_too() {
    let fixture = Fixture::new().await;
    let resource = legacy_fixture(&fixture).await;
    sqlx::query("UPDATE analysis_candidate_admissions SET contract_name = 'missing-owner'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(fixture.first.migrate_scoped_resources().await.is_err());
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM flaggo_schema_versions ORDER BY component")
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(versions, [3, 5]);
    let owners: Vec<String> =
        sqlx::query_scalar("SELECT contract_name FROM decision_contract_resources")
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(owners.as_slice(), std::slice::from_ref(&resource.name));
    sqlx::query("UPDATE analysis_candidate_admissions SET contract_name = ?")
        .bind(&resource.name)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(fixture.second.migrate_scoped_resources().await.unwrap());
    assert!(
        fixture
            .first
            .get_current_contract(&resource)
            .await
            .unwrap()
            .is_some()
    );
    fixture.close().await;
}

#[tokio::test]
async fn publication_rolls_back_all_authority() {
    let fixture = Fixture::new().await;
    c03_atomic_failure(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn admission_conforms_across_independent_clients() {
    let fixture = Fixture::new().await;
    c04_admission(&fixture).await;
    fixture.close().await;
}

#[tokio::test]
async fn activation_conforms_across_independent_clients() {
    let fixture = Fixture::new().await;
    c05_activation(&fixture).await;
    fixture.close().await;
}
