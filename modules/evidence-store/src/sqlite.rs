use std::{num::NonZeroU16, str::FromStr, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow, SqliteSynchronous},
};

use crate::{
    DecisionScope, EvidenceAssociation, EvidenceDiagnostic, EvidenceMaterializationCommit,
    EvidenceObservation, EvidenceSignal, EvidenceStore, EvidenceStoreCommitResult,
    EvidenceStoreError, EvidenceStoreHealth, MaterializationKey, MaterializerVersions,
    StoredEvidenceAssociation, StoredEvidenceObservation, StoredSelectorSnapshot,
};

const COMPONENT_NAME: &str = "evidence-store";
const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const CREATE_SCHEMA_VERSIONS: &str = "
    CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
        component TEXT PRIMARY KEY,
        version INTEGER NOT NULL
    )
";
const CREATE_SELECTOR_SNAPSHOTS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_selector_snapshots (
        snapshot_digest TEXT PRIMARY KEY,
        snapshot_json BLOB NOT NULL,
        fetched_at_unix_ms INTEGER NOT NULL
    )
";
const CREATE_STATE: &str = "
    CREATE TABLE IF NOT EXISTS evidence_store_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        active_snapshot_digest TEXT NULL,
        FOREIGN KEY(active_snapshot_digest)
            REFERENCES evidence_selector_snapshots(snapshot_digest)
    )
";
const CREATE_CHECKPOINTS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_materializer_checkpoints (
        snapshot_digest TEXT NOT NULL,
        materializer_version TEXT NOT NULL,
        decoder_version TEXT NOT NULL,
        identity_version TEXT NOT NULL,
        projection_version TEXT NOT NULL,
        selector_protocol_version TEXT NOT NULL,
        last_inbox_batch_id INTEGER NOT NULL CHECK(last_inbox_batch_id > 0),
        updated_at_unix_ms INTEGER NOT NULL,
        PRIMARY KEY(
            snapshot_digest,
            materializer_version,
            decoder_version,
            identity_version,
            projection_version,
            selector_protocol_version
        ),
        FOREIGN KEY(snapshot_digest)
            REFERENCES evidence_selector_snapshots(snapshot_digest)
    )
";
const CREATE_OBSERVATIONS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observations (
        observation_id TEXT PRIMARY KEY,
        logical_source_id TEXT NOT NULL,
        content_digest TEXT NOT NULL,
        application TEXT NOT NULL,
        environment TEXT NOT NULL,
        signal_type TEXT NOT NULL
            CHECK(signal_type IN ('log', 'metric', 'span', 'span-event')),
        instrumentation_scope TEXT NOT NULL,
        signal_name TEXT NOT NULL,
        metric_kind TEXT NULL,
        metric_unit TEXT NULL,
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
        selector_protocol_version TEXT NOT NULL,
        created_at_unix_ms INTEGER NOT NULL
    )
";
const CREATE_OBSERVATION_INDEXES: &str = "
    CREATE INDEX IF NOT EXISTS ix_evidence_observations_scope_time
    ON evidence_observations(
        application,
        environment,
        observed_at_unix_nano,
        observation_id
    );

    CREATE INDEX IF NOT EXISTS ix_evidence_observations_logical_source
    ON evidence_observations(logical_source_id, observation_id)
";
const CREATE_PROVENANCE: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observation_provenance (
        observation_id TEXT NOT NULL,
        inbox_batch_id INTEGER NOT NULL CHECK(inbox_batch_id > 0),
        candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
        snapshot_digest TEXT NOT NULL,
        recorded_at_unix_ms INTEGER NOT NULL,
        PRIMARY KEY(
            observation_id,
            inbox_batch_id,
            candidate_ordinal,
            snapshot_digest
        ),
        FOREIGN KEY(observation_id)
            REFERENCES evidence_observations(observation_id),
        FOREIGN KEY(snapshot_digest)
            REFERENCES evidence_selector_snapshots(snapshot_digest)
    )
";
const CREATE_ASSOCIATIONS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observation_associations (
        observation_id TEXT NOT NULL,
        snapshot_digest TEXT NOT NULL,
        contract_digest TEXT NOT NULL,
        evidence_name TEXT NOT NULL,
        contract_attribute TEXT NOT NULL,
        correlation_json BLOB NOT NULL,
        source_json BLOB NOT NULL,
        associated_at_unix_ms INTEGER NOT NULL,
        PRIMARY KEY(
            observation_id,
            snapshot_digest,
            contract_digest,
            evidence_name
        ),
        FOREIGN KEY(observation_id)
            REFERENCES evidence_observations(observation_id),
        FOREIGN KEY(snapshot_digest)
            REFERENCES evidence_selector_snapshots(snapshot_digest)
    )
";
const CREATE_ASSOCIATION_INDEX: &str = "
    CREATE INDEX IF NOT EXISTS ix_evidence_associations_contract
    ON evidence_observation_associations(
        contract_digest,
        evidence_name,
        observation_id
    )
";
const CREATE_DIAGNOSTICS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_materializer_diagnostics (
        diagnostic_id TEXT PRIMARY KEY,
        snapshot_digest TEXT NOT NULL,
        inbox_batch_id INTEGER NOT NULL CHECK(inbox_batch_id > 0),
        candidate_ordinal INTEGER NULL CHECK(candidate_ordinal >= 0),
        code TEXT NOT NULL,
        message TEXT NOT NULL,
        detail_json BLOB NULL,
        created_at_unix_ms INTEGER NOT NULL,
        FOREIGN KEY(snapshot_digest)
            REFERENCES evidence_selector_snapshots(snapshot_digest)
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
        first_observation_id TEXT NOT NULL,
        second_observation_id TEXT NOT NULL,
        first_content_digest TEXT NOT NULL,
        second_content_digest TEXT NOT NULL,
        detected_at_unix_ms INTEGER NOT NULL,
        FOREIGN KEY(first_observation_id)
            REFERENCES evidence_observations(observation_id),
        FOREIGN KEY(second_observation_id)
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

    async fn initialize(&self) -> Result<(), EvidenceStoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        sqlx::query(CREATE_SCHEMA_VERSIONS)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        sqlx::query(
            "INSERT INTO flaggo_schema_versions(component, version)
             VALUES (?, ?)
             ON CONFLICT(component) DO NOTHING",
        )
        .bind(COMPONENT_NAME)
        .bind(SCHEMA_VERSION)
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
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

        for statement in [
            CREATE_SELECTOR_SNAPSHOTS,
            CREATE_STATE,
            CREATE_CHECKPOINTS,
            CREATE_OBSERVATIONS,
            CREATE_OBSERVATION_INDEXES,
            CREATE_PROVENANCE,
            CREATE_ASSOCIATIONS,
            CREATE_ASSOCIATION_INDEX,
            CREATE_DIAGNOSTICS,
            CREATE_DIAGNOSTIC_INDEX,
            CREATE_CONFLICTS,
        ] {
            sqlx::query(statement)
                .execute(&mut *transaction)
                .await
                .map_err(EvidenceStoreError::unavailable)?;
        }
        sqlx::query(
            "INSERT INTO evidence_store_state(singleton, active_snapshot_digest)
             VALUES (1, NULL)
             ON CONFLICT(singleton) DO NOTHING",
        )
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)
    }

    async fn stored_checkpoint(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        key: &MaterializationKey,
    ) -> Result<Option<u64>, EvidenceStoreError> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT last_inbox_batch_id
             FROM evidence_materializer_checkpoints
             WHERE snapshot_digest = ?
               AND materializer_version = ?
               AND decoder_version = ?
               AND identity_version = ?
               AND projection_version = ?
               AND selector_protocol_version = ?",
        )
        .bind(&key.snapshot_digest)
        .bind(&key.versions.materializer)
        .bind(&key.versions.decoder)
        .bind(&key.versions.identity)
        .bind(&key.versions.projection)
        .bind(&key.versions.selector_protocol)
        .fetch_optional(&mut **transaction)
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
        let candidate_ordinal = diagnostic.candidate_ordinal.map(i64::from);
        let rows = sqlx::query(
            "INSERT INTO evidence_materializer_diagnostics(
                diagnostic_id,
                snapshot_digest,
                inbox_batch_id,
                candidate_ordinal,
                code,
                message,
                detail_json,
                created_at_unix_ms
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(diagnostic_id) DO NOTHING",
        )
        .bind(&diagnostic.diagnostic_id)
        .bind(&diagnostic.snapshot_digest)
        .bind(to_i64("inbox batch ID", diagnostic.inbox_batch_id)?)
        .bind(candidate_ordinal)
        .bind(&diagnostic.code)
        .bind(&diagnostic.message)
        .bind(&diagnostic.detail_json)
        .bind(created_at_unix_ms)
        .execute(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?
        .rows_affected();
        Ok(rows)
    }

    async fn insert_conflicts(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        observation: &EvidenceObservation,
        snapshot_digest: &str,
        inbox_batch_id: u64,
        candidate_ordinal: u32,
        created_at_unix_ms: i64,
    ) -> Result<(u64, u64), EvidenceStoreError> {
        let rows = sqlx::query(
            "SELECT observation_id, content_digest
             FROM evidence_observations
             WHERE logical_source_id = ?
               AND observation_id <> ?
               AND content_digest <> ?",
        )
        .bind(&observation.logical_source_id)
        .bind(&observation.observation_id)
        .bind(&observation.content_digest)
        .fetch_all(&mut **transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        let mut conflicts_created = 0;
        let mut diagnostics_created = 0;
        for row in rows {
            let existing_id: String = row
                .try_get("observation_id")
                .map_err(EvidenceStoreError::unavailable)?;
            let existing_digest: String = row
                .try_get("content_digest")
                .map_err(EvidenceStoreError::unavailable)?;
            let (
                first_observation_id,
                second_observation_id,
                first_content_digest,
                second_content_digest,
            ) = if existing_id <= observation.observation_id {
                (
                    existing_id,
                    observation.observation_id.clone(),
                    existing_digest,
                    observation.content_digest.clone(),
                )
            } else {
                (
                    observation.observation_id.clone(),
                    existing_id,
                    observation.content_digest.clone(),
                    existing_digest,
                )
            };
            let conflict_id = digest_parts(&[
                "evidence-conflict/v1",
                &observation.logical_source_id,
                &first_observation_id,
                &second_observation_id,
            ]);
            let inserted = sqlx::query(
                "INSERT INTO evidence_observation_conflicts(
                    conflict_id,
                    logical_source_id,
                    first_observation_id,
                    second_observation_id,
                    first_content_digest,
                    second_content_digest,
                    detected_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(conflict_id) DO NOTHING",
            )
            .bind(&conflict_id)
            .bind(&observation.logical_source_id)
            .bind(&first_observation_id)
            .bind(&second_observation_id)
            .bind(&first_content_digest)
            .bind(&second_content_digest)
            .bind(created_at_unix_ms)
            .execute(&mut **transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?
            .rows_affected();
            conflicts_created += inserted;
            if inserted == 1 {
                diagnostics_created += Self::insert_diagnostic(
                    transaction,
                    &EvidenceDiagnostic {
                        diagnostic_id: conflict_id,
                        snapshot_digest: snapshot_digest.to_owned(),
                        inbox_batch_id,
                        candidate_ordinal: Some(candidate_ordinal),
                        code: "logical-source-conflict".to_owned(),
                        message: "Distinct candidate content shares one logical source identity."
                            .to_owned(),
                        detail_json: Some(
                            serde_json::to_vec(&serde_json::json!({
                                "firstContentDigest": first_content_digest,
                                "firstObservationId": first_observation_id,
                                "logicalSourceId": observation.logical_source_id,
                                "secondContentDigest": second_content_digest,
                                "secondObservationId": second_observation_id
                            }))
                            .expect("conflict detail JSON must serialize"),
                        ),
                    },
                    created_at_unix_ms,
                )
                .await?;
            }
        }
        Ok((conflicts_created, diagnostics_created))
    }
}

#[async_trait]
impl EvidenceStore for SqliteEvidenceStore {
    async fn activate_snapshot(
        &self,
        snapshot: StoredSelectorSnapshot,
    ) -> Result<(), EvidenceStoreError> {
        validate_digest("snapshot digest", &snapshot.snapshot_digest)?;
        validate_json("selector snapshot", &snapshot.payload)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        sqlx::query(
            "INSERT INTO evidence_selector_snapshots(
                snapshot_digest,
                snapshot_json,
                fetched_at_unix_ms
             )
             VALUES (?, ?, ?)
             ON CONFLICT(snapshot_digest) DO NOTHING",
        )
        .bind(&snapshot.snapshot_digest)
        .bind(&snapshot.payload)
        .bind(snapshot.fetched_at.timestamp_millis())
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        let stored_payload: Vec<u8> = sqlx::query_scalar(
            "SELECT snapshot_json
             FROM evidence_selector_snapshots
             WHERE snapshot_digest = ?",
        )
        .bind(&snapshot.snapshot_digest)
        .fetch_one(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        if stored_payload != snapshot.payload {
            return Err(EvidenceStoreError::CorruptData(format!(
                "selector snapshot {} has inconsistent content",
                snapshot.snapshot_digest
            )));
        }
        sqlx::query(
            "UPDATE evidence_store_state
             SET active_snapshot_digest = ?
             WHERE singleton = 1",
        )
        .bind(&snapshot.snapshot_digest)
        .execute(&mut *transaction)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(EvidenceStoreError::unavailable)
    }

    async fn load_active_snapshot(
        &self,
    ) -> Result<Option<StoredSelectorSnapshot>, EvidenceStoreError> {
        let row = sqlx::query(
            "SELECT snapshots.snapshot_digest,
                    snapshots.snapshot_json,
                    snapshots.fetched_at_unix_ms
             FROM evidence_store_state AS state
             JOIN evidence_selector_snapshots AS snapshots
               ON snapshots.snapshot_digest = state.active_snapshot_digest
             WHERE state.singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        row.map(|row| {
            Ok(StoredSelectorSnapshot {
                snapshot_digest: row
                    .try_get("snapshot_digest")
                    .map_err(EvidenceStoreError::unavailable)?,
                payload: row
                    .try_get("snapshot_json")
                    .map_err(EvidenceStoreError::unavailable)?,
                fetched_at: datetime_from_millis(
                    "snapshot fetched time",
                    row.try_get("fetched_at_unix_ms")
                        .map_err(EvidenceStoreError::unavailable)?,
                )?,
            })
        })
        .transpose()
    }

    async fn checkpoint(
        &self,
        key: &MaterializationKey,
    ) -> Result<Option<u64>, EvidenceStoreError> {
        validate_key(key)?;
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT last_inbox_batch_id
             FROM evidence_materializer_checkpoints
             WHERE snapshot_digest = ?
               AND materializer_version = ?
               AND decoder_version = ?
               AND identity_version = ?
               AND projection_version = ?
               AND selector_protocol_version = ?",
        )
        .bind(&key.snapshot_digest)
        .bind(&key.versions.materializer)
        .bind(&key.versions.decoder)
        .bind(&key.versions.identity)
        .bind(&key.versions.projection)
        .bind(&key.versions.selector_protocol)
        .fetch_optional(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        value
            .map(|raw| positive_u64("checkpoint batch ID", raw))
            .transpose()
    }

    async fn commit(
        &self,
        commit: EvidenceMaterializationCommit,
    ) -> Result<EvidenceStoreCommitResult, EvidenceStoreError> {
        validate_commit(&commit)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(EvidenceStoreError::unavailable)?;
        if Self::stored_checkpoint(&mut transaction, &commit.key)
            .await?
            .is_some_and(|value| value >= commit.inbox_batch_id)
        {
            transaction
                .rollback()
                .await
                .map_err(EvidenceStoreError::unavailable)?;
            return Ok(EvidenceStoreCommitResult {
                already_checkpointed: true,
                ..EvidenceStoreCommitResult::default()
            });
        }

        let created_at_unix_ms = Utc::now().timestamp_millis();
        let mut result = EvidenceStoreCommitResult::default();
        for write in &commit.observations {
            let observation = &write.observation;
            let inserted = sqlx::query(
                "INSERT INTO evidence_observations(
                    observation_id,
                    logical_source_id,
                    content_digest,
                    application,
                    environment,
                    signal_type,
                    instrumentation_scope,
                    signal_name,
                    metric_kind,
                    metric_unit,
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
                    selector_protocol_version,
                    created_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(observation_id) DO NOTHING",
            )
            .bind(&observation.observation_id)
            .bind(&observation.logical_source_id)
            .bind(&observation.content_digest)
            .bind(&observation.scope.application)
            .bind(&observation.scope.environment)
            .bind(observation.signal.as_str())
            .bind(&observation.instrumentation_scope)
            .bind(&observation.signal_name)
            .bind(&observation.metric_kind)
            .bind(&observation.metric_unit)
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
            .bind(&observation.versions.selector_protocol)
            .bind(created_at_unix_ms)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?
            .rows_affected();
            if inserted == 1 {
                result.observations_created += 1;
            } else {
                verify_existing_observation(&mut transaction, observation).await?;
                result.duplicate_observations += 1;
            }

            let (conflicts, conflict_diagnostics) = Self::insert_conflicts(
                &mut transaction,
                observation,
                &commit.key.snapshot_digest,
                commit.inbox_batch_id,
                write.candidate_ordinal,
                created_at_unix_ms,
            )
            .await?;
            result.conflicts_created += conflicts;
            result.diagnostics_created += conflict_diagnostics;

            result.provenance_created += sqlx::query(
                "INSERT INTO evidence_observation_provenance(
                    observation_id,
                    inbox_batch_id,
                    candidate_ordinal,
                    snapshot_digest,
                    recorded_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT DO NOTHING",
            )
            .bind(&observation.observation_id)
            .bind(to_i64("inbox batch ID", commit.inbox_batch_id)?)
            .bind(i64::from(write.candidate_ordinal))
            .bind(&commit.key.snapshot_digest)
            .bind(created_at_unix_ms)
            .execute(&mut *transaction)
            .await
            .map_err(EvidenceStoreError::unavailable)?
            .rows_affected();

            for association in &write.associations {
                result.associations_created += insert_association(
                    &mut transaction,
                    observation,
                    association,
                    created_at_unix_ms,
                )
                .await?;
            }
        }

        for diagnostic in &commit.diagnostics {
            result.diagnostics_created +=
                Self::insert_diagnostic(&mut transaction, diagnostic, created_at_unix_ms).await?;
        }

        sqlx::query(
            "INSERT INTO evidence_materializer_checkpoints(
                snapshot_digest,
                materializer_version,
                decoder_version,
                identity_version,
                projection_version,
                selector_protocol_version,
                last_inbox_batch_id,
                updated_at_unix_ms
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(
                snapshot_digest,
                materializer_version,
                decoder_version,
                identity_version,
                projection_version,
                selector_protocol_version
             )
             DO UPDATE SET
                last_inbox_batch_id = excluded.last_inbox_batch_id,
                updated_at_unix_ms = excluded.updated_at_unix_ms
             WHERE evidence_materializer_checkpoints.last_inbox_batch_id
                   < excluded.last_inbox_batch_id",
        )
        .bind(&commit.key.snapshot_digest)
        .bind(&commit.key.versions.materializer)
        .bind(&commit.key.versions.decoder)
        .bind(&commit.key.versions.identity)
        .bind(&commit.key.versions.projection)
        .bind(&commit.key.versions.selector_protocol)
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
        let active_snapshot_digest: Option<String> = sqlx::query_scalar(
            "SELECT active_snapshot_digest
             FROM evidence_store_state
             WHERE singleton = 1",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        let observation_count =
            count(&self.pool, "SELECT COUNT(*) FROM evidence_observations").await?;
        let association_count = count(
            &self.pool,
            "SELECT COUNT(*) FROM evidence_observation_associations",
        )
        .await?;
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
        let newest_observed_at_unix_nano = newest
            .map(|value| parse_u64("observation time", &value))
            .transpose()?;
        Ok(EvidenceStoreHealth {
            active_snapshot_digest,
            observation_count,
            association_count,
            provenance_count,
            diagnostic_count,
            conflict_count,
            newest_observed_at_unix_nano,
        })
    }

    async fn list_observations(
        &self,
        scope: &DecisionScope,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceObservation>, EvidenceStoreError> {
        let rows = sqlx::query(
            "SELECT observation_id,
                    logical_source_id,
                    content_digest,
                    application,
                    environment,
                    signal_type,
                    instrumentation_scope,
                    signal_name,
                    metric_kind,
                    metric_unit,
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
                    selector_protocol_version,
                    created_at_unix_ms
             FROM evidence_observations
             WHERE application = ?
               AND environment = ?
             ORDER BY length(observed_at_unix_nano) DESC,
                      observed_at_unix_nano DESC,
                      observation_id
             LIMIT ?",
        )
        .bind(&scope.application)
        .bind(&scope.environment)
        .bind(i64::from(limit.get()))
        .fetch_all(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        rows.iter().map(decode_observation).collect()
    }

    async fn list_associations(
        &self,
        scope: &DecisionScope,
        contract_digest: &str,
        limit: NonZeroU16,
    ) -> Result<Vec<StoredEvidenceAssociation>, EvidenceStoreError> {
        validate_digest("contract digest", contract_digest)?;
        let rows = sqlx::query(
            "SELECT associations.observation_id,
                    observations.application,
                    observations.environment,
                    associations.snapshot_digest,
                    associations.contract_digest,
                    associations.evidence_name,
                    associations.contract_attribute,
                    associations.correlation_json,
                    associations.source_json
             FROM evidence_observation_associations AS associations
             JOIN evidence_observations AS observations
               ON observations.observation_id = associations.observation_id
             WHERE observations.application = ?
               AND observations.environment = ?
               AND associations.contract_digest = ?
             ORDER BY associations.evidence_name,
                      associations.observation_id
             LIMIT ?",
        )
        .bind(&scope.application)
        .bind(&scope.environment)
        .bind(contract_digest)
        .bind(i64::from(limit.get()))
        .fetch_all(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        rows.iter().map(decode_association).collect()
    }
}

async fn insert_association(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    observation: &EvidenceObservation,
    association: &EvidenceAssociation,
    associated_at_unix_ms: i64,
) -> Result<u64, EvidenceStoreError> {
    validate_association(association)?;
    let rows = sqlx::query(
        "INSERT INTO evidence_observation_associations(
            observation_id,
            snapshot_digest,
            contract_digest,
            evidence_name,
            contract_attribute,
            correlation_json,
            source_json,
            associated_at_unix_ms
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(&observation.observation_id)
    .bind(&association.snapshot_digest)
    .bind(&association.contract_digest)
    .bind(&association.evidence_name)
    .bind(&association.contract_attribute)
    .bind(&association.correlation_json)
    .bind(&association.source_json)
    .bind(associated_at_unix_ms)
    .execute(&mut **transaction)
    .await
    .map_err(EvidenceStoreError::unavailable)?
    .rows_affected();
    Ok(rows)
}

async fn verify_existing_observation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    observation: &EvidenceObservation,
) -> Result<(), EvidenceStoreError> {
    let row = sqlx::query(
        "SELECT logical_source_id, content_digest, payload_json
         FROM evidence_observations
         WHERE observation_id = ?",
    )
    .bind(&observation.observation_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(EvidenceStoreError::unavailable)?;
    let logical_source_id: String = row
        .try_get("logical_source_id")
        .map_err(EvidenceStoreError::unavailable)?;
    let content_digest: String = row
        .try_get("content_digest")
        .map_err(EvidenceStoreError::unavailable)?;
    let payload_json: Vec<u8> = row
        .try_get("payload_json")
        .map_err(EvidenceStoreError::unavailable)?;
    if logical_source_id != observation.logical_source_id
        || content_digest != observation.content_digest
        || payload_json != observation.payload_json
    {
        return Err(EvidenceStoreError::CorruptData(format!(
            "observation {} has inconsistent content",
            observation.observation_id
        )));
    }
    Ok(())
}

fn validate_commit(commit: &EvidenceMaterializationCommit) -> Result<(), EvidenceStoreError> {
    validate_key(&commit.key)?;
    if commit.inbox_batch_id == 0 {
        return Err(EvidenceStoreError::InvalidWrite(
            "inbox batch ID must be positive".to_owned(),
        ));
    }
    for write in &commit.observations {
        validate_observation(&write.observation)?;
        for association in &write.associations {
            validate_association(association)?;
            if association.snapshot_digest != commit.key.snapshot_digest {
                return Err(EvidenceStoreError::InvalidWrite(
                    "association snapshot digest must equal the checkpoint snapshot digest"
                        .to_owned(),
                ));
            }
        }
    }
    for diagnostic in &commit.diagnostics {
        validate_diagnostic(diagnostic)?;
        if diagnostic.snapshot_digest != commit.key.snapshot_digest
            || diagnostic.inbox_batch_id != commit.inbox_batch_id
        {
            return Err(EvidenceStoreError::InvalidWrite(
                "diagnostic provenance must equal the committed batch and snapshot".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_key(key: &MaterializationKey) -> Result<(), EvidenceStoreError> {
    validate_digest("snapshot digest", &key.snapshot_digest)?;
    validate_versions(&key.versions)
}

fn validate_versions(versions: &MaterializerVersions) -> Result<(), EvidenceStoreError> {
    for (name, value) in [
        ("materializer version", &versions.materializer),
        ("decoder version", &versions.decoder),
        ("identity version", &versions.identity),
        ("projection version", &versions.projection),
        ("selector protocol version", &versions.selector_protocol),
    ] {
        if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
            return Err(EvidenceStoreError::InvalidWrite(format!(
                "{name} must be 1-64 non-control characters"
            )));
        }
    }
    Ok(())
}

fn validate_observation(observation: &EvidenceObservation) -> Result<(), EvidenceStoreError> {
    for (name, digest) in [
        ("observation ID", &observation.observation_id),
        ("logical source ID", &observation.logical_source_id),
        ("content digest", &observation.content_digest),
    ] {
        validate_digest(name, digest)?;
    }
    if let Some(contract_digest) = &observation.contract_digest {
        validate_digest("observation contract digest", contract_digest)?;
    }
    if observation.scope.application.is_empty()
        || observation.scope.environment.is_empty()
        || observation.instrumentation_scope.is_empty()
        || observation.signal_name.is_empty()
        || observation.observed_time_source.is_empty()
    {
        return Err(EvidenceStoreError::InvalidWrite(
            "observation scope, signal, and time source must not be empty".to_owned(),
        ));
    }
    validate_json("observation payload", &observation.payload_json)?;
    validate_versions(&observation.versions)
}

fn validate_association(association: &EvidenceAssociation) -> Result<(), EvidenceStoreError> {
    validate_digest("association snapshot digest", &association.snapshot_digest)?;
    validate_digest("association contract digest", &association.contract_digest)?;
    if association.evidence_name.is_empty() || association.contract_attribute.is_empty() {
        return Err(EvidenceStoreError::InvalidWrite(
            "association evidence and contract attribute names must not be empty".to_owned(),
        ));
    }
    validate_json("association correlation", &association.correlation_json)?;
    validate_json("association source", &association.source_json)
}

fn validate_diagnostic(diagnostic: &EvidenceDiagnostic) -> Result<(), EvidenceStoreError> {
    validate_digest("diagnostic ID", &diagnostic.diagnostic_id)?;
    validate_digest("diagnostic snapshot digest", &diagnostic.snapshot_digest)?;
    if diagnostic.inbox_batch_id == 0 || diagnostic.code.is_empty() || diagnostic.message.is_empty()
    {
        return Err(EvidenceStoreError::InvalidWrite(
            "diagnostic provenance, code, and message are required".to_owned(),
        ));
    }
    if let Some(detail) = &diagnostic.detail_json {
        validate_json("diagnostic detail", detail)?;
    }
    Ok(())
}

fn validate_digest(name: &str, value: &str) -> Result<(), EvidenceStoreError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(EvidenceStoreError::InvalidWrite(format!(
            "{name} must be a lowercase sha256 digest"
        )));
    }
    Ok(())
}

fn validate_json(name: &str, value: &[u8]) -> Result<(), EvidenceStoreError> {
    serde_json::from_slice::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|error| {
            EvidenceStoreError::InvalidWrite(format!("{name} is invalid JSON: {error}"))
        })
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
    let versions = MaterializerVersions {
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
        selector_protocol: row
            .try_get("selector_protocol_version")
            .map_err(EvidenceStoreError::unavailable)?,
    };
    Ok(StoredEvidenceObservation {
        observation: EvidenceObservation {
            observation_id: row
                .try_get("observation_id")
                .map_err(EvidenceStoreError::unavailable)?,
            logical_source_id: row
                .try_get("logical_source_id")
                .map_err(EvidenceStoreError::unavailable)?,
            content_digest: row
                .try_get("content_digest")
                .map_err(EvidenceStoreError::unavailable)?,
            scope: DecisionScope {
                application: row
                    .try_get("application")
                    .map_err(EvidenceStoreError::unavailable)?,
                environment: row
                    .try_get("environment")
                    .map_err(EvidenceStoreError::unavailable)?,
            },
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
            versions,
        },
        created_at: datetime_from_millis(
            "observation creation time",
            row.try_get("created_at_unix_ms")
                .map_err(EvidenceStoreError::unavailable)?,
        )?,
    })
}

fn decode_association(row: &SqliteRow) -> Result<StoredEvidenceAssociation, EvidenceStoreError> {
    Ok(StoredEvidenceAssociation {
        observation_id: row
            .try_get("observation_id")
            .map_err(EvidenceStoreError::unavailable)?,
        scope: DecisionScope {
            application: row
                .try_get("application")
                .map_err(EvidenceStoreError::unavailable)?,
            environment: row
                .try_get("environment")
                .map_err(EvidenceStoreError::unavailable)?,
        },
        snapshot_digest: row
            .try_get("snapshot_digest")
            .map_err(EvidenceStoreError::unavailable)?,
        contract_digest: row
            .try_get("contract_digest")
            .map_err(EvidenceStoreError::unavailable)?,
        evidence_name: row
            .try_get("evidence_name")
            .map_err(EvidenceStoreError::unavailable)?,
        contract_attribute: row
            .try_get("contract_attribute")
            .map_err(EvidenceStoreError::unavailable)?,
        correlation_json: row
            .try_get("correlation_json")
            .map_err(EvidenceStoreError::unavailable)?,
        source_json: row
            .try_get("source_json")
            .map_err(EvidenceStoreError::unavailable)?,
    })
}

async fn count(pool: &SqlitePool, query: &'static str) -> Result<u64, EvidenceStoreError> {
    let value: i64 = sqlx::query_scalar(query)
        .fetch_one(pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
    nonnegative_u64("row count", value)
}

fn datetime_from_millis(name: &str, value: i64) -> Result<DateTime<Utc>, EvidenceStoreError> {
    Utc.timestamp_millis_opt(value).single().ok_or_else(|| {
        EvidenceStoreError::CorruptData(format!("{name} {value} is outside the supported range"))
    })
}

fn positive_u64(name: &str, value: i64) -> Result<u64, EvidenceStoreError> {
    if value <= 0 {
        return Err(EvidenceStoreError::CorruptData(format!(
            "{name} must be positive, found {value}"
        )));
    }
    Ok(value.cast_unsigned())
}

fn nonnegative_u64(name: &str, value: i64) -> Result<u64, EvidenceStoreError> {
    if value < 0 {
        return Err(EvidenceStoreError::CorruptData(format!(
            "{name} must not be negative, found {value}"
        )));
    }
    Ok(value.cast_unsigned())
}

fn to_i64(name: &str, value: u64) -> Result<i64, EvidenceStoreError> {
    i64::try_from(value).map_err(|_| {
        EvidenceStoreError::InvalidWrite(format!("{name} must fit in a signed 64-bit integer"))
    })
}

fn parse_u64(name: &str, value: &str) -> Result<u64, EvidenceStoreError> {
    value.parse::<u64>().map_err(|error| {
        EvidenceStoreError::CorruptData(format!("{name} '{value}' is invalid: {error}"))
    })
}

fn digest_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("string length must fit in u64")
                .to_be_bytes(),
        );
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}
