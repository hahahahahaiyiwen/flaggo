//! Pure decisions over durable workspace facts. No function acquires ownership,
//! verifies artifact bytes, or persists its result. Adapters must protect the
//! read/plan/write interval with their authority and integrity mechanisms.
//!
//! The serde records preserve the local v1 representation, not a required
//! distributed storage layout. Manifest hashes use compact encoding; proposals
//! and other local records use pretty encoding.

use std::time::Duration;

use chrono::{DateTime, Utc};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisCycle, AnalysisError, AnalysisProfile, CandidateProposal,
    CandidateRecord, CycleLimits, CycleOutcome, WorkflowHandoff,
};
use flaggo_contract::{CandidateOwnership, ContractResourceId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ExecutionReceipt, ExecutionRequest, ExecutionReservation, ExecutionState, HandoffPublication,
    WorkspaceArtifact, sha256_digest,
};

pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCycle {
    pub schema_version: u32,
    pub cycle_id: Option<String>,
}

impl CurrentCycle {
    pub fn empty() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            cycle_id: None,
        }
    }
}

/// In-process access facts after the adapter has verified the live claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CycleAccess<'a> {
    pub active_cycle_id: Option<&'a str>,
    pub writable: bool,
    pub sealed: bool,
}

impl<'a> CycleAccess<'a> {
    pub fn resumed(cycle_id: &'a str, sealed: bool) -> Self {
        Self {
            active_cycle_id: Some(cycle_id),
            writable: !sealed,
            sealed,
        }
    }

    pub fn require_active(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        if self.active_cycle_id != Some(cycle_id) {
            return Err(AnalysisError::workspace(
                "operation does not target the active claimed cycle",
            ));
        }
        Ok(())
    }

    pub fn require_writable(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        if self.active_cycle_id != Some(cycle_id) || !self.writable || self.sealed {
            return Err(AnalysisError::workspace(
                "analysis workspace is no longer writable",
            ));
        }
        Ok(())
    }
}

fn workspace_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::workspace(error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleState {
    pub schema_version: u32,
    pub contract_digest: String,
    pub accepted_at: DateTime<Utc>,
    pub last_terminal_completed_at: Option<DateTime<Utc>>,
    pub next_eligible_at: DateTime<Utc>,
}

impl ScheduleState {
    pub fn new(contract: &AnalysisContract) -> Result<Self, AnalysisError> {
        Ok(Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            contract_digest: contract.contract_digest.clone(),
            accepted_at: contract.accepted_at,
            last_terminal_completed_at: None,
            next_eligible_at: contract
                .eligible_after(contract.accepted_at)
                .map_err(workspace_error)?,
        })
    }

    pub fn completed(
        contract: &AnalysisContract,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, AnalysisError> {
        Ok(Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            contract_digest: contract.contract_digest.clone(),
            accepted_at: contract.accepted_at,
            last_terminal_completed_at: Some(completed_at),
            next_eligible_at: contract
                .eligible_after(completed_at)
                .map_err(workspace_error)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CycleStatus {
    Active,
    Superseding,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCycle {
    pub schema_version: u32,
    pub workspace_id: String,
    pub cycle_id: String,
    pub contract_name: String,
    pub contract_digest: String,
    pub analysis_profile: AnalysisProfile,
    pub opened_at: DateTime<Utc>,
    pub status: CycleStatus,
    pub replacement_digest: Option<String>,
    #[serde(default)]
    pub latest_handoff: Option<String>,
}

impl StoredCycle {
    pub fn new(
        workspace_id: &str,
        cycle_id: &str,
        contract: &AnalysisContract,
        analysis_profile: &AnalysisProfile,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: workspace_id.into(),
            cycle_id: cycle_id.into(),
            contract_name: contract.name.clone(),
            contract_digest: contract.contract_digest.clone(),
            analysis_profile: analysis_profile.clone(),
            opened_at: now,
            status: CycleStatus::Active,
            replacement_digest: None,
            latest_handoff: None,
        }
    }

    pub fn as_analysis_cycle(
        &self,
        scope: &flaggo_analysis_domain::AnalysisAuthority,
        resumed: bool,
    ) -> AnalysisCycle {
        AnalysisCycle {
            scope: scope.clone(),
            workspace_id: self.workspace_id.clone(),
            cycle_id: self.cycle_id.clone(),
            contract_name: self.contract_name.clone(),
            contract_digest: self.contract_digest.clone(),
            analysis_profile: self.analysis_profile.clone(),
            opened_at: self.opened_at,
            resumed,
        }
    }

    pub fn superseding(&self, replacement_digest: Option<&str>) -> Self {
        let mut cycle = self.clone();
        cycle.status = CycleStatus::Superseding;
        cycle.replacement_digest = replacement_digest.map(ToOwned::to_owned);
        cycle
    }

    pub fn terminal(&self, outcome: &CycleOutcome) -> Self {
        let mut cycle = self.clone();
        cycle.status = CycleStatus::Terminal;
        if let CycleOutcome::Superseded { replacement_digest } = outcome {
            cycle.replacement_digest = replacement_digest.clone();
        }
        cycle
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentCyclePlan {
    /// Reconcile the durable terminal outcome before considering eligibility.
    ReconcileTerminal,
    /// Record the attempt, then derive access from durable seal/proposal presence.
    Resume,
    Supersede,
    Clear,
}

/// A terminal journal takes precedence even when cycle.json still says active.
pub fn plan_current_cycle(
    cycle: Option<&StoredCycle>,
    contract_digest: &str,
    has_terminal: bool,
) -> CurrentCyclePlan {
    if has_terminal {
        CurrentCyclePlan::ReconcileTerminal
    } else if let Some(cycle) = cycle
        && matches!(cycle.status, CycleStatus::Active | CycleStatus::Superseding)
    {
        if cycle.contract_digest == contract_digest {
            CurrentCyclePlan::Resume
        } else {
            CurrentCyclePlan::Supersede
        }
    } else {
        CurrentCyclePlan::Clear
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityPlan {
    pub schedule: ScheduleState,
    pub replace_schedule: bool,
    pub eligible: bool,
}

/// Apply only after resolving the previous current cycle.
pub fn plan_eligibility(
    schedule: &ScheduleState,
    contract: &AnalysisContract,
    now: DateTime<Utc>,
) -> Result<EligibilityPlan, AnalysisError> {
    let replace_schedule = schedule.contract_digest != contract.contract_digest;
    let schedule = if replace_schedule {
        ScheduleState::new(contract)?
    } else {
        schedule.clone()
    };
    Ok(EligibilityPlan {
        eligible: now >= schedule.next_eligible_at,
        schedule,
        replace_schedule,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredOutcome {
    pub schema_version: u32,
    pub completed_at: DateTime<Utc>,
    pub outcome: CycleOutcome,
}

pub fn validate_terminal_contract(
    stored: &AnalysisContract,
    requested: &AnalysisContract,
) -> Result<(), AnalysisError> {
    if stored.resource()? != requested.resource()?
        || stored.contract_digest != requested.contract_digest
    {
        return Err(AnalysisError::workspace(
            "terminal cycle contract does not match its durable contract snapshot",
        ));
    }
    Ok(())
}

pub fn validate_terminal_candidate(
    outcome: &CycleOutcome,
    stored: Option<&CandidateRecord>,
) -> Result<(), AnalysisError> {
    if let CycleOutcome::Candidate { candidate } = outcome
        && stored != Some(candidate)
    {
        return Err(AnalysisError::workspace(
            "terminal Candidate does not match the durable Candidate record",
        ));
    }
    Ok(())
}

/// Identical terminal retries retain the original completion timestamp.
pub fn plan_outcome(
    existing: Option<&StoredOutcome>,
    outcome: &CycleOutcome,
    completed_at: DateTime<Utc>,
) -> Result<StoredOutcome, AnalysisError> {
    if let Some(existing) = existing {
        if existing.outcome != *outcome {
            return Err(AnalysisError::workspace(
                "terminal cycle outcome cannot be changed",
            ));
        }
        return Ok(existing.clone());
    }
    Ok(StoredOutcome {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        completed_at,
        outcome: outcome.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisManifest {
    pub schema_version: u32,
    pub workspace_id: String,
    pub cycle_id: String,
    pub analysis_profile: AnalysisProfile,
    pub files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// Cycle-relative logical path with '/' separators, not a host path.
    pub path: String,
    pub size: u64,
    pub digest: String,
}

pub fn is_manifest_control_file(relative: &str) -> bool {
    matches!(
        relative,
        "cycle.json"
            | "budget.json"
            | "analysis-manifest.json"
            | "proposal.pending.json"
            | "proposal.json"
            | "candidate.json"
            | "outcome.json"
    ) || relative.starts_with("attempts/")
        || relative.starts_with("analysis/.copilot/")
}

impl AnalysisManifest {
    pub fn new(
        workspace_id: &str,
        cycle_id: &str,
        analysis_profile: AnalysisProfile,
        mut files: Vec<ManifestFile>,
    ) -> Self {
        files.retain(|file| !is_manifest_control_file(&file.path));
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: workspace_id.into(),
            cycle_id: cycle_id.into(),
            analysis_profile,
            files,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, AnalysisError> {
        serde_json::to_vec(self).map_err(workspace_error)
    }

    /// Preserve the legacy serde acceptance rules; membership is verified separately.
    pub fn decode(bytes: &[u8]) -> Result<Self, AnalysisError> {
        serde_json::from_slice(bytes).map_err(workspace_error)
    }

    pub fn verify(&self, actual: &Self) -> Result<(), AnalysisError> {
        if self != actual {
            return Err(AnalysisError::workspace(
                "analysis workspace changed after its manifest was sealed",
            ));
        }
        Ok(())
    }
}

pub fn decode_proposal(bytes: &[u8]) -> Result<CandidateProposal, AnalysisError> {
    serde_json::from_slice(bytes).map_err(workspace_error)
}

pub fn encode_proposal(proposal: &CandidateProposal) -> Result<Vec<u8>, AnalysisError> {
    serde_json::to_vec_pretty(proposal).map_err(workspace_error)
}

pub fn validate_proposal_rules(
    existing: &CandidateProposal,
    rules: &Value,
) -> Result<(), AnalysisError> {
    if existing.rules != *rules {
        return Err(AnalysisError::ProposalConflict(
            "a different Candidate proposal is already committed for this cycle".into(),
        ));
    }
    Ok(())
}

/// The manifest bytes must come from a verified stored manifest or a rebuilt
/// manifest. Recovery preserves the original attempt even under a new owner.
pub fn plan_proposal(
    existing: Option<&CandidateProposal>,
    attempt_id: &str,
    rules: Value,
    manifest_bytes: &[u8],
) -> Result<CandidateProposal, AnalysisError> {
    if let Some(existing) = existing {
        validate_proposal_rules(existing, &rules)?;
        verify_proposal_manifest(existing, manifest_bytes, true)?;
        return Ok(existing.clone());
    }
    Ok(CandidateProposal {
        attempt_id: attempt_id.into(),
        rules,
        analysis_manifest_digest: sha256_digest(manifest_bytes),
    })
}

pub fn verify_proposal_manifest(
    proposal: &CandidateProposal,
    manifest_bytes: &[u8],
    recovering: bool,
) -> Result<(), AnalysisError> {
    if sha256_digest(manifest_bytes) != proposal.analysis_manifest_digest {
        return Err(AnalysisError::workspace(if recovering {
            "recovered analysis manifest does not match the durable proposal"
        } else {
            "sealed analysis manifest does not match the durable proposal"
        }));
    }
    Ok(())
}

pub fn verify_proposal_record(
    proposal: &CandidateProposal,
    stored: &CandidateProposal,
) -> Result<(), AnalysisError> {
    if stored != proposal {
        return Err(AnalysisError::workspace(
            "Candidate proposal record conflicts with its recovery journal",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateRecordPlan {
    Append,
    Unchanged,
}

pub fn validate_candidate_binding(
    contract_name: &str,
    contract_digest: &str,
    proposal: &CandidateProposal,
    candidate: &CandidateRecord,
) -> Result<(), AnalysisError> {
    if candidate.contract_name != contract_name
        || candidate.contract_digest != contract_digest
        || proposal.analysis_manifest_digest != candidate.analysis_manifest_digest
    {
        return Err(AnalysisError::workspace(
            "Candidate response does not match the sealed analysis manifest",
        ));
    }
    Ok(())
}

pub fn plan_candidate_record(
    existing: Option<&CandidateRecord>,
    candidate: &CandidateRecord,
) -> Result<CandidateRecordPlan, AnalysisError> {
    match existing {
        Some(existing) if existing != candidate => Err(AnalysisError::workspace(
            "Candidate response conflicts with the durable cycle record",
        )),
        Some(_) => Ok(CandidateRecordPlan::Unchanged),
        None => Ok(CandidateRecordPlan::Append),
    }
}

/// Source/result handles must already have passed adapter scope and byte-integrity checks.
pub fn plan_execution_reservation(
    request: &ExecutionRequest,
    epoch: u64,
    existing: Option<&ExecutionReceipt>,
) -> Result<ExecutionReservation, AnalysisError> {
    validate_execution_request(request)?;
    if let Some(existing) = existing {
        if existing.request != *request {
            return Err(AnalysisError::InvalidState(
                "execution idempotency key conflicts with its recorded request".into(),
            ));
        }
        return Ok(ExecutionReservation::Existing(existing.clone()));
    }
    Ok(ExecutionReservation::Granted(ExecutionReceipt {
        request: request.clone(),
        epoch,
        state: ExecutionState::Pending,
    }))
}

pub fn validate_execution_request(request: &ExecutionRequest) -> Result<(), AnalysisError> {
    if request.profile.is_empty() || request.timeout_ms == 0 || request.output_limit_bytes == 0 {
        return Err(AnalysisError::InvalidState(
            "execution requires a profile and positive bounds".into(),
        ));
    }
    Ok(())
}

pub fn validate_execution_receipt(
    receipt: &ExecutionReceipt,
    cycle_id: &str,
    execution_id: &str,
) -> Result<(), AnalysisError> {
    if receipt.request.execution_id != execution_id || receipt.request.source.cycle_id != cycle_id {
        return Err(AnalysisError::InvalidState(
            "execution receipt identity mismatch".into(),
        ));
    }
    Ok(())
}

pub fn plan_execution_completion(
    receipt: &ExecutionReceipt,
    epoch: u64,
    cycle_id: &str,
    execution_id: &str,
    result: &WorkspaceArtifact,
) -> Result<ExecutionReceipt, AnalysisError> {
    if receipt.epoch != epoch
        || receipt.request.execution_id != execution_id
        || receipt.request.source.cycle_id != cycle_id
        || result.cycle_id != cycle_id
        || result.attempt_id != receipt.request.source.attempt_id
    {
        return Err(AnalysisError::InvalidState(
            "execution result has a stale fence or mismatched identity".into(),
        ));
    }
    let completed = ExecutionState::Completed {
        result: result.clone(),
    };
    if receipt.state != ExecutionState::Pending && receipt.state != completed {
        return Err(AnalysisError::InvalidState(
            "execution result conflicts with its committed receipt".into(),
        ));
    }
    let mut receipt = receipt.clone();
    receipt.state = completed;
    Ok(receipt)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRecord {
    pub schema_version: u32,
    pub attempt_id: String,
    pub recorded_at: DateTime<Utc>,
    pub summary: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub cycle_id: Option<String>,
    #[serde(default)]
    pub contract_digest: Option<String>,
    #[serde(default)]
    pub previous_handoff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<WorkspaceArtifact>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandoffPlan {
    /// None on an immutable retry.
    pub append: Option<HandoffRecord>,
    /// None for a metadata-free checkpoint; do not change the pointer.
    pub advance_to: Option<String>,
}

pub fn validate_handoff_publication(publication: &HandoffPublication) -> Result<(), AnalysisError> {
    if publication.summary.trim().is_empty()
        || publication
            .metadata
            .as_ref()
            .is_some_and(|value| !value.is_object())
        || serde_json::to_vec(&publication.metadata)
            .map_err(workspace_error)?
            .len()
            > 16_384
    {
        return Err(AnalysisError::InvalidState(
            "handoff requires a non-empty summary and at most 16 KiB of object metadata".into(),
        ));
    }
    Ok(())
}

pub fn plan_handoff(
    publication: &HandoffPublication,
    artifact: &WorkspaceArtifact,
    cycle: &StoredCycle,
    existing: Option<&HandoffRecord>,
    expected_previous: Option<&str>,
    now: DateTime<Utc>,
) -> Result<HandoffPlan, AnalysisError> {
    validate_handoff_publication(publication)?;
    let append = if let Some(existing) = existing {
        if existing.summary != publication.summary
            || existing.metadata != publication.metadata
            || existing.cycle_id.as_deref() != Some(&artifact.cycle_id)
            || existing.contract_digest.as_deref() != Some(&cycle.contract_digest)
            || existing.artifact.as_ref() != Some(artifact)
            || existing.previous_handoff.as_deref() != expected_previous
            || (cycle.latest_handoff.as_deref() != Some(&artifact.attempt_id)
                && cycle.latest_handoff != existing.previous_handoff)
        {
            return Err(AnalysisError::InvalidState(
                "committed handoff cannot be changed or republished out of order".into(),
            ));
        }
        None
    } else {
        if cycle.latest_handoff.as_deref() != expected_previous {
            return Err(AnalysisError::InvalidState(
                "workflow publication revision conflict".into(),
            ));
        }
        Some(HandoffRecord {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            attempt_id: artifact.attempt_id.clone(),
            recorded_at: now,
            summary: publication.summary.clone(),
            metadata: publication.metadata.clone(),
            cycle_id: Some(artifact.cycle_id.clone()),
            contract_digest: Some(cycle.contract_digest.clone()),
            previous_handoff: cycle.latest_handoff.clone(),
            artifact: Some(artifact.clone()),
        })
    };
    Ok(HandoffPlan {
        append,
        advance_to: publication
            .metadata
            .as_ref()
            .map(|_| artifact.attempt_id.clone()),
    })
}

pub fn validate_handoff_binding(
    record: &HandoffRecord,
    cycle_id: &str,
    contract_digest: &str,
    attempt_id: &str,
) -> Result<(), AnalysisError> {
    if record.schema_version != WORKSPACE_SCHEMA_VERSION
        || record.attempt_id != attempt_id
        || record.cycle_id.as_deref() != Some(cycle_id)
        || record.contract_digest.as_deref() != Some(contract_digest)
    {
        return Err(AnalysisError::InvalidState(
            "latest handoff does not match its bound cycle and attempt".into(),
        ));
    }
    Ok(())
}

pub fn validate_handoff_artifact(
    record: &HandoffRecord,
    artifact: &WorkspaceArtifact,
    publication: &HandoffPublication,
    attempt_id: &str,
) -> Result<(), AnalysisError> {
    if artifact.attempt_id != attempt_id
        || publication.summary != record.summary
        || publication.metadata != record.metadata
    {
        return Err(AnalysisError::InvalidState(
            "committed handoff differs from its immutable artifact".into(),
        ));
    }
    Ok(())
}

pub fn workflow_handoff(record: HandoffRecord) -> Result<WorkflowHandoff, AnalysisError> {
    let metadata = record.metadata.filter(Value::is_object).ok_or_else(|| {
        AnalysisError::InvalidState("latest workflow handoff has no object metadata".into())
    })?;
    Ok(WorkflowHandoff {
        attempt_id: record.attempt_id,
        summary: record.summary,
        metadata,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CycleBudget {
    pub limits: CycleLimits,
    pub attempts: u32,
    pub execution_millis: u64,
    pub reservation: Option<AttemptReservation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptReservation {
    pub attempt_id: String,
    pub millis: u64,
}

pub fn validate_attempt_limits(
    limits: &CycleLimits,
    timeout: Duration,
) -> Result<(), AnalysisError> {
    if limits.max_attempts == 0 || limits.execution_seconds == 0 || timeout.is_zero() {
        return Err(AnalysisError::InvalidState(
            "cycle execution limits must be positive".into(),
        ));
    }
    Ok(())
}

pub fn plan_attempt_reservation(
    existing: Option<&CycleBudget>,
    attempt_id: &str,
    limits: &CycleLimits,
    timeout: Duration,
) -> Result<(CycleBudget, Duration), AnalysisError> {
    validate_attempt_limits(limits, timeout)?;
    let mut budget = existing.cloned().unwrap_or_else(|| CycleBudget {
        limits: limits.clone(),
        attempts: 0,
        execution_millis: 0,
        reservation: None,
    });
    if budget.limits != *limits {
        return Err(AnalysisError::InvalidState(
            "cycle limits differ from their durable configuration".into(),
        ));
    }
    let total = limits.execution_seconds.checked_mul(1_000).ok_or_else(|| {
        AnalysisError::InvalidState("cycle time budget overflows milliseconds".into())
    })?;
    if budget.attempts >= limits.max_attempts || budget.execution_millis >= total {
        return Err(AnalysisError::LimitExceeded(
            "attempt count or accumulated execution time exhausted".into(),
        ));
    }
    let requested = u64::try_from(timeout.as_millis()).map_err(workspace_error)?;
    let allowance = requested.min(total - budget.execution_millis);
    if allowance == 0 {
        return Err(AnalysisError::InvalidState(
            "attempt timeout is below one millisecond".into(),
        ));
    }
    // Replacing a reservation never refunds the previous (possibly crashed) attempt.
    budget.attempts += 1;
    budget.execution_millis += allowance;
    budget.reservation = Some(AttemptReservation {
        attempt_id: attempt_id.into(),
        millis: allowance,
    });
    Ok((budget, Duration::from_millis(allowance)))
}

pub fn plan_attempt_settlement(
    existing: &CycleBudget,
    attempt_id: &str,
    elapsed: Duration,
) -> Result<CycleBudget, AnalysisError> {
    let mut budget = existing.clone();
    let reservation = budget.reservation.take().ok_or_else(|| {
        AnalysisError::InvalidState("attempt has no execution reservation".into())
    })?;
    if reservation.attempt_id != attempt_id {
        return Err(AnalysisError::InvalidState(
            "execution reservation belongs to another attempt".into(),
        ));
    }
    let elapsed = u64::try_from(elapsed.as_millis())
        .map_err(workspace_error)?
        .min(reservation.millis);
    budget.execution_millis = budget
        .execution_millis
        .checked_sub(reservation.millis - elapsed)
        .ok_or_else(|| {
            AnalysisError::InvalidState("execution reservation exceeds the charged budget".into())
        })?;
    Ok(budget)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptFailure {
    pub schema_version: u32,
    pub attempt_id: String,
    pub recorded_at: DateTime<Utc>,
    pub error: String,
}

impl AttemptFailure {
    /// Failure recording itself neither charges nor refunds the reserved budget.
    pub fn new(attempt_id: &str, error: &str, recorded_at: DateTime<Utc>) -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            attempt_id: attempt_id.into(),
            recorded_at,
            error: error.into(),
        }
    }
}

/// Durable owner facts are not proof of liveness; adapters must verify live
/// ownership and retain their admission guard through the admission transaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRecord {
    pub schema_version: u32,
    #[serde(default)]
    pub epoch: u64,
    pub attempt_id: String,
    pub claimed_at: DateTime<Utc>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub cycle_id: Option<String>,
}

pub fn admission_owner_matches(
    claim: &ClaimRecord,
    ownership: &CandidateOwnership,
    cycle_id: &str,
) -> bool {
    claim.schema_version == WORKSPACE_SCHEMA_VERSION
        && claim.active
        && claim.epoch != 0
        && claim.epoch.to_string() == ownership.epoch
        && claim.attempt_id == ownership.attempt_id
        && claim.cycle_id.as_deref() == Some(cycle_id)
}

/// This matches the local capability fence check, not admission liveness.
pub fn validate_claim_epoch(record: &ClaimRecord, epoch: u64) -> Result<(), AnalysisError> {
    if record.schema_version != WORKSPACE_SCHEMA_VERSION || record.epoch != epoch {
        return Err(AnalysisError::workspace("workspace fencing epoch is stale"));
    }
    Ok(())
}

/// Sealed provenance uses the proposal's original attempt, not the current owner.
pub struct AdmissionIdentity<'a> {
    pub resource: &'a ContractResourceId,
    pub workspace_id: &'a str,
    pub cycle_id: &'a str,
    pub contract_digest: &'a str,
    pub attempt_id: &'a str,
    pub analysis_manifest_digest: &'a str,
}

pub fn admission_cycle_matches(
    admission: &AdmissionIdentity<'_>,
    bound_resource: &ContractResourceId,
    bound_workspace_id: &str,
    current_cycle_id: Option<&str>,
    cycle: &StoredCycle,
    contract: &AnalysisContract,
    has_terminal: bool,
) -> Result<bool, AnalysisError> {
    Ok(bound_resource == admission.resource
        && bound_workspace_id == admission.workspace_id
        && current_cycle_id == Some(admission.cycle_id)
        && cycle.status == CycleStatus::Active
        && cycle.workspace_id == admission.workspace_id
        && cycle.cycle_id == admission.cycle_id
        && cycle.contract_digest == admission.contract_digest
        && contract.resource()? == *admission.resource
        && contract.contract_digest == admission.contract_digest
        && !has_terminal)
}

pub fn admission_proposal_matches(
    admission: &AdmissionIdentity<'_>,
    proposal: &CandidateProposal,
) -> bool {
    proposal.attempt_id == admission.attempt_id
        && proposal.analysis_manifest_digest == admission.analysis_manifest_digest
}
