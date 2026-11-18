use super::*;
use async_trait::async_trait;
use flaggo_contract::{ContractResourceId, ResourceScope};
use flaggo_contract_store::{
    AcceptedContractVersion, ContractStoreWriteResult, ContractVersionPage,
    SqliteContractVersionStore,
};

/// Local authority adapter. Both logical stores share the same pool and transactions.
#[derive(Clone)]
pub struct SqliteContractAuthority {
    contracts: SqliteContractVersionStore,
    executables: SqliteExecutableStore,
    admission_fence: Option<Arc<dyn AdmissionFence>>,
}

impl SqliteContractAuthority {
    pub async fn open(
        path: impl AsRef<Path>,
        canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
    ) -> AuthorityResult<Self> {
        let executables = SqliteExecutableStore::open(path, canonicalizer.clone()).await?;
        let contracts =
            SqliteContractVersionStore::from_pool(executables.pool.clone(), canonicalizer);
        Ok(Self {
            contracts,
            executables,
            admission_fence: None,
        })
    }

    /// Admission fails closed until a composition supplies its authoritative fence.
    pub fn with_admission_fence(mut self, fence: Arc<dyn AdmissionFence>) -> Self {
        self.admission_fence = Some(fence);
        self
    }

    pub async fn initialize(&self) -> AuthorityResult<()> {
        self.contracts.initialize(None).await?;
        self.executables.initialize(None).await?;
        Ok(())
    }

    /// Explicit, atomic local migration; no startup reset or silent namespace reinterpretation.
    pub async fn migrate_scoped_resources(&self) -> AuthorityResult<bool> {
        let mut tx = self.executables.pool.begin_with("BEGIN IMMEDIATE").await?;
        let contracts = self.contracts.migrate_v3_to_v4_in(&mut tx).await?;
        let executables = self
            .executables
            .migrate_scoped_resources_in(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(contracts || executables)
    }

    pub fn contracts(&self) -> &SqliteContractVersionStore {
        &self.contracts
    }

    pub fn executables(&self) -> &SqliteExecutableStore {
        &self.executables
    }

    pub async fn close(&self) {
        self.executables.close().await;
    }
}

impl From<sqlx::Error> for AuthorityError {
    fn from(error: sqlx::Error) -> Self {
        Self::Executable(error.into())
    }
}

async fn active_in(
    connection: &mut SqliteConnection,
    digest: &str,
) -> Result<Option<StoredExecutable>> {
    sqlx::query("SELECT executable_digest, contract_digest, executable_json,
        checked_executable_json, provenance_json, lifecycle_state, state_version, created_at, activated_at
        FROM decision_executables WHERE contract_digest = ? AND lifecycle_state = 'active'")
        .bind(digest).fetch_optional(connection).await?
        .map(|row| read_executable(&row)).transpose()
}

#[async_trait]
impl ContractAuthority for SqliteContractAuthority {
    async fn publish_ready(
        &self,
        version: &AcceptedContractVersion,
        default: &StoredExecutable,
        authored: Option<&StoredExecutable>,
    ) -> AuthorityResult<ReadyPublication> {
        for executable in std::iter::once(default).chain(authored) {
            self.executables.validate_stored(executable)?;
            if executable.executable.contract_digest != version.contract_digest {
                return Err(argument(
                    "executable",
                    "Publication artifacts must bind the accepted contract.",
                )
                .into());
            }
        }
        if !default.executable.rules.is_empty() {
            return Err(argument("default", "A default executable must not contain rules.").into());
        }
        let resource = ContractResourceId::try_from(&version.contract)
            .map_err(flaggo_contract_store::ContractStoreError::from)?;
        let digest = &version.contract_digest;
        let mut tx = self.executables.pool.begin_with("BEGIN IMMEDIATE").await?;
        let created = self.contracts.put_in(&mut tx, version, None).await?
            == ContractStoreWriteResult::Created;
        let accepted = self
            .contracts
            .get_in(&mut tx, &resource, digest, None)
            .await?
            .ok_or_else(|| integrity("Published contract is missing from its transaction."))?;
        let mut active = active_in(&mut tx, digest).await?;
        let mut bootstrap_activations = Vec::new();
        if active.is_none() {
            insert_candidate(&mut tx, default).await?;
            self.executables
                .activate_in(
                    &mut tx,
                    digest,
                    &default.executable_digest,
                    Some(None),
                    None,
                )
                .await?;
            bootstrap_activations.push(default.executable_digest.clone());
            active = active_in(&mut tx, digest).await?;
        }
        if let Some(authored) = authored
            && active
                .as_ref()
                .is_some_and(|active| active.executable_digest == default.executable_digest)
        {
            insert_candidate(&mut tx, authored).await?;
            self.executables
                .activate_in(
                    &mut tx,
                    digest,
                    &authored.executable_digest,
                    Some(Some(&default.executable_digest)),
                    None,
                )
                .await?;
            bootstrap_activations.push(authored.executable_digest.clone());
            active = active_in(&mut tx, digest).await?;
        }
        let active =
            active.ok_or_else(|| integrity("Publication did not establish runtime authority."))?;
        self.contracts
            .set_current_in(&mut tx, &resource, digest, None)
            .await?;
        tx.commit().await?;
        Ok(ReadyPublication {
            accepted,
            active,
            created,
            bootstrap_activations,
        })
    }

    async fn admit_current(
        &self,
        executable: &StoredExecutable,
        admission: &AnalysisCandidateAdmission,
        ownership: &flaggo_contract::CandidateOwnership,
    ) -> AuthorityResult<AnalysisCandidateStoreResult> {
        let authority = self.clone();
        let executable = executable.clone();
        let admission = admission.clone();
        let ownership = ownership.clone();
        // HTTP cancellation must not release the ownership gate while SQLx may
        // still be completing an already-issued commit on its worker thread.
        tokio::spawn(async move {
            let _ownership_guard = authority
                .admission_fence
                .as_ref()
                .ok_or(ExecutableStoreError::OwnershipConflict)?
                .acquire(&admission, &ownership)?;
            let mut tx = authority
                .executables
                .pool
                .begin_with("BEGIN IMMEDIATE")
                .await?;
            let current: Option<String> = sqlx::query_scalar(
                "SELECT contract_digest FROM decision_contract_current WHERE resource_key = ?",
            )
            .bind(
                admission
                    .resource
                    .resource_key()
                    .map_err(ExecutableStoreError::from)?,
            )
            .fetch_optional(&mut *tx)
            .await?;
            if current.as_deref() != Some(&admission.contract_digest) {
                return Err(ExecutableStoreError::CurrentContractConflict {
                    contract_name: admission.resource.name.clone(),
                    expected: admission.contract_digest.clone(),
                    actual: current,
                }
                .into());
            }
            let result = authority
                .executables
                .put_analysis_candidate_in(&mut tx, &executable, &admission, None)
                .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|error| ExecutableStoreError::Dependency(Box::new(error)))?
    }

    async fn get_contract(
        &self,
        resource: &ContractResourceId,
        digest: &str,
    ) -> AuthorityResult<Option<AcceptedContractVersion>> {
        Ok(self.contracts.get(resource, digest, None).await?)
    }

    async fn get_current_contract(
        &self,
        resource: &ContractResourceId,
    ) -> AuthorityResult<Option<AcceptedContractVersion>> {
        Ok(self.contracts.get_current(resource, None).await?)
    }

    async fn list_current_contracts(
        &self,
        scope: &ResourceScope,
    ) -> AuthorityResult<Vec<AcceptedContractVersion>> {
        Ok(self.contracts.list_all_current(scope, None).await?)
    }

    async fn list_contract_versions(
        &self,
        resource: &ContractResourceId,
        limit: i32,
        cursor: Option<&str>,
    ) -> AuthorityResult<ContractVersionPage> {
        Ok(self.contracts.list(resource, limit, cursor, None).await?)
    }

    async fn get_executable(&self, digest: &str) -> AuthorityResult<Option<StoredExecutable>> {
        Ok(self.executables.get(digest, None).await?)
    }

    async fn get_active_executable(
        &self,
        digest: &str,
    ) -> AuthorityResult<Option<StoredExecutable>> {
        Ok(self.executables.get_active(digest, None).await?)
    }

    async fn list_executables(
        &self,
        digest: &str,
        limit: i32,
        cursor: Option<&str>,
        history: bool,
    ) -> AuthorityResult<ExecutablePage> {
        Ok(if history {
            self.executables
                .list_executables(digest, limit, cursor, None)
                .await?
        } else {
            self.executables
                .list_candidates(digest, limit, cursor, None)
                .await?
        })
    }

    async fn activate_candidate(
        &self,
        command: &CandidateActivationCommand,
    ) -> AuthorityResult<CandidateActivationResult> {
        Ok(self.executables.activate_candidate(command, None).await?)
    }

    async fn list_pending(
        &self,
        limit: i32,
        after: Option<&str>,
    ) -> AuthorityResult<AnalysisCandidateActivationPage> {
        Ok(self.executables.list_pending(limit, after, None).await?)
    }

    async fn resolve(
        &self,
        command: &AnalysisCandidateResolutionCommand,
    ) -> AuthorityResult<AnalysisCandidateResolutionResult> {
        Ok(self.executables.resolve(command, None).await?)
    }

    async fn readiness(&self) -> AuthorityResult<(bool, bool)> {
        Ok((
            self.contracts.is_available(None).await?,
            self.executables.is_available(None).await?,
        ))
    }
}
