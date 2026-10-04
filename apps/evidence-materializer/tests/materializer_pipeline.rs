use std::{
    num::NonZeroU16,
    str::FromStr,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, TimeDelta, Utc};
use flaggo_evidence_materializer::{CompiledContractCatalog, EvidenceMaterializer, decode_batch};
use flaggo_evidence_store::{AuthorityScope, EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::{
    DEFAULT_OTLP_PROFILE_VERSION, InboxClock, NewRawOtlpBatch, OtlpProfileVersion, OtlpSignal,
    OtlpTransportCompression, OtlpWireEncoding, RawOtlpInbox, RawOtlpInboxLimits,
    SqliteRawOtlpInbox,
};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
        trace::v1::ExportTraceServiceRequest,
    },
    common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    metrics::v1::{
        ExponentialHistogram, ExponentialHistogramDataPoint, Gauge, Histogram, HistogramDataPoint,
        Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum, Summary, SummaryDataPoint,
        metric, number_data_point,
    },
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span, span},
};
use prost::Message;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const CONTRACT_DIGEST_ONE: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const CONTRACT_DIGEST_TWO: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn decodes_every_signal_shape_from_both_wire_encodings() {
    let fixture = Fixture::new().await;
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![application_log()])
        .await;
    fixture
        .append_metrics(OtlpWireEncoding::Protobuf, all_metric_shapes())
        .await;
    fixture.append_traces(OtlpWireEncoding::ProtobufJson).await;

    let batches = fixture
        .inbox
        .read_after(None, NonZeroU16::new(10).unwrap())
        .await
        .unwrap();
    assert_eq!(batches.len(), 3);
    assert_eq!(decode_batch(&batches[0]).candidate_count(), 1);
    assert_eq!(decode_batch(&batches[1]).candidate_count(), 5);
    assert_eq!(decode_batch(&batches[2]).candidate_count(), 2);
    assert!(
        batches
            .iter()
            .flat_map(|batch| decode_batch(batch).diagnostics)
            .next()
            .is_none()
    );

    fixture.close().await;
}

#[tokio::test]
async fn decodes_original_protobuf_field_names_for_every_signal() {
    let fixture = Fixture::new().await;
    fixture
        .append_raw(
            OtlpSignal::Logs,
            br#"{"resource_logs":[{"scope_logs":[{"log_records":[{"time_unix_nano":"1","body":{"string_value":"hello"}}]}]}]}"#,
        )
        .await;
    fixture
        .append_raw(
            OtlpSignal::Metrics,
            br#"{"resource_metrics":[{"scope_metrics":[{"metrics":[{"name":"load","gauge":{"data_points":[{"time_unix_nano":"1","as_int":"2"}]}}]}]}]}"#,
        )
        .await;
    fixture
        .append_raw(
            OtlpSignal::Traces,
            br#"{"resource_spans":[{"scope_spans":[{"spans":[{"name":"work","start_time_unix_nano":"1","end_time_unix_nano":"2"}]}]}]}"#,
        )
        .await;

    let batches = fixture
        .inbox
        .read_after(None, NonZeroU16::new(10).unwrap())
        .await
        .unwrap();
    assert_eq!(batches.len(), 3);
    assert!(
        batches
            .iter()
            .all(|batch| decode_batch(batch).candidate_count() == 1)
    );
    fixture.close().await;
}

#[tokio::test]
async fn timestamp_less_duplicate_deliveries_create_no_duplicate_provenance() {
    let fixture = Fixture::new().await;
    let catalog = contract_catalog(CONTRACT_DIGEST_ONE);
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(10).unwrap(),
    );
    materializer
        .activate_catalog(
            &catalog,
            "\"catalog-before-delivery\"".to_owned(),
            Utc::now(),
        )
        .await
        .unwrap();

    fixture
        .append_timestamp_less_signals(OtlpWireEncoding::Protobuf)
        .await;
    let first_received_at = fixture
        .inbox
        .read_after(None, NonZeroU16::new(1).unwrap())
        .await
        .unwrap()[0]
        .received_at;
    fixture.clock.advance(TimeDelta::seconds(1));
    fixture
        .append_timestamp_less_signals(OtlpWireEncoding::Protobuf)
        .await;

    let result = materializer.run_once(&catalog).await.unwrap();
    assert_eq!(result.batches_read, 6);
    assert_eq!(result.observations_created, 4);
    assert_eq!(result.duplicate_observations, 4);
    assert_eq!(result.provenance_created, 4);
    assert_eq!(result.diagnostics_created, 8);

    let observations = fixture
        .store
        .list_observations(
            &authority("local", "test-app", "test-env"),
            NonZeroU16::new(10).unwrap(),
        )
        .await
        .unwrap();
    let first_received_at_unix_nano =
        u64::try_from(first_received_at.timestamp_nanos_opt().unwrap()).unwrap();
    assert_eq!(observations.len(), 4);
    assert!(observations.iter().all(|stored| {
        stored.observation.observed_time_source == "inbox.received_at"
            && stored.observation.observed_at_unix_nano == first_received_at_unix_nano
    }));

    let health = fixture.store.inspect().await.unwrap();
    assert_eq!(health.observation_count, 4);
    assert_eq!(health.provenance_count, 4);
    assert_eq!(health.diagnostic_count, 8);
    fixture.close().await;
}

#[tokio::test]
async fn materializes_protocol_and_selected_telemetry_with_conflict_rejection() {
    let fixture = Fixture::new().await;
    let authority = authority("local", "test-app", "test-env");
    let catalog_one = contract_catalog(CONTRACT_DIGEST_ONE);
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(10).unwrap(),
    );
    materializer
        .activate_catalog(&catalog_one, "\"catalog-one\"".to_owned(), Utc::now())
        .await
        .unwrap();

    fixture
        .append_logs(
            OtlpWireEncoding::ProtobufJson,
            vec![
                valid_decision_log(),
                application_log(),
                uncorrelated_application_log(),
                malformed_decision_log(),
            ],
        )
        .await;
    fixture
        .append_metrics(OtlpWireEncoding::Protobuf, vec![gauge_metric(1)])
        .await;
    fixture
        .append_metrics(OtlpWireEncoding::ProtobufJson, vec![gauge_metric(2)])
        .await;
    fixture.append_traces(OtlpWireEncoding::ProtobufJson).await;

    let first = materializer.run_once(&catalog_one).await.unwrap();
    assert_eq!(first.batches_read, 4);
    assert_eq!(first.observations_created, 6);
    assert_eq!(first.diagnostics_created, 2);
    assert_eq!(first.conflicts_created, 1);
    assert_eq!(first.provenance_created, 6);

    let no_work = materializer.run_once(&catalog_one).await.unwrap();
    assert_eq!(no_work.batches_read, 0);

    let observations = fixture
        .store
        .list_observations(&authority, NonZeroU16::new(20).unwrap())
        .await
        .unwrap();
    assert_eq!(observations.len(), 6);
    let protocol = observations
        .iter()
        .find(|stored| stored.observation.protocol_kind.as_deref() == Some("decision.received"))
        .expect("decision observation");
    assert_eq!(
        protocol.observation.decision_id.as_deref(),
        Some("01956fd2-bc5d-4ad9-8e4b-2a5db5b55511")
    );
    assert_eq!(
        protocol.observation.contract_digest.as_deref(),
        Some(CONTRACT_DIGEST_ONE)
    );

    let catalog_two = contract_catalog(CONTRACT_DIGEST_TWO);
    materializer
        .activate_catalog(&catalog_two, "\"catalog-two\"".to_owned(), Utc::now())
        .await
        .unwrap();

    let deactivated = CompiledContractCatalog::empty();
    materializer
        .activate_catalog(&deactivated, "\"catalog-empty\"".to_owned(), Utc::now())
        .await
        .unwrap();
    let mut inactive_log = application_log();
    inactive_log.time_unix_nano = 2_000;
    inactive_log.observed_time_unix_nano = 2_001;
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![inactive_log])
        .await;
    let inactive = materializer.run_once(&deactivated).await.unwrap();
    assert_eq!(inactive.batches_read, 1);
    assert_eq!(inactive.observations_created, 0);

    let log_only = log_contract_catalog(CONTRACT_DIGEST_ONE);
    materializer
        .activate_catalog(&log_only, "\"catalog-reactivated\"".to_owned(), Utc::now())
        .await
        .unwrap();
    assert_eq!(
        materializer.run_once(&log_only).await.unwrap().batches_read,
        0
    );

    let mut future_log = application_log();
    future_log.time_unix_nano = 3_000;
    future_log.observed_time_unix_nano = 3_001;
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![future_log])
        .await;
    let future = materializer.run_once(&log_only).await.unwrap();
    assert_eq!(future.batches_read, 1);
    assert_eq!(future.observations_created, 1);

    let restarted = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(10).unwrap(),
    );
    assert_eq!(restarted.run_once(&log_only).await.unwrap().batches_read, 0);
    let health = fixture.store.inspect().await.unwrap();
    assert_eq!(health.observation_count, 7);
    assert_eq!(health.provenance_count, 7);
    assert_eq!(health.conflict_count, 1);
    assert_eq!(health.diagnostic_count, 2);

    fixture.close().await;
}

#[tokio::test]
async fn rejects_changed_content_for_explicit_log_and_span_identities() {
    let fixture = Fixture::new().await;
    let catalog = log_and_span_contract_catalog(CONTRACT_DIGEST_ONE);
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(10).unwrap(),
    );
    materializer
        .activate_catalog(&catalog, "\"catalog-identities\"".to_owned(), Utc::now())
        .await
        .unwrap();

    let mut first_log = application_log();
    first_log.attributes.push(string_kv(
        "flaggo.observation.id",
        "01956fd2-bc5d-4ad9-8e4b-2a5db5b55512",
    ));
    let mut changed_log = first_log.clone();
    changed_log.severity_text = "changed".to_owned();
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![first_log, changed_log])
        .await;

    let first_span = test_span(vec![string_kv("worker.id", "worker-1")]);
    let mut changed_span = first_span.clone();
    changed_span.attributes = vec![string_kv("worker.id", "worker-2")];
    fixture
        .append_trace_spans(
            OtlpWireEncoding::ProtobufJson,
            vec![first_span, changed_span],
        )
        .await;

    let result = materializer.run_once(&catalog).await.unwrap();
    assert_eq!(result.batches_read, 2);
    assert_eq!(result.observations_created, 2);
    assert_eq!(result.duplicate_observations, 0);
    assert_eq!(result.provenance_created, 2);
    assert_eq!(result.conflicts_created, 2);
    assert_eq!(result.diagnostics_created, 2);

    let health = fixture.store.inspect().await.unwrap();
    assert_eq!(health.observation_count, 2);
    assert_eq!(health.provenance_count, 2);
    assert_eq!(health.conflict_count, 2);
    assert_eq!(health.diagnostic_count, 2);
    fixture.close().await;
}

#[tokio::test]
async fn isolates_mixed_authorities_and_rejects_legacy_scope_fallbacks() {
    let fixture = Fixture::new().await;
    let catalog = log_contract_catalog(CONTRACT_DIGEST_ONE);
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(10).unwrap(),
    );

    materializer
        .activate_catalog(&catalog, "\"catalog-mixed\"".to_owned(), Utc::now())
        .await
        .unwrap();
    fixture
        .append_mixed_authority_logs(OtlpWireEncoding::ProtobufJson)
        .await;
    let result = materializer.run_once(&catalog).await.unwrap();

    assert_eq!(result.observations_created, 1);
    assert_eq!(result.diagnostics_created, 1);
    assert_eq!(
        fixture
            .store
            .list_observations(
                &authority("local", "test-app", "test-env"),
                NonZeroU16::new(10).unwrap(),
            )
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        fixture
            .store
            .list_observations(
                &authority("acme", "test-app", "test-env"),
                NonZeroU16::new(10).unwrap(),
            )
            .await
            .unwrap()
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn reports_checkpoint_backlog_and_evidence_freshness() {
    let fixture = Fixture::new().await;
    let catalog = contract_catalog(CONTRACT_DIGEST_ONE);
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![application_log()])
        .await;
    fixture.clock.advance(TimeDelta::seconds(1));
    fixture
        .append_logs(OtlpWireEncoding::ProtobufJson, vec![application_log()])
        .await;
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        NonZeroU16::new(1).unwrap(),
    );

    let before = materializer.inspect().await.unwrap();
    assert_eq!(before.checkpoint_batch_id, None);
    assert_eq!(before.pending_batch_count, 2);
    assert!(before.oldest_pending_received_at.is_some());
    assert!(before.newest_pending_received_at.is_some());
    assert_eq!(before.evidence_store.newest_observed_at_unix_nano, None);

    let first = materializer.run_once(&catalog).await.unwrap();
    assert_eq!(first.batches_read, 1);
    let between = materializer.inspect().await.unwrap();
    assert_eq!(between.checkpoint_batch_id, Some(1));
    assert_eq!(between.pending_batch_count, 1);
    assert!(between.oldest_pending_received_at.is_some());
    assert!(
        between
            .evidence_store
            .newest_observed_at_unix_nano
            .is_some()
    );

    let second = materializer.run_once(&catalog).await.unwrap();
    assert_eq!(second.batches_read, 1);
    let after = materializer.inspect().await.unwrap();
    assert_eq!(after.checkpoint_batch_id, Some(2));
    assert_eq!(after.pending_batch_count, 0);
    assert_eq!(after.oldest_pending_received_at, None);
    assert_eq!(after.newest_pending_received_at, None);
    assert!(after.evidence_store.newest_observed_at_unix_nano.is_some());

    fixture.close().await;
}

struct Fixture {
    _directory: TempDir,
    inbox: SqliteRawOtlpInbox,
    store: SqliteEvidenceStore,
    clock: Arc<TestClock>,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite://{}",
            directory.path().join("telemetry.db").display()
        );
        let clock = Arc::new(TestClock::new(Utc::now()));
        let inbox = SqliteRawOtlpInbox::connect_with_clock(
            &database_url,
            RawOtlpInboxLimits::default(),
            clock.clone(),
        )
        .await
        .unwrap();
        let store = SqliteEvidenceStore::connect(&database_url).await.unwrap();
        Self {
            _directory: directory,
            inbox,
            store,
            clock,
        }
    }

    async fn append_logs(&self, encoding: OtlpWireEncoding, records: Vec<LogRecord>) {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(resource()),
                scope_logs: vec![
                    ScopeLogs {
                        scope: Some(scope("@flaggo/sdk")),
                        log_records: records
                            .iter()
                            .filter(|record| record.event_name == "flaggo.decision.received")
                            .cloned()
                            .collect(),
                        schema_url: "https://flaggo.dev/sdk".to_owned(),
                    },
                    ScopeLogs {
                        scope: Some(scope("demo.logs")),
                        log_records: records
                            .into_iter()
                            .filter(|record| record.event_name != "flaggo.decision.received")
                            .collect(),
                        schema_url: String::new(),
                    },
                ],
                schema_url: "https://opentelemetry.io/schemas/1.30.0".to_owned(),
            }],
        };
        self.append(OtlpSignal::Logs, encoding, &request).await;
    }

    async fn append_mixed_authority_logs(&self, encoding: OtlpWireEncoding) {
        let scope_logs = |record| ScopeLogs {
            scope: Some(scope("demo.logs")),
            log_records: vec![record],
            schema_url: String::new(),
        };
        let request = ExportLogsServiceRequest {
            resource_logs: vec![
                ResourceLogs {
                    resource: Some(resource_for("local", "test-app", "test-env")),
                    scope_logs: vec![scope_logs(application_log())],
                    schema_url: String::new(),
                },
                ResourceLogs {
                    resource: Some(resource_for("acme", "test-app", "test-env")),
                    scope_logs: vec![scope_logs(application_log())],
                    schema_url: String::new(),
                },
                ResourceLogs {
                    resource: Some(Resource {
                        attributes: vec![
                            string_kv("service.name", "test-app"),
                            string_kv("deployment.environment.name", "test-env"),
                        ],
                        ..Default::default()
                    }),
                    scope_logs: vec![scope_logs(application_log())],
                    schema_url: String::new(),
                },
            ],
        };
        self.append(OtlpSignal::Logs, encoding, &request).await;
    }

    async fn append_metrics(&self, encoding: OtlpWireEncoding, metrics: Vec<Metric>) {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(resource()),
                scope_metrics: vec![ScopeMetrics {
                    scope: Some(scope("demo.metrics")),
                    metrics,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };
        self.append(OtlpSignal::Metrics, encoding, &request).await;
    }

    async fn append_traces(&self, encoding: OtlpWireEncoding) {
        self.append_traces_with_times(encoding, 100, 200, 150).await;
    }

    async fn append_timestamp_less_signals(&self, encoding: OtlpWireEncoding) {
        let mut log = application_log();
        log.time_unix_nano = 0;
        log.observed_time_unix_nano = 0;
        self.append_logs(encoding, vec![log]).await;

        let mut metric = gauge_metric(1);
        let Some(metric::Data::Gauge(gauge)) = metric.data.as_mut() else {
            unreachable!("gauge metric helper must produce gauge data");
        };
        gauge.data_points[0].start_time_unix_nano = 0;
        gauge.data_points[0].time_unix_nano = 0;
        self.append_metrics(encoding, vec![metric]).await;
        self.append_traces_with_times(encoding, 0, 0, 0).await;
    }

    async fn append_traces_with_times(
        &self,
        encoding: OtlpWireEncoding,
        start_time_unix_nano: u64,
        end_time_unix_nano: u64,
        event_time_unix_nano: u64,
    ) {
        let mut span = test_span(vec![string_kv("worker.id", "worker-1")]);
        span.start_time_unix_nano = start_time_unix_nano;
        span.end_time_unix_nano = end_time_unix_nano;
        span.events = vec![span::Event {
            time_unix_nano: event_time_unix_nano,
            name: "demo.event".to_owned(),
            attributes: vec![string_kv("event.kind", "test")],
            ..Default::default()
        }];
        self.append_trace_spans(encoding, vec![span]).await;
    }

    async fn append_trace_spans(&self, encoding: OtlpWireEncoding, spans: Vec<Span>) {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(resource()),
                scope_spans: vec![ScopeSpans {
                    scope: Some(scope("demo.traces")),
                    spans,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };
        self.append(OtlpSignal::Traces, encoding, &request).await;
    }

    async fn append<T: Message + serde::Serialize>(
        &self,
        signal: OtlpSignal,
        encoding: OtlpWireEncoding,
        request: &T,
    ) {
        let payload = match encoding {
            OtlpWireEncoding::Protobuf => request.encode_to_vec(),
            OtlpWireEncoding::ProtobufJson => serde_json::to_vec(request).unwrap(),
        };
        self.inbox
            .append(NewRawOtlpBatch::new(
                signal,
                encoding,
                OtlpTransportCompression::Identity,
                OtlpProfileVersion::from_str(DEFAULT_OTLP_PROFILE_VERSION).unwrap(),
                payload,
            ))
            .await
            .unwrap();
    }

    async fn append_raw(&self, signal: OtlpSignal, payload: &[u8]) {
        self.inbox
            .append(NewRawOtlpBatch::new(
                signal,
                OtlpWireEncoding::ProtobufJson,
                OtlpTransportCompression::Identity,
                OtlpProfileVersion::from_str(DEFAULT_OTLP_PROFILE_VERSION).unwrap(),
                payload.to_vec(),
            ))
            .await
            .unwrap();
    }

    async fn close(&self) {
        self.inbox.close().await;
        self.store.close().await;
    }
}

struct TestClock {
    current: Mutex<DateTime<Utc>>,
}

impl TestClock {
    fn new(current: DateTime<Utc>) -> Self {
        Self {
            current: Mutex::new(current),
        }
    }

    fn advance(&self, duration: TimeDelta) {
        let mut current = self.current.lock().expect("test clock lock");
        *current += duration;
    }
}

impl InboxClock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.current.lock().expect("test clock lock")
    }
}

fn resource() -> Resource {
    resource_for("local", "test-app", "test-env")
}

fn resource_for(tenant: &str, application: &str, environment: &str) -> Resource {
    Resource {
        attributes: vec![
            string_kv("flaggo.tenant", tenant),
            string_kv("flaggo.application", application),
            string_kv("flaggo.environment", environment),
            string_kv("service.name", application),
            string_kv("deployment.environment.name", environment),
        ],
        ..Default::default()
    }
}

fn scope(name: &str) -> InstrumentationScope {
    InstrumentationScope {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        attributes: vec![string_kv("scope.region", "west")],
        ..Default::default()
    }
}

fn application_log() -> LogRecord {
    LogRecord {
        time_unix_nano: 1_000,
        observed_time_unix_nano: 1_001,
        event_name: "demo.latency".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::KvlistValue(
                opentelemetry_proto::tonic::common::v1::KeyValueList {
                    values: vec![
                        string_kv("duplicate", "first"),
                        string_kv("duplicate", "second"),
                        KeyValue {
                            key: "array".to_owned(),
                            value: Some(AnyValue {
                                value: Some(any_value::Value::ArrayValue(
                                    opentelemetry_proto::tonic::common::v1::ArrayValue {
                                        values: vec![
                                            AnyValue {
                                                value: Some(any_value::Value::IntValue(i64::MIN)),
                                            },
                                            AnyValue {
                                                value: Some(any_value::Value::DoubleValue(-0.0)),
                                            },
                                            AnyValue {
                                                value: Some(any_value::Value::BytesValue(vec![
                                                    0, 255,
                                                ])),
                                            },
                                        ],
                                    },
                                )),
                            }),
                            ..Default::default()
                        },
                    ],
                },
            )),
        }),
        attributes: vec![string_kv("worker.id", "worker-1")],
        ..Default::default()
    }
}

fn test_span(attributes: Vec<KeyValue>) -> Span {
    Span {
        trace_id: vec![1; 16],
        span_id: vec![2; 8],
        name: "demo.operation".to_owned(),
        start_time_unix_nano: 100,
        end_time_unix_nano: 200,
        attributes,
        ..Default::default()
    }
}

fn valid_decision_log() -> LogRecord {
    let result = "4";
    LogRecord {
        time_unix_nano: 900,
        observed_time_unix_nano: 901,
        event_name: "flaggo.decision.received".to_owned(),
        attributes: vec![
            string_kv("flaggo.signal", "decision.received"),
            string_kv("flaggo.decision.id", "01956fd2-bc5d-4ad9-8e4b-2a5db5b55511"),
            string_kv("flaggo.contract.name", "demo.contract"),
            string_kv("flaggo.contract.digest", CONTRACT_DIGEST_ONE),
            string_kv(
                "flaggo.executable.digest",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            string_kv("flaggo.result.json", result),
            string_kv(
                "flaggo.result.hash",
                &format!("sha256:{:x}", Sha256::digest(result.as_bytes())),
            ),
            string_kv("flaggo.evaluation.source", "default"),
            string_kv("flaggo.correlation.workerId", "worker-1"),
        ],
        ..Default::default()
    }
}

fn uncorrelated_application_log() -> LogRecord {
    LogRecord {
        time_unix_nano: 1_002,
        observed_time_unix_nano: 1_003,
        event_name: "demo.latency".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(
                "missing correlation".to_owned(),
            )),
        }),
        ..Default::default()
    }
}

fn malformed_decision_log() -> LogRecord {
    LogRecord {
        time_unix_nano: 902,
        event_name: "flaggo.decision.received".to_owned(),
        attributes: vec![string_kv("flaggo.signal", "decision.received")],
        ..Default::default()
    }
}

fn all_metric_shapes() -> Vec<Metric> {
    vec![
        gauge_metric(1),
        Metric {
            name: "demo.sum".to_owned(),
            unit: "1".to_owned(),
            data: Some(metric::Data::Sum(Sum {
                data_points: vec![number_point(2)],
                aggregation_temporality: 2,
                is_monotonic: true,
            })),
            ..Default::default()
        },
        Metric {
            name: "demo.histogram".to_owned(),
            unit: "ms".to_owned(),
            data: Some(metric::Data::Histogram(Histogram {
                data_points: vec![HistogramDataPoint {
                    attributes: vec![string_kv("worker.id", "worker-1")],
                    start_time_unix_nano: 100,
                    time_unix_nano: 200,
                    count: 1,
                    sum: Some(-0.0),
                    bucket_counts: vec![0, 1],
                    explicit_bounds: vec![10.0],
                    min: Some(f64::NEG_INFINITY),
                    max: Some(f64::INFINITY),
                    ..Default::default()
                }],
                aggregation_temporality: 1,
            })),
            ..Default::default()
        },
        Metric {
            name: "demo.exponential".to_owned(),
            unit: "ms".to_owned(),
            data: Some(metric::Data::ExponentialHistogram(
                ExponentialHistogram {
                    data_points: vec![ExponentialHistogramDataPoint {
                        attributes: vec![string_kv("worker.id", "worker-1")],
                        start_time_unix_nano: 100,
                        time_unix_nano: 200,
                        count: 1,
                        sum: Some(1.0),
                        positive: Some(
                            opentelemetry_proto::tonic::metrics::v1::exponential_histogram_data_point::Buckets {
                                offset: 0,
                                bucket_counts: vec![1],
                            },
                        ),
                        ..Default::default()
                    }],
                    aggregation_temporality: 2,
                },
            )),
            ..Default::default()
        },
        Metric {
            name: "demo.summary".to_owned(),
            unit: "ms".to_owned(),
            data: Some(metric::Data::Summary(Summary {
                data_points: vec![SummaryDataPoint {
                    attributes: vec![string_kv("worker.id", "worker-1")],
                    start_time_unix_nano: 100,
                    time_unix_nano: 200,
                    count: 1,
                    sum: 1.0,
                    quantile_values: vec![
                        opentelemetry_proto::tonic::metrics::v1::summary_data_point::ValueAtQuantile {
                            quantile: 0.5,
                            value: 1.0,
                        },
                    ],
                    ..Default::default()
                }],
            })),
            ..Default::default()
        },
    ]
}

fn gauge_metric(value: i64) -> Metric {
    Metric {
        name: "demo.load".to_owned(),
        unit: "1".to_owned(),
        data: Some(metric::Data::Gauge(Gauge {
            data_points: vec![number_point(value)],
        })),
        ..Default::default()
    }
}

fn number_point(value: i64) -> NumberDataPoint {
    NumberDataPoint {
        attributes: vec![string_kv("worker.id", "worker-1")],
        start_time_unix_nano: 100,
        time_unix_nano: 200,
        value: Some(number_data_point::Value::AsInt(value)),
        ..Default::default()
    }
}

fn string_kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
        ..Default::default()
    }
}

fn contract_catalog(contract_digest: &str) -> CompiledContractCatalog {
    compile_catalog(contract_digest, evidence_sources())
}

fn log_contract_catalog(contract_digest: &str) -> CompiledContractCatalog {
    compile_catalog(contract_digest, vec![evidence_sources().remove(0)])
}

fn log_and_span_contract_catalog(contract_digest: &str) -> CompiledContractCatalog {
    let sources = evidence_sources();
    compile_catalog(
        contract_digest,
        vec![sources[0].clone(), sources[2].clone()],
    )
}

fn compile_catalog(
    contract_digest: &str,
    evidence: Vec<serde_json::Value>,
) -> CompiledContractCatalog {
    let payload = serde_json::to_vec(&json!({
        "contracts": [{
            "contractDigest": contract_digest,
            "contract": {
                "authority": {
                    "tenant": "local",
                    "application": "test-app",
                    "environment": "test-env"
                },
                "name": "demo.contract",
                "learning": {
                    "evidence": evidence
                }
            }
        }]
    }))
    .unwrap();
    CompiledContractCatalog::compile(payload).unwrap()
}

fn evidence_sources() -> Vec<serde_json::Value> {
    vec![
        json!({
            "source": {
                "kind": "log",
                "scope": "demo.logs",
                "name": "demo.latency",
                "correlation": {}
            }
        }),
        json!({
            "source": {
                "kind": "metric",
                "scope": "demo.metrics",
                "name": "demo.load",
                "metricKind": "gauge",
                "unit": "1",
                "correlation": {}
            }
        }),
        json!({
            "source": {
                "kind": "span",
                "scope": "demo.traces",
                "name": "demo.operation",
                "correlation": {}
            }
        }),
        json!({
            "source": {
                "kind": "spanEvent",
                "scope": "demo.traces",
                "spanName": "demo.operation",
                "name": "demo.event",
                "correlation": {}
            }
        }),
    ]
}

fn authority(tenant: &str, application: &str, environment: &str) -> AuthorityScope {
    AuthorityScope::new(
        tenant.to_owned(),
        application.to_owned(),
        environment.to_owned(),
    )
    .unwrap()
}
