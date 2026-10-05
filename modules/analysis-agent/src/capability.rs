use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use flaggo_analysis_domain::{
    AnalysisContract, AnalysisError, AttemptContext, CandidateRecord, EvidenceCutoff,
};
use flaggo_analysis_workspace::WorkspaceLease;

pub struct YieldSignal {
    reason: RwLock<Option<String>>,
}

impl Default for YieldSignal {
    fn default() -> Self {
        Self {
            reason: RwLock::new(None),
        }
    }
}

impl YieldSignal {
    pub fn request(&self, reason: impl Into<String>) {
        if let Ok(mut current) = self.reason.write() {
            current.get_or_insert_with(|| reason.into());
        }
    }

    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.reason.read().is_ok_and(|reason| reason.is_some())
    }

    #[must_use]
    pub fn reason(&self) -> Option<String> {
        self.reason.read().ok().and_then(|reason| reason.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentContractCheck {
    pub expected_digest: String,
    pub current_digest: Option<String>,
    pub is_current: bool,
}

#[async_trait]
pub trait AnalysisTools: Send + Sync {
    async fn check_current_contract(&self) -> Result<CurrentContractCheck, AnalysisError>;

    async fn commit_evidence_cutoff(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<EvidenceCutoff, AnalysisError>;

    async fn describe_evidence(&self) -> Result<Value, AnalysisError>;

    async fn query_evidence(&self, sql: &str) -> Result<Value, AnalysisError>;

    async fn propose_executable(&self, rules: Value) -> Result<CandidateRecord, AnalysisError>;

    async fn proposed_candidate(&self) -> Result<Option<CandidateRecord>, AnalysisError>;

    fn yield_signal(&self) -> Arc<YieldSignal>;
}

#[async_trait]
pub trait AnalysisCapabilityFactory: Send + Sync {
    async fn create(
        &self,
        context: AttemptContext,
        contract: AnalysisContract,
        workspace: Arc<dyn WorkspaceLease>,
    ) -> Result<Arc<dyn AnalysisTools>, AnalysisError>;
}
