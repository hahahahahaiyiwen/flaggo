use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use flaggo_analysis_domain::{
    AnalysisContract, AnalysisCycle, AnalysisError, AnalysisProfile, CandidateProposal,
    CandidateRecord, CycleOutcome, EvidenceCutoff,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CyclePreparation {
    pub cycle: AnalysisCycle,
    pub eligible_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareCycleResult {
    Ready(CyclePreparation),
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

    async fn commit_cutoff(
        &self,
        cycle_id: &str,
        cutoff: DateTime<Utc>,
        watermark: u64,
    ) -> Result<EvidenceCutoff, AnalysisError>;

    async fn cutoff(&self, cycle_id: &str) -> Result<Option<EvidenceCutoff>, AnalysisError>;

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
        contract_name: &str,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceLease>, AnalysisError>;
}
