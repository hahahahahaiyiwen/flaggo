use std::{collections::BTreeMap, num::NonZeroU32, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AuthorityScope, EvidenceSignal, EvidenceStoreError};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ObservationWatermark(u64);

impl ObservationWatermark {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSourceSelector {
    pub signal: EvidenceSignal,
    pub instrumentation_scope: String,
    pub signal_name: String,
    pub metric_kind: Option<String>,
    pub metric_unit: Option<String>,
    pub parent_span_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceQueryScope {
    pub authority: AuthorityScope,
    pub contract_digest: String,
    pub sources: Vec<EvidenceSourceSelector>,
}

#[derive(Clone, Copy, Debug)]
pub struct EvidenceQueryLimits {
    pub max_rows: NonZeroU32,
    pub max_bytes: usize,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct EvidenceQueryRequest {
    pub sql: String,
    pub cutoff_unix_nano: u64,
    pub watermark: ObservationWatermark,
    pub limits: EvidenceQueryLimits,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQueryResult {
    pub normalized_sql: String,
    pub columns: Vec<String>,
    pub rows: Vec<BTreeMap<String, Value>>,
    pub observation_ids: Vec<String>,
    pub result_bytes: usize,
}

#[async_trait]
pub trait EvidenceAnalysisStore: Send + Sync {
    async fn capture_watermark(&self) -> Result<ObservationWatermark, EvidenceStoreError>;

    async fn query(
        &self,
        scope: &EvidenceQueryScope,
        request: EvidenceQueryRequest,
    ) -> Result<EvidenceQueryResult, EvidenceStoreError>;
}
