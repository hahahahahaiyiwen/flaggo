use std::{num::NonZeroU16, str::FromStr, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::{
    Row, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
};

use crate::policy::validate_diagnostic;
use crate::{
    AuthorityScope, EvidenceDiagnostic, EvidenceMaterializationCommit, EvidenceObservation,
    EvidenceSignal, EvidenceStore, EvidenceStoreCommitResult, EvidenceStoreError,
    EvidenceStoreHealth, ExistingLogicalObservation, MaterializerVersions, ObservationDisposition,
    ProjectionClaim, SourceKey, StoredContractCatalog, StoredEvidenceObservation, plan_observation,
    validate_catalog, validate_commit, validate_observation, verify_observation_identity,
};

const COMPONENT_NAME: &str = "evidence-store";
const SCHEMA_VERSION: i64 = 5;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const CREATE_SCHEMA_VERSIONS: &str = "
    CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
        component TEXT PRIMARY KEY,
        version INTEGER NOT NULL
    )
";
const CREATE_CATALOG_STATE: &str = "
    CREATE TABLE IF NOT EXISTS evidence_contract_catalog (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        etag TEXT NOT NULL,
        catalog_json BLOB NOT NULL,
        fetched_at_unix_ms INTEGER NOT NULL
    )
";
const CREATE_CHECKPOINTS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_materializer_checkpoints (
        materializer_version TEXT NOT NULL,
        decoder_version TEXT NOT NULL,
        identity_version TEXT NOT NULL,
        projection_version TEXT NOT NULL,
        routing_version TEXT NOT NULL,
        last_inbox_batch_id INTEGER NOT NULL CHECK(last_inbox_batch_id > 0),
        updated_at_unix_ms INTEGER NOT NULL,
        PRIMARY KEY(
            materializer_version,
            decoder_version,
            identity_version,
            projection_version,
            routing_version
        )
    )
";
const CREATE_PROJECTION_CLAIM: &str = "
    CREATE TABLE evidence_projection_claim (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        epoch INTEGER NOT NULL CHECK(epoch >= 0)
    );
    INSERT INTO evidence_projection_claim(singleton, epoch) VALUES (1, 0)
";
const CREATE_OBSERVATIONS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observations (
        observation_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
        observation_id TEXT NOT NULL UNIQUE,
        logical_source_id TEXT NOT NULL,
        content_digest TEXT NOT NULL,
        tenant TEXT NOT NULL,
        application TEXT NOT NULL,
        environment TEXT NOT NULL,
        signal_type TEXT NOT NULL
            CHECK(signal_type IN ('log', 'metric', 'span', 'span-event')),
        instrumentation_scope TEXT NOT NULL,
        signal_name TEXT NOT NULL,
        metric_kind TEXT NULL,
        metric_unit TEXT NULL,
        parent_span_name TEXT NULL,
        observed_at_unix_nano TEXT NOT NULL,
        observed_time_source TEXT NOT NULL,
        protocol_kind TEXT NULL,
        decision_id TEXT NULL,
        contract_digest TEXT NULL,
        payload_json BLOB NOT NULL,
        materializer_version TEXT NOT NULL,
        decoder_version TEXT NOT NULL,
        identity_version TEXT NOT NULL,
        projection_version TEXT NOT NULL,
        routing_version TEXT NOT NULL,
        created_at_unix_ms INTEGER NOT NULL
    )
";
const CREATE_OBSERVATION_INDEXES: &str = "
    CREATE INDEX IF NOT EXISTS ix_evidence_observations_authority_time
    ON evidence_observations(
        tenant,
        application,
        environment,
        observed_at_unix_nano,
        observation_sequence,
        observation_id
    );

    CREATE INDEX IF NOT EXISTS ix_evidence_observations_route
    ON evidence_observations(
        tenant,
        application,
        environment,
        signal_type,
        instrumentation_scope,
        signal_name,
        metric_kind,
        metric_unit,
        parent_span_name
    );

    CREATE INDEX IF NOT EXISTS ix_evidence_observations_logical_source
    ON evidence_observations(logical_source_id, observation_id);

    CREATE UNIQUE INDEX IF NOT EXISTS ux_evidence_observations_logical_version
    ON evidence_observations(
        logical_source_id,
        decoder_version,
        identity_version,
        projection_version
    )
";
const CREATE_PROVENANCE: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observation_provenance (
        observation_id TEXT NOT NULL,
        inbox_batch_id INTEGER NOT NULL CHECK(inbox_batch_id > 0),
        candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
        recorded_at_unix_ms INTEGER NOT NULL,
        PRIMARY KEY(observation_id, inbox_batch_id, candidate_ordinal),
        FOREIGN KEY(observation_id)
            REFERENCES evidence_observations(observation_id)
    )
";
const CREATE_DIAGNOSTICS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_materializer_diagnostics (
        diagnostic_id TEXT PRIMARY KEY,
        inbox_batch_id INTEGER NOT NULL CHECK(inbox_batch_id > 0),
        candidate_ordinal INTEGER NULL CHECK(candidate_ordinal >= 0),
        code TEXT NOT NULL,
        message TEXT NOT NULL,
        detail_json BLOB NULL,
        created_at_unix_ms INTEGER NOT NULL
    )
";
const CREATE_DIAGNOSTIC_INDEX: &str = "
    CREATE INDEX IF NOT EXISTS ix_evidence_diagnostics_batch
    ON evidence_materializer_diagnostics(inbox_batch_id, diagnostic_id)
";
const CREATE_CONFLICTS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observation_conflicts (
        conflict_id TEXT PRIMARY KEY,
        logical_source_id TEXT NOT NULL,
        accepted_observation_id TEXT NOT NULL,
        accepted_content_digest TEXT NOT NULL,
        rejected_content_digest TEXT NOT NULL,
        rejected_inbox_batch_id INTEGER NOT NULL CHECK(rejected_inbox_batch_id > 0),
        rejected_candidate_ordinal INTEGER NOT NULL CHECK(rejected_candidate_ordinal >= 0),
        decoder_version TEXT NOT NULL,
        identity_version TEXT NOT NULL,
        projection_version TEXT NOT NULL,
        detected_at_unix_ms INTEGER NOT NULL,
        FOREIGN KEY(accepted_observation_id)
            REFERENCES evidence_observations(observation_id)
    )
";

#[derive(Clone)]
pub struct SqliteEvidenceStore {
    pool: SqlitePool,
}

impl SqliteEvidenceStore {
    pub async fn connect(database_url: &str) -> Result<Self, EvidenceStoreError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(EvidenceStoreError::unavailable)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(BUSY_TIMEOUT)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(BUSY_TIMEOUT)
            .connect_with(options)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        let store = Self { pool };
        if let Err(error) = store.initialize().await {
            store.pool.close().await;
            return Err(error);
        }
        Ok(store)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn migrate_v4_to_v5(database_url: &str) -> Result<bool, EvidenceStoreError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(EvidenceStoreError::unavailable)?
            .create_if_missing(false)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        let result = async {
            let mut transaction = pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            let found: i64 = sqlx::query_scalar(
                "SELECT version FROM flaggo_schema_versions WHERE component = ?",
            )
            .bind(COMPONENT_NAME)
            .fetch_one(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
            if found == SCHEMA_VERSION {
                return Ok(false);
            }
            if found != 4 {
                return Err(EvidenceStoreError::UnsupportedSchemaVersion {
                    component: COMPONENT_NAME,
                    found,
                    expected: SCHEMA_VERSION,
                });
            }
            sqlx::raw_sql(CREATE_PROJECTION_CLAIM)
                .execute(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            sqlx::query("UPDATE flaggo_schema_versions SET version = ? WHERE component = ?")
                .bind(SCHEMA_VERSION)
                .bind(COMPONENT_NAME)
                .execute(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            transaction
                .commit()
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            Ok(true)
        }
        .await;
        pool.close().await;
        result
    }

    async fn initialize(&self) -> Result<(), EvidenceStoreError> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        sqlx::query(CREATE_SCHEMA_VERSIONS)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        let inserted = sqlx::query(
            "INSERT INTO flaggo_schema_versions(component, version)
             VALUES (?, ?)
             ON CONFLICT(component) DO NOTHING",
        )
        .bind(COMPONENT_NAME)
        .bind(SCHEMA_VERSION)
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?
        .rows_affected()
            == 1;
        let found_version: i64 =
            sqlx::query_scalar("SELECT version FROM flaggo_schema_versions WHERE component = ?")
                .bind(COMPONENT_NAME)
                .fetch_one(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        if found_version != SCHEMA_VERSION {
            transaction
                .rollback()
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            return Err(EvidenceStoreError::UnsupportedSchemaVersion {
                component: COMPONENT_NAME,
                found: found_version,
                expected: SCHEMA_VERSION,
            });
        }

        if inserted {
            sqlx::raw_sql(CREATE_PROJECTION_CLAIM)
                .execute(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        } else {
            let epoch: i64 = sqlx::query_scalar(
                "SELECT epoch FROM evidence_projection_claim WHERE singleton = 1",
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
            if epoch < 0 {
                return Err(EvidenceStoreError::CorruptData(
                    "negative projection epoch".into(),
                ));
            }
        }
        for statement in [
            CREATE_CATALOG_STATE,
            CREATE_CHECKPOINTS,
            CREATE_OBSERVATIONS,
            CREATE_OBSERVATION_INDEXES,
            CREATE_PROVENANCE,
            CREATE_DIAGNOSTICS,
            CREATE_DIAGNOSTIC_INDEX,
            CREATE_CONFLICTS,
        ] {
            sqlx::query(statement)
                .execute(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        }
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)
    }

    async fn stored_checkpoint(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<Option<u64>, EvidenceStoreError> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(last_inbox_batch_id)
             FROM evidence_materializer_checkpoints",
        )
        .fetch_one(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        value
            .map(|raw| positive_u64("checkpoint batch ID", raw))
            .transpose()
    }

    async fn insert_diagnostic(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        diagnostic: &EvidenceDiagnostic,
        created_at_unix_ms: i64,
    ) -> Result<u64, EvidenceStoreError> {
        validate_diagnostic(diagnostic)?;
        sqlx::query(
            "INSERT INTO evidence_materializer_diagnostics(
                diagnostic_id,
                inbox_batch_id,
                candidate_ordinal,
                code,
                message,
                detail_json,
                created_at_unix_ms
             )
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(diagnostic_id) DO NOTHING",
        )
        .bind(&diagnostic.diagnostic_id)
        .bind(to_i64("inbox batch ID", diagnostic.inbox_batch_id)?)
        .bind(diagnostic.candidate_ordinal.map(i64::from))
        .bind(&diagnostic.code)
        .bind(&diagnostic.message)
        .bind(&diagnostic.detail_json)
        .bind(created_at_unix_ms)
        .execute(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)
        .map(|result| result.rows_affected())
    }

    async fn stored_logical_observation(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        observation: &EvidenceObservation,
    ) -> Result<Option<(String, String)>, EvidenceStoreError> {
        let row = sqlx::query(
            "SELECT observation_id, content_digest
             FROM evidence_observations
             WHERE logical_source_id = ?
             ORDER BY observation_sequence
             LIMIT 1",
        )
        .bind(&observation.logical_source_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        row.map(|row| {
            Ok((
                row.try_get("observation_id")
                    .map_err(EvidenceStoreError::unavailable)?,
                row.try_get("content_digest")
                    .map_err(EvidenceStoreError::unavailable)?,
            ))
        })
        .transpose()
    }

    async fn insert_conflict(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        observation: &EvidenceObservation,
        accepted: ExistingLogicalObservation<'_>,
        conflict_id: &str,
        diagnostic: &EvidenceDiagnostic,
        created_at_unix_ms: i64,
    ) -> Result<(u64, u64), EvidenceStoreError> {
        let inserted = sqlx::query(
            "INSERT INTO evidence_observation_conflicts(
                conflict_id,
                logical_source_id,
                accepted_observation_id,
                accepted_content_digest,
                rejected_content_digest,
                rejected_inbox_batch_id,
                rejected_candidate_ordinal,
                decoder_version,
                identity_version,
                projection_version,
                detected_at_unix_ms
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(conflict_id) DO NOTHING",
        )
        .bind(conflict_id)
        .bind(&observation.logical_source_id)
        .bind(accepted.observation_id)
        .bind(accepted.content_digest)
        .bind(&observation.content_digest)
        .bind(to_i64("inbox batch ID", diagnostic.inbox_batch_id)?)
        .bind(diagnostic.candidate_ordinal.map(i64::from))
        .bind(&observation.versions.decoder)
        .bind(&observation.versions.identity)
        .bind(&observation.versions.projection)
        .bind(created_at_unix_ms)
        .execute(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?
        .rows_affected();
        let diagnostics_created = if inserted == 1 {
            Self::insert_diagnostic(transaction, diagnostic, created_at_unix_ms).await?
        } else {
            0
        };
        Ok((inserted, diagnostics_created))
    }
}

#[async_trait]
impl EvidenceStore for SqliteEvidenceStore {
    async fn projection_claim(&self) -> Result<Option<ProjectionClaim>, EvidenceStoreError> {
        let epoch: i64 =
            sqlx::query_scalar("SELECT epoch FROM evidence_projection_claim WHERE singleton = 1")
                .fetch_one(&self.pool)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        if epoch == 0 {
            return Ok(None);
        }
        Ok(Some(ProjectionClaim {
            epoch: positive_u64("projection epoch", epoch)?,
        }))
    }

    async fn replace_projection_claim(
        &self,
        expected: Option<ProjectionClaim>,
    ) -> Result<ProjectionClaim, EvidenceStoreError> {
        let epoch = expected
            .map(|claim| i64::try_from(claim.epoch))
            .transpose()
            .map_err(|_| {
                EvidenceStoreError::InvalidWrite("projection epoch exceeds supported range".into())
            })?
            .unwrap_or(0);
        if expected.is_some() && epoch == 0 {
            return Err(EvidenceStoreError::InvalidWrite(
                "projection epoch must be positive".into(),
            ));
        }
        let next = epoch
            .checked_add(1)
            .ok_or_else(|| EvidenceStoreError::InvalidWrite("projection epoch exhausted".into()))?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        let changed = sqlx::query(
            "UPDATE evidence_projection_claim SET epoch = ? WHERE singleton = 1 AND epoch = ?",
        )
        .bind(next)
        .bind(epoch)
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?
        .rows_affected();
        if changed != 1 {
            return Err(EvidenceStoreError::ProjectionConflict);
        }
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        Ok(ProjectionClaim {
            epoch: u64::try_from(next)
                .map_err(|_| EvidenceStoreError::CorruptData("negative projection epoch".into()))?,
        })
    }

    async fn save_catalog(
        &self,
        claim: ProjectionClaim,
        catalog: StoredContractCatalog,
    ) -> Result<(), EvidenceStoreError> {
        validate_catalog(&catalog)?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        validate_projection_claim(&mut transaction, claim).await?;
        sqlx::query(
            "INSERT INTO evidence_contract_catalog(
                singleton,
                etag,
                catalog_json,
                fetched_at_unix_ms
             )
             VALUES (1, ?, ?, ?)
             ON CONFLICT(singleton) DO UPDATE SET
                etag = excluded.etag,
                catalog_json = excluded.catalog_json,
                fetched_at_unix_ms = excluded.fetched_at_unix_ms",
        )
        .bind(&catalog.etag)
        .bind(&catalog.payload)
        .bind(catalog.fetched_at.timestamp_millis())
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)
    }

    async fn load_catalog(&self) -> Result<Option<StoredContractCatalog>, EvidenceStoreError> {
        let row = sqlx::query(
            "SELECT etag, catalog_json, fetched_at_unix_ms
             FROM evidence_contract_catalog
             WHERE singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        row.map(|row| {
            let catalog = StoredContractCatalog {
                etag: row
                    .try_get("etag")
                    .map_err(EvidenceStoreError::unavailable)?,
                payload: row
                    .try_get("catalog_json")
                    .map_err(EvidenceStoreError::unavailable)?,
                fetched_at: datetime_from_millis(
                    "catalog fetched time",
                    row.try_get("fetched_at_unix_ms")
                        .map_err(EvidenceStoreError::unavailable)?,
                )?,
            };
            validate_catalog(&catalog)?;
            Ok(catalog)
        })
        .transpose()
    }

    async fn forward_checkpoint(&self) -> Result<Option<u64>, EvidenceStoreError> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(last_inbox_batch_id)
             FROM evidence_materializer_checkpoints",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        value
            .map(|raw| positive_u64("checkpoint batch ID", raw))
            .transpose()
    }

    async fn commit(
        &self,
        claim: ProjectionClaim,
        expected_checkpoint: Option<u64>,
        commit: EvidenceMaterializationCommit,
    ) -> Result<EvidenceStoreCommitResult, EvidenceStoreError> {
        validate_commit(&commit)?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        validate_projection_claim(&mut transaction, claim).await?;
        let checkpoint = Self::stored_checkpoint(&mut transaction).await?;
        if checkpoint.is_some_and(|value| value >= commit.inbox_batch_id) {
            transaction
                .rollback()
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            return Ok(EvidenceStoreCommitResult {
                already_checkpointed: true,
                ..EvidenceStoreCommitResult::default()
            });
        }
        if checkpoint != expected_checkpoint {
            return Err(EvidenceStoreError::ProjectionConflict);
        }

        let created_at_unix_ms = Utc::now().timestamp_millis();
        let mut result = EvidenceStoreCommitResult::default();
        for write in &commit.observations {
            let observation = &write.observation;
            let stored = Self::stored_logical_observation(&mut transaction, observation).await?;
            let accepted = stored
                .as_ref()
                .map(|(id, digest)| ExistingLogicalObservation {
                    observation_id: id,
                    content_digest: digest,
                });
            match plan_observation(write, commit.inbox_batch_id, accepted, None)? {
                ObservationDisposition::Insert => {}
                ObservationDisposition::Duplicate => {
                    result.duplicate_observations += 1;
                    continue;
                }
                ObservationDisposition::Conflict {
                    conflict_id,
                    diagnostic,
                } => {
                    let (conflicts, diagnostics) = Self::insert_conflict(
                        &mut transaction,
                        observation,
                        accepted.expect("conflict requires an accepted logical source"),
                        &conflict_id,
                        &diagnostic,
                        created_at_unix_ms,
                    )
                    .await?;
                    result.conflicts_created += conflicts;
                    result.diagnostics_created += diagnostics;
                    continue;
                }
            }

            let inserted = sqlx::query(
                "INSERT INTO evidence_observations(
                    observation_id,
                    logical_source_id,
                    content_digest,
                    tenant,
                    application,
                    environment,
                    signal_type,
                    instrumentation_scope,
                    signal_name,
                    metric_kind,
                    metric_unit,
                    parent_span_name,
                    observed_at_unix_nano,
                    observed_time_source,
                    protocol_kind,
                    decision_id,
                    contract_digest,
                    payload_json,
                    materializer_version,
                    decoder_version,
                    identity_version,
                    projection_version,
                    routing_version,
                    created_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(observation_id) DO NOTHING",
            )
            .bind(&observation.observation_id)
            .bind(&observation.logical_source_id)
            .bind(&observation.content_digest)
            .bind(&observation.authority.tenant)
            .bind(&observation.authority.application)
            .bind(&observation.authority.environment)
            .bind(observation.source.signal.as_str())
            .bind(&observation.source.instrumentation_scope)
            .bind(&observation.source.signal_name)
            .bind(&observation.source.metric_kind)
            .bind(&observation.source.metric_unit)
            .bind(&observation.source.parent_span_name)
            .bind(observation.observed_at_unix_nano.to_string())
            .bind(&observation.observed_time_source)
            .bind(&observation.protocol_kind)
            .bind(&observation.decision_id)
            .bind(&observation.contract_digest)
            .bind(&observation.payload_json)
            .bind(&observation.versions.materializer)
            .bind(&observation.versions.decoder)
            .bind(&observation.versions.identity)
            .bind(&observation.versions.projection)
            .bind(&observation.versions.routing)
            .bind(created_at_unix_ms)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?
            .rows_affected();
            if inserted != 1 {
                verify_existing_observation(&mut transaction, observation).await?;
                result.duplicate_observations += 1;
                continue;
            }
            result.observations_created += 1;
            result.provenance_created += sqlx::query(
                "INSERT INTO evidence_observation_provenance(
                    observation_id,
                    inbox_batch_id,
                    candidate_ordinal,
                    recorded_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&observation.observation_id)
            .bind(to_i64("inbox batch ID", commit.inbox_batch_id)?)
            .bind(i64::from(write.candidate_ordinal))
            .bind(created_at_unix_ms)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?
            .rows_affected();
        }

        for diagnostic in &commit.diagnostics {
            result.diagnostics_created +=
                Self::insert_diagnostic(&mut transaction, diagnostic, created_at_unix_ms).await?;
        }

        sqlx::query(
            "INSERT INTO evidence_materializer_checkpoints(
                materializer_version,
                decoder_version,
                identity_version,
                projection_version,
                routing_version,
                last_inbox_batch_id,
                updated_at_unix_ms
             )
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(
                materializer_version,
                decoder_version,
                identity_version,
                projection_version,
                routing_version
             )
             DO UPDATE SET
                last_inbox_batch_id = excluded.last_inbox_batch_id,
                updated_at_unix_ms = excluded.updated_at_unix_ms
             WHERE evidence_materializer_checkpoints.last_inbox_batch_id
                   < excluded.last_inbox_batch_id",
        )
        .bind(&commit.key.versions.materializer)
        .bind(&commit.key.versions.decoder)
        .bind(&commit.key.versions.identity)
        .bind(&commit.key.versions.projection)
        .bind(&commit.key.versions.routing)
        .bind(to_i64("inbox batch ID", commit.inbox_batch_id)?)
        .bind(created_at_unix_ms)
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        Ok(result)
    }

    async fn inspect(&self) -> Result<EvidenceStoreHealth, EvidenceStoreError> {
        let has_cached_catalog =
            count(&self.pool, "SELECT COUNT(*) FROM evidence_contract_catalog").await? == 1;
        let observation_count =
            count(&self.pool, "SELECT COUNT(*) FROM evidence_observations").await?;
        let provenance_count = count(
            &self.pool,
            "SELECT COUNT(*) FROM evidence_observation_provenance",
        )
        .await?;
        let diagnostic_count = count(
            &self.pool,
            "SELECT COUNT(*) FROM evidence_materializer_diagnostics",
        )
        .await?;
        let conflict_count = count(
            &self.pool,
            "SELECT COUNT(*) FROM evidence_observation_conflicts",
        )
        .await?;
        let newest: Option<String> = sqlx::query_scalar(
            "SELECT observed_at_unix_nano
             FROM evidence_observations
             ORDER BY length(observed_at_unix_nano) DESC,
                      observed_at_unix_nano DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        Ok(EvidenceStoreHealth {
            has_cached_catalog,
            observation_count,
            provenance_count,
            diagnostic_count,
            conflict_count,
            newest_observed_at_unix_nano: newest
                .map(|value| parse_u64("observation time", &value))
                .transpose()?,
        })
    }

    async fn list_observations(
        &self,
        authority: &AuthorityScope,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceObservation>, EvidenceStoreError> {
        let rows = sqlx::query(
            "SELECT observation_id,
                    logical_source_id,
                    content_digest,
                    tenant,
                    application,
                    environment,
                    signal_type,
                    instrumentation_scope,
                    signal_name,
                    metric_kind,
                    metric_unit,
                    parent_span_name,
                    observed_at_unix_nano,
                    observed_time_source,
                    protocol_kind,
                    decision_id,
                    contract_digest,
                    payload_json,
                    materializer_version,
                    decoder_version,
                    identity_version,
                    projection_version,
                    routing_version,
                    created_at_unix_ms
             FROM evidence_observations
             WHERE tenant = ?
               AND application = ?
               AND environment = ?
             ORDER BY length(observed_at_unix_nano) DESC,
                      observed_at_unix_nano DESC,
                      observation_id
             LIMIT ?",
        )
        .bind(&authority.tenant)
        .bind(&authority.application)
        .bind(&authority.environment)
        .bind(i64::from(limit.get()))
        .fetch_all(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        rows.iter().map(decode_observation).collect()
    }
}

async fn verify_existing_observation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    observation: &EvidenceObservation,
) -> Result<(), EvidenceStoreError> {
    let row = sqlx::query(
        "SELECT observation_id,
                logical_source_id,
                content_digest,
                tenant,
                application,
                environment,
                signal_type,
                instrumentation_scope,
                signal_name,
                metric_kind,
                metric_unit,
                parent_span_name,
                observed_at_unix_nano,
                observed_time_source,
                protocol_kind,
                decision_id,
                contract_digest,
                payload_json,
                materializer_version,
                decoder_version,
                identity_version,
                projection_version,
                routing_version,
                created_at_unix_ms
         FROM evidence_observations
         WHERE observation_id = ?",
    )
    .bind(&observation.observation_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(EvidenceStoreError::unavailable)?;
    verify_observation_identity(&decode_observation(&row)?.observation, observation)
}

async fn validate_projection_claim(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    claim: ProjectionClaim,
) -> Result<(), EvidenceStoreError> {
    let epoch = i64::try_from(claim.epoch).map_err(|_| EvidenceStoreError::ProjectionConflict)?;
    let current: i64 =
        sqlx::query_scalar("SELECT epoch FROM evidence_projection_claim WHERE singleton = 1")
            .fetch_one(&mut **transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
    if epoch <= 0 || current != epoch {
        return Err(EvidenceStoreError::ProjectionConflict);
    }
    Ok(())
}

fn decode_observation(row: &SqliteRow) -> Result<StoredEvidenceObservation, EvidenceStoreError> {
    let signal_value: String = row
        .try_get("signal_type")
        .map_err(EvidenceStoreError::unavailable)?;
    let signal = EvidenceSignal::from_storage(&signal_value).ok_or_else(|| {
        EvidenceStoreError::CorruptData(format!("unknown evidence signal '{signal_value}'"))
    })?;
    let observed_at: String = row
        .try_get("observed_at_unix_nano")
        .map_err(EvidenceStoreError::unavailable)?;
    let observation = EvidenceObservation {
        observation_id: row
            .try_get("observation_id")
            .map_err(EvidenceStoreError::unavailable)?,
        logical_source_id: row
            .try_get("logical_source_id")
            .map_err(EvidenceStoreError::unavailable)?,
        content_digest: row
            .try_get("content_digest")
            .map_err(EvidenceStoreError::unavailable)?,
        authority: AuthorityScope {
            tenant: row
                .try_get("tenant")
                .map_err(EvidenceStoreError::unavailable)?,
            application: row
                .try_get("application")
                .map_err(EvidenceStoreError::unavailable)?,
            environment: row
                .try_get("environment")
                .map_err(EvidenceStoreError::unavailable)?,
        },
        source: SourceKey {
            signal,
            instrumentation_scope: row
                .try_get("instrumentation_scope")
                .map_err(EvidenceStoreError::unavailable)?,
            signal_name: row
                .try_get("signal_name")
                .map_err(EvidenceStoreError::unavailable)?,
            metric_kind: row
                .try_get("metric_kind")
                .map_err(EvidenceStoreError::unavailable)?,
            metric_unit: row
                .try_get("metric_unit")
                .map_err(EvidenceStoreError::unavailable)?,
            parent_span_name: row
                .try_get("parent_span_name")
                .map_err(EvidenceStoreError::unavailable)?,
        },
        observed_at_unix_nano: parse_u64("observation time", &observed_at)?,
        observed_time_source: row
            .try_get("observed_time_source")
            .map_err(EvidenceStoreError::unavailable)?,
        protocol_kind: row
            .try_get("protocol_kind")
            .map_err(EvidenceStoreError::unavailable)?,
        decision_id: row
            .try_get("decision_id")
            .map_err(EvidenceStoreError::unavailable)?,
        contract_digest: row
            .try_get("contract_digest")
            .map_err(EvidenceStoreError::unavailable)?,
        payload_json: row
            .try_get("payload_json")
            .map_err(EvidenceStoreError::unavailable)?,
        versions: MaterializerVersions {
            materializer: row
                .try_get("materializer_version")
                .map_err(EvidenceStoreError::unavailable)?,
            decoder: row
                .try_get("decoder_version")
                .map_err(EvidenceStoreError::unavailable)?,
            identity: row
                .try_get("identity_version")
                .map_err(EvidenceStoreError::unavailable)?,
            projection: row
                .try_get("projection_version")
                .map_err(EvidenceStoreError::unavailable)?,
            routing: row
                .try_get("routing_version")
                .map_err(EvidenceStoreError::unavailable)?,
        },
    };
    validate_observation(&observation)?;
    Ok(StoredEvidenceObservation {
        observation,
        created_at: datetime_from_millis(
            "observation creation time",
            row.try_get("created_at_unix_ms")
                .map_err(EvidenceStoreError::unavailable)?,
        )?,
    })
}

async fn count(pool: &SqlitePool, query: &'static str) -> Result<u64, EvidenceStoreError> {
    let value: i64 = sqlx::query_scalar(query)
        .fetch_one(pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
    u64::try_from(value)
        .map_err(|_| EvidenceStoreError::CorruptData("negative row count".to_owned()))
}

fn positive_u64(name: &str, value: i64) -> Result<u64, EvidenceStoreError> {
    let value = u64::try_from(value)
        .map_err(|_| EvidenceStoreError::CorruptData(format!("{name} is negative")))?;
    if value == 0 {
        return Err(EvidenceStoreError::CorruptData(format!(
            "{name} must be positive"
        )));
    }
    Ok(value)
}

fn parse_u64(name: &str, value: &str) -> Result<u64, EvidenceStoreError> {
    value
        .parse()
        .map_err(|error| EvidenceStoreError::CorruptData(format!("{name} is invalid: {error}")))
}

fn to_i64(name: &str, value: u64) -> Result<i64, EvidenceStoreError> {
    i64::try_from(value)
        .map_err(|_| EvidenceStoreError::InvalidWrite(format!("{name} exceeds i64::MAX")))
}

fn datetime_from_millis(name: &str, value: i64) -> Result<DateTime<Utc>, EvidenceStoreError> {
    Utc.timestamp_millis_opt(value).single().ok_or_else(|| {
        EvidenceStoreError::CorruptData(format!("{name} is outside the supported range"))
    })
}
