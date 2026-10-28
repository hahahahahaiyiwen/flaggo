use chrono::{DateTime, FixedOffset};
use flaggo_contract::{DecisionExecutable, JsonValue};
use flaggo_contract_store::timestamp;
use flaggo_expression::CheckedDecisionExecutable;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutableLifecycleState {
    Candidate,
    Active,
    Inactive,
}

impl ExecutableLifecycleState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Candidate => "Candidate",
            Self::Active => "Active",
            Self::Inactive => "Inactive",
        }
    }
}

impl Serialize for ExecutableLifecycleState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::Candidate => 0,
            Self::Active => 1,
            Self::Inactive => 2,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredExecutable {
    pub executable_digest: String,
    pub executable: DecisionExecutable,
    pub checked_executable: CheckedDecisionExecutable,
    pub state: ExecutableLifecycleState,
    pub state_version: i64,
    #[serde(with = "timestamp")]
    pub created_at: DateTime<FixedOffset>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_time"
    )]
    pub activated_at: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<JsonValue>,
}

fn optional_time<S: Serializer>(
    value: &Option<DateTime<FixedOffset>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => timestamp::serialize(value, serializer),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExecutableStoreWriteResult {
    Created,
    Existing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisCandidateAdmission {
    pub workspace_id: String,
    pub cycle_id: String,
    pub attempt_id: String,
    pub contract_name: String,
    pub contract_digest: String,
    pub executable_digest: String,
    pub analysis_manifest_digest: String,
    #[serde(with = "timestamp")]
    pub created_at: DateTime<FixedOffset>,
}

impl AnalysisCandidateAdmission {
    pub(crate) fn identity_matches(&self, other: &Self) -> bool {
        self.workspace_id == other.workspace_id
            && self.cycle_id == other.cycle_id
            && self.attempt_id == other.attempt_id
            && self.contract_name == other.contract_name
            && self.contract_digest == other.contract_digest
            && self.executable_digest == other.executable_digest
            && self.analysis_manifest_digest == other.analysis_manifest_digest
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCandidateStoreResult {
    pub executable_digest: String,
    #[serde(with = "timestamp")]
    pub created_at: DateTime<FixedOffset>,
    pub lifecycle_state: ExecutableLifecycleState,
    pub created: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutablePage {
    pub executables: Vec<StoredExecutable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationResult {
    pub executable_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_executable_digest: Option<String>,
    pub state_version: i64,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutableLifecycleToken {
    pub executable_digest: String,
    pub state_version: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateActivationCommand {
    pub contract_name: String,
    pub contract_digest: String,
    pub executable_digest: String,
    pub expected_candidate_state_version: i64,
    pub expected_active_executable: ExecutableLifecycleToken,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateActivationResult {
    pub executable_digest: String,
    pub previous_executable_digest: String,
    pub state_version: i64,
    pub superseded_candidate_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCandidateActivationSnapshot {
    pub admission: AnalysisCandidateAdmission,
    pub candidate_state_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_executable: Option<ExecutableLifecycleToken>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCandidateActivationPage {
    pub candidates: Vec<AnalysisCandidateActivationSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_contract_name: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum AnalysisCandidateResolutionAction {
    Activate,
    Reject,
}

#[derive(Clone, Debug)]
pub struct AnalysisCandidateResolutionCommand {
    pub admission: AnalysisCandidateAdmission,
    pub expected_candidate_state_version: i64,
    pub expected_active_executable: Option<ExecutableLifecycleToken>,
    pub action: AnalysisCandidateResolutionAction,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AnalysisCandidateResolutionOutcome {
    Activated,
    Rejected,
    Superseded,
    Yielded,
    AlreadyResolved,
}

impl Serialize for AnalysisCandidateResolutionOutcome {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::Activated => 0,
            Self::Rejected => 1,
            Self::Superseded => 2,
            Self::Yielded => 3,
            Self::AlreadyResolved => 4,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCandidateResolutionResult {
    pub outcome: AnalysisCandidateResolutionOutcome,
    pub executable_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_executable_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_version: Option<i64>,
    pub superseded_candidate_count: u64,
}
