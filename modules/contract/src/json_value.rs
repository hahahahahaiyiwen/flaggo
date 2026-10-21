use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

// Keep literal member order for validation issues without changing serde_json's
// workspace-wide map representation (and other services' persisted bytes).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<JsonValue>),
    Object(IndexMap<String, JsonValue>),
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Self::from_raw(&raw, 0).map_err(serde::de::Error::custom)
    }
}

impl JsonValue {
    fn from_raw(raw: &RawValue, depth: usize) -> Result<Self, serde_json::Error> {
        let text = raw.get();
        match text.as_bytes().first() {
            // Re-parsing raw children must not reset Serde's default recursion budget.
            Some(b'[' | b'{') if depth >= 127 => {
                Err(serde::de::Error::custom("recursion limit exceeded"))
            }
            Some(b'n') => serde_json::from_str::<()>(text).map(|()| Self::Null),
            Some(b't' | b'f') => serde_json::from_str(text).map(Self::Bool),
            Some(b'"') => serde_json::from_str(text).map(Self::String),
            Some(b'[') => {
                let values: Vec<&RawValue> = serde_json::from_str(text)?;
                values
                    .into_iter()
                    .map(|value| Self::from_raw(value, depth + 1))
                    .collect::<Result<_, _>>()
                    .map(Self::Array)
            }
            Some(b'{') => {
                let values: IndexMap<String, &RawValue> = serde_json::from_str(text)?;
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, Self::from_raw(value, depth + 1)?)))
                    .collect::<Result<_, _>>()
                    .map(Self::Object)
            }
            // Serde otherwise stores lexical -0 as a float, losing its int64 syntax.
            _ if text == "-0" => Ok(Self::Number(0.into())),
            _ => serde_json::from_str(text).map(Self::Number),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Self::Number(value) = self {
            value.as_i64()
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Number(value) = self {
            value.as_f64()
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&[Self]> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&IndexMap<String, Self>> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(value) => Self::Bool(value),
            serde_json::Value::Number(value) => Self::Number(value),
            serde_json::Value::String(value) => Self::String(value),
            serde_json::Value::Array(values) => {
                Self::Array(values.into_iter().map(Self::from).collect())
            }
            serde_json::Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from(value)))
                    .collect(),
            ),
        }
    }
}
