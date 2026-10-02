use chrono::{DateTime, Utc};
use flaggo_evidence_store::EvidenceSignal;
use flaggo_raw_otlp_inbox::{OtlpSignal, OtlpWireEncoding, RawOtlpInboxBatch};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
        trace::v1::ExportTraceServiceRequest,
    },
    common::v1::{InstrumentationScope, KeyValue, any_value},
    logs::v1::LogRecord,
    metrics::v1::{
        Exemplar, ExponentialHistogramDataPoint, HistogramDataPoint, Metric, NumberDataPoint,
        SummaryDataPoint, exemplar, exponential_histogram_data_point, metric, number_data_point,
    },
    resource::v1::Resource,
    trace::v1::{Span, span},
};
use prost::Message;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::candidate::{Candidate, any_value_json, attributes_json, encode_hex, lookup_attribute};

const EXPLICIT_OBSERVATION_ID_ATTRIBUTE: &str = "flaggo.observation.id";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecoderDiagnostic {
    pub candidate_ordinal: Option<u32>,
    pub code: String,
    pub message: String,
    pub detail: Value,
}

#[derive(Clone, Debug, Default)]
pub struct DecodedBatch {
    pub(crate) candidates: Vec<Candidate>,
    pub diagnostics: Vec<DecoderDiagnostic>,
}

impl DecodedBatch {
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }
}

pub fn decode_batch(batch: &RawOtlpInboxBatch) -> DecodedBatch {
    match batch.signal {
        OtlpSignal::Logs => {
            decode_message::<ExportLogsServiceRequest>(&batch.payload, batch.wire_encoding)
                .map(|request| decompose_logs(request, batch.received_at))
                .unwrap_or_else(decode_failure)
        }
        OtlpSignal::Metrics => {
            decode_message::<ExportMetricsServiceRequest>(&batch.payload, batch.wire_encoding)
                .map(|request| decompose_metrics(request, batch.received_at))
                .unwrap_or_else(decode_failure)
        }
        OtlpSignal::Traces => {
            decode_message::<ExportTraceServiceRequest>(&batch.payload, batch.wire_encoding)
                .map(|request| decompose_traces(request, batch.received_at))
                .unwrap_or_else(decode_failure)
        }
    }
}

fn decode_message<T>(payload: &[u8], encoding: OtlpWireEncoding) -> Result<T, String>
where
    T: Default + DeserializeOwned + Message,
{
    match encoding {
        OtlpWireEncoding::ProtobufJson => {
            serde_json::from_slice(payload).map_err(|error| error.to_string())
        }
        OtlpWireEncoding::Protobuf => T::decode(payload).map_err(|error| error.to_string()),
    }
}

fn decode_failure(error: String) -> DecodedBatch {
    DecodedBatch {
        candidates: Vec::new(),
        diagnostics: vec![DecoderDiagnostic {
            candidate_ordinal: None,
            code: "otlp.decode_failed".to_owned(),
            message: "The retained OTLP batch could not be decoded.".to_owned(),
            detail: json!({ "error": error }),
        }],
    }
}

fn decompose_logs(request: ExportLogsServiceRequest, received_at: DateTime<Utc>) -> DecodedBatch {
    let mut decoded = DecodedBatch::default();
    for resource_logs in request.resource_logs {
        for scope_logs in resource_logs.scope_logs {
            for record in scope_logs.log_records {
                let ordinal = next_ordinal(&decoded.candidates);
                let (observed_at_unix_nano, observed_time_source) = log_time(&record, received_at);
                if observed_time_source == "inbox.received_at" {
                    decoded
                        .diagnostics
                        .push(timestamp_fallback_diagnostic(ordinal));
                }

                let explicit_identity =
                    lookup_attribute(&record.attributes, EXPLICIT_OBSERVATION_ID_ATTRIBUTE)
                        .ok()
                        .and_then(|value| match &value.value {
                            Some(any_value::Value::StringValue(value)) if !value.is_empty() => {
                                Some(value.clone())
                            }
                            _ => None,
                        });
                decoded.candidates.push(Candidate {
                    ordinal,
                    signal: EvidenceSignal::Log,
                    instrumentation_scope: scope_name(scope_logs.scope.as_ref()),
                    signal_name: record.event_name.clone(),
                    metric_kind: None,
                    metric_unit: None,
                    observed_at_unix_nano,
                    observed_time_source,
                    resource: resource_logs.resource.clone(),
                    resource_schema_url: resource_logs.schema_url.clone(),
                    scope: scope_logs.scope.clone(),
                    scope_schema_url: scope_logs.schema_url.clone(),
                    signal_attributes: record.attributes.clone(),
                    parent_span_name: None,
                    parent_span_attributes: Vec::new(),
                    payload: log_record_json(&record),
                    identity: explicit_identity.as_ref().map_or(
                        Value::Null,
                        |value| json!({ "explicitObservationId": value }),
                    ),
                    identity_is_content_derived: explicit_identity.is_none(),
                });
            }
        }
    }
    decoded
}

fn decompose_metrics(
    request: ExportMetricsServiceRequest,
    received_at: DateTime<Utc>,
) -> DecodedBatch {
    let mut decoded = DecodedBatch::default();
    for resource_metrics in request.resource_metrics {
        for scope_metrics in resource_metrics.scope_metrics {
            for metric in scope_metrics.metrics {
                let context = MetricContext {
                    resource: resource_metrics.resource.clone(),
                    resource_schema_url: resource_metrics.schema_url.clone(),
                    scope: scope_metrics.scope.clone(),
                    scope_schema_url: scope_metrics.schema_url.clone(),
                };
                decompose_metric(&mut decoded, &metric, &context, received_at);
            }
        }
    }
    decoded
}

fn decompose_traces(
    request: ExportTraceServiceRequest,
    received_at: DateTime<Utc>,
) -> DecodedBatch {
    let mut decoded = DecodedBatch::default();
    for resource_spans in request.resource_spans {
        for scope_spans in resource_spans.scope_spans {
            for span in scope_spans.spans {
                let span_ordinal = next_ordinal(&decoded.candidates);
                let (span_time, span_time_source) =
                    native_time(span.end_time_unix_nano, received_at);
                if span_time_source == "inbox.received_at" {
                    decoded
                        .diagnostics
                        .push(timestamp_fallback_diagnostic(span_ordinal));
                }
                let span_identity = json!({
                    "spanId": encode_hex(&span.span_id),
                    "traceId": encode_hex(&span.trace_id)
                });
                decoded.candidates.push(Candidate {
                    ordinal: span_ordinal,
                    signal: EvidenceSignal::Span,
                    instrumentation_scope: scope_name(scope_spans.scope.as_ref()),
                    signal_name: span.name.clone(),
                    metric_kind: None,
                    metric_unit: None,
                    observed_at_unix_nano: span_time,
                    observed_time_source: span_time_source,
                    resource: resource_spans.resource.clone(),
                    resource_schema_url: resource_spans.schema_url.clone(),
                    scope: scope_spans.scope.clone(),
                    scope_schema_url: scope_spans.schema_url.clone(),
                    signal_attributes: span.attributes.clone(),
                    parent_span_name: None,
                    parent_span_attributes: Vec::new(),
                    payload: span_json(&span),
                    identity: span_identity.clone(),
                    identity_is_content_derived: false,
                });

                for (event_index, event) in span.events.iter().enumerate() {
                    let ordinal = next_ordinal(&decoded.candidates);
                    let (event_time, event_time_source) =
                        native_time(event.time_unix_nano, received_at);
                    if event_time_source == "inbox.received_at" {
                        decoded
                            .diagnostics
                            .push(timestamp_fallback_diagnostic(ordinal));
                    }
                    decoded.candidates.push(Candidate {
                        ordinal,
                        signal: EvidenceSignal::SpanEvent,
                        instrumentation_scope: scope_name(scope_spans.scope.as_ref()),
                        signal_name: event.name.clone(),
                        metric_kind: None,
                        metric_unit: None,
                        observed_at_unix_nano: event_time,
                        observed_time_source: event_time_source,
                        resource: resource_spans.resource.clone(),
                        resource_schema_url: resource_spans.schema_url.clone(),
                        scope: scope_spans.scope.clone(),
                        scope_schema_url: scope_spans.schema_url.clone(),
                        signal_attributes: event.attributes.clone(),
                        parent_span_name: Some(span.name.clone()),
                        parent_span_attributes: span.attributes.clone(),
                        payload: json!({
                            "event": span_event_json(event),
                            "eventIndex": event_index.to_string(),
                            "parentSpan": span_json(&span)
                        }),
                        identity: json!({
                            "eventIndex": event_index.to_string(),
                            "parentSpan": span_identity
                        }),
                        identity_is_content_derived: false,
                    });
                }
            }
        }
    }
    decoded
}

struct MetricContext {
    resource: Option<Resource>,
    resource_schema_url: String,
    scope: Option<InstrumentationScope>,
    scope_schema_url: String,
}

fn decompose_metric(
    decoded: &mut DecodedBatch,
    metric: &Metric,
    context: &MetricContext,
    received_at: DateTime<Utc>,
) {
    let Some(data) = metric.data.as_ref() else {
        decoded.diagnostics.push(DecoderDiagnostic {
            candidate_ordinal: None,
            code: "metric.data_missing".to_owned(),
            message: "An OTLP metric has no recognized data type.".to_owned(),
            detail: json!({ "metricName": metric.name }),
        });
        return;
    };

    match data {
        metric::Data::Gauge(gauge) => {
            for point in &gauge.data_points {
                push_metric_point(
                    decoded,
                    metric,
                    context,
                    received_at,
                    "gauge",
                    None,
                    point.start_time_unix_nano,
                    point.time_unix_nano,
                    point.attributes.clone(),
                    number_point_json(point),
                );
            }
        }
        metric::Data::Sum(sum) => {
            for point in &sum.data_points {
                push_metric_point(
                    decoded,
                    metric,
                    context,
                    received_at,
                    "sum",
                    Some(json!({
                        "aggregationTemporality": sum.aggregation_temporality,
                        "isMonotonic": sum.is_monotonic
                    })),
                    point.start_time_unix_nano,
                    point.time_unix_nano,
                    point.attributes.clone(),
                    number_point_json(point),
                );
            }
        }
        metric::Data::Histogram(histogram) => {
            for point in &histogram.data_points {
                push_metric_point(
                    decoded,
                    metric,
                    context,
                    received_at,
                    "histogram",
                    Some(json!({
                        "aggregationTemporality": histogram.aggregation_temporality
                    })),
                    point.start_time_unix_nano,
                    point.time_unix_nano,
                    point.attributes.clone(),
                    histogram_point_json(point),
                );
            }
        }
        metric::Data::ExponentialHistogram(histogram) => {
            for point in &histogram.data_points {
                push_metric_point(
                    decoded,
                    metric,
                    context,
                    received_at,
                    "exponentialHistogram",
                    Some(json!({
                        "aggregationTemporality": histogram.aggregation_temporality
                    })),
                    point.start_time_unix_nano,
                    point.time_unix_nano,
                    point.attributes.clone(),
                    exponential_histogram_point_json(point),
                );
            }
        }
        metric::Data::Summary(summary) => {
            for point in &summary.data_points {
                push_metric_point(
                    decoded,
                    metric,
                    context,
                    received_at,
                    "summary",
                    None,
                    point.start_time_unix_nano,
                    point.time_unix_nano,
                    point.attributes.clone(),
                    summary_point_json(point),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_metric_point(
    decoded: &mut DecodedBatch,
    metric: &Metric,
    context: &MetricContext,
    received_at: DateTime<Utc>,
    kind: &str,
    aggregation: Option<Value>,
    start_time_unix_nano: u64,
    time_unix_nano: u64,
    attributes: Vec<KeyValue>,
    data_point: Value,
) {
    let ordinal = next_ordinal(&decoded.candidates);
    let (observed_at_unix_nano, observed_time_source) = native_time(time_unix_nano, received_at);
    if observed_time_source == "inbox.received_at" {
        decoded
            .diagnostics
            .push(timestamp_fallback_diagnostic(ordinal));
    }
    decoded.candidates.push(Candidate {
        ordinal,
        signal: EvidenceSignal::Metric,
        instrumentation_scope: scope_name(context.scope.as_ref()),
        signal_name: metric.name.clone(),
        metric_kind: Some(kind.to_owned()),
        metric_unit: Some(metric.unit.clone()),
        observed_at_unix_nano,
        observed_time_source,
        resource: context.resource.clone(),
        resource_schema_url: context.resource_schema_url.clone(),
        scope: context.scope.clone(),
        scope_schema_url: context.scope_schema_url.clone(),
        signal_attributes: attributes.clone(),
        parent_span_name: None,
        parent_span_attributes: Vec::new(),
        payload: json!({
            "aggregation": aggregation,
            "dataPoint": data_point,
            "description": metric.description,
            "metadata": attributes_json(&metric.metadata),
            "name": metric.name,
            "unit": metric.unit
        }),
        identity: json!({
            "aggregation": aggregation,
            "attributes": attributes_json(&attributes),
            "endTimeUnixNano": time_unix_nano.to_string(),
            "startTimeUnixNano": start_time_unix_nano.to_string()
        }),
        identity_is_content_derived: false,
    });
}

fn log_time(record: &LogRecord, received_at: DateTime<Utc>) -> (u64, &'static str) {
    if record.time_unix_nano != 0 {
        (record.time_unix_nano, "log.time_unix_nano")
    } else if record.observed_time_unix_nano != 0 {
        (
            record.observed_time_unix_nano,
            "log.observed_time_unix_nano",
        )
    } else {
        (received_at_unix_nano(received_at), "inbox.received_at")
    }
}

fn native_time(value: u64, received_at: DateTime<Utc>) -> (u64, &'static str) {
    if value == 0 {
        (received_at_unix_nano(received_at), "inbox.received_at")
    } else {
        (value, "signal.time_unix_nano")
    }
}

fn received_at_unix_nano(received_at: DateTime<Utc>) -> u64 {
    u64::try_from(
        received_at
            .timestamp_nanos_opt()
            .unwrap_or_else(|| received_at.timestamp_millis() * 1_000_000),
    )
    .unwrap_or(0)
}

fn timestamp_fallback_diagnostic(ordinal: u32) -> DecoderDiagnostic {
    DecoderDiagnostic {
        candidate_ordinal: Some(ordinal),
        code: "observation.timestamp_fallback".to_owned(),
        message: "The signal has no native timestamp; inbox receipt time was used.".to_owned(),
        detail: json!({ "fallback": "inbox.received_at" }),
    }
}

fn next_ordinal(candidates: &[Candidate]) -> u32 {
    u32::try_from(candidates.len())
        .expect("an OTLP batch cannot contain more than u32::MAX candidates")
}

fn scope_name(scope: Option<&InstrumentationScope>) -> String {
    scope.map_or_else(String::new, |scope| scope.name.clone())
}

fn log_record_json(record: &LogRecord) -> Value {
    json!({
        "attributes": attributes_json(&record.attributes),
        "body": record.body.as_ref().map(any_value_json),
        "droppedAttributesCount": record.dropped_attributes_count,
        "eventName": record.event_name,
        "flags": record.flags,
        "observedTimeUnixNano": record.observed_time_unix_nano.to_string(),
        "severityNumber": record.severity_number,
        "severityText": record.severity_text,
        "spanId": encode_hex(&record.span_id),
        "timeUnixNano": record.time_unix_nano.to_string(),
        "traceId": encode_hex(&record.trace_id)
    })
}

fn span_json(value: &Span) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "droppedAttributesCount": value.dropped_attributes_count,
        "droppedEventsCount": value.dropped_events_count,
        "droppedLinksCount": value.dropped_links_count,
        "endTimeUnixNano": value.end_time_unix_nano.to_string(),
        "events": value.events.iter().map(span_event_json).collect::<Vec<_>>(),
        "flags": value.flags,
        "kind": value.kind,
        "links": value.links.iter().map(span_link_json).collect::<Vec<_>>(),
        "name": value.name,
        "parentSpanId": encode_hex(&value.parent_span_id),
        "spanId": encode_hex(&value.span_id),
        "startTimeUnixNano": value.start_time_unix_nano.to_string(),
        "status": value.status.as_ref().map(|status| json!({
            "code": status.code,
            "message": status.message
        })),
        "traceId": encode_hex(&value.trace_id),
        "traceState": value.trace_state
    })
}

fn span_event_json(value: &span::Event) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "droppedAttributesCount": value.dropped_attributes_count,
        "name": value.name,
        "timeUnixNano": value.time_unix_nano.to_string()
    })
}

fn span_link_json(value: &span::Link) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "droppedAttributesCount": value.dropped_attributes_count,
        "flags": value.flags,
        "spanId": encode_hex(&value.span_id),
        "traceId": encode_hex(&value.trace_id),
        "traceState": value.trace_state
    })
}

fn number_point_json(value: &NumberDataPoint) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "exemplars": value.exemplars.iter().map(exemplar_json).collect::<Vec<_>>(),
        "flags": value.flags,
        "startTimeUnixNano": value.start_time_unix_nano.to_string(),
        "timeUnixNano": value.time_unix_nano.to_string(),
        "value": match value.value {
            None => Value::Null,
            Some(number_data_point::Value::AsDouble(value)) => double_json(value),
            Some(number_data_point::Value::AsInt(value)) => {
                json!({ "int64": value.to_string() })
            }
        }
    })
}

fn histogram_point_json(value: &HistogramDataPoint) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "bucketCounts": value.bucket_counts.iter().map(u64::to_string).collect::<Vec<_>>(),
        "count": value.count.to_string(),
        "exemplars": value.exemplars.iter().map(exemplar_json).collect::<Vec<_>>(),
        "explicitBounds": value.explicit_bounds.iter().copied().map(double_json).collect::<Vec<_>>(),
        "flags": value.flags,
        "max": value.max.map(double_json),
        "min": value.min.map(double_json),
        "startTimeUnixNano": value.start_time_unix_nano.to_string(),
        "sum": value.sum.map(double_json),
        "timeUnixNano": value.time_unix_nano.to_string()
    })
}

fn exponential_histogram_point_json(value: &ExponentialHistogramDataPoint) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "count": value.count.to_string(),
        "exemplars": value.exemplars.iter().map(exemplar_json).collect::<Vec<_>>(),
        "flags": value.flags,
        "max": value.max.map(double_json),
        "min": value.min.map(double_json),
        "negative": value.negative.as_ref().map(exponential_buckets_json),
        "positive": value.positive.as_ref().map(exponential_buckets_json),
        "scale": value.scale,
        "startTimeUnixNano": value.start_time_unix_nano.to_string(),
        "sum": value.sum.map(double_json),
        "timeUnixNano": value.time_unix_nano.to_string(),
        "zeroCount": value.zero_count.to_string(),
        "zeroThreshold": double_json(value.zero_threshold)
    })
}

fn exponential_buckets_json(value: &exponential_histogram_data_point::Buckets) -> Value {
    json!({
        "bucketCounts": value.bucket_counts.iter().map(u64::to_string).collect::<Vec<_>>(),
        "offset": value.offset
    })
}

fn summary_point_json(value: &SummaryDataPoint) -> Value {
    json!({
        "attributes": attributes_json(&value.attributes),
        "count": value.count.to_string(),
        "flags": value.flags,
        "quantileValues": value.quantile_values.iter().map(|quantile| json!({
            "quantile": double_json(quantile.quantile),
            "value": double_json(quantile.value)
        })).collect::<Vec<_>>(),
        "startTimeUnixNano": value.start_time_unix_nano.to_string(),
        "sum": double_json(value.sum),
        "timeUnixNano": value.time_unix_nano.to_string()
    })
}

fn exemplar_json(value: &Exemplar) -> Value {
    json!({
        "filteredAttributes": attributes_json(&value.filtered_attributes),
        "spanId": encode_hex(&value.span_id),
        "timeUnixNano": value.time_unix_nano.to_string(),
        "traceId": encode_hex(&value.trace_id),
        "value": match value.value {
            None => Value::Null,
            Some(exemplar::Value::AsDouble(value)) => double_json(value),
            Some(exemplar::Value::AsInt(value)) => json!({ "int64": value.to_string() })
        }
    })
}

fn double_json(value: f64) -> Value {
    json!({ "doubleBits": format!("{:016x}", value.to_bits()) })
}
