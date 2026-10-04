use std::{
    collections::HashSet,
    error::Error,
    fmt::{self, Display, Formatter},
};

use flaggo_raw_otlp_inbox::{OtlpSignal, OtlpWireEncoding};
use opentelemetry_proto::tonic::collector::{
    logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
    trace::v1::ExportTraceServiceRequest,
};
use prost::Message;
use serde::de::{self, Deserialize, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

pub enum OtlpExportRequest {
    Logs(ExportLogsServiceRequest),
    Metrics(ExportMetricsServiceRequest),
    Traces(ExportTraceServiceRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtlpDecodeError {
    message: String,
}

impl OtlpDecodeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for OtlpDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OtlpDecodeError {}

pub fn decode_export_request(
    signal: OtlpSignal,
    encoding: OtlpWireEncoding,
    payload: &[u8],
) -> Result<OtlpExportRequest, OtlpDecodeError> {
    match (signal, encoding) {
        (OtlpSignal::Logs, OtlpWireEncoding::Protobuf) => ExportLogsServiceRequest::decode(payload)
            .map(OtlpExportRequest::Logs)
            .map_err(decode_error),
        (OtlpSignal::Metrics, OtlpWireEncoding::Protobuf) => {
            ExportMetricsServiceRequest::decode(payload)
                .map(OtlpExportRequest::Metrics)
                .map_err(decode_error)
        }
        (OtlpSignal::Traces, OtlpWireEncoding::Protobuf) => {
            ExportTraceServiceRequest::decode(payload)
                .map(OtlpExportRequest::Traces)
                .map_err(decode_error)
        }
        (signal, OtlpWireEncoding::ProtobufJson) => decode_json(signal, payload),
    }
}

fn decode_json(signal: OtlpSignal, payload: &[u8]) -> Result<OtlpExportRequest, OtlpDecodeError> {
    let normalized = serde_json::from_slice::<NormalizedProtoJson>(payload)
        .map_err(decode_error)?
        .0;
    validate_envelope(signal, &normalized)?;
    match signal {
        OtlpSignal::Logs => serde_json::from_value(normalized)
            .map(OtlpExportRequest::Logs)
            .map_err(decode_error),
        OtlpSignal::Metrics => serde_json::from_value(normalized)
            .map(OtlpExportRequest::Metrics)
            .map_err(decode_error),
        OtlpSignal::Traces => serde_json::from_value(normalized)
            .map(OtlpExportRequest::Traces)
            .map_err(decode_error),
    }
}

fn validate_envelope(signal: OtlpSignal, value: &Value) -> Result<(), OtlpDecodeError> {
    let object = value
        .as_object()
        .ok_or_else(|| OtlpDecodeError::new("OTLP export request must be a JSON object"))?;
    let expected = match signal {
        OtlpSignal::Logs => "resourceLogs",
        OtlpSignal::Metrics => "resourceMetrics",
        OtlpSignal::Traces => "resourceSpans",
    };
    if object.keys().any(|key| key != expected) {
        return Err(OtlpDecodeError::new(format!(
            "OTLP export request contains a field other than '{expected}'"
        )));
    }
    Ok(())
}

fn decode_error(error: impl Display) -> OtlpDecodeError {
    OtlpDecodeError::new(error.to_string())
}

struct NormalizedProtoJson(Value);

impl<'de> Deserialize<'de> for NormalizedProtoJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(NormalizedProtoJsonVisitor)
    }
}

struct NormalizedProtoJsonVisitor;

impl<'de> Visitor<'de> for NormalizedProtoJsonVisitor {
    type Value = NormalizedProtoJson;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(NormalizedProtoJson)
            .ok_or_else(|| E::custom("JSON number must be finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(NormalizedProtoJson(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element::<NormalizedProtoJson>()? {
            values.push(value.0);
        }
        Ok(NormalizedProtoJson(Value::Array(values)))
    }

    fn visit_map<A>(self, mut entries: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        let mut keys = HashSet::with_capacity(entries.size_hint().unwrap_or(0));
        while let Some(raw_key) = entries.next_key::<String>()? {
            let key = protobuf_json_name(&raw_key);
            if !keys.insert(key.clone()) {
                return Err(A::Error::custom(format!(
                    "duplicate protobuf JSON field '{key}'"
                )));
            }
            let value = entries.next_value::<NormalizedProtoJson>()?;
            object.insert(key, value.0);
        }
        reject_ambiguous_oneofs::<A::Error>(&object)?;
        Ok(NormalizedProtoJson(Value::Object(object)))
    }
}

fn protobuf_json_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut uppercase_next = false;
    for character in name.chars() {
        if character == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            result.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(character);
        }
    }
    result
}

fn reject_ambiguous_oneofs<E>(object: &Map<String, Value>) -> Result<(), E>
where
    E: de::Error,
{
    const ONEOF_GROUPS: &[&[&str]] = &[
        &[
            "stringValue",
            "boolValue",
            "intValue",
            "doubleValue",
            "arrayValue",
            "kvlistValue",
            "bytesValue",
        ],
        &[
            "gauge",
            "sum",
            "histogram",
            "exponentialHistogram",
            "summary",
        ],
        &["asDouble", "asInt"],
    ];
    for group in ONEOF_GROUPS {
        let present = group
            .iter()
            .filter(|field| object.contains_key(**field))
            .copied()
            .collect::<Vec<_>>();
        if present.len() > 1 {
            return Err(E::custom(format!(
                "multiple protobuf oneof fields are present: {}",
                present.join(", ")
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_and_original_field_names_for_every_signal() {
        let cases = [
            (
                OtlpSignal::Logs,
                br#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"timeUnixNano":"1","body":{"stringValue":"hello"}}]}]}]}"#
                    .as_slice(),
                br#"{"resource_logs":[{"scope_logs":[{"log_records":[{"time_unix_nano":"1","body":{"string_value":"hello"}}]}]}]}"#
                    .as_slice(),
            ),
            (
                OtlpSignal::Metrics,
                br#"{"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"load","gauge":{"dataPoints":[{"timeUnixNano":"1","asInt":"2"}]}}]}]}]}"#
                    .as_slice(),
                br#"{"resource_metrics":[{"scope_metrics":[{"metrics":[{"name":"load","gauge":{"data_points":[{"time_unix_nano":"1","as_int":"2"}]}}]}]}]}"#
                    .as_slice(),
            ),
            (
                OtlpSignal::Traces,
                br#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"name":"work","startTimeUnixNano":"1","endTimeUnixNano":"2"}]}]}]}"#
                    .as_slice(),
                br#"{"resource_spans":[{"scope_spans":[{"spans":[{"name":"work","start_time_unix_nano":"1","end_time_unix_nano":"2"}]}]}]}"#
                    .as_slice(),
            ),
        ];

        for (signal, canonical, original) in cases {
            assert_eq!(
                encoded(decode_json(signal, canonical).expect("canonical field names")),
                encoded(decode_json(signal, original).expect("original field names"))
            );
        }
    }

    #[test]
    fn rejects_exact_and_alias_duplicate_fields_at_any_depth() {
        for payload in [
            br#"{"resourceLogs":[],"resourceLogs":[]}"#.as_slice(),
            br#"{"resourceLogs":[],"resource_logs":[]}"#.as_slice(),
            br#"{"resourceLogs":[{"scopeLogs":[],"scope_logs":[]}]}"#.as_slice(),
        ] {
            let error = decode_json(OtlpSignal::Logs, payload)
                .err()
                .expect("duplicate field");
            assert!(error.to_string().contains("duplicate protobuf JSON field"));
        }
    }

    #[test]
    fn rejects_multiple_oneof_members() {
        let payload = br#"{"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"load","gauge":{"dataPoints":[{"asInt":"1","asDouble":2.0}]}}]}]}]}"#;

        let error = decode_json(OtlpSignal::Metrics, payload)
            .err()
            .expect("ambiguous metric point value");

        assert!(error.to_string().contains("multiple protobuf oneof fields"));
    }

    #[test]
    fn rejects_unknown_envelope_fields_but_ignores_unknown_nested_fields() {
        let error = decode_json(OtlpSignal::Logs, br#"{"resourceLogs":[],"futureField":[]}"#)
            .err()
            .expect("unknown envelope field");
        assert!(
            error
                .to_string()
                .contains("field other than 'resourceLogs'")
        );

        let decoded = decode_json(
            OtlpSignal::Logs,
            br#"{"resourceLogs":[{"future_field":true}]}"#,
        )
        .expect("unknown nested field");
        let OtlpExportRequest::Logs(request) = decoded else {
            panic!("logs request");
        };
        assert_eq!(request.resource_logs.len(), 1);
    }

    fn encoded(request: OtlpExportRequest) -> Vec<u8> {
        match request {
            OtlpExportRequest::Logs(request) => request.encode_to_vec(),
            OtlpExportRequest::Metrics(request) => request.encode_to_vec(),
            OtlpExportRequest::Traces(request) => request.encode_to_vec(),
        }
    }
}
