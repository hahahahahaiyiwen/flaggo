//! Shared artifact validation. Adapters decode their own rows, but must use these
//! checks before returning persisted executables, including on cached reads.

use flaggo_contract::{
    DecisionExecutable, JsonValue, digests::ExpressionCanonicalizer, digests::executable_digest,
    strict_json,
};
use flaggo_expression::CheckedDecisionExecutable;

use crate::policy::{argument, integrity};
use crate::{ExecutableLifecycleState, ExecutableStoreError, StoredExecutable, checked};

type Result<T> = std::result::Result<T, ExecutableStoreError>;

pub struct ExecutableArtifacts {
    pub executable: DecisionExecutable,
    pub checked_executable: CheckedDecisionExecutable,
    pub provenance: Option<JsonValue>,
}

pub fn decode_persisted_artifacts(
    executable_digest_value: &str,
    contract_digest: &str,
    executable_json: &[u8],
    checked_executable_json: &[u8],
    provenance_json: Option<&[u8]>,
) -> Result<ExecutableArtifacts> {
    let executable: DecisionExecutable = strict_json::deserialize(executable_json)?;
    if executable.contract_digest != contract_digest {
        return Err(integrity(format!(
            "Stored executable contract digest '{contract_digest}' does not match payload '{}'.",
            executable.contract_digest
        )));
    }
    let computed = executable_digest(&executable, None)?;
    if computed != executable_digest_value {
        return Err(integrity(format!(
            "Stored executable digest '{executable_digest_value}' does not match payload '{computed}'."
        )));
    }
    // Checked DTOs retain their historical parser, not StrictJson token limits.
    let checked_executable = checked::read(checked_executable_json)?
        .ok_or_else(|| integrity("Stored checked executable representation is null."))?;
    if !rules_align(&executable, &checked_executable)? {
        return Err(integrity(
            "Stored checked executable rules do not match executable rules.",
        ));
    }
    Ok(ExecutableArtifacts {
        executable,
        checked_executable,
        provenance: provenance_json.map(strict_json::deserialize).transpose()?,
    })
}

pub fn validate_new_executable(
    stored: &StoredExecutable,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<()> {
    if stored.state != ExecutableLifecycleState::Candidate
        || stored.state_version != 0
        || stored.activated_at.is_some()
    {
        return Err(argument(
            "stored",
            "New executables must enter the store as version-zero candidates.",
        ));
    }
    if !rules_align(&stored.executable, &stored.checked_executable)? {
        return Err(argument(
            "stored",
            "Checked executable rules must align with executable rules.",
        ));
    }
    let digest = executable_digest(&stored.executable, canonicalizer)?;
    if digest != stored.executable_digest {
        return Err(argument(
            "stored",
            format!(
                "Executable digest mismatch: expected {digest}, received {}.",
                stored.executable_digest
            ),
        ));
    }
    if canonicalizer.is_some()
        && executable_digest(&stored.executable, None)? != stored.executable_digest
    {
        return Err(argument(
            "stored",
            "Executable expressions must use their canonical stored form.",
        ));
    }
    Ok(())
}

fn rules_align(
    executable: &DecisionExecutable,
    checked: &CheckedDecisionExecutable,
) -> Result<bool> {
    if executable.rules.len() != checked.rules.len() {
        return Ok(false);
    }
    for (rule, checked) in executable.rules.iter().zip(&checked.rules) {
        let rule = rule.as_ref().ok_or_else(|| {
            flaggo_contract::ContractError::InvalidModel("Null executable rule.".into())
        })?;
        let checked = checked.as_ref().ok_or_else(|| {
            flaggo_contract::ContractError::InvalidModel("Null checked executable rule.".into())
        })?;
        if rule.name != checked.name {
            return Ok(false);
        }
    }
    Ok(true)
}
