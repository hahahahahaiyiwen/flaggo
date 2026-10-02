use std::{num::NonZeroU16, str::FromStr};

use chrono::Utc;
use flaggo_evidence_materializer::{CompiledSelectorSnapshot, EvidenceMaterializer, decode_batch};
use flaggo_evidence_store::{DecisionScope, EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::{
    DEFAULT_OTLP_PROFILE_VERSION, NewRawOtlpBatch, OtlpProfileVersion, OtlpSignal,
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
async fn materializes_protocol_and_selected_telemetry_with_backfill_and_conflicts() {
    let fixture = Fixture::new().await;
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

    let scope = DecisionScope::new("test-app".to_owned(), "test-env".to_owned()).unwrap();
    let snapshot_one = selector_snapshot(CONTRACT_DIGEST_ONE);
    let materializer = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        scope.clone(),
        NonZeroU16::new(10).unwrap(),
    );
    materializer
        .activate_snapshot(&snapshot_one, Utc::now())
        .await
        .unwrap();

    let first = materializer.run_once(&snapshot_one).await.unwrap();
    assert_eq!(first.batches_read, 4);
    assert_eq!(first.observations_created, 6);
    assert_eq!(first.associations_created, 5);
    assert_eq!(first.diagnostics_created, 3);
    assert_eq!(first.conflicts_created, 1);
    assert_eq!(first.provenance_created, 6);

    let no_work = materializer.run_once(&snapshot_one).await.unwrap();
    assert_eq!(no_work.batches_read, 0);

    let observations = fixture
        .store
        .list_observations(&scope, NonZeroU16::new(20).unwrap())
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
    assert_eq!(
        fixture
            .store
            .list_associations(&scope, CONTRACT_DIGEST_ONE, NonZeroU16::new(20).unwrap())
            .await
            .unwrap()
            .len(),
        5
    );

    let snapshot_two = selector_snapshot(CONTRACT_DIGEST_TWO);
    materializer
        .activate_snapshot(&snapshot_two, Utc::now())
        .await
        .unwrap();
    let backfill = materializer.run_once(&snapshot_two).await.unwrap();
    assert_eq!(backfill.batches_read, 4);
    assert_eq!(backfill.observations_created, 0);
    assert_eq!(backfill.duplicate_observations, 6);
    assert_eq!(backfill.provenance_created, 6);
    assert_eq!(backfill.associations_created, 5);
    assert_eq!(
        fixture
            .store
            .list_associations(&scope, CONTRACT_DIGEST_TWO, NonZeroU16::new(20).unwrap())
            .await
            .unwrap()
            .len(),
        5
    );

    let restarted = EvidenceMaterializer::new(
        fixture.inbox.clone(),
        fixture.store.clone(),
        scope,
        NonZeroU16::new(10).unwrap(),
    );
    assert_eq!(
        restarted
            .run_once(&snapshot_two)
            .await
            .unwrap()
            .batches_read,
        0
    );
    let health = fixture.store.inspect().await.unwrap();
    assert_eq!(health.observation_count, 6);
    assert_eq!(health.association_count, 10);
    assert_eq!(health.provenance_count, 12);
    assert_eq!(health.conflict_count, 1);
    assert_eq!(health.diagnostic_count, 5);

    fixture.close().await;
}

struct Fixture {
    _directory: TempDir,
    inbox: SqliteRawOtlpInbox,
    store: SqliteEvidenceStore,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite://{}",
            directory.path().join("telemetry.db").display()
        );
        let inbox = SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
            .await
            .unwrap();
        let store = SqliteEvidenceStore::connect(&database_url).await.unwrap();
        Self {
            _directory: directory,
            inbox,
            store,
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
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(resource()),
                scope_spans: vec![ScopeSpans {
                    scope: Some(scope("demo.traces")),
                    spans: vec![Span {
                        trace_id: vec![1; 16],
                        span_id: vec![2; 8],
                        name: "demo.operation".to_owned(),
                        start_time_unix_nano: 100,
                        end_time_unix_nano: 200,
                        attributes: vec![string_kv("worker.id", "worker-1")],
                        events: vec![span::Event {
                            time_unix_nano: 150,
                            name: "demo.event".to_owned(),
                            attributes: vec![string_kv("event.kind", "test")],
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
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

    async fn close(&self) {
        self.inbox.close().await;
        self.store.close().await;
    }
}

fn resource() -> Resource {
    Resource {
        attributes: vec![
            string_kv("service.name", "test-app"),
            string_kv("deployment.environment.name", "test-env"),
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

fn selector_snapshot(contract_digest: &str) -> CompiledSelectorSnapshot {
    let snapshot_digest = snapshot_digest("demo.contract", contract_digest);
    let payload = serde_json::to_vec(&json!({
        "snapshotDigest": snapshot_digest,
        "contracts": [{
            "name": "demo.contract",
            "contractDigest": contract_digest,
            "contract": {
                "name": "demo.contract",
                "learning": {
                    "evidence": [
                        {
                            "name": "latency",
                            "attribute": "latencyMs",
                            "correlateBy": ["workerId"],
                            "source": {
                                "kind": "log",
                                "scope": "demo.logs",
                                "name": "demo.latency",
                                "correlation": {
                                    "workerId": {
                                        "location": "signal",
                                        "attribute": "worker.id"
                                    }
                                }
                            }
                        },
                        {
                            "name": "load",
                            "attribute": "load",
                            "correlateBy": ["workerId"],
                            "source": {
                                "kind": "metric",
                                "scope": "demo.metrics",
                                "name": "demo.load",
                                "metricKind": "gauge",
                                "unit": "1",
                                "correlation": {
                                    "workerId": {
                                        "location": "signal",
                                        "attribute": "worker.id"
                                    }
                                }
                            },
                        },
                        {
                            "name": "operation",
                            "attribute": "operation",
                            "correlateBy": ["application", "region"],
                            "source": {
                                "kind": "span",
                                "scope": "demo.traces",
                                "name": "demo.operation",
                                "correlation": {
                                    "application": {
                                        "location": "resource",
                                        "attribute": "service.name"
                                    },
                                    "region": {
                                        "location": "scope",
                                        "attribute": "scope.region"
                                    }
                                }
                            }
                        },
                        {
                            "name": "operation_event",
                            "attribute": "operationEvent",
                            "correlateBy": ["workerId"],
                            "source": {
                                "kind": "spanEvent",
                                "scope": "demo.traces",
                                "spanName": "demo.operation",
                                "name": "demo.event",
                                "correlation": {
                                    "workerId": {
                                        "location": "parentSpan",
                                        "attribute": "worker.id"
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        }]
    }))
    .unwrap();
    CompiledSelectorSnapshot::compile(payload).unwrap()
}

fn snapshot_digest(name: &str, contract_digest: &str) -> String {
    let mut hasher = Sha256::new();
    for part in ["flaggo-selector-snapshot-v1", name, contract_digest] {
        hasher.update(u64::try_from(part.len()).unwrap().to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}
