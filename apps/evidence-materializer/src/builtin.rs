use std::collections::BTreeMap;

use opentelemetry_proto::tonic::common::v1::{AnyValue, any_value};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use flaggo_evidence_store::EvidenceSignal;

use crate::candidate::Candidate;

const SDK_SCOPE: &str = "@flaggo/sdk";
const EVENT_NAME: &str = "flaggo.decision.received";

pub(crate) struct BuiltInDecision {
    pub decision_id: String,
    pub contract_digest: String,
}

pub(crate) struct BuiltInDiagnostic {
    pub code: String,
    pub message: String,
    pub detail: Value,
}

pub(crate) enum BuiltInEvaluation {
    NotBuiltIn,
    Valid(BuiltInDecision),
    Invalid(BuiltInDiagnostic),
}

pub(crate) fn evaluate(candidate: &Candidate) -> BuiltInEvaluation {
    if candidate.signal != EvidenceSignal::Log {
        return BuiltInEvaluation::NotBuiltIn;
    }
    if candidate.instrumentation_scope != SDK_SCOPE {
        return BuiltInEvaluation::NotBuiltIn;
    }
    let has_protocol_marker = candidate
        .signal_attributes
        .iter()
        .any(|attribute| attribute.key == "flaggo.signal");
    if candidate.signal_name != EVENT_NAME && !has_protocol_marker {
        return BuiltInEvaluation::NotBuiltIn;
    }

    match validate(candidate) {
        Ok(decision) => BuiltInEvaluation::Valid(decision),
        Err(reason) => BuiltInEvaluation::Invalid(BuiltInDiagnostic {
            code: "protocol.decision_received_invalid".to_owned(),
            message: "A Flaggo decision-received log failed strict protocol validation.".to_owned(),
            detail: json!({ "reason": reason }),
        }),
    }
}

fn validate(candidate: &Candidate) -> Result<BuiltInDecision, String> {
    if candidate.signal_name != EVENT_NAME {
        return Err(format!("event name must be '{EVENT_NAME}'"));
    }
    let attributes = unique_attributes(candidate)?;
    require_string(&attributes, "flaggo.signal", |value| {
        value == "decision.received"
    })?;
    let decision_id = require_string(&attributes, "flaggo.decision.id", is_uuid_v4)?;
    require_string(&attributes, "flaggo.contract.name", is_decision_name)?;
    let contract_digest = require_string(&attributes, "flaggo.contract.digest", is_sha256_digest)?;
    require_string(&attributes, "flaggo.executable.digest", is_sha256_digest)?;
    let result_json = require_string(&attributes, "flaggo.result.json", |value| {
        !value.is_empty() && serde_json::from_str::<Value>(value).is_ok()
    })?;
    let result_hash = require_string(&attributes, "flaggo.result.hash", is_sha256_digest)?;
    if result_hash != sha256(result_json.as_bytes()) {
        return Err("flaggo.result.hash does not match flaggo.result.json".to_owned());
    }
    let evaluation_source = require_string(&attributes, "flaggo.evaluation.source", |value| {
        matches!(value, "default" | "rule")
    })?;
    let evaluation_rule = optional_string(&attributes, "flaggo.evaluation.rule")?;
    match (evaluation_source.as_str(), evaluation_rule) {
        ("default", None) => {}
        ("rule", Some(rule)) if is_member_name(&rule) => {}
        ("default", Some(_)) => {
            return Err("flaggo.evaluation.rule must be absent for default evaluation".to_owned());
        }
        ("rule", None) => {
            return Err("flaggo.evaluation.rule is required for rule evaluation".to_owned());
        }
        _ => return Err("flaggo.evaluation.rule is invalid".to_owned()),
    }
    if let Some(correlation_id) = optional_string(&attributes, "flaggo.request.correlation_id")?
        && (correlation_id.is_empty()
            || correlation_id.contains('\r')
            || correlation_id.contains('\n'))
    {
        return Err("flaggo.request.correlation_id is invalid".to_owned());
    }

    for (key, value) in &attributes {
        if is_known_attribute(key) {
            continue;
        }
        let Some(name) = key.strip_prefix("flaggo.correlation.") else {
            return Err(format!("unexpected protocol attribute '{key}'"));
        };
        if !is_correlation_name(name) || !is_correlation_value(value) {
            return Err(format!("invalid correlation attribute '{key}'"));
        }
    }

    Ok(BuiltInDecision {
        decision_id,
        contract_digest,
    })
}

fn unique_attributes(candidate: &Candidate) -> Result<BTreeMap<&str, &AnyValue>, String> {
    let mut attributes = BTreeMap::new();
    for attribute in &candidate.signal_attributes {
        if attribute.key_strindex != 0 {
            return Err("string-table attribute keys are not valid for logs".to_owned());
        }
        let value = attribute
            .value
            .as_ref()
            .ok_or_else(|| format!("attribute '{}' has no value", attribute.key))?;
        if attributes.insert(attribute.key.as_str(), value).is_some() {
            return Err(format!(
                "attribute '{}' occurs more than once",
                attribute.key
            ));
        }
    }
    Ok(attributes)
}

fn require_string(
    attributes: &BTreeMap<&str, &AnyValue>,
    key: &str,
    predicate: impl FnOnce(&str) -> bool,
) -> Result<String, String> {
    let value = optional_string(attributes, key)?
        .ok_or_else(|| format!("required attribute '{key}' is missing"))?;
    if !predicate(&value) {
        return Err(format!("attribute '{key}' is invalid"));
    }
    Ok(value)
}

fn optional_string(
    attributes: &BTreeMap<&str, &AnyValue>,
    key: &str,
) -> Result<Option<String>, String> {
    match attributes.get(key) {
        None => Ok(None),
        Some(AnyValue {
            value: Some(any_value::Value::StringValue(value)),
        }) => Ok(Some(value.clone())),
        Some(_) => Err(format!("attribute '{key}' must be a string")),
    }
}

fn is_known_attribute(value: &str) -> bool {
    matches!(
        value,
        "flaggo.signal"
            | "flaggo.decision.id"
            | "flaggo.contract.name"
            | "flaggo.contract.digest"
            | "flaggo.executable.digest"
            | "flaggo.result.json"
            | "flaggo.result.hash"
            | "flaggo.evaluation.source"
            | "flaggo.evaluation.rule"
            | "flaggo.request.correlation_id"
    )
}

fn is_correlation_value(value: &AnyValue) -> bool {
    match value.value {
        Some(any_value::Value::StringValue(_))
        | Some(any_value::Value::BoolValue(_))
        | Some(any_value::Value::IntValue(_)) => true,
        Some(any_value::Value::DoubleValue(value)) => value.is_finite(),
        _ => false,
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_uuid_v4(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
        })
}

fn is_decision_name(value: &str) -> bool {
    is_bounded_name(value, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
    })
}

fn is_member_name(value: &str) -> bool {
    is_decision_name(value)
}

fn is_correlation_name(value: &str) -> bool {
    is_bounded_name(value, |byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_bounded_name(value: &str, valid: impl Fn(u8) -> bool) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.bytes().all(valid)
}

fn sha256(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use opentelemetry_proto::tonic::common::v1::KeyValue;
    use serde_json::Value;

    use super::*;

    #[test]
    fn non_log_signals_cannot_become_valid_protocol_events() {
        for signal in [
            EvidenceSignal::Metric,
            EvidenceSignal::Span,
            EvidenceSignal::SpanEvent,
        ] {
            let candidate = candidate(signal, EVENT_NAME, valid_protocol_attributes());
            assert!(matches!(
                evaluate(&candidate),
                BuiltInEvaluation::NotBuiltIn
            ));
        }
    }

    #[test]
    fn non_log_protocol_lookalikes_do_not_produce_protocol_diagnostics() {
        for signal in [
            EvidenceSignal::Metric,
            EvidenceSignal::Span,
            EvidenceSignal::SpanEvent,
        ] {
            let candidate = candidate(
                signal,
                "application.signal",
                vec![string_attribute("flaggo.signal", "decision.received")],
            );
            assert!(matches!(
                evaluate(&candidate),
                BuiltInEvaluation::NotBuiltIn
            ));
        }
    }

    fn candidate(
        signal: EvidenceSignal,
        signal_name: &str,
        signal_attributes: Vec<KeyValue>,
    ) -> Candidate {
        Candidate {
            ordinal: 0,
            signal,
            instrumentation_scope: SDK_SCOPE.to_owned(),
            signal_name: signal_name.to_owned(),
            metric_kind: None,
            metric_unit: None,
            observed_at_unix_nano: 1,
            observed_time_source: "signal.time_unix_nano",
            resource: None,
            resource_schema_url: String::new(),
            scope: None,
            scope_schema_url: String::new(),
            signal_attributes,
            parent_span_name: None,
            payload: Value::Null,
            identity: Value::Null,
            identity_is_content_derived: false,
        }
    }

    fn valid_protocol_attributes() -> Vec<KeyValue> {
        vec![
            string_attribute("flaggo.signal", "decision.received"),
            string_attribute("flaggo.decision.id", "00000000-0000-4000-8000-000000000000"),
            string_attribute("flaggo.contract.name", "parallelism"),
            string_attribute(
                "flaggo.contract.digest",
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            ),
            string_attribute(
                "flaggo.executable.digest",
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            ),
            string_attribute("flaggo.result.json", "4"),
            string_attribute("flaggo.result.hash", &sha256(b"4")),
            string_attribute("flaggo.evaluation.source", "default"),
        ]
    }

    fn string_attribute(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.to_owned(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(value.to_owned())),
            }),
            key_strindex: 0,
        }
    }
}
