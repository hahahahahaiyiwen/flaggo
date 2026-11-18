use std::{collections::BTreeMap, num::NonZeroU32, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{AuthorityScope, EvidenceSignal, EvidenceStoreError, EvidenceValue};

pub const EVIDENCE_RESULT_FORMAT: &str = "flaggo.evidence.query-result/v1";
pub const EVIDENCE_VALUE_FORMAT: &str = "flaggo.evidence.value/v1";
pub const PYTHON_EVIDENCE_DECODER: &str = include_str!("../python/flaggo_evidence.py");

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSourceSelector {
    pub signal: EvidenceSignal,
    pub instrumentation_scope: String,
    pub signal_name: String,
    pub metric_kind: Option<String>,
    pub metric_unit: Option<String>,
    pub parent_span_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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
    pub profile: String,
    pub sql: String,
    pub limits: EvidenceQueryLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceValueKind {
    Null,
    Int64,
    Float64,
    Text,
    Boolean,
    Json,
    Variant,
}

/// Output metadata prepared before execution, independent of whether any rows exist.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceColumn {
    pub name: String,
    /// Provider metadata, never a portable type identifier.
    #[serde(rename = "providerType")]
    pub data_type: Option<String>,
    pub kind: EvidenceValueKind,
    pub nullable: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRelationColumn {
    pub name: &'static str,
    #[serde(rename = "providerType")]
    pub data_type: Option<&'static str>,
    pub kind: EvidenceValueKind,
    pub nullable: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceRelation {
    pub name: &'static str,
    pub columns: &'static [EvidenceRelationColumn],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQueryResult {
    pub profile: String,
    pub normalized_sql: String,
    pub columns: Vec<EvidenceColumn>,
    pub rows: Vec<BTreeMap<String, EvidenceValue>>,
    /// IDs projected by the query, not complete aggregate lineage.
    #[serde(rename = "projectedObservationIds")]
    pub observation_ids: Vec<String>,
    pub result_bytes: usize,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQueryCapabilities {
    pub profile: &'static str,
    pub result_format: &'static str,
    pub value_format: &'static str,
    pub dialect: &'static str,
    pub statements: &'static [&'static str],
    pub relations: &'static [EvidenceRelation],
    pub features: &'static [&'static str],
    pub functions: &'static [&'static str],
    pub semantics: &'static [&'static str],
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
