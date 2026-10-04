use std::{
    error::Error,
    num::{NonZeroU16, NonZeroU64},
    str::FromStr,
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

mod sqlite;

pub use sqlite::SqliteRawOtlpInbox;

pub const DEFAULT_MAX_RETAINED_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_HARD_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
pub const DEFAULT_OTLP_PROFILE_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OtlpSignal {
    Logs,
    Metrics,
    Traces,
}

impl OtlpSignal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Metrics => "metrics",
            Self::Traces => "traces",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "logs" => Some(Self::Logs),
            "metrics" => Some(Self::Metrics),
            "traces" => Some(Self::Traces),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OtlpWireEncoding {
    Protobuf,
    ProtobufJson,
}

impl OtlpWireEncoding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Protobuf => "protobuf",
            Self::ProtobufJson => "protobuf-json",
        }
    }

    #[must_use]
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Protobuf => "application/x-protobuf",
            Self::ProtobufJson => "application/json",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "protobuf" => Some(Self::Protobuf),
            "protobuf-json" => Some(Self::ProtobufJson),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OtlpTransportCompression {
    Identity,
    Gzip,
}

impl OtlpTransportCompression {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "identity" => Some(Self::Identity),
            "gzip" => Some(Self::Gzip),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OtlpProfileVersion(String);

impl OtlpProfileVersion {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for OtlpProfileVersion {
    type Err = OtlpProfileVersionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || value.len() > 64
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(OtlpProfileVersionError);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("OTLP profile version must be 1-64 non-control characters without surrounding whitespace")]
pub struct OtlpProfileVersionError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRawOtlpBatch {
    pub signal: OtlpSignal,
    pub wire_encoding: OtlpWireEncoding,
    pub transport_compression: OtlpTransportCompression,
    pub otlp_profile_version: OtlpProfileVersion,
    pub payload: Vec<u8>,
}

impl NewRawOtlpBatch {
    #[must_use]
    pub fn new(
        signal: OtlpSignal,
        wire_encoding: OtlpWireEncoding,
        transport_compression: OtlpTransportCompression,
        otlp_profile_version: OtlpProfileVersion,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            signal,
            wire_encoding,
            transport_compression,
            otlp_profile_version,
            payload,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InboxBatchId(NonZeroU64);

impl InboxBatchId {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PayloadSha256([u8; 32]);

impl PayloadSha256 {
    #[must_use]
    pub const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawOtlpInboxBatch {
    pub inbox_batch_id: InboxBatchId,
    pub signal: OtlpSignal,
    pub wire_encoding: OtlpWireEncoding,
    pub media_type: String,
    pub transport_compression: OtlpTransportCompression,
    pub otlp_profile_version: OtlpProfileVersion,
    pub received_at: DateTime<Utc>,
    pub payload_length: u64,
    pub payload_sha256: PayloadSha256,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxAppendReceipt {
    pub inbox_batch_id: InboxBatchId,
    pub received_at: DateTime<Utc>,
    pub payload_length: u64,
    pub payload_sha256: PayloadSha256,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawOtlpInboxLimits {
    max_retained_payload_bytes: u64,
    hard_retention: Duration,
}

impl RawOtlpInboxLimits {
    pub fn new(
        max_retained_payload_bytes: u64,
        hard_retention: Duration,
    ) -> Result<Self, RawOtlpInboxLimitsError> {
        if max_retained_payload_bytes == 0 || max_retained_payload_bytes > i64::MAX.cast_unsigned()
        {
            return Err(RawOtlpInboxLimitsError::InvalidMaximumPayloadBytes);
        }
        if hard_retention.as_millis() == 0
            || hard_retention.as_millis() > i64::MAX.cast_unsigned().into()
        {
            return Err(RawOtlpInboxLimitsError::InvalidHardRetention);
        }
        Ok(Self {
            max_retained_payload_bytes,
            hard_retention,
        })
    }

    #[must_use]
    pub const fn max_retained_payload_bytes(self) -> u64 {
        self.max_retained_payload_bytes
    }

    #[must_use]
    pub const fn hard_retention(self) -> Duration {
        self.hard_retention
    }

    pub(crate) fn hard_retention_millis(self) -> i64 {
        i64::try_from(self.hard_retention.as_millis())
            .expect("validated hard retention must fit in i64 milliseconds")
    }
}

impl Default for RawOtlpInboxLimits {
    fn default() -> Self {
        Self {
            max_retained_payload_bytes: DEFAULT_MAX_RETAINED_PAYLOAD_BYTES,
            hard_retention: DEFAULT_HARD_RETENTION,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RawOtlpInboxLimitsError {
    #[error("maximum retained payload bytes must be between 1 and i64::MAX")]
    InvalidMaximumPayloadBytes,
    #[error("hard retention must be at least one millisecond and fit in i64 milliseconds")]
    InvalidHardRetention,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawOtlpInboxHealth {
    pub retained_batch_count: u64,
    pub retained_payload_bytes: u64,
    pub oldest_retained_at: Option<DateTime<Utc>>,
    pub newest_retained_at: Option<DateTime<Utc>>,
    pub earliest_replay_at: Option<DateTime<Utc>>,
    pub expired_batch_count: u64,
    pub expired_payload_bytes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawOtlpInboxBacklog {
    pub batch_count: u64,
    pub oldest_received_at: Option<DateTime<Utc>>,
    pub newest_received_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawOtlpInboxRetentionResult {
    pub expired_batch_count: u64,
    pub expired_payload_bytes: u64,
}

pub trait InboxClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemInboxClock;

impl InboxClock for SystemInboxClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Error)]
pub enum RawOtlpInboxError {
    #[error(
        "raw OTLP inbox capacity exceeded: retained {retained_payload_bytes} bytes, incoming \
         {incoming_payload_bytes} bytes, maximum {max_retained_payload_bytes} bytes"
    )]
    CapacityExceeded {
        retained_payload_bytes: u64,
        incoming_payload_bytes: u64,
        max_retained_payload_bytes: u64,
    },
    #[error("unsupported {component} schema version {found}; expected schema version {expected}")]
    UnsupportedSchemaVersion {
        component: &'static str,
        found: i64,
        expected: i64,
    },
    #[error("raw OTLP inbox contains corrupt data: {0}")]
    CorruptData(String),
    #[error("raw OTLP inbox is unavailable")]
    Unavailable {
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl RawOtlpInboxError {
    pub(crate) fn unavailable(source: impl Error + Send + Sync + 'static) -> Self {
        Self::Unavailable {
            source: Box::new(source),
        }
    }
}

#[async_trait]
pub trait RawOtlpInbox: Send + Sync {
    async fn append(&self, batch: NewRawOtlpBatch)
    -> Result<InboxAppendReceipt, RawOtlpInboxError>;

    async fn read_after(
        &self,
        after: Option<InboxBatchId>,
        limit: NonZeroU16,
    ) -> Result<Vec<RawOtlpInboxBatch>, RawOtlpInboxError>;

    async fn inspect_after(
        &self,
        after: Option<InboxBatchId>,
    ) -> Result<RawOtlpInboxBacklog, RawOtlpInboxError>;

    async fn inspect(&self) -> Result<RawOtlpInboxHealth, RawOtlpInboxError>;
}

#[async_trait]
pub trait RawOtlpInboxRetention: Send + Sync {
    async fn enforce_retention(&self) -> Result<RawOtlpInboxRetentionResult, RawOtlpInboxError>;
}
