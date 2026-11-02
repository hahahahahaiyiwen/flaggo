use crate::timestamp;
use base64::{
    Engine,
    alphabet::STANDARD,
    engine::{GeneralPurpose, GeneralPurposeConfig, general_purpose::URL_SAFE_NO_PAD},
};
use chrono::{DateTime, FixedOffset};
use flaggo_contract::digests::is_sha256_digest;

#[derive(Debug, thiserror::Error)]
#[error("Invalid storage cursor.")]
pub struct InvalidCursor;

pub fn encode(
    time: DateTime<FixedOffset>,
    digest: &str,
) -> Result<String, timestamp::TimestampError> {
    Ok(URL_SAFE_NO_PAD.encode(format!("{}\n{digest}", timestamp::format_exact(time)?)))
}

pub fn decode(cursor: &str) -> Result<(String, String), InvalidCursor> {
    let mut padded = cursor.replace('-', "+").replace('_', "/");
    padded.extend(std::iter::repeat_n('=', (4 - padded.len() % 4) % 4));
    let filtered: String = padded
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '\t' | '\r' | '\n'))
        .collect();
    let bytes = GeneralPurpose::new(
        &STANDARD,
        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    )
    .decode(filtered)
    .map_err(|_| InvalidCursor)?;
    let payload = String::from_utf8_lossy(&bytes);
    let (time, digest) = payload.split_once('\n').ok_or(InvalidCursor)?;
    if !is_sha256_digest(digest) {
        return Err(InvalidCursor);
    }
    let time = timestamp::parse_exact(time).map_err(|_| InvalidCursor)?;
    Ok((
        timestamp::format_exact(time).map_err(|_| InvalidCursor)?,
        digest.into(),
    ))
}
