use std::{num::NonZeroU32, path::Path, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceAnalysisStore, EvidenceMaterializationCommit, EvidenceObservation,
    EvidenceObservationWrite, EvidenceQueryLimits, EvidenceQueryRequest, EvidenceQueryScope,
    EvidenceSignal, EvidenceSourceSelector, EvidenceStore, EvidenceStoreError, EvidenceValue,
    ForwardMaterializationKey, MaterializerVersions, SQLITE_EVIDENCE_QUERY_CAPABILITIES, SourceKey,
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
                authorization: flaggo_raw_otlp_inbox::OtlpAuthorizationEnvelope::new(
                    "trusted-test".into(),
                    vec![],
                )
                .unwrap(),
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
    let started = std::time::Instant::now();
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "SELECT count(*) FROM observations a CROSS JOIN observations b \
              CROSS JOIN observations c CROSS JOIN observations d"
                    .to_owned(),
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
                    profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                    sql: "SELECT count(*) AS samples FROM observations".to_owned(),
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
async fn subsequent_live_queries_include_late_materialization_of_old_events() {
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
    let query = EvidenceQueryRequest {
        profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
        sql: "SELECT observation_id, json_extract(payload_json, '$.value') AS value \
              FROM observations ORDER BY observation_sequence"
            .to_owned(),
        limits: limits(),
    };
    let initial = reader
        .query(&scope(), query.clone())
        .await
        .expect("initial query");
    assert_eq!(initial.rows.len(), 1);

    writer
        .commit(commit(2, observation("late", 50, 2)))
        .await
        .expect("late observation");
    let result = reader.query(&scope(), query).await.expect("bounded query");

    assert_eq!(2, result.rows.len());
    assert_eq!(
        Some(&EvidenceValue::Int64 { value: 1 }),
        result.rows[0].get("value")
    );
    assert_eq!(
        result.rows[1].get("value"),
        Some(&EvidenceValue::Int64 { value: 2 })
    );
    assert!(
        result
            .observation_ids
            .contains(&digest(b"observation:first"))
    );
    assert!(
        result
            .observation_ids
            .contains(&digest(b"observation:late"))
    );
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn dropping_a_query_interrupts_backend_work_before_connection_reuse() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
    let mut seed = commit(1, observation("first", 100, 1));
    seed.observations = (0..100)
        .map(|index| EvidenceObservationWrite {
            observation: observation(&format!("row-{index}"), 100, index),
            candidate_ordinal: u32::try_from(index).unwrap(),
        })
        .collect();
    writer.commit(seed).await.unwrap();
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .unwrap();
    let request = EvidenceQueryRequest {
        profile: reader.query_capabilities().profile.to_owned(),
        sql: "SELECT count(*) FROM observations a CROSS JOIN observations b CROSS JOIN observations c \
              CROSS JOIN observations d CROSS JOIN observations e CROSS JOIN observations f".to_owned(),
        limits: EvidenceQueryLimits { timeout: Duration::from_secs(30), ..limits() },
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), reader.query(&scope(), request))
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        for _ in 0..3 {
            reader
                .query(
                    &scope(),
                    EvidenceQueryRequest {
                        profile: reader.query_capabilities().profile.to_owned(),
                        sql: "SELECT count(*) FROM observations".to_owned(),
                        limits: limits(),
                    },
                )
                .await
                .unwrap();
        }
        reader.close().await;
    })
    .await
    .expect("abandoned backend work must stop, not wait for its original deadline");
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
    assert_eq!(capabilities.relations[0].name, "observations");
    assert_eq!(capabilities.relations[0].columns.len(), 16);
    assert_eq!(
        serde_json::to_value(capabilities.relations[0].columns[0]).unwrap(),
        serde_json::json!({"name": "observation_sequence", "providerType": "INTEGER", "kind":"int64", "nullable":null})
    );
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "WITH values_by_source AS (\
                          SELECT signal_name, json_extract(payload_json, '$.value') AS value \
                          FROM observations\
                      ) \
                      SELECT signal_name, count(*) AS samples, avg(value) AS average \
                      FROM values_by_source GROUP BY signal_name"
                    .to_owned(),
                limits: limits(),
            },
        )
        .await
        .expect("aggregate query");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0].get("samples"),
        Some(&EvidenceValue::Int64 { value: 2 })
    );
    assert_eq!(
        result.rows[0].get("average"),
        Some(&EvidenceValue::Float64 {
            bits: 2.0_f64.to_bits()
        })
    );
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn distinguishes_complete_empty_and_truncated_results_with_one_row_lookahead() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
    let mut seed = commit(1, observation("first", 100, 1));
    seed.observations = (0..125)
        .map(|index| EvidenceObservationWrite {
            observation: observation(&format!("row-{index}"), 100, index),
            candidate_ordinal: u32::try_from(index).unwrap(),
        })
        .collect();
    writer.commit(seed).await.unwrap();
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .unwrap();
    for (max_rows, saved, truncated) in [(100, 100, true), (125, 125, false), (200, 125, false)] {
        let result = reader.query(&scope(), EvidenceQueryRequest {
            profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
            sql: "SELECT observation_sequence FROM observations ORDER BY observation_sequence".to_owned(),
            limits: EvidenceQueryLimits { max_rows: NonZeroU32::new(max_rows).unwrap(), max_bytes: 65536, ..limits() },
        }).await.unwrap();
        assert_eq!(result.rows.len(), saved);
        assert_eq!(result.truncated, truncated);
    }
    let empty = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "SELECT observation_sequence FROM observations WHERE observation_sequence < 0"
                    .to_owned(),
                limits: limits(),
            },
        )
        .await
        .unwrap();
    assert!(empty.rows.is_empty());
    assert!(!empty.truncated);
    assert_eq!(
        serde_json::to_value(&empty.columns).unwrap(),
        serde_json::json!([{"name": "observation_sequence", "providerType": "INTEGER", "kind":"int64", "nullable":null}])
    );
    let bytes = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "SELECT observation_sequence FROM observations".to_owned(),
                limits: EvidenceQueryLimits {
                    max_bytes: 1,
                    ..limits()
                },
            },
        )
        .await;
    assert!(matches!(bytes, Err(EvidenceStoreError::QueryLimit("byte"))));
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn prepares_aliases_and_declared_types_even_for_empty_results() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
    writer
        .commit(commit(1, observation("first", 100, 1)))
        .await
        .unwrap();
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .unwrap();
    let projection = "SELECT observation_sequence AS sequence, signal_name AS name, \
        json_extract(payload_json, '$.value') AS value, observation_sequence + 1 AS next, \
        CAST(observation_sequence AS TEXT) AS cast_value FROM observations";
    for suffix in [" WHERE 0", "", ""] {
        let result = reader
            .query(
                &scope(),
                EvidenceQueryRequest {
                    profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                    sql: format!("{projection}{suffix}"),
                    limits: limits(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&result.columns).unwrap(),
            serde_json::json!([
                {"name": "sequence", "providerType": "INTEGER", "kind":"int64", "nullable":null},
                {"name": "name", "providerType": "TEXT", "kind":"text", "nullable":null},
                {"name": "value", "providerType": null, "kind":"variant", "nullable":null},
                {"name": "next", "providerType": null, "kind":"variant", "nullable":null},
                {"name": "cast_value", "providerType": null, "kind":"variant", "nullable":null}
            ])
        );
    }
    for sql in [
        "SELECT observation_id AS duplicate, signal_name AS duplicate FROM observations",
        "SELECT observation_id AS Duplicate, signal_name AS duplicate FROM observations",
        "SELECT observation_id, observation_id FROM observations WHERE 0",
        "WITH sample AS (SELECT * FROM observations) SELECT observation_id AS duplicate, \
         signal_name AS duplicate FROM sample",
    ] {
        assert!(
            matches!(
                reader.query(&scope(), EvidenceQueryRequest {
                    profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                    sql: sql.to_owned(),
                    limits: limits(),
                }).await,
                Err(EvidenceStoreError::InvalidQuery(message)) if message.contains("duplicate output")
            ),
            "{sql}"
        );
    }
    reader.close().await;
    writer.close().await;
}

#[tokio::test]
async fn live_queries_preserve_authority_exact_or_null_contract_and_source_filters() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
    let mut seed = commit(1, observation("matching", 100, 1));
    let mut observations = vec![observation("matching", 100, 1)];
    let mut null_contract = observation("null-contract", 200, 2);
    null_contract.contract_digest = None;
    observations.push(null_contract);
    for field in [
        "tenant",
        "application",
        "environment",
        "contract",
        "signal",
        "scope",
        "name",
        "kind",
        "unit",
        "parent",
    ] {
        let mut other = observation(field, 100, 3);
        match field {
            "tenant" => other.authority.tenant = "other".to_owned(),
            "application" => other.authority.application = "other".to_owned(),
            "environment" => other.authority.environment = "other".to_owned(),
            "contract" => other.contract_digest = Some(digest(b"other")),
            "signal" => other.source.signal = EvidenceSignal::Span,
            "scope" => other.source.instrumentation_scope = "other".to_owned(),
            "name" => other.source.signal_name = "other".to_owned(),
            "kind" | "unit" => {
                other.source.signal = EvidenceSignal::Metric;
                other.source.metric_kind =
                    Some(if field == "kind" { "sum" } else { "gauge" }.to_owned());
                other.source.metric_unit =
                    Some(if field == "unit" { "s" } else { "ms" }.to_owned());
            }
            "parent" => {
                other.source.signal = EvidenceSignal::SpanEvent;
                other.source.parent_span_name = Some("other".to_owned());
            }
            _ => unreachable!(),
        }
        observations.push(other);
    }
    let mut metric = observation("metric-match", 100, 4);
    metric.source.signal = EvidenceSignal::Metric;
    metric.source.metric_kind = Some("gauge".to_owned());
    metric.source.metric_unit = Some("ms".to_owned());
    observations.push(metric);
    let mut event = observation("event-match", 100, 5);
    event.source.signal = EvidenceSignal::SpanEvent;
    event.source.parent_span_name = Some("parent".to_owned());
    observations.push(event);
    seed.observations = observations
        .into_iter()
        .enumerate()
        .map(|(index, observation)| EvidenceObservationWrite {
            observation,
            candidate_ordinal: u32::try_from(index).unwrap(),
        })
        .collect();
    writer.commit(seed).await.unwrap();
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .unwrap();
    let result = reader
        .query(
            &scope(),
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "SELECT observation_id FROM observations ORDER BY observation_sequence"
                    .to_owned(),
                limits: limits(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.rows.len(), 2);
    assert!(
        result
            .observation_ids
            .contains(&digest(b"observation:matching"))
    );
    assert!(
        result
            .observation_ids
            .contains(&digest(b"observation:null-contract"))
    );
    let mut metric_scope = scope();
    metric_scope.sources[0].signal = EvidenceSignal::Metric;
    metric_scope.sources[0].metric_kind = Some("gauge".to_owned());
    metric_scope.sources[0].metric_unit = Some("ms".to_owned());
    let mut event_scope = scope();
    event_scope.sources[0].signal = EvidenceSignal::SpanEvent;
    event_scope.sources[0].parent_span_name = Some("parent".to_owned());
    for (scope, expected) in [
        (&metric_scope, "metric-match"),
        (&event_scope, "event-match"),
    ] {
        let result = reader
            .query(
                scope,
                EvidenceQueryRequest {
                    profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                    sql: "SELECT observation_id FROM observations".to_owned(),
                    limits: limits(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            result.observation_ids,
            vec![digest(format!("observation:{expected}").as_bytes())]
        );
    }
    let mut combined = scope();
    combined.sources.extend(metric_scope.sources);
    combined.sources.extend(event_scope.sources);
    let combined_result = reader
        .query(
            &combined,
            EvidenceQueryRequest {
                profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                sql: "SELECT observation_id FROM observations".to_owned(),
                limits: limits(),
            },
        )
        .await
        .unwrap();
    assert_eq!(combined_result.rows.len(), 4);
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
        "WITH observations AS (SELECT 1) SELECT * FROM observations",
        "WITH RECURSIVE counter AS (SELECT 1) SELECT * FROM counter",
    ] {
        assert!(
            matches!(
                reader
                    .query(
                        &scope(),
                        EvidenceQueryRequest {
                            profile: SQLITE_EVIDENCE_QUERY_CAPABILITIES.profile.to_owned(),
                            sql: sql.to_owned(),
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

#[tokio::test]
async fn negotiated_cells_preserve_precision_variant_columns_and_unsigned_timestamps() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
    writer
        .commit(commit(1, observation("wide", u64::MAX, 1)))
        .await
        .unwrap();
    let reader = SqliteEvidenceAnalysisStore::connect(&database_url)
        .await
        .unwrap();
    let mut request = EvidenceQueryRequest {
        profile: "unsupported/v1".to_owned(),
        sql: "SELECT 9223372036854775807 AS largest, -9223372036854775808 AS smallest, \
              1.0 AS real_value, '1' AS text_value, NULL AS missing, 1e999 AS infinity, \
              observed_at_unix_nano FROM observations"
            .to_owned(),
        limits: limits(),
    };
    assert!(matches!(reader.query(&scope(), request.clone()).await,
        Err(EvidenceStoreError::InvalidQuery(message)) if message.contains("unsupported evidence query profile")));
    request.profile = reader.query_capabilities().profile.to_owned();
    let result = reader.query(&scope(), request).await.unwrap();
    let row = &result.rows[0];
    assert_eq!(row["largest"], EvidenceValue::Int64 { value: i64::MAX });
    assert_eq!(row["smallest"], EvidenceValue::Int64 { value: i64::MIN });
    assert_eq!(
        row["real_value"],
        EvidenceValue::Float64 {
            bits: 1.0_f64.to_bits()
        }
    );
    assert_eq!(
        row["infinity"],
        EvidenceValue::Float64 {
            bits: f64::INFINITY.to_bits()
        }
    );
    assert_eq!(
        row["text_value"],
        EvidenceValue::Text {
            value: "1".to_owned()
        }
    );
    assert_eq!(row["missing"], EvidenceValue::Null {});
    assert_eq!(
        row["observed_at_unix_nano"],
        EvidenceValue::Text {
            value: u64::MAX.to_string()
        }
    );
    assert!(
        result.observation_ids.is_empty(),
        "aggregates and unprojected IDs are not lineage"
    );
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
