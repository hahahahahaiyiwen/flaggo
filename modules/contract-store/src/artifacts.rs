//! Persisted accepted-contract validation shared by all authority adapters.

use flaggo_contract::{
    AuthorityScope, ContractResourceId, DecisionContract, digests::contract_digest, strict_json,
};

use crate::ContractStoreError;

/// Validate raw persisted bytes against their authoritative resource key and
/// digest. Timestamp decoding and row access remain the adapter's responsibility.
pub fn decode_persisted_contract(
    resource_key: &str,
    digest: &str,
    payload: &[u8],
) -> Result<DecisionContract, ContractStoreError> {
    let contract: DecisionContract = strict_json::deserialize(payload)?;
    let actual = ContractResourceId::try_from(&contract)
        .and_then(|resource| resource.resource_key())
        .map_err(|error| {
            ContractStoreError::Integrity(format!(
                "Stored contract resource identity is invalid: {error}"
            ))
        })?;
    if actual != resource_key {
        return Err(ContractStoreError::Integrity(format!(
            "Stored contract resource '{actual}' does not match key '{resource_key}'.",
        )));
    }
    validate_scope(&contract.authority)?;
    let computed = contract_digest(&contract, None)?;
    if computed != digest {
        return Err(ContractStoreError::Integrity(format!(
            "Stored contract digest '{digest}' does not match payload '{computed}'.",
        )));
    }
    Ok(contract)
}

pub(crate) fn validate_scope(scope: &AuthorityScope) -> Result<(), ContractStoreError> {
    for (parameter, value) in [
        ("scope.Tenant", &scope.tenant),
        ("scope.Application", &scope.application),
        ("scope.Environment", &scope.environment),
    ] {
        let Some(value) = value else {
            return Err(ContractStoreError::NullArgument(parameter));
        };
        if value.trim().is_empty() {
            return Err(ContractStoreError::InvalidArgument {
                parameter,
                message: "The value cannot be an empty string or composed entirely of whitespace."
                    .into(),
            });
        }
    }
    Ok(())
}
