use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use flaggo_analysis_domain::{
    AnalysisContract, AnalysisError, AttemptContext, CandidateRecord, EvidenceCutoff,
};
use flaggo_analysis_workspace::WorkspaceLease;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AnalysisRunStatus {
    Active,
    ContractSuperseded {
        #[serde(skip_serializing_if = "Option::is_none")]
        replacement_digest: Option<String>,
    },
    ServiceShutdown,
}

pub struct YieldSignal {
    status: RwLock<AnalysisRunStatus>,
}

impl Default for YieldSignal {
    fn default() -> Self {
        Self {
            status: RwLock::new(AnalysisRunStatus::Active),
        }
    }
}

impl YieldSignal {
    pub fn request_contract_superseded(
        &self,
        replacement_digest: Option<String>,
    ) -> Result<(), AnalysisError> {
        self.request(AnalysisRunStatus::ContractSuperseded { replacement_digest })
    }

    pub fn request_shutdown(&self) -> Result<(), AnalysisError> {
        self.request(AnalysisRunStatus::ServiceShutdown)
    }

    pub fn status(&self) -> Result<AnalysisRunStatus, AnalysisError> {
        self.status
            .read()
            .map(|status| status.clone())
            .map_err(|_| AnalysisError::Agent("analysis control lock was poisoned".to_owned()))
    }

    fn request(&self, requested: AnalysisRunStatus) -> Result<(), AnalysisError> {
        let mut status = self
            .status
            .write()
            .map_err(|_| AnalysisError::Agent("analysis control lock was poisoned".to_owned()))?;
        if *status == AnalysisRunStatus::Active {
            *status = requested;
        }
        Ok(())
    }
}

#[async_trait]
pub trait AnalysisTools: Send + Sync {
    async fn check_cycle_status(&self) -> Result<AnalysisRunStatus, AnalysisError>;

    async fn commit_cutoff(&self, cutoff: DateTime<Utc>) -> Result<EvidenceCutoff, AnalysisError>;

    async fn describe(&self) -> Result<Value, AnalysisError>;

    async fn run_sql(&self, sql: &str) -> Result<Value, AnalysisError>;

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
