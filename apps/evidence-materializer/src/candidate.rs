use opentelemetry_proto::tonic::{
    common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value},
    resource::v1::Resource,
};
use serde_json::{Value, json};
use thiserror::Error;

use flaggo_evidence_store::EvidenceSignal;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub ordinal: u32,
    pub signal: EvidenceSignal,
    pub instrumentation_scope: String,
    pub signal_name: String,
    pub metric_kind: Option<String>,
    pub metric_unit: Option<String>,
    pub observed_at_unix_nano: u64,
    pub observed_time_source: &'static str,
    pub resource: Option<Resource>,
    pub resource_schema_url: String,
    pub scope: Option<InstrumentationScope>,
    pub scope_schema_url: String,
    pub signal_attributes: Vec<KeyValue>,
    pub parent_span_name: Option<String>,
    pub parent_span_attributes: Vec<KeyValue>,
    pub payload: Value,
    pub identity: Value,
    pub identity_is_content_derived: bool,
}

impl Candidate {
    pub fn canonical_payload(&self) -> Value {
        json!({
            "resource": resource_json(self.resource.as_ref()),
            "resourceSchemaUrl": self.resource_schema_url,
            "scope": scope_json(self.scope.as_ref()),
            "scopeSchemaUrl": self.scope_schema_url,
            "signal": {
                "kind": self.signal.as_str(),
                "metricKind": self.metric_kind,
                "metricUnit": self.metric_unit,
                "name": self.signal_name,
                "payload": self.payload
            }
        })
    }

    pub fn canonical_identity(&self) -> Value {
        json!({
            "resource": resource_json(self.resource.as_ref()),
            "resourceSchemaUrl": self.resource_schema_url,
            "scope": scope_json(self.scope.as_ref()),
            "scopeSchemaUrl": self.scope_schema_url,
            "signal": {
                "identity": self.identity,
                "kind": self.signal.as_str(),
                "metricKind": self.metric_kind,
                "metricUnit": self.metric_unit,
                "name": self.signal_name
            }
        })
    }

    pub fn resource_attributes(&self) -> &[KeyValue] {
        self.resource
            .as_ref()
            .map_or(&[], |resource| resource.attributes.as_slice())
    }

    pub fn scope_attributes(&self) -> &[KeyValue] {
        self.scope
            .as_ref()
            .map_or(&[], |scope| scope.attributes.as_slice())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttributeLocation {
    Resource,
    Scope,
    Signal,
    ParentSpan,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum AttributeLookupError {
    #[error("attribute '{0}' is missing")]
    Missing(String),
    #[error("attribute '{0}' occurs more than once")]
    Ambiguous(String),
    #[error("attribute '{0}' has no AnyValue")]
    Empty(String),
}

pub(crate) fn lookup_attribute<'a>(
    attributes: &'a [KeyValue],
    key: &str,
) -> Result<&'a AnyValue, AttributeLookupError> {
    let mut matches = attributes.iter().filter(|attribute| attribute.key == key);
    let Some(attribute) = matches.next() else {
        return Err(AttributeLookupError::Missing(key.to_owned()));
    };
    if matches.next().is_some() {
        return Err(AttributeLookupError::Ambiguous(key.to_owned()));
    }
    attribute
        .value
        .as_ref()
        .ok_or_else(|| AttributeLookupError::Empty(key.to_owned()))
}

pub(crate) fn any_value_json(value: &AnyValue) -> Value {
    match &value.value {
        None => json!({ "type": "empty" }),
        Some(any_value::Value::StringValue(value)) => {
            json!({ "type": "string", "value": value })
        }
        Some(any_value::Value::BoolValue(value)) => {
            json!({ "type": "bool", "value": value })
        }
        Some(any_value::Value::IntValue(value)) => {
            json!({ "type": "int64", "value": value.to_string() })
        }
        Some(any_value::Value::DoubleValue(value)) => {
            json!({
                "bits": format!("{:016x}", value.to_bits()),
                "type": "double"
            })
        }
        Some(any_value::Value::ArrayValue(value)) => {
            json!({
                "type": "array",
                "value": value.values.iter().map(any_value_json).collect::<Vec<_>>()
            })
        }
        Some(any_value::Value::KvlistValue(value)) => {
            json!({
                "type": "keyValueList",
                "value": value.values.iter().map(key_value_json).collect::<Vec<_>>()
            })
        }
        Some(any_value::Value::BytesValue(value)) => {
            json!({
                "type": "bytes",
                "value": encode_hex(value)
            })
        }
        Some(any_value::Value::StringValueStrindex(value)) => {
            json!({
                "type": "stringTableIndex",
                "value": value.to_string()
            })
        }
    }
}

pub(crate) fn key_value_json(value: &KeyValue) -> Value {
    json!({
        "key": value.key,
        "keyStringTableIndex": value.key_strindex,
        "value": value.value.as_ref().map_or_else(
            || json!({ "type": "empty" }),
            any_value_json
        )
    })
}

pub(crate) fn attributes_json(attributes: &[KeyValue]) -> Value {
    Value::Array(attributes.iter().map(key_value_json).collect())
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String must succeed");
    }
    encoded
}

fn resource_json(resource: Option<&Resource>) -> Value {
    resource.map_or(Value::Null, |resource| {
        json!({
            "attributes": attributes_json(&resource.attributes),
            "droppedAttributesCount": resource.dropped_attributes_count,
            "entityRefs": resource.entity_refs.iter().map(|entity| json!({
                "descriptionKeys": entity.description_keys,
                "idKeys": entity.id_keys,
                "schemaUrl": entity.schema_url,
                "type": entity.r#type
            })).collect::<Vec<_>>()
        })
    })
}

fn scope_json(scope: Option<&InstrumentationScope>) -> Value {
    scope.map_or(Value::Null, |scope| {
        json!({
            "attributes": attributes_json(&scope.attributes),
            "droppedAttributesCount": scope.dropped_attributes_count,
            "name": scope.name,
            "version": scope.version
        })
    })
}

#[cfg(test)]
mod tests {
    use opentelemetry_proto::tonic::common::v1::{
        AnyValue, ArrayValue, KeyValue, KeyValueList, any_value,
    };
    use serde_json::json;

    use super::any_value_json;

    #[test]
    fn canonicalizes_every_recursive_any_value_variant_without_precision_loss() {
        let value = AnyValue {
            value: Some(any_value::Value::KvlistValue(KeyValueList {
                values: vec![
                    KeyValue {
                        key: "empty".to_owned(),
                        key_strindex: 0,
                        value: Some(AnyValue { value: None }),
                    },
                    KeyValue {
                        key: "array".to_owned(),
                        key_strindex: 0,
                        value: Some(AnyValue {
                            value: Some(any_value::Value::ArrayValue(ArrayValue {
                                values: vec![
                                    AnyValue {
                                        value: Some(any_value::Value::StringValue(
                                            "value".to_owned(),
                                        )),
                                    },
                                    AnyValue {
                                        value: Some(any_value::Value::BoolValue(true)),
                                    },
                                    AnyValue {
                                        value: Some(any_value::Value::IntValue(i64::MIN)),
                                    },
                                    AnyValue {
                                        value: Some(any_value::Value::DoubleValue(-0.0)),
                                    },
                                    AnyValue {
                                        value: Some(any_value::Value::BytesValue(vec![0x00, 0xff])),
                                    },
                                ],
                            })),
                        }),
                    },
                ],
            })),
        };

        assert_eq!(
            any_value_json(&value),
            json!({
                "type": "keyValueList",
                "value": [
                    {
                        "key": "empty",
                        "keyStringTableIndex": 0,
                        "value": { "type": "empty" }
                    },
                    {
                        "key": "array",
                        "keyStringTableIndex": 0,
                        "value": {
                            "type": "array",
                            "value": [
                                { "type": "string", "value": "value" },
                                { "type": "bool", "value": true },
                                { "type": "int64", "value": "-9223372036854775808" },
                                { "bits": "8000000000000000", "type": "double" },
                                { "type": "bytes", "value": "00ff" }
                            ]
                        }
                    }
                ]
            })
        );
    }
}
