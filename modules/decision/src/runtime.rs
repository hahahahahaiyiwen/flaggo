use crate::{Cancellation, DecisionError};
use flaggo_contract::digests::is_sha256_digest;
use flaggo_contract::validation::{is_decision_name, validate_runtime_input};
use flaggo_contract::{
    ContractError, DecisionContract, DecisionExecutable, RuntimeDecision, RuntimeInput,
};
use flaggo_expression::{
    CheckedDecisionExecutable, CompiledDecisionExecutable, ExecutableCompiler,
    ExecutableMaterializationError,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

pub const MAXIMUM_COMPILED_EXECUTABLES: usize = 1024;

pub struct ContractVersion {
    pub contract_digest: String,
    pub contract: DecisionContract,
}

pub struct ActiveExecutable {
    pub executable_digest: String,
    pub executable: DecisionExecutable,
    pub checked_executable: CheckedDecisionExecutable,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreReadError {
    #[error(transparent)]
    Integrity(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Dependency(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Timeout(Box<dyn std::error::Error + Send + Sync>),
    #[error("The operation was canceled.")]
    Cancelled,
    #[error(transparent)]
    InvalidArgument(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Native(Box<dyn std::error::Error + Send + Sync>),
}

pub trait ContractVersionReader: Send + Sync {
    fn get_contract(
        &self,
        contract_name: &str,
        contract_digest: &str,
        cancellation: Cancellation,
    ) -> impl Future<Output = Result<Option<Arc<ContractVersion>>, StoreReadError>> + Send;
}

pub trait ActiveExecutableReader: Send + Sync {
    fn get_active(
        &self,
        contract_digest: &str,
        cancellation: Cancellation,
    ) -> impl Future<Output = Result<Option<Arc<ActiveExecutable>>, StoreReadError>> + Send;
}

type Materialized = Result<Arc<CompiledDecisionExecutable>, Arc<ExecutableMaterializationError>>;
type Entry = Arc<OnceLock<Materialized>>;
type CacheKey = (String, String);

#[derive(Default)]
struct Cache {
    entries: HashMap<CacheKey, Entry>,
    order: VecDeque<(CacheKey, Entry)>,
}

pub struct DecisionRuntime<C, E> {
    contracts: C,
    executables: E,
    compiler: ExecutableCompiler,
    cache: Mutex<Cache>,
}

impl<C: ContractVersionReader, E: ActiveExecutableReader> DecisionRuntime<C, E> {
    pub fn new(contracts: C, executables: E) -> Self {
        Self {
            contracts,
            executables,
            compiler: ExecutableCompiler::default(),
            cache: Mutex::new(Cache::default()),
        }
    }

    pub async fn decide(
        &self,
        contract_name: &str,
        contract_digest: &str,
        input: &RuntimeInput,
        cancellation: Cancellation,
    ) -> Result<RuntimeDecision, DecisionError> {
        if !is_decision_name(contract_name) {
            return Err(DecisionError::InvalidIdentity(
                "contractName is not a valid decision name.",
            ));
        }
        if !is_sha256_digest(contract_digest) {
            return Err(DecisionError::InvalidIdentity(
                "contractDigest must be a lowercase sha256 digest.",
            ));
        }
        let version = self
            .contracts
            .get_contract(contract_name, contract_digest, cancellation.clone())
            .await
            .map_err(|error| {
                store_error(
                    error,
                    "The stored contract version failed integrity validation.",
                )
            })?
            .ok_or_else(|| DecisionError::ContractNotFound {
                contract_name: contract_name.into(),
                contract_digest: contract_digest.into(),
            })?;
        let mut issues = Vec::new();
        validate_runtime_input(&version.contract, input, &mut issues)?;
        if !issues.is_empty() {
            return Err(DecisionError::InputValidation(issues));
        }
        let active = self
            .executables
            .get_active(contract_digest, cancellation.clone())
            .await
            .map_err(|error| {
                store_error(error, "The active executable failed integrity validation.")
            })?
            .ok_or_else(|| DecisionError::ActiveExecutableNotFound(contract_digest.into()))?;
        if active.executable.contract_digest != contract_digest {
            return Err(DecisionError::Integrity {
                message: "The active executable is bound to a different contract digest.",
                source: None,
            });
        }
        let compiled = self.materialize(&version, &active)?;
        crate::evaluate(&version.contract, &compiled, input, cancellation)
    }

    fn materialize(
        &self,
        version: &ContractVersion,
        active: &ActiveExecutable,
    ) -> Result<Arc<CompiledDecisionExecutable>, DecisionError> {
        let representation = active
            .checked_executable
            .rules
            .iter()
            .map(|rule| {
                let rule = rule
                    .as_ref()
                    .ok_or_else(|| ContractError::InvalidModel("null checked rule".into()))?;
                Ok(format!(
                    "{}:{}:{}",
                    rule.name,
                    rule.predicate.checked_ast_digest,
                    rule.result
                        .as_ref()
                        .map_or("-", |result| result.checked_ast_digest.as_str())
                ))
            })
            .collect::<Result<Vec<_>, ContractError>>()?
            .join("|");
        let key = (active.executable_digest.clone(), representation);
        let (entry, inserted) = {
            let mut cache = self
                .cache
                .lock()
                .expect("compiled executable cache poisoned");
            match cache.entries.get(&key) {
                Some(entry) => (Arc::clone(entry), false),
                None => {
                    let entry = Arc::new(OnceLock::new());
                    cache.entries.insert(key.clone(), Arc::clone(&entry));
                    (entry, true)
                }
            }
        };
        let result = entry.get_or_init(|| {
            self.compiler
                .materialize(
                    &version.contract,
                    &version.contract_digest,
                    &active.executable,
                    &active.executable_digest,
                    &active.checked_executable,
                )
                .map(Arc::new)
                .map_err(Arc::new)
        });
        match result {
            Ok(compiled) => {
                if inserted {
                    let mut cache = self
                        .cache
                        .lock()
                        .expect("compiled executable cache poisoned");
                    cache.order.push_back((key, Arc::clone(&entry)));
                    while cache.entries.len() > MAXIMUM_COMPILED_EXECUTABLES {
                        let Some((key, oldest)) = cache.order.pop_front() else {
                            break;
                        };
                        remove_entry(&mut cache, &key, &oldest);
                    }
                }
                Ok(Arc::clone(compiled))
            }
            Err(error) => {
                if matches!(
                    error.as_ref(),
                    ExecutableMaterializationError::Invalid { .. }
                ) {
                    let mut cache = self
                        .cache
                        .lock()
                        .expect("compiled executable cache poisoned");
                    remove_entry(&mut cache, &key, &entry);
                    return Err(DecisionError::Integrity {
                        message: "The active executable could not be materialized.",
                        source: Some(Box::new(Arc::clone(error))),
                    });
                }
                Err(DecisionError::MaterializationNative(Arc::clone(error)))
            }
        }
    }
}

fn remove_entry(cache: &mut Cache, key: &CacheKey, entry: &Entry) {
    if cache
        .entries
        .get(key)
        .is_some_and(|current| Arc::ptr_eq(current, entry))
    {
        cache.entries.remove(key);
    }
}

fn store_error(error: StoreReadError, message: &'static str) -> DecisionError {
    match error {
        StoreReadError::Integrity(source) => DecisionError::Integrity {
            message,
            source: Some(source),
        },
        error => DecisionError::Store(error),
    }
}

#[cfg(test)]
mod tests;
