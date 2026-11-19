//! Product decisions over transactionally read state, independent of persistence.
//! Adapters retain locks/fences, atomic writes, CAS and rollback. Never apply a
//! plan after its input snapshot has changed; failed CAS must roll back all effects.

use chrono::{DateTime, FixedOffset};
use flaggo_contract::digests::{ExpressionCanonicalizer, is_sha256_digest};
use flaggo_contract_store::AcceptedContractVersion;

use crate::*;
use AnalysisCandidateResolutionOutcome as Outcome;
use ExecutableLifecycleState as State;

pub const AUTHORITY_POLICY_CAPABILITY: &str = "flaggo.authority.policy/v1";
type Result<T> = std::result::Result<T, ExecutableStoreError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleSnapshot {
    pub state: State,
    pub state_version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateActivationPlan {
    pub previous: ExecutableLifecycleToken,
    pub state_version: i64,
}

pub struct ResolutionSnapshot<'a> {
    pub candidate: LifecycleSnapshot,
    pub created_at: DateTime<FixedOffset>,
    pub current_contract: Option<&'a str>,
    /// Latest pending candidate for this resource/current contract, ordered by
    /// created_at, executable digest, workspace ID, cycle ID (all descending).
    pub newest_candidate: Option<&'a str>,
    pub active: Option<&'a ExecutableLifecycleToken>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionPlan {
    Yielded,
    AlreadyResolved {
        state_version: i64,
    },
    /// Supersede pending admissions of this resource whose contract is not current.
    SupersedeStale {
        state_version: i64,
    },
    /// Reject the selected candidate, then supersede all other pending admissions
    /// of the resource, including those for older contract versions.
    Reject {
        state_version: i64,
    },
    /// Deactivate the exact previous token, activate the selected candidate and
    /// supersede all other pending admissions of the resource atomically.
    Activate {
        previous: Option<ExecutableLifecycleToken>,
        state_version: i64,
    },
}

impl ResolutionPlan {
    pub fn result(
        &self,
        executable_digest: &str,
        superseded_candidate_count: u64,
    ) -> AnalysisCandidateResolutionResult {
        let (outcome, previous, version) = match self {
            Self::Yielded => (Outcome::Yielded, None, None),
            Self::AlreadyResolved { state_version } => {
                (Outcome::AlreadyResolved, None, Some(*state_version))
            }
            Self::SupersedeStale { state_version } => {
                (Outcome::Superseded, None, Some(*state_version))
            }
            Self::Reject { state_version } => (Outcome::Rejected, None, Some(*state_version)),
            Self::Activate {
                previous,
                state_version,
            } => (
                Outcome::Activated,
                previous
                    .as_ref()
                    .map(|value| value.executable_digest.clone()),
                Some(*state_version),
            ),
        };
        AnalysisCandidateResolutionResult {
            outcome,
            executable_digest: executable_digest.into(),
            previous_executable_digest: previous,
            state_version: version,
            superseded_candidate_count,
        }
    }
}

pub fn validate_publication(
    version: &AcceptedContractVersion,
    default: &StoredExecutable,
    authored: Option<&StoredExecutable>,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<()> {
    for executable in std::iter::once(default).chain(authored) {
        artifacts::validate_new_executable(executable, canonicalizer)?;
        if executable.executable.contract_digest != version.contract_digest {
            return Err(argument(
                "executable",
                "Publication artifacts must bind the accepted contract.",
            ));
        }
    }
    if !default.executable.rules.is_empty() {
        return Err(argument(
            "default",
            "A default executable must not contain rules.",
        ));
    }
    Ok(())
}

/// Ordered bootstrap activations. An existing learned executable is never reset
/// by redeployment; only a missing active or the default can advance bootstrap.
pub fn plan_bootstrap<'a>(
    default: &'a StoredExecutable,
    authored: Option<&'a StoredExecutable>,
    active_digest: Option<&str>,
) -> Vec<&'a StoredExecutable> {
    let mut steps = Vec::new();
    if active_digest.is_none() {
        steps.push(default);
    }
    if (active_digest.is_none() || active_digest == Some(default.executable_digest.as_str()))
        && let Some(authored) = authored
    {
        steps.push(authored);
    }
    steps
}

pub fn validate_admission(
    stored: &StoredExecutable,
    admission: &AnalysisCandidateAdmission,
) -> Result<()> {
    admission.resource.resource_key()?;
    if admission.contract_digest != stored.executable.contract_digest
        || admission.executable_digest != stored.executable_digest
        || admission.created_at != stored.created_at
    {
        return Err(argument(
            "admission",
            "Candidate admission identity must match the stored executable.",
        ));
    }
    if [
        &admission.workspace_id,
        &admission.cycle_id,
        &admission.attempt_id,
        &admission.resource.name,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || !is_sha256_digest(&admission.analysis_manifest_digest)
    {
        return Err(argument(
            "admission",
            "Candidate admission identity and a valid analysis manifest digest are required.",
        ));
    }
    Ok(())
}

/// `existing` is the admission for the resource/workspace/cycle key; `state`
/// belongs to its executable, or to the proposed executable when no row exists.
/// `None` means insert; `Some` is the immutable retry response.
pub fn plan_admission(
    admission: &AnalysisCandidateAdmission,
    existing: Option<&AnalysisCandidateAdmission>,
    state: Option<State>,
) -> Result<Option<AnalysisCandidateStoreResult>> {
    if let Some(existing) = existing {
        if !existing.identity_matches(admission) {
            return Err(ExecutableStoreError::AdmissionConflict {
                workspace_id: admission.workspace_id.clone(),
                cycle_id: admission.cycle_id.clone(),
            });
        }
        return Ok(Some(AnalysisCandidateStoreResult {
            executable_digest: existing.executable_digest.clone(),
            created_at: existing.created_at,
            lifecycle_state: state
                .ok_or_else(|| integrity("Candidate admission references a missing executable."))?,
            created: false,
        }));
    }
    if let Some(state) = state
        && state != State::Candidate
    {
        return Err(ExecutableStoreError::LifecycleConflict {
            executable_digest: admission.executable_digest.clone(),
            state,
        });
    }
    Ok(None)
}

pub fn require_current(
    resource: &flaggo_contract::ContractResourceId,
    expected: &str,
    current: Option<&str>,
) -> Result<()> {
    if current != Some(expected) {
        return Err(ExecutableStoreError::CurrentContractConflict {
            contract_name: resource.name.clone(),
            expected: expected.into(),
            actual: current.map(Into::into),
        });
    }
    Ok(())
}

pub fn plan_activation(
    contract: &str,
    digest: &str,
    selected_contract: &str,
    active: Option<&ExecutableLifecycleToken>,
    expected: Option<Option<&str>>,
) -> Result<ActivationResult> {
    binding(digest, contract, selected_contract)?;
    if let Some(expected) = expected
        && active.map(|value| value.executable_digest.as_str()) != expected
    {
        return Err(activation_conflict(expected, active));
    }
    let changed = active.is_none_or(|value| value.executable_digest != digest);
    Ok(ActivationResult {
        executable_digest: digest.into(),
        previous_executable_digest: active.map(|value| value.executable_digest.clone()),
        state_version: if changed {
            next_version(active.map_or(0, |value| value.state_version))?
        } else {
            active.expect("unchanged active executable").state_version
        },
        changed,
    })
}

pub fn validate_candidate_activation(command: &CandidateActivationCommand) -> Result<()> {
    command.resource.resource_key()?;
    for (parameter, value) in [
        ("command.ContractName", &command.resource.name),
        ("command.ContractDigest", &command.contract_digest),
        ("command.ExecutableDigest", &command.executable_digest),
    ] {
        if value.trim().is_empty() {
            return Err(argument(
                parameter,
                "The value cannot be an empty string or composed entirely of whitespace.",
            ));
        }
    }
    if command.expected_candidate_state_version < 0 {
        return Err(range(
            "command",
            "Expected Candidate state version cannot be negative.",
        ));
    }
    if command.expected_active_executable.state_version < 0 {
        return Err(range(
            "command",
            "Expected active state version cannot be negative.",
        ));
    }
    Ok(())
}

pub fn plan_candidate_activation(
    command: &CandidateActivationCommand,
    selected_contract: &str,
    candidate: LifecycleSnapshot,
    current: Option<&str>,
    active: Option<&ExecutableLifecycleToken>,
) -> Result<CandidateActivationPlan> {
    validate_candidate_activation(command)?;
    binding(
        &command.executable_digest,
        &command.contract_digest,
        selected_contract,
    )?;
    if candidate.state != State::Candidate
        || candidate.state_version != command.expected_candidate_state_version
    {
        return Err(candidate_conflict(command, candidate));
    }
    require_current(&command.resource, &command.contract_digest, current)?;
    if active != Some(&command.expected_active_executable) {
        return Err(activation_conflict(
            Some(&command.expected_active_executable.executable_digest),
            active,
        ));
    }
    let previous = active.expect("matching non-null lifecycle token").clone();
    Ok(CandidateActivationPlan {
        state_version: next_version(previous.state_version.max(candidate.state_version))?,
        previous,
    })
}

pub fn validate_resolution(command: &AnalysisCandidateResolutionCommand) -> Result<()> {
    if command.expected_candidate_state_version < 0 {
        return Err(range(
            "command",
            "Expected Candidate state version cannot be negative.",
        ));
    }
    if command
        .expected_active_executable
        .as_ref()
        .is_some_and(|value| value.state_version < 0)
    {
        return Err(range(
            "command",
            "Expected active executable state version cannot be negative.",
        ));
    }
    let admission = &command.admission;
    admission.resource.resource_key()?;
    if [
        &admission.workspace_id,
        &admission.cycle_id,
        &admission.resource.name,
        &admission.contract_digest,
        &admission.executable_digest,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(argument(
            "command",
            "Candidate resolution requires complete admission identity.",
        ));
    }
    Ok(())
}

pub fn plan_resolution(
    command: &AnalysisCandidateResolutionCommand,
    snapshot: &ResolutionSnapshot<'_>,
) -> Result<ResolutionPlan> {
    validate_resolution(command)?;
    let candidate = snapshot.candidate;
    if candidate.state != State::Candidate {
        return Ok(ResolutionPlan::AlreadyResolved {
            state_version: candidate.state_version,
        });
    }
    if candidate.state_version != command.expected_candidate_state_version
        || snapshot.created_at != command.admission.created_at
    {
        return Ok(ResolutionPlan::Yielded);
    }
    if snapshot.current_contract != Some(command.admission.contract_digest.as_str()) {
        return Ok(ResolutionPlan::SupersedeStale {
            state_version: next_version(candidate.state_version)?,
        });
    }
    if snapshot.newest_candidate != Some(command.admission.executable_digest.as_str()) {
        return Ok(ResolutionPlan::Yielded);
    }
    if matches!(command.action, AnalysisCandidateResolutionAction::Reject) {
        return Ok(ResolutionPlan::Reject {
            state_version: next_version(candidate.state_version)?,
        });
    }
    if snapshot.active != command.expected_active_executable.as_ref() {
        return Ok(ResolutionPlan::Yielded);
    }
    Ok(ResolutionPlan::Activate {
        previous: snapshot.active.cloned(),
        state_version: next_version(
            snapshot
                .active
                .map_or(candidate.state_version, |value| value.state_version),
        )?,
    })
}

pub fn parse_state(value: &str) -> Result<State> {
    match value {
        "candidate" => Ok(State::Candidate),
        "active" => Ok(State::Active),
        "inactive" => Ok(State::Inactive),
        invalid => Err(integrity(format!(
            "Unknown executable lifecycle state '{invalid}'."
        ))),
    }
}

pub fn next_version(version: i64) -> Result<i64> {
    version
        .checked_add(1)
        .ok_or_else(|| integrity("Executable lifecycle state version is exhausted."))
}

fn binding(digest: &str, expected: &str, actual: &str) -> Result<()> {
    if expected != actual {
        return Err(ExecutableStoreError::ContractMismatch {
            executable_digest: digest.into(),
            expected_contract_digest: expected.into(),
            actual_contract_digest: actual.into(),
        });
    }
    Ok(())
}

pub fn candidate_conflict(
    command: &CandidateActivationCommand,
    candidate: LifecycleSnapshot,
) -> ExecutableStoreError {
    ExecutableStoreError::CandidateConflict {
        executable_digest: command.executable_digest.clone(),
        expected: command.expected_candidate_state_version,
        state: candidate.state,
        actual: candidate.state_version,
    }
}

pub fn activation_conflict(
    expected: Option<&str>,
    active: Option<&ExecutableLifecycleToken>,
) -> ExecutableStoreError {
    ExecutableStoreError::ActivationConflict {
        expected: expected.map(Into::into),
        actual: active.map(|value| value.executable_digest.clone()),
    }
}

pub(crate) fn argument(
    parameter: &'static str,
    message: impl Into<String>,
) -> ExecutableStoreError {
    ExecutableStoreError::InvalidArgument {
        parameter,
        message: message.into(),
        out_of_range: false,
    }
}

pub(crate) fn range(parameter: &'static str, message: impl Into<String>) -> ExecutableStoreError {
    ExecutableStoreError::InvalidArgument {
        parameter,
        message: message.into(),
        out_of_range: true,
    }
}

pub(crate) fn integrity(message: impl Into<String>) -> ExecutableStoreError {
    ExecutableStoreError::Integrity(message.into())
}
