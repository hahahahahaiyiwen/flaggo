use std::sync::Arc;

use async_trait::async_trait;
use flaggo_core_conformance::evidence::{self, EvidenceFixture, InboxFixture, QueryCases};
use flaggo_evidence_store::{
    EvidenceAnalysisStore, EvidenceStore, SqliteEvidenceAnalysisStore, SqliteEvidenceStore,
};
use flaggo_raw_otlp_inbox::{RawOtlpInbox, RawOtlpInboxLimits, SqliteRawOtlpInbox};
use sqlx::SqlSafeStr;

struct Fixture {
    _directory: tempfile::TempDir,
    url: String,
    inbox: Arc<SqliteRawOtlpInbox>,
    writer: Arc<SqliteEvidenceStore>,
    reader: Arc<SqliteEvidenceAnalysisStore>,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}",
            directory
                .path()
                .join("conformance.db")
                .to_string_lossy()
                .replace('\\', "/")
        );
        let inbox = Arc::new(
            SqliteRawOtlpInbox::connect(&url, RawOtlpInboxLimits::default())
                .await
                .unwrap(),
        );
        let writer = Arc::new(SqliteEvidenceStore::connect(&url).await.unwrap());
        let reader = Arc::new(SqliteEvidenceAnalysisStore::connect(&url).await.unwrap());
        Self {
            _directory: directory,
            url,
            inbox,
            writer,
            reader,
        }
    }

    async fn fault(&self, enabled: bool, table: &str, name: &str) {
        let pool = sqlx::SqlitePool::connect(&self.url).await.unwrap();
        let sql = if enabled {
            format!(
                "CREATE TRIGGER {name} BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'conformance fault'); END;"
            )
        } else {
            format!("DROP TRIGGER {name}")
        };
        sqlx::query(sqlx::AssertSqlSafe(sql).into_sql_str())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}

#[async_trait]
impl InboxFixture for Fixture {
    fn inbox(&self) -> Arc<dyn RawOtlpInbox> {
        self.inbox.clone()
    }
    async fn fresh_inbox_client(&self) -> Arc<dyn RawOtlpInbox> {
        Arc::new(
            SqliteRawOtlpInbox::connect(&self.url, RawOtlpInboxLimits::default())
                .await
                .unwrap(),
        )
    }
    async fn fail_append(&self, enabled: bool) {
        self.fault(enabled, "raw_otlp_inbox_batches", "fail_inbox")
            .await;
    }
}

#[async_trait]
impl EvidenceFixture for Fixture {
    fn writer(&self) -> Arc<dyn EvidenceStore> {
        self.writer.clone()
    }
    async fn fresh_writer(&self) -> Arc<dyn EvidenceStore> {
        Arc::new(SqliteEvidenceStore::connect(&self.url).await.unwrap())
    }
    fn reader(&self) -> Arc<dyn EvidenceAnalysisStore> {
        self.reader.clone()
    }
    async fn fail_projection_commit(&self, enabled: bool) {
        self.fault(
            enabled,
            "evidence_materializer_checkpoints",
            "fail_projection",
        )
        .await;
    }
    fn queries(&self) -> QueryCases {
        QueryCases {
            ordered_ids: "SELECT observation_id FROM observations ORDER BY observation_sequence",
            empty: "SELECT observation_id FROM observations WHERE 0",
            aggregate: "SELECT count(*) AS samples FROM observations",
            typed_values: "SELECT 9223372036854775807 AS largest, -9223372036854775808 AS smallest, 1.0 AS real_value, '1' AS text_value, NULL AS missing FROM observations",
            forbidden: &[
                "DELETE FROM observations",
                "SELECT * FROM evidence_observations",
                "SELECT load_extension('test') FROM observations",
            ],
            corpus: serde_json::from_slice(include_bytes!(
                "../../../contracts/fixtures/evidence-query-sqlite-v1.json"
            ))
            .unwrap(),
        }
    }
}

#[tokio::test]
async fn inbox_acceptance_outcomes() {
    evidence::c08_durable_acceptance(&Fixture::new().await).await;
}

#[tokio::test]
async fn projection_outcomes() {
    evidence::c09_atomic_projection(&Fixture::new().await).await;
}

#[tokio::test]
async fn query_outcomes() {
    evidence::c11_negotiated_queries(&Fixture::new().await).await;
}
