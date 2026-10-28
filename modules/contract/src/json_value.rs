use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

#[derive(Clone, Debug)]
pub struct JsonNumber {
    raw: Box<RawValue>,
    finite: Option<serde_json::Number>,
    double: f64,
}

impl JsonNumber {
    fn from_raw(raw: &RawValue) -> Result<Self, serde_json::Error> {
        let text = raw.get();
        if !matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
            return Err(serde::de::Error::custom("expected a JSON number"));
        }
        let double = text.parse::<f64>().map_err(serde::de::Error::custom)?;
        let finite = if text == "-0" {
            Some(0.into())
        } else if double.is_finite() {
            Some(serde_json::from_str(text)?)
        } else {
            None
        };
        Ok(Self {
            raw: raw.to_owned(),
            finite,
            double,
        })
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.finite.as_ref().and_then(serde_json::Number::as_i64)
    }

    pub fn as_f64(&self) -> Option<f64> {
        Some(self.double)
    }
}

impl PartialEq for JsonNumber {
    fn eq(&self, other: &Self) -> bool {
        match (&self.finite, &other.finite) {
            (Some(left), Some(right)) => left == right,
            (None, None) => self.raw.get() == other.raw.get(),
            _ => false,
        }
    }
}

impl Serialize for JsonNumber {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Self::from_raw(&raw).map_err(serde::de::Error::custom)
    }
}

impl From<serde_json::Number> for JsonNumber {
    fn from(value: serde_json::Number) -> Self {
        Self {
            double: value
                .as_f64()
                .expect("serde_json numbers have a double representation"),
            raw: RawValue::from_string(value.to_string()).expect("serde_json emits valid numbers"),
            finite: Some(value),
        }
    }
}

// Keep literal member order for validation issues without changing serde_json's
// workspace-wide map representation (and other services' persisted bytes).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
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
            _ => JsonNumber::from_raw(raw).map(Self::Number),
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
            serde_json::Value::Number(value) => Self::Number(value.into()),
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
