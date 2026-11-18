//! Semantic authority contract v2 for explicitly scoped v4 resources.
//! Mutations commit their preconditions and complete outcomes atomically.
use async_trait::async_trait;
use flaggo_contract::{ContractResourceId, ResourceScope};
use flaggo_contract_store::{AcceptedContractVersion, ContractStoreError, ContractVersionPage};

use crate::*;

#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    #[error(transparent)]
    Contract(#[from] ContractStoreError),
    #[error(transparent)]
    Executable(#[from] ExecutableStoreError),
}

pub type AuthorityResult<T> = Result<T, AuthorityError>;

pub struct ReadyPublication {
    pub accepted: AcceptedContractVersion,
    pub active: StoredExecutable,
    pub created: bool,
    pub bootstrap_activations: Vec<String>,
}

/// One implementation must own the complete authority transaction domain.
/// A successful publication exposes no accepted version without activation.
/// Admission checks currentness before retry lookup, preserving stale ordering.
/// Active reads must observe acknowledged activation or a later committed one.
#[async_trait]
pub trait ContractAuthority: Send + Sync {
    async fn publish_ready(
        &self,
        version: &AcceptedContractVersion,
        default: &StoredExecutable,
        authored: Option<&StoredExecutable>,
    ) -> AuthorityResult<ReadyPublication>;

    async fn admit_current(
        &self,
        executable: &StoredExecutable,
        admission: &AnalysisCandidateAdmission,
    ) -> AuthorityResult<AnalysisCandidateStoreResult>;

    async fn get_contract(
        &self,
        resource: &ContractResourceId,
        digest: &str,
    ) -> AuthorityResult<Option<AcceptedContractVersion>>;
    async fn get_current_contract(
        &self,
        resource: &ContractResourceId,
    ) -> AuthorityResult<Option<AcceptedContractVersion>>;
    async fn list_current_contracts(
        &self,
        scope: &ResourceScope,
    ) -> AuthorityResult<Vec<AcceptedContractVersion>>;
    async fn list_contract_versions(
        &self,
        resource: &ContractResourceId,
        limit: i32,
        cursor: Option<&str>,
    ) -> AuthorityResult<ContractVersionPage>;
    async fn get_executable(&self, digest: &str) -> AuthorityResult<Option<StoredExecutable>>;
    async fn get_active_executable(
        &self,
        digest: &str,
    ) -> AuthorityResult<Option<StoredExecutable>>;
    async fn list_executables(
        &self,
        digest: &str,
        limit: i32,
        cursor: Option<&str>,
        history: bool,
    ) -> AuthorityResult<ExecutablePage>;
    async fn activate_candidate(
        &self,
        command: &CandidateActivationCommand,
    ) -> AuthorityResult<CandidateActivationResult>;
    async fn list_pending(
        &self,
        limit: i32,
        after: Option<&str>,
    ) -> AuthorityResult<AnalysisCandidateActivationPage>;
    async fn resolve(
        &self,
        command: &AnalysisCandidateResolutionCommand,
    ) -> AuthorityResult<AnalysisCandidateResolutionResult>;
    async fn readiness(&self) -> AuthorityResult<(bool, bool)>;
}
