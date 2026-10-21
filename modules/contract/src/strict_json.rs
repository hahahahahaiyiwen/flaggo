use crate::ContractError;
use serde::Deserializer;
use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::cell::Cell;
use std::collections::HashSet;
use std::fmt;

pub const MAXIMUM_DOCUMENT_BYTES: usize = 262_144;
pub const MAXIMUM_DEPTH: usize = 16;
pub const MAXIMUM_STRING_BYTES: usize = 16_384;

pub fn deserialize<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ContractError> {
    validate(bytes)?;
    Ok(serde_json::from_slice(bytes)?)
}

pub fn validate(bytes: &[u8]) -> Result<(), ContractError> {
    if bytes.len() > MAXIMUM_DOCUMENT_BYTES {
        return Err(error("JSON exceeds the document byte limit").into());
    }
    let raw: &RawValue = serde_json::from_slice(bytes)?;
    let invalid_unicode = Cell::new(false);
    validate_value(raw, 0, &invalid_unicode).map_err(|error| {
        if invalid_unicode.get() {
            ContractError::InvalidUnicode(error)
        } else {
            ContractError::Json(error)
        }
    })
}

fn error(message: impl fmt::Display) -> serde_json::Error {
    serde::de::Error::custom(message)
}

fn validate_value(
    raw: &RawValue,
    depth: usize,
    invalid_unicode: &Cell<bool>,
) -> Result<(), serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(raw.get());
    match raw.get().as_bytes().first() {
        Some(b'{') => {
            if depth >= MAXIMUM_DEPTH {
                return Err(error("JSON exceeds the maximum depth"));
            }
            deserializer.deserialize_map(Tokens {
                depth,
                invalid_unicode,
            })
        }
        Some(b'[') => {
            if depth >= MAXIMUM_DEPTH {
                return Err(error("JSON exceeds the maximum depth"));
            }
            deserializer.deserialize_seq(Tokens {
                depth,
                invalid_unicode,
            })
        }
        Some(b'"') => {
            // RawValue already checked JSON syntax; failure here is Unicode decoding.
            let value: String =
                serde_json::from_str(raw.get()).inspect_err(|_| invalid_unicode.set(true))?;
            check_string(&value)
        }
        _ => Ok(()),
    }
}

fn check_string(value: &str) -> Result<(), serde_json::Error> {
    if value.len() > MAXIMUM_STRING_BYTES {
        return Err(error("JSON string/property exceeds the string byte limit"));
    }
    Ok(())
}

struct Tokens<'a> {
    depth: usize,
    invalid_unicode: &'a Cell<bool>,
}

impl<'de> Visitor<'de> for Tokens<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON object or array without duplicate members")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut names = HashSet::new();
        while let Some(name) = map
            .next_key::<String>()
            .inspect_err(|_| self.invalid_unicode.set(true))?
        {
            check_string(&name).map_err(serde::de::Error::custom)?;
            if !names.insert(name) {
                return Err(serde::de::Error::custom("duplicate JSON property"));
            }
            let value: &RawValue = map.next_value()?;
            validate_value(value, self.depth + 1, self.invalid_unicode)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(value) = seq.next_element::<&RawValue>()? {
            validate_value(value, self.depth + 1, self.invalid_unicode)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(())
    }
}
