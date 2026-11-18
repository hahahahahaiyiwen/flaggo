mod authority;
#[cfg(feature = "sqlite")]
mod checked;
mod models;
#[cfg(feature = "sqlite")]
mod schema;

pub use authority::*;
pub use models::*;
#[cfg(feature = "sqlite")]
pub use schema::SCHEMA_VERSION;

#[cfg(feature = "sqlite")]
use chrono::{DateTime, FixedOffset, Utc};
use flaggo_contract::ContractError;
#[cfg(feature = "sqlite")]
use flaggo_contract::digests::{ExpressionCanonicalizer, executable_digest, is_sha256_digest};
#[cfg(feature = "sqlite")]
use flaggo_contract::{ContractResourceId, DecisionExecutable, strict_json};
#[cfg(feature = "sqlite")]
use flaggo_contract_store::cursor;
use flaggo_contract_store::timestamp;
#[cfg(feature = "sqlite")]
use flaggo_decision::{ActiveExecutable, ActiveExecutableReader, Cancellation, StoreReadError};
#[cfg(feature = "sqlite")]
use flaggo_expression::CheckedDecisionExecutable;
#[cfg(feature = "sqlite")]
use sqlx::{
    Row, SqliteConnection, SqlitePool, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
};
#[cfg(feature = "sqlite")]
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum ExecutableStoreError {
    #[error("{message} (Parameter '{parameter}')")]
    InvalidArgument {
        parameter: &'static str,
        message: String,
        out_of_range: bool,
    },
    #[error(
        "Unsupported executable-store schema version {0}; expected 6. Migrate scoped resources explicitly."
    )]
    UnsupportedSchema(i64),
    #[error("Executable '{0}' does not exist.")]
    NotFound(String),
    #[error(
        "Executable '{executable_digest}' binds contract '{actual_contract_digest}', not '{expected_contract_digest}'."
    )]
    ContractMismatch {
        executable_digest: String,
        expected_contract_digest: String,
        actual_contract_digest: String,
    },
    #[error("Active executable changed: expected '{}', found '{}'.", .expected.as_deref().unwrap_or("<none>"), .actual.as_deref().unwrap_or("<none>"))]
    ActivationConflict {
        expected: Option<String>,
        actual: Option<String>,
    },
    #[error("Candidate '{executable_digest}' changed: expected candidate state version '{expected}', found '{}' state version '{actual}'.", .state.label())]
    CandidateConflict {
        executable_digest: String,
        expected: i64,
        state: ExecutableLifecycleState,
        actual: i64,
    },
    #[error("Contract '{contract_name}' current digest changed: expected '{expected}', found '{}'.", .actual.as_deref().unwrap_or("<none>"))]
    CurrentContractConflict {
        contract_name: String,
        expected: String,
        actual: Option<String>,
    },
    #[error(
        "Workspace '{workspace_id}' cycle '{cycle_id}' already admitted a different Candidate."
    )]
    AdmissionConflict {
        workspace_id: String,
        cycle_id: String,
    },
    #[error("Executable '{executable_digest}' is already in lifecycle state '{}' and cannot be admitted as an inactive Candidate.", .state.label())]
    LifecycleConflict {
        executable_digest: String,
        state: ExecutableLifecycleState,
    },
    #[error("{0}")]
    Integrity(String),
    #[error("The operation was canceled.")]
    Cancelled,
    #[error(transparent)]
    Timestamp(#[from] timestamp::TimestampError),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Dependency(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Timeout(Box<dyn std::error::Error + Send + Sync>),
    #[cfg(feature = "sqlite")]
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl ExecutableStoreError {
    pub fn is_timeout(&self) -> bool {
        #[cfg(feature = "sqlite")]
        if matches!(self, Self::Database(sqlx::Error::PoolTimedOut)) {
            return true;
        }
        matches!(self, Self::Timeout(_))
    }

    pub fn is_dependency(&self) -> bool {
        #[cfg(feature = "sqlite")]
        if matches!(self, Self::Database(_)) {
            return true;
        }
        matches!(self, Self::Dependency(_) | Self::Timeout(_))
    }
}

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteExecutableStore;
#[cfg(feature = "sqlite")]
pub use sqlite::authority::SqliteContractAuthority;

#[cfg(feature = "sqlite")]
mod sqlite {
    use super::*;

    pub(super) mod authority;
    mod lifecycle;
    mod migration;

    type Result<T> = std::result::Result<T, ExecutableStoreError>;
    type Clock = Arc<dyn Fn() -> DateTime<FixedOffset> + Send + Sync>;

    #[derive(Clone)]
    pub struct SqliteExecutableStore {
        pool: SqlitePool,
        canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
        clock: Clock,
    }

    impl SqliteExecutableStore {
        pub async fn open(
            path: impl AsRef<Path>,
            canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
        ) -> Result<Self> {
            let pool = SqlitePoolOptions::new()
                .max_connections(4)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(path)
                        .create_if_missing(true)
                        .foreign_keys(true)
                        .busy_timeout(Duration::from_secs(5)),
                )
                .await?;
            Ok(Self {
                pool,
                canonicalizer,
                clock: Arc::new(|| Utc::now().fixed_offset()),
            })
        }

        pub fn with_clock(mut self, clock: Clock) -> Self {
            self.clock = clock;
            self
        }

        pub async fn close(&self) {
            self.pool.close().await;
        }

        pub async fn initialize(&self, cancellation: Cancellation) -> Result<()> {
            check(&cancellation)?;
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
            component TEXT PRIMARY KEY, version INTEGER NOT NULL
        )",
            )
            .execute(&self.pool)
            .await?;
            check(&cancellation)?;
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let version: Option<i64> = sqlx::query_scalar(
                "SELECT version FROM flaggo_schema_versions WHERE component = 'executable-store'",
            )
            .fetch_optional(&mut *transaction)
            .await?;
            match version {
                Some(SCHEMA_VERSION) | None => {}
                Some(version) => return Err(ExecutableStoreError::UnsupportedSchema(version)),
            }
            check(&cancellation)?;
            sqlx::raw_sql(schema::SCHEMA)
                .execute(&mut *transaction)
                .await?;
            check(&cancellation)?;
            transaction.commit().await?;
            Ok(())
        }

        pub async fn put_candidate(
            &self,
            stored: &StoredExecutable,
            cancellation: Cancellation,
        ) -> Result<ExecutableStoreWriteResult> {
            self.validate_stored(stored)?;
            check(&cancellation)?;
            let mut connection = self.pool.acquire().await?;
            check(&cancellation)?;
            insert_candidate(&mut connection, stored).await
        }

        pub async fn put_analysis_candidate(
            &self,
            stored: &StoredExecutable,
            admission: &AnalysisCandidateAdmission,
            cancellation: Cancellation,
        ) -> Result<AnalysisCandidateStoreResult> {
            self.validate_stored(stored)?;
            validate_admission(stored, admission)?;
            check(&cancellation)?;
            let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let result = self
                .put_analysis_candidate_in(&mut tx, stored, admission, cancellation.clone())
                .await?;
            check(&cancellation)?;
            tx.commit().await?;
            Ok(result)
        }

        async fn put_analysis_candidate_in(
            &self,
            connection: &mut SqliteConnection,
            stored: &StoredExecutable,
            admission: &AnalysisCandidateAdmission,
            cancellation: Cancellation,
        ) -> Result<AnalysisCandidateStoreResult> {
            self.validate_stored(stored)?;
            validate_admission(stored, admission)?;
            check(&cancellation)?;
            let resource_key = admission.resource.resource_key()?;
            let existing = sqlx::query(
                "SELECT resource_key, workspace_id, cycle_id, attempt_id, contract_name,
            contract_digest, executable_digest, analysis_manifest_digest, created_at
            FROM analysis_candidate_admissions WHERE resource_key = ? AND workspace_id = ? AND cycle_id = ?",
            )
            .bind(&resource_key)
            .bind(&admission.workspace_id)
            .bind(&admission.cycle_id)
            .fetch_optional(&mut *connection)
            .await?;
            if let Some(row) = existing {
                let existing = read_admission(&row, false)?;
                if !existing.identity_matches(admission) {
                    return Err(ExecutableStoreError::AdmissionConflict {
                        workspace_id: admission.workspace_id.clone(),
                        cycle_id: admission.cycle_id.clone(),
                    });
                }
                let state: Option<String> = sqlx::query_scalar(
                    "SELECT lifecycle_state FROM decision_executables WHERE executable_digest = ?",
                )
                .bind(&existing.executable_digest)
                .fetch_optional(&mut *connection)
                .await?;
                let state = parse_state(state.as_deref().ok_or_else(|| {
                    integrity("Candidate admission references a missing executable.")
                })?)?;
                check(&cancellation)?;
                return Ok(AnalysisCandidateStoreResult {
                    executable_digest: existing.executable_digest,
                    created_at: existing.created_at,
                    lifecycle_state: state,
                    created: false,
                });
            }
            let state: Option<String> = sqlx::query_scalar(
                "SELECT lifecycle_state FROM decision_executables WHERE executable_digest = ?",
            )
            .bind(&stored.executable_digest)
            .fetch_optional(&mut *connection)
            .await?;
            if let Some(state) = state {
                let state = parse_state(&state)?;
                if state != ExecutableLifecycleState::Candidate {
                    return Err(ExecutableStoreError::LifecycleConflict {
                        executable_digest: stored.executable_digest.clone(),
                        state,
                    });
                }
            }
            insert_candidate(connection, stored).await?;
            sqlx::query(
                "INSERT INTO analysis_candidate_admissions(
            resource_key, workspace_id, cycle_id, attempt_id, contract_name, contract_digest, executable_digest,
            analysis_manifest_digest, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&resource_key)
            .bind(&admission.workspace_id)
            .bind(&admission.cycle_id)
            .bind(&admission.attempt_id)
            .bind(&admission.resource.name)
            .bind(&admission.contract_digest)
            .bind(&admission.executable_digest)
            .bind(&admission.analysis_manifest_digest)
            .bind(timestamp::format_exact(admission.created_at)?)
            .execute(&mut *connection)
            .await?;
            check(&cancellation)?;
            Ok(AnalysisCandidateStoreResult {
                executable_digest: admission.executable_digest.clone(),
                created_at: admission.created_at,
                lifecycle_state: ExecutableLifecycleState::Candidate,
                created: true,
            })
        }

        pub async fn get(
            &self,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<Option<StoredExecutable>> {
            check(&cancellation)?;
            let row = sqlx::query("SELECT executable_digest, contract_digest, executable_json,
            checked_executable_json, provenance_json, lifecycle_state, state_version, created_at, activated_at
            FROM decision_executables WHERE executable_digest = ?")
            .bind(digest).fetch_optional(&self.pool).await?;
            check(&cancellation)?;
            row.map(|row| read_executable(&row)).transpose()
        }

        pub async fn get_active(
            &self,
            contract_digest: &str,
            cancellation: Cancellation,
        ) -> Result<Option<StoredExecutable>> {
            check(&cancellation)?;
            let row = sqlx::query("SELECT executable_digest, contract_digest, executable_json,
            checked_executable_json, provenance_json, lifecycle_state, state_version, created_at, activated_at
            FROM decision_executables WHERE contract_digest = ? AND lifecycle_state = 'active'")
            .bind(contract_digest).fetch_optional(&self.pool).await?;
            check(&cancellation)?;
            row.map(|row| read_executable(&row)).transpose()
        }

        pub async fn list_candidates(
            &self,
            digest: &str,
            page_size: i32,
            cursor: Option<&str>,
            cancellation: Cancellation,
        ) -> Result<ExecutablePage> {
            self.list(digest, page_size, cursor, Some("candidate"), cancellation)
                .await
        }

        pub async fn list_executables(
            &self,
            digest: &str,
            page_size: i32,
            cursor: Option<&str>,
            cancellation: Cancellation,
        ) -> Result<ExecutablePage> {
            self.list(digest, page_size, cursor, None, cancellation)
                .await
        }

        async fn list(
            &self,
            digest: &str,
            page_size: i32,
            cursor: Option<&str>,
            state: Option<&str>,
            cancellation: Cancellation,
        ) -> Result<ExecutablePage> {
            page_limit(page_size, 200)?;
            let boundary = cursor.map(decode_cursor).transpose()?;
            check(&cancellation)?;
            let rows = sqlx::query("SELECT executable_digest, contract_digest, executable_json,
            checked_executable_json, provenance_json, lifecycle_state, state_version, created_at, activated_at
            FROM decision_executables WHERE contract_digest = ? AND (? IS NULL OR lifecycle_state = ?)
            AND (? IS NULL OR created_at < ? OR (created_at = ? AND executable_digest < ?))
            ORDER BY created_at DESC, executable_digest DESC LIMIT ?")
            .bind(digest).bind(state).bind(state)
            .bind(boundary.as_ref().map(|value| &value.0)).bind(boundary.as_ref().map(|value| &value.0))
            .bind(boundary.as_ref().map(|value| &value.0)).bind(boundary.as_ref().map(|value| &value.1))
            .bind(page_size + 1).fetch_all(&self.pool).await?;
            check(&cancellation)?;
            let mut executables = rows
                .iter()
                .map(read_executable)
                .collect::<Result<Vec<_>>>()?;
            let next_cursor = if executables.len() > page_size as usize {
                executables.pop();
                let last = executables.last().expect("nonempty bounded page");
                Some(encode_cursor(last.created_at, &last.executable_digest)?)
            } else {
                None
            };
            Ok(ExecutablePage {
                executables,
                next_cursor,
            })
        }

        pub async fn is_available(&self, cancellation: Cancellation) -> Result<bool> {
            let probe: Result<bool> = async {
            check(&cancellation)?;
            let version = sqlx::query(
                "SELECT version FROM flaggo_schema_versions WHERE component = 'executable-store'"
            ).fetch_optional(&self.pool).await?;
            let Some(version) = version else { return Ok(false); };
            if version.try_get_raw("version")?.type_info().name() != "INTEGER"
                || version.try_get::<i64, _>("version")? != SCHEMA_VERSION {
                return Ok(false);
            }
            sqlx::query("SELECT executable_digest, contract_digest, executable_json,
                checked_executable_json, provenance_json, lifecycle_state, state_version,
                created_at, activated_at FROM decision_executables LIMIT 0")
                .fetch_all(&self.pool).await?;
            sqlx::query("SELECT resource_key, workspace_id, cycle_id, attempt_id, contract_name, contract_digest,
                executable_digest, analysis_manifest_digest, created_at FROM analysis_candidate_admissions LIMIT 0")
                .fetch_all(&self.pool).await?;
            check(&cancellation)?;
            Ok(true)
        }.await;
            match probe {
                Err(ExecutableStoreError::Database(sqlx::Error::Database(_))) => Ok(false),
                other => other,
            }
        }

        fn validate_stored(&self, stored: &StoredExecutable) -> Result<()> {
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
            let canonicalizer = self
                .canonicalizer
                .as_deref()
                .map(|value| value as &dyn ExpressionCanonicalizer);
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
    }

    impl flaggo_decision::ReadinessProbe for SqliteExecutableStore {
        async fn is_available(
            &self,
            cancellation: Cancellation,
        ) -> std::result::Result<bool, StoreReadError> {
            self.is_available(cancellation).await.map_err(read_error)
        }
    }

    impl ActiveExecutableReader for SqliteExecutableStore {
        async fn get_active(
            &self,
            digest: &str,
            cancellation: Cancellation,
        ) -> std::result::Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
            self.get_active(digest, cancellation)
                .await
                .map(|stored| {
                    stored.map(|stored| {
                        Arc::new(ActiveExecutable {
                            executable_digest: stored.executable_digest,
                            executable: stored.executable,
                            checked_executable: stored.checked_executable,
                        })
                    })
                })
                .map_err(read_error)
        }
    }

    fn read_error(error: ExecutableStoreError) -> StoreReadError {
        match error {
            ExecutableStoreError::Integrity(_) => StoreReadError::Integrity(Box::new(error)),
            ExecutableStoreError::Cancelled => StoreReadError::Cancelled,
            ExecutableStoreError::Database(sqlx::Error::PoolTimedOut) => {
                StoreReadError::Timeout(Box::new(error))
            }
            ExecutableStoreError::Database(
                sqlx::Error::Database(_) | sqlx::Error::Io(_) | sqlx::Error::PoolClosed,
            ) => StoreReadError::Dependency(Box::new(error)),
            ExecutableStoreError::InvalidArgument { .. }
            | ExecutableStoreError::Contract(ContractError::Serialization(_)) => {
                StoreReadError::InvalidArgument(Box::new(error))
            }
            _ => StoreReadError::Native(Box::new(error)),
        }
    }

    async fn insert_candidate(
        connection: &mut SqliteConnection,
        stored: &StoredExecutable,
    ) -> Result<ExecutableStoreWriteResult> {
        let rows = sqlx::query("INSERT INTO decision_executables(
        executable_digest, contract_digest, executable_json, checked_executable_json, provenance_json,
        lifecycle_state, state_version, created_at, activated_at)
        VALUES (?, ?, ?, ?, ?, 'candidate', 0, ?, NULL) ON CONFLICT(executable_digest) DO NOTHING")
        .bind(&stored.executable_digest).bind(&stored.executable.contract_digest)
        .bind(strict_json::serialize(&stored.executable)?)
        .bind(strict_json::serialize(&stored.checked_executable)?)
        .bind(stored.provenance.as_ref().map(strict_json::serialize).transpose()?)
        .bind(timestamp::format_exact(stored.created_at)?).execute(connection).await?.rows_affected();
        Ok(if rows == 1 {
            ExecutableStoreWriteResult::Created
        } else {
            ExecutableStoreWriteResult::Existing
        })
    }

    fn read_executable(row: &SqliteRow) -> Result<StoredExecutable> {
        let contract_digest: String = row.try_get("contract_digest")?;
        let executable: DecisionExecutable =
            strict_json::deserialize(&row.try_get::<Vec<u8>, _>("executable_json")?)?;
        if executable.contract_digest != contract_digest {
            return Err(integrity(format!(
                "Stored executable contract digest '{contract_digest}' does not match payload '{}'.",
                executable.contract_digest
            )));
        }
        let digest: String = row.try_get("executable_digest")?;
        let computed = executable_digest(&executable, None)?;
        if computed != digest {
            return Err(integrity(format!(
                "Stored executable digest '{digest}' does not match payload '{computed}'."
            )));
        }
        // Checked DTOs use JsonSerializer directly in the reference, not StrictJson token validation.
        let checked = checked::read(&row.try_get::<Vec<u8>, _>("checked_executable_json")?)?
            .ok_or_else(|| integrity("Stored checked executable representation is null."))?;
        if !rules_align(&executable, &checked)? {
            return Err(integrity(
                "Stored checked executable rules do not match executable rules.",
            ));
        }
        let provenance = row
            .try_get::<Option<Vec<u8>>, _>("provenance_json")?
            .map(|bytes| strict_json::deserialize(&bytes))
            .transpose()?;
        let state = parse_state(row.try_get("lifecycle_state")?)?;
        let created_at = timestamp::parse_exact(row.try_get("created_at")?)?;
        let activated_at = row
            .try_get::<Option<&str>, _>("activated_at")?
            .map(timestamp::parse_exact)
            .transpose()?;
        Ok(StoredExecutable {
            executable_digest: digest,
            executable,
            checked_executable: checked,
            state,
            state_version: row.try_get("state_version")?,
            created_at,
            activated_at,
            provenance,
        })
    }

    fn rules_align(
        executable: &DecisionExecutable,
        checked: &CheckedDecisionExecutable,
    ) -> Result<bool> {
        if executable.rules.len() != checked.rules.len() {
            return Ok(false);
        }
        for (rule, checked) in executable.rules.iter().zip(&checked.rules) {
            let rule = rule
                .as_ref()
                .ok_or_else(|| ContractError::InvalidModel("Null executable rule.".into()))?;
            let checked = checked.as_ref().ok_or_else(|| {
                ContractError::InvalidModel("Null checked executable rule.".into())
            })?;
            if rule.name != checked.name {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn validate_admission(
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

    fn read_admission(row: &SqliteRow, exact: bool) -> Result<AnalysisCandidateAdmission> {
        let resource = ContractResourceId::from_resource_key(row.try_get("resource_key")?)?;
        if resource.name != row.try_get::<String, _>("contract_name")? {
            return Err(integrity(
                "Admission resource key and contract name disagree.",
            ));
        }
        let text: &str = row.try_get("created_at")?;
        let created_at = if exact {
            timestamp::parse_exact(text)?
        } else {
            // The idempotency lookup uses DateTimeOffset.Parse rather than ParseExact("O").
            DateTime::parse_from_rfc3339(text)
                .map_err(|error| timestamp::TimestampError(error.to_string()))?
        };
        Ok(AnalysisCandidateAdmission {
            workspace_id: row.try_get("workspace_id")?,
            cycle_id: row.try_get("cycle_id")?,
            attempt_id: row.try_get("attempt_id")?,
            resource,
            contract_digest: row.try_get("contract_digest")?,
            executable_digest: row.try_get("executable_digest")?,
            analysis_manifest_digest: row.try_get("analysis_manifest_digest")?,
            created_at,
        })
    }

    fn parse_state(value: &str) -> Result<ExecutableLifecycleState> {
        match value {
            "candidate" => Ok(ExecutableLifecycleState::Candidate),
            "active" => Ok(ExecutableLifecycleState::Active),
            "inactive" => Ok(ExecutableLifecycleState::Inactive),
            invalid => Err(integrity(format!(
                "Unknown executable lifecycle state '{invalid}'."
            ))),
        }
    }

    fn argument(parameter: &'static str, message: impl Into<String>) -> ExecutableStoreError {
        ExecutableStoreError::InvalidArgument {
            parameter,
            message: message.into(),
            out_of_range: false,
        }
    }

    fn range(parameter: &'static str, message: impl Into<String>) -> ExecutableStoreError {
        ExecutableStoreError::InvalidArgument {
            parameter,
            message: message.into(),
            out_of_range: true,
        }
    }

    fn page_limit(size: i32, maximum: i32) -> Result<()> {
        if !(1..=maximum).contains(&size) {
            return Err(range(
                "pageSize",
                format!("Page size must be between 1 and {maximum}."),
            ));
        }
        Ok(())
    }

    fn integrity(message: impl Into<String>) -> ExecutableStoreError {
        ExecutableStoreError::Integrity(message.into())
    }

    fn check(cancellation: &Cancellation) -> Result<()> {
        if cancellation
            .as_ref()
            .is_some_and(|value| value.load(Ordering::Acquire))
        {
            Err(ExecutableStoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn encode_cursor(time: DateTime<FixedOffset>, digest: &str) -> Result<String> {
        Ok(cursor::encode(time, digest)?)
    }

    fn decode_cursor(cursor: &str) -> Result<(String, String)> {
        self::cursor::decode(cursor).map_err(|_| argument("cursor", "Invalid executable cursor."))
    }
}
