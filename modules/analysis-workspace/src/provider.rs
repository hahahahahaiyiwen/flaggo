use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const WORKSPACE_CAPABILITY: &str = "flaggo.workspace/v2";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceClaim {
    pub resource: flaggo_contract::ContractResourceId,
    pub workspace_id: String,
    pub attempt_id: String,
    pub epoch: u64,
}

impl WorkspaceClaim {
    pub fn candidate_ownership(&self) -> flaggo_contract::CandidateOwnership {
        flaggo_contract::CandidateOwnership {
            attempt_id: self.attempt_id.clone(),
            epoch: self.epoch.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceArtifact {
    pub resource: flaggo_contract::ContractResourceId,
    pub workspace_id: String,
    pub cycle_id: String,
    pub attempt_id: String,
    pub digest: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffPublication {
    pub summary: String,
    pub metadata: Option<Value>,
}

pub const EXECUTION_CAPABILITY: &str = "flaggo.execution/v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionRequest {
    pub execution_id: String,
    pub source: WorkspaceArtifact,
    /// Analysis-relative virtual directory for module imports, not a host path.
    pub source_directory: String,
    pub profile: String,
    pub timeout_ms: u64,
    pub output_limit_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExecutionState {
    /// A process may have started. Absence of a receipt is never permission to rerun.
    Pending,
    Completed {
        result: WorkspaceArtifact,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub request: ExecutionRequest,
    pub epoch: u64,
    pub state: ExecutionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionReservation {
    Granted(ExecutionReceipt),
    Existing(ExecutionReceipt),
}

use flaggo_analysis_domain::{
    AnalysisContract, AnalysisCycle, AnalysisError, AnalysisProfile, CandidateProposal,
    CandidateRecord, CycleLimits, CycleOutcome, WorkflowHandoff,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CyclePreparation {
    pub cycle: AnalysisCycle,
    pub eligible_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareCycleResult {
    Ready(Box<CyclePreparation>),
    NotEligible { eligible_at: DateTime<Utc> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEntry {
    pub name: String,
    pub kind: WorkspaceEntryKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceFileInfo {
    pub is_file: bool,
    pub is_directory: bool,
    pub size: u64,
    pub modified_at: DateTime<Utc>,
}

#[async_trait]
pub trait WorkspaceFileSystem: Send + Sync {
    async fn read_text(&self, path: &str) -> Result<String, AnalysisError>;

    async fn write_text(&self, path: &str, content: &str) -> Result<(), AnalysisError>;

    async fn append_text(&self, path: &str, content: &str) -> Result<(), AnalysisError>;

    async fn exists(&self, path: &str) -> Result<bool, AnalysisError>;

    async fn metadata(&self, path: &str) -> Result<WorkspaceFileInfo, AnalysisError>;

    async fn create_directory(&self, path: &str, recursive: bool) -> Result<(), AnalysisError>;

    async fn read_directory(&self, path: &str) -> Result<Vec<WorkspaceEntry>, AnalysisError>;

    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), AnalysisError>;

    async fn rename(&self, source: &str, destination: &str) -> Result<(), AnalysisError>;
}

#[async_trait]
pub trait WorkspaceLease: Send + Sync {
    fn resource(&self) -> &flaggo_contract::ContractResourceId;

    fn claim_token(&self) -> WorkspaceClaim;

    /// Revoke this claim and every capability derived from it before returning.
    async fn release(&self) -> Result<(), AnalysisError>;

    fn workspace_id(&self) -> &str;

    fn contract_name(&self) -> &str;

    async fn prepare_cycle(
        &self,
        contract: &AnalysisContract,
        analysis_profile: &AnalysisProfile,
        attempt_id: &str,
        now: DateTime<Utc>,
    ) -> Result<PrepareCycleResult, AnalysisError>;

    async fn filesystem(
        &self,
        cycle_id: &str,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceFileSystem>, AnalysisError>;

    async fn append_audit(
        &self,
        cycle_id: &str,
        category: &str,
        record: Value,
    ) -> Result<(), AnalysisError>;

    async fn seal_analysis_manifest(&self, cycle_id: &str) -> Result<String, AnalysisError>;

    async fn prepare_candidate_proposal(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        rules: Value,
    ) -> Result<CandidateProposal, AnalysisError>;

    async fn candidate_record(
        &self,
        cycle_id: &str,
    ) -> Result<Option<CandidateRecord>, AnalysisError>;

    async fn record_candidate(
        &self,
        cycle_id: &str,
        candidate: &CandidateRecord,
    ) -> Result<(), AnalysisError>;

    async fn mark_superseding(
        &self,
        cycle_id: &str,
        replacement_digest: Option<&str>,
    ) -> Result<(), AnalysisError>;

    async fn record_handoff(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        summary: &str,
        metadata: Option<Value>,
        expected_previous: Option<&str>,
    ) -> Result<(), AnalysisError> {
        let content = serde_json::to_vec(&HandoffPublication {
            summary: summary.into(),
            metadata,
        })
        .map_err(|error| AnalysisError::workspace(error.to_string()))?;
        let artifact = self.put_artifact(cycle_id, attempt_id, &content).await?;
        self.commit_handoff(&artifact, expected_previous).await
    }

    /// Upload immutable, scoped bytes without changing committed workflow state.
    async fn put_artifact(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        bytes: &[u8],
    ) -> Result<WorkspaceArtifact, AnalysisError>;

    async fn read_artifact(&self, artifact: &WorkspaceArtifact) -> Result<Vec<u8>, AnalysisError>;

    /// Compare the previous committed attempt and publish under the current fence.
    /// Identical retries reconcile an interrupted publication; conflicting retries fail.
    async fn commit_handoff(
        &self,
        artifact: &WorkspaceArtifact,
        expected_previous: Option<&str>,
    ) -> Result<(), AnalysisError>;

    /// Atomically reserve a scoped idempotency key before any external execution.
    async fn reserve_execution(
        &self,
        request: &ExecutionRequest,
    ) -> Result<ExecutionReservation, AnalysisError>;

    async fn execution_receipt(
        &self,
        cycle_id: &str,
        execution_id: &str,
    ) -> Result<Option<ExecutionReceipt>, AnalysisError>;

    /// Publish an immutable result under the original execution's still-current fence.
    async fn complete_execution(
        &self,
        cycle_id: &str,
        execution_id: &str,
        result: &WorkspaceArtifact,
    ) -> Result<ExecutionReceipt, AnalysisError>;

    async fn latest_handoff(
        &self,
        cycle_id: &str,
    ) -> Result<Option<WorkflowHandoff>, AnalysisError>;

    async fn pending_proposal(
        &self,
        cycle_id: &str,
    ) -> Result<Option<CandidateProposal>, AnalysisError>;

    async fn reserve_attempt(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        limits: &CycleLimits,
        timeout: Duration,
    ) -> Result<Duration, AnalysisError>;

    async fn settle_attempt(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        elapsed: Duration,
    ) -> Result<(), AnalysisError>;

    async fn record_attempt_failure(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        error: &str,
    ) -> Result<(), AnalysisError>;

    async fn complete_cycle(
        &self,
        contract: &AnalysisContract,
        cycle_id: &str,
        outcome: &CycleOutcome,
        completed_at: DateTime<Utc>,
    ) -> Result<(), AnalysisError>;
}

#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    async fn claim(
        &self,
        resource: &flaggo_contract::ContractResourceId,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceLease>, AnalysisError>;
}
