use std::{error::Error, num::NonZeroU16};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

mod analysis;
mod sqlite;
mod sqlite_analysis;

pub use analysis::{
    EvidenceAnalysisStore, EvidenceQueryLimits, EvidenceQueryRequest, EvidenceQueryResult,
    EvidenceQueryScope, EvidenceSourceSelector, ObservationWatermark,
};
pub use sqlite::SqliteEvidenceStore;
pub use sqlite_analysis::SqliteEvidenceAnalysisStore;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AuthorityScope {
    pub tenant: String,
    pub application: String,
    pub environment: String,
}

impl AuthorityScope {
    pub fn new(
        tenant: String,
        application: String,
        environment: String,
    ) -> Result<Self, EvidenceStoreError> {
        for (name, value) in [
            ("tenant", &tenant),
            ("application", &application),
            ("environment", &environment),
        ] {
            if !is_authority_identifier(value) {
                return Err(EvidenceStoreError::InvalidWrite(format!(
                    "{name} must match ^[A-Za-z][A-Za-z0-9._-]{{0,127}}$"
                )));
            }
        }
        Ok(Self {
            tenant,
            application,
            environment,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceSignal {
    Log,
    Metric,
    Span,
    SpanEvent,
}

impl EvidenceSignal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Metric => "metric",
            Self::Span => "span",
            Self::SpanEvent => "span-event",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "log" => Some(Self::Log),
            "metric" => Some(Self::Metric),
            "span" => Some(Self::Span),
            "span-event" => Some(Self::SpanEvent),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceKey {
    pub signal: EvidenceSignal,
    pub instrumentation_scope: String,
    pub signal_name: String,
    pub metric_kind: Option<String>,
    pub metric_unit: Option<String>,
    pub parent_span_name: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MaterializerVersions {
    pub materializer: String,
    pub decoder: String,
    pub identity: String,
    pub projection: String,
    pub routing: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ForwardMaterializationKey {
    pub versions: MaterializerVersions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredContractCatalog {
    pub etag: String,
    pub payload: Vec<u8>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceObservation {
    pub observation_id: String,
    pub logical_source_id: String,
    pub content_digest: String,
    pub authority: AuthorityScope,
    pub source: SourceKey,
    pub observed_at_unix_nano: u64,
    pub observed_time_source: String,
    pub protocol_kind: Option<String>,
    pub decision_id: Option<String>,
    pub contract_digest: Option<String>,
    pub payload_json: Vec<u8>,
    pub versions: MaterializerVersions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceObservationWrite {
    pub observation: EvidenceObservation,
    pub candidate_ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceDiagnostic {
    pub diagnostic_id: String,
    pub inbox_batch_id: u64,
    pub candidate_ordinal: Option<u32>,
    pub code: String,
    pub message: String,
    pub detail_json: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceMaterializationCommit {
    pub key: ForwardMaterializationKey,
    pub inbox_batch_id: u64,
    pub observations: Vec<EvidenceObservationWrite>,
    pub diagnostics: Vec<EvidenceDiagnostic>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvidenceStoreCommitResult {
    pub already_checkpointed: bool,
    pub observations_created: u64,
    pub duplicate_observations: u64,
    pub provenance_created: u64,
    pub diagnostics_created: u64,
    pub conflicts_created: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceStoreHealth {
    pub has_cached_catalog: bool,
    pub observation_count: u64,
    pub provenance_count: u64,
    pub diagnostic_count: u64,
    pub conflict_count: u64,
    pub newest_observed_at_unix_nano: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredEvidenceObservation {
    pub observation: EvidenceObservation,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum EvidenceStoreError {
    #[error("unsupported {component} schema version {found}; expected schema version {expected}")]
    UnsupportedSchemaVersion {
        component: &'static str,
        found: i64,
        expected: i64,
    },
    #[error("evidence store contains corrupt data: {0}")]
    CorruptData(String),
    #[error("invalid evidence write: {0}")]
    InvalidWrite(String),
    #[error("invalid evidence query: {0}")]
    InvalidQuery(String),
    #[error("evidence query exceeded its {0} limit")]
    QueryLimit(&'static str),
    #[error("evidence store is unavailable")]
    Unavailable {
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl EvidenceStoreError {
    pub(crate) fn unavailable(source: impl Error + Send + Sync + 'static) -> Self {
        Self::Unavailable {
            source: Box::new(source),
        }
    }
}

#[async_trait]
pub trait EvidenceStore: Send + Sync {
    async fn save_catalog(&self, catalog: StoredContractCatalog) -> Result<(), EvidenceStoreError>;

    async fn load_catalog(&self) -> Result<Option<StoredContractCatalog>, EvidenceStoreError>;

    async fn forward_checkpoint(
        &self,
        key: &ForwardMaterializationKey,
    ) -> Result<Option<u64>, EvidenceStoreError>;

    async fn commit(
        &self,
        commit: EvidenceMaterializationCommit,
    ) -> Result<EvidenceStoreCommitResult, EvidenceStoreError>;

    async fn inspect(&self) -> Result<EvidenceStoreHealth, EvidenceStoreError>;

    async fn list_observations(
        &self,
        authority: &AuthorityScope,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceObservation>, EvidenceStoreError>;
}

fn is_authority_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
