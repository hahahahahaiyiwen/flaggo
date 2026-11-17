use super::*;
use async_trait::async_trait;
use flaggo_contract_store::{
    AcceptedContractVersion, ContractStoreWriteResult, ContractVersionPage,
    SqliteContractVersionStore,
};

/// Local authority adapter. Both logical stores share the same pool and transactions.
#[derive(Clone)]
pub struct SqliteContractAuthority {
    contracts: SqliteContractVersionStore,
    executables: SqliteExecutableStore,
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
        })
    }

    pub async fn initialize(&self) -> AuthorityResult<()> {
        self.contracts.initialize(None).await?;
        self.executables.initialize(None).await?;
        Ok(())
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
        let name = &version.contract.name;
        let digest = &version.contract_digest;
        let mut tx = self.executables.pool.begin_with("BEGIN IMMEDIATE").await?;
        let created = self.contracts.put_in(&mut tx, version, None).await?
            == ContractStoreWriteResult::Created;
        let accepted = self
            .contracts
            .get_in(&mut tx, name, digest, None)
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
            .set_current_in(&mut tx, name, digest, None)
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
    ) -> AuthorityResult<AnalysisCandidateStoreResult> {
        let mut tx = self.executables.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: Option<String> = sqlx::query_scalar(
            "SELECT contract_digest FROM decision_contract_current WHERE contract_name = ?",
        )
        .bind(&admission.contract_name)
        .fetch_optional(&mut *tx)
        .await?;
        if current.as_deref() != Some(&admission.contract_digest) {
            return Err(ExecutableStoreError::CurrentContractConflict {
                contract_name: admission.contract_name.clone(),
                expected: admission.contract_digest.clone(),
                actual: current,
            }
            .into());
        }
        let result = self
            .executables
            .put_analysis_candidate_in(&mut tx, executable, admission, None)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn get_contract(
        &self,
        name: &str,
        digest: &str,
    ) -> AuthorityResult<Option<AcceptedContractVersion>> {
        Ok(self.contracts.get(name, digest, None).await?)
    }

    async fn get_current_contract(
        &self,
        name: &str,
    ) -> AuthorityResult<Option<AcceptedContractVersion>> {
        Ok(self.contracts.get_current(name, None).await?)
    }

    async fn list_current_contracts(&self) -> AuthorityResult<Vec<AcceptedContractVersion>> {
        Ok(self.contracts.list_all_current(None).await?)
    }

    async fn list_contract_versions(
        &self,
        name: &str,
        limit: i32,
        cursor: Option<&str>,
    ) -> AuthorityResult<ContractVersionPage> {
        Ok(self.contracts.list(name, limit, cursor, None).await?)
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
