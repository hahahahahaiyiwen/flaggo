use std::{num::NonZeroU32, path::Path, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceAnalysisStore, EvidenceMaterializationCommit, EvidenceObservation,
    EvidenceObservationWrite, EvidenceQueryLimits, EvidenceQueryRequest, EvidenceQueryScope,
    EvidenceSignal, EvidenceSourceSelector, EvidenceStore, EvidenceStoreError,
    ForwardMaterializationKey, MaterializerVersions, ObservationWatermark, SourceKey,
    SqliteEvidenceAnalysisStore, SqliteEvidenceStore,
};
use flaggo_raw_otlp_inbox::{
    DEFAULT_OTLP_PROFILE_VERSION, NewRawOtlpBatch, OtlpSignal, OtlpTransportCompression,
    OtlpWireEncoding, RawOtlpInbox, RawOtlpInboxLimits, SqliteRawOtlpInbox,
};
use sha2::{Digest, Sha256};
use sqlx::{
    Connection,
    sqlite::{SqliteConnectOptions, SqliteConnection},
};

#[tokio::test]
async fn ingestion_and_materialization_commit_during_an_analysis_snapshot() {
    let directory = tempfile::tempdir().expect("temporary shared database");
    let database_url = sqlite_url(&directory.path().join("flaggo.db"));
    let inbox = SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
        .await
        .expect("inbox");
    let writer = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("writer");
    writer
        .commit(commit(1, observation("first", 100, 1)))
        .await
        .expect("seed");
    let options = SqliteConnectOptions::from_str(&database_url)
        .expect("URL")
        .read_only(true);
    let mut reader = SqliteConnection::connect_with(&options)
        .await
        .expect("analysis connection");
    sqlx::query("BEGIN")
        .execute(&mut reader)
        .await
        .expect("read transaction");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM evidence_observations")
        .fetch_one(&mut reader)
        .await
        .expect("establish read snapshot");
    assert_eq!(count, 1);

    tokio::time::timeout(Duration::from_secs(2), async {
        inbox
            .append(NewRawOtlpBatch {
                signal: OtlpSignal::Logs,
                wire_encoding: OtlpWireEncoding::ProtobufJson,
                transport_compression: OtlpTransportCompression::Identity,
                otlp_profile_version: DEFAULT_OTLP_PROFILE_VERSION.parse().expect("profile"),
                payload: br#"{"resourceLogs":[]}"#.to_vec(),
            })
            .await
            .expect("ingestion while analysis reads");
        writer
            .commit(commit(2, observation("second", 100, 2)))
            .await
            .expect("materialization while analysis reads");
    })
    .await
    .expect("writers must not wait for analysis to finish");
    assert_eq!(writer.inspect().await.expect("health").observation_count, 2);
    let frozen: i64 = sqlx::query_scalar("SELECT count(*) FROM evidence_observations")
        .fetch_one(&mut reader)
        .await
        .expect("frozen snapshot");
    assert_eq!(frozen, 1);
    sqlx::query("ROLLBACK")
        .execute(&mut reader)
        .await
        .expect("release snapshot");
    let current: i64 = sqlx::query_scalar("SELECT count(*) FROM evidence_observations")
        .fetch_one(&mut reader)
        .await
        .expect("next snapshot");
    assert_eq!(current, 2);
    reader.close().await.expect("close reader");
    inbox.close().await;
    writer.close().await;
}

#[tokio::test]
async fn query_timeout_interrupts_sqlite_and_releases_the_connection() {
    let directory = tempfile::tempdir().expect("temporary database");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("writer");
    let mut seed = commit(1, observation("first", 100, 1));
    seed.observations = (0..100)
        .map(|index| EvidenceObservationWrite {
            observation: observation(&format!("row-{index}"), 100, index),
            candidate_ordinal: u32::try_from(index).expect("ordinal"),
        })
        .collect();
    writer.commit(seed).await.expect("seed");
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .expect("reader");
    let watermark = reader.capture_watermark().await.expect("watermark");
    let started = std::time::Instant::now();
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                sql: "SELECT count(*) FROM observations a CROSS JOIN observations b \
              CROSS JOIN observations c CROSS JOIN observations d"
                    .to_owned(),
                cutoff_unix_nano: 100,
                watermark,
                limits: EvidenceQueryLimits {
                    timeout: Duration::from_millis(20),
                    ..limits()
                },
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(EvidenceStoreError::QueryLimit("time"))
    ));
    for _ in 0..3 {
        reader
            .query(
                &scope(),
                EvidenceQueryRequest {
                    sql: "SELECT count(*) AS samples FROM observations".to_owned(),
                    cutoff_unix_nano: 100,
                    watermark,
                    limits: limits(),
                },
            )
            .await
            .expect("connection reusable after timeout");
    }
    reader.close().await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "SQLite work must stop, not just its waiter"
    );
    writer.close().await;
}

#[tokio::test]
async fn freezes_queries_at_the_committed_cutoff_and_watermark() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("writer");
    writer
        .commit(commit(1, observation("first", 100, 1)))
        .await
        .expect("first observation");
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .expect("analysis reader");
    let watermark = reader.capture_watermark().await.expect("watermark");
    assert_eq!(ObservationWatermark::new(1), watermark);

    writer
        .commit(commit(2, observation("late", 100, 2)))
        .await
        .expect("late observation");
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                sql: "SELECT observation_id, json_extract(payload_json, '$.value') AS value \
                      FROM observations ORDER BY observation_sequence"
                    .to_owned(),
                cutoff_unix_nano: 100,
                watermark,
                limits: limits(),
            },
        )
        .await
        .expect("bounded query");

    assert_eq!(1, result.rows.len());
    assert_eq!(
        Some(&serde_json::Value::from(1)),
        result.rows[0].get("value")
    );
    assert_eq!(vec![digest(b"observation:first")], result.observation_ids);
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn supports_bounded_aggregate_analysis_with_named_ctes() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("writer");
    writer
        .commit(commit(1, observation("first", 100, 1)))
        .await
        .expect("first observation");
    writer
        .commit(commit(2, observation("second", 100, 3)))
        .await
        .expect("second observation");
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .expect("analysis reader");

    let capabilities = reader.query_capabilities();
    assert!(capabilities.functions.contains(&"avg"));
    assert!(capabilities.features.contains(&"grouping"));
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                sql: "WITH values_by_source AS (\
                          SELECT signal_name, json_extract(payload_json, '$.value') AS value \
                          FROM observations\
                      ) \
                      SELECT signal_name, count(*) AS samples, avg(value) AS average \
                      FROM values_by_source GROUP BY signal_name"
                    .to_owned(),
                cutoff_unix_nano: 100,
                watermark: reader.capture_watermark().await.expect("watermark"),
                limits: limits(),
            },
        )
        .await
        .expect("aggregate query");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get("samples"), Some(&serde_json::json!(2)));
    assert_eq!(result.rows[0].get("average"), Some(&serde_json::json!(2.0)));
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn rejects_mutation_physical_tables_and_unapproved_functions() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("writer");
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .expect("analysis reader");

    for sql in [
        "DELETE FROM observations",
        "SELECT * FROM evidence_observations",
        "SELECT load_extension('anything') FROM observations",
        "SELECT * FROM json_each('{}')",
        "SELECT * FROM observations; SELECT * FROM observations",
    ] {
        assert!(
            matches!(
                reader
                    .query(
                        &scope(),
                        EvidenceQueryRequest {
                            sql: sql.to_owned(),
                            cutoff_unix_nano: 100,
                            watermark: ObservationWatermark::new(0),
                            limits: limits(),
                        },
                    )
                    .await,
                Err(EvidenceStoreError::InvalidQuery(_))
            ),
            "query should be rejected: {sql}"
        );
    }

    reader.close().await;
    writer.close().await;
}

fn limits() -> EvidenceQueryLimits {
    EvidenceQueryLimits {
        max_rows: NonZeroU32::new(10).expect("nonzero"),
        max_bytes: 4096,
        timeout: Duration::from_secs(1),
    }
}

fn scope() -> EvidenceQueryScope {
    EvidenceQueryScope {
        authority: authority(),
        contract_digest: digest(b"contract"),
        sources: vec![EvidenceSourceSelector {
            signal: EvidenceSignal::Log,
            instrumentation_scope: "worker".to_owned(),
            signal_name: "worker.latency".to_owned(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        }],
    }
}

fn commit(inbox_batch_id: u64, observation: EvidenceObservation) -> EvidenceMaterializationCommit {
    EvidenceMaterializationCommit {
        key: ForwardMaterializationKey {
            versions: versions(),
        },
        inbox_batch_id,
        observations: vec![EvidenceObservationWrite {
            observation,
            candidate_ordinal: 0,
        }],
        diagnostics: Vec::new(),
    }
}

fn observation(key: &str, observed_at_unix_nano: u64, value: i64) -> EvidenceObservation {
    let payload =
        serde_json::to_vec(&serde_json::json!({ "value": value })).expect("observation JSON");
    EvidenceObservation {
        observation_id: digest(format!("observation:{key}").as_bytes()),
        logical_source_id: digest(format!("logical:{key}").as_bytes()),
        content_digest: digest(&payload),
        authority: authority(),
        source: SourceKey {
            signal: EvidenceSignal::Log,
            instrumentation_scope: "worker".to_owned(),
            signal_name: "worker.latency".to_owned(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        },
        observed_at_unix_nano,
        observed_time_source: "timeUnixNano".to_owned(),
        protocol_kind: None,
        decision_id: None,
        contract_digest: Some(digest(b"contract")),
        payload_json: payload,
        versions: versions(),
    }
}

fn versions() -> MaterializerVersions {
    MaterializerVersions {
        materializer: "2".to_owned(),
        decoder: "1".to_owned(),
        identity: "2".to_owned(),
        projection: "2".to_owned(),
        routing: "1".to_owned(),
    }
}

fn authority() -> AuthorityScope {
    AuthorityScope::new("local".to_owned(), "worker".to_owned(), "test".to_owned())
        .expect("valid authority")
}

#[allow(dead_code)]
fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
        .expect("fixed time")
        .to_utc()
}

fn digest(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
}
