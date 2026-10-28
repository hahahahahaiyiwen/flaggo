use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use serde::{Deserialize, Deserializer, Serializer};

#[derive(Debug, thiserror::Error)]
#[error("Invalid round-trip timestamp: {0}")]
pub struct TimestampError(pub String);

fn valid(value: DateTime<FixedOffset>) -> Result<DateTime<FixedOffset>, TimestampError> {
    if !(1..=9999).contains(&value.year())
        || !(1..=9999).contains(&value.with_timezone(&Utc).year())
        || value.timestamp_subsec_nanos() >= 1_000_000_000
        || value.offset().local_minus_utc().abs() > 14 * 3600
        || value.offset().local_minus_utc() % 60 != 0
    {
        return Err(TimestampError("outside the DateTimeOffset range".into()));
    }
    Ok(value
        .with_nanosecond(value.timestamp_subsec_nanos() / 100 * 100)
        .expect("finite subsecond timestamp"))
}

pub fn parse_exact(value: &str) -> Result<DateTime<FixedOffset>, TimestampError> {
    let bytes = value.as_bytes();
    if !matches!(bytes.len(), 28 | 33)
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 | 30 => *byte == b':',
            19 => *byte == b'.',
            27 if bytes.len() == 28 => *byte == b'Z',
            27 => matches!(*byte, b'+' | b'-'),
            _ => byte.is_ascii_digit(),
        })
    {
        return Err(TimestampError(
            "expected yyyy-MM-ddTHH:mm:ss.fffffffzzz".into(),
        ));
    }
    let value =
        DateTime::parse_from_rfc3339(value).map_err(|error| TimestampError(error.to_string()))?;
    valid(value)
}

pub fn format_exact(value: DateTime<FixedOffset>) -> Result<String, TimestampError> {
    let value = valid(value)?.with_timezone(&Utc);
    Ok(format!(
        "{}.{:07}+00:00",
        value.format("%Y-%m-%dT%H:%M:%S"),
        value.timestamp_subsec_nanos() / 100
    ))
}

pub fn serialize<S: Serializer>(
    value: &DateTime<FixedOffset>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value = valid(*value).map_err(serde::ser::Error::custom)?;
    let fraction = format!("{:07}", value.timestamp_subsec_nanos() / 100);
    let fraction = fraction.trim_end_matches('0');
    serializer.serialize_str(&format!(
        "{}{}{}",
        value.format("%Y-%m-%dT%H:%M:%S"),
        if fraction.is_empty() {
            String::new()
        } else {
            format!(".{fraction}")
        },
        value.format("%:z")
    ))
}

pub fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<DateTime<FixedOffset>, D::Error> {
    let value = String::deserialize(deserializer)?;
    valid(DateTime::parse_from_rfc3339(&value).map_err(serde::de::Error::custom)?)
        .map_err(serde::de::Error::custom)
}
