use std::{error::Error, num::NonZeroU16};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

mod sqlite;

pub use sqlite::SqliteEvidenceStore;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DecisionScope {
    pub application: String,
    pub environment: String,
}

impl DecisionScope {
    pub fn new(application: String, environment: String) -> Result<Self, EvidenceStoreError> {
        if application.is_empty() || environment.is_empty() {
            return Err(EvidenceStoreError::InvalidWrite(
                "application and environment must not be empty".to_owned(),
            ));
        }
        Ok(Self {
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
pub struct MaterializerVersions {
    pub materializer: String,
    pub decoder: String,
    pub identity: String,
    pub projection: String,
    pub selector_protocol: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MaterializationKey {
    pub snapshot_digest: String,
    pub versions: MaterializerVersions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSelectorSnapshot {
    pub snapshot_digest: String,
    pub payload: Vec<u8>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceObservation {
    pub observation_id: String,
    pub logical_source_id: String,
    pub content_digest: String,
    pub scope: DecisionScope,
    pub signal: EvidenceSignal,
    pub instrumentation_scope: String,
    pub signal_name: String,
    pub metric_kind: Option<String>,
    pub metric_unit: Option<String>,
    pub observed_at_unix_nano: u64,
    pub observed_time_source: String,
    pub protocol_kind: Option<String>,
    pub decision_id: Option<String>,
    pub contract_digest: Option<String>,
    pub payload_json: Vec<u8>,
    pub versions: MaterializerVersions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceAssociation {
    pub snapshot_digest: String,
    pub contract_digest: String,
    pub evidence_name: String,
    pub contract_attribute: String,
    pub correlation_json: Vec<u8>,
    pub source_json: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceObservationWrite {
    pub observation: EvidenceObservation,
    pub candidate_ordinal: u32,
    pub associations: Vec<EvidenceAssociation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceDiagnostic {
    pub diagnostic_id: String,
    pub snapshot_digest: String,
    pub inbox_batch_id: u64,
    pub candidate_ordinal: Option<u32>,
    pub code: String,
    pub message: String,
    pub detail_json: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceMaterializationCommit {
    pub key: MaterializationKey,
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
    pub associations_created: u64,
    pub diagnostics_created: u64,
    pub conflicts_created: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceStoreHealth {
    pub active_snapshot_digest: Option<String>,
    pub observation_count: u64,
    pub association_count: u64,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredEvidenceAssociation {
    pub observation_id: String,
    pub scope: DecisionScope,
    pub snapshot_digest: String,
    pub contract_digest: String,
    pub evidence_name: String,
    pub contract_attribute: String,
    pub correlation_json: Vec<u8>,
    pub source_json: Vec<u8>,
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
    async fn activate_snapshot(
        &self,
        snapshot: StoredSelectorSnapshot,
    ) -> Result<(), EvidenceStoreError>;

    async fn load_active_snapshot(
        &self,
    ) -> Result<Option<StoredSelectorSnapshot>, EvidenceStoreError>;

    async fn checkpoint(&self, key: &MaterializationKey)
    -> Result<Option<u64>, EvidenceStoreError>;

    async fn commit(
        &self,
        commit: EvidenceMaterializationCommit,
    ) -> Result<EvidenceStoreCommitResult, EvidenceStoreError>;

    async fn inspect(&self) -> Result<EvidenceStoreHealth, EvidenceStoreError>;

    async fn list_observations(
        &self,
        scope: &DecisionScope,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceObservation>, EvidenceStoreError>;

    async fn list_associations(
        &self,
        scope: &DecisionScope,
        contract_digest: &str,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceAssociation>, EvidenceStoreError>;
}
