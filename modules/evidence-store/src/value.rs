use serde::{Deserialize, Serialize};

/// Lossless cells for `flaggo.evidence.value/v1`; not the legacy query envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum EvidenceValue {
    Null {},
    Int64 {
        #[serde(with = "decimal")]
        value: i64,
    },
    Float64 {
        #[serde(with = "bits")]
        bits: u64,
    },
    Text {
        value: String,
    },
    Boolean {
        value: bool,
    },
    Json {
        #[serde(with = "json_text")]
        value: String,
    },
}

mod decimal {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let text = String::deserialize(deserializer)?;
        let value: i64 = text.parse().map_err(D::Error::custom)?;
        if value.to_string() != text {
            return Err(D::Error::custom("int64 requires canonical decimal text"));
        }
        Ok(value)
    }
}

mod bits {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{value:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() != 16
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(D::Error::custom(
                "float64 bits require sixteen lowercase hexadecimal digits",
            ));
        }
        u64::from_str_radix(&text, 16).map_err(D::Error::custom)
    }
}

mod json_text {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _, ser::Error as _};

    // Validate syntax without parsing numbers into a potentially lossy numeric type.
    fn validate(text: &str) -> Result<(), serde_json::Error> {
        serde_json::from_str::<serde::de::IgnoredAny>(text).map(|_| ())
    }

    pub fn serialize<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
        validate(value).map_err(S::Error::custom)?;
        serializer.serialize_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        let text = String::deserialize(deserializer)?;
        validate(&text).map_err(D::Error::custom)?;
        Ok(text)
    }
}
