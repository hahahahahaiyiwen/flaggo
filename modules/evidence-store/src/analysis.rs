use std::{collections::BTreeMap, num::NonZeroU32, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AuthorityScope, EvidenceSignal, EvidenceStoreError};

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
    pub limits: EvidenceQueryLimits,
}

/// Output metadata prepared before execution, independent of whether any rows exist.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceColumn {
    pub name: String,
    /// A declared SQLite type, or null when SQLite cannot reliably report one.
    #[serde(rename = "type")]
    pub data_type: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceRelationColumn {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub data_type: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceRelation {
    pub name: &'static str,
    pub columns: &'static [EvidenceRelationColumn],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQueryResult {
    pub normalized_sql: String,
    pub columns: Vec<EvidenceColumn>,
    pub rows: Vec<BTreeMap<String, Value>>,
    pub observation_ids: Vec<String>,
    pub result_bytes: usize,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQueryCapabilities {
    pub dialect: &'static str,
    pub statements: &'static [&'static str],
    pub relations: &'static [EvidenceRelation],
    pub features: &'static [&'static str],
    pub functions: &'static [&'static str],
}

#[async_trait]
pub trait EvidenceAnalysisStore: Send + Sync {
    fn query_capabilities(&self) -> EvidenceQueryCapabilities;

    /// Query currently committed evidence in the authorized scope, without a cycle watermark.
    async fn query(
        &self,
        scope: &EvidenceQueryScope,
        request: EvidenceQueryRequest,
    ) -> Result<EvidenceQueryResult, EvidenceStoreError>;
}
