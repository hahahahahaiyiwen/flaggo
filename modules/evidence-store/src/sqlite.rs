use std::{num::NonZeroU16, str::FromStr, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow, SqliteSynchronous},
};

use crate::{
    AuthorityScope, EvidenceDiagnostic, EvidenceMaterializationCommit, EvidenceMaterializationMode,
    EvidenceObservation, EvidenceSignal, EvidenceStore, EvidenceStoreCommitResult,
    EvidenceStoreError, EvidenceStoreHealth, ForwardMaterializationKey, MaterializerVersions,
    SourceKey, StoredContractCatalog, StoredEvidenceObservation,
};

const COMPONENT_NAME: &str = "evidence-store";
const SCHEMA_VERSION: i64 = 2;
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
const CREATE_OBSERVATIONS: &str = "
    CREATE TABLE IF NOT EXISTS evidence_observations (
        observation_id TEXT PRIMARY KEY,
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
    ON evidence_observations(logical_source_id, observation_id)
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
        key: &ForwardMaterializationKey,
    ) -> Result<Option<u64>, EvidenceStoreError> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT last_inbox_batch_id
             FROM evidence_materializer_checkpoints
             WHERE materializer_version = ?
               AND decoder_version = ?
               AND identity_version = ?
               AND projection_version = ?
               AND routing_version = ?",
        )
        .bind(&key.versions.materializer)
        .bind(&key.versions.decoder)
        .bind(&key.versions.identity)
        .bind(&key.versions.projection)
        .bind(&key.versions.routing)
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

    async fn insert_conflicts(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        observation: &EvidenceObservation,
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
            let (first_id, second_id, first_digest, second_digest) =
                if existing_id <= observation.observation_id {
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
                &first_id,
                &second_id,
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
            .bind(&first_id)
            .bind(&second_id)
            .bind(&first_digest)
            .bind(&second_digest)
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
                        inbox_batch_id,
                        candidate_ordinal: Some(candidate_ordinal),
                        code: "logical-source-conflict".to_owned(),
                        message: "Distinct candidate content shares one logical source identity."
                            .to_owned(),
                        detail_json: Some(
                            serde_json::to_vec(&serde_json::json!({
                                "firstContentDigest": first_digest,
                                "firstObservationId": first_id,
                                "logicalSourceId": observation.logical_source_id,
                                "secondContentDigest": second_digest,
                                "secondObservationId": second_id
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
    async fn save_catalog(&self, catalog: StoredContractCatalog) -> Result<(), EvidenceStoreError> {
        validate_catalog(&catalog)?;
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
        .execute(&self.pool)
        .await
        .map_err(EvidenceStoreError::unavailable)?;
        Ok(())
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

    async fn forward_checkpoint(
        &self,
        key: &ForwardMaterializationKey,
    ) -> Result<Option<u64>, EvidenceStoreError> {
        validate_key(key)?;
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT last_inbox_batch_id
             FROM evidence_materializer_checkpoints
             WHERE materializer_version = ?
               AND decoder_version = ?
               AND identity_version = ?
               AND projection_version = ?
               AND routing_version = ?",
        )
        .bind(&key.versions.materializer)
        .bind(&key.versions.decoder)
        .bind(&key.versions.identity)
        .bind(&key.versions.projection)
        .bind(&key.versions.routing)
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
        if commit.mode == EvidenceMaterializationMode::Forward
            && Self::stored_checkpoint(&mut transaction, &commit.key)
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
            if inserted == 1 {
                result.observations_created += 1;
            } else {
                verify_existing_observation(&mut transaction, observation).await?;
                result.duplicate_observations += 1;
            }

            let (conflicts, conflict_diagnostics) = Self::insert_conflicts(
                &mut transaction,
                observation,
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
                    recorded_at_unix_ms
                 )
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT DO NOTHING",
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

        if commit.mode == EvidenceMaterializationMode::Forward {
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
        }
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
    let mut existing = decode_observation(&row)?.observation;
    existing.versions.routing = observation.versions.routing.clone();
    if existing != *observation {
        return Err(EvidenceStoreError::CorruptData(format!(
            "observation {} has inconsistent content",
            observation.observation_id
        )));
    }
    Ok(())
}

fn validate_catalog(catalog: &StoredContractCatalog) -> Result<(), EvidenceStoreError> {
    if catalog.etag.is_empty() || catalog.etag.chars().any(char::is_control) {
        return Err(EvidenceStoreError::InvalidWrite(
            "catalog ETag must be non-empty and contain no control characters".to_owned(),
        ));
    }
    validate_json("contract catalog", &catalog.payload)
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
    }
    for diagnostic in &commit.diagnostics {
        validate_diagnostic(diagnostic)?;
        if diagnostic.inbox_batch_id != commit.inbox_batch_id {
            return Err(EvidenceStoreError::InvalidWrite(
                "diagnostic batch must equal the committed batch".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_key(key: &ForwardMaterializationKey) -> Result<(), EvidenceStoreError> {
    validate_versions(&key.versions)
}

fn validate_versions(versions: &MaterializerVersions) -> Result<(), EvidenceStoreError> {
    for (name, value) in [
        ("materializer version", &versions.materializer),
        ("decoder version", &versions.decoder),
        ("identity version", &versions.identity),
        ("projection version", &versions.projection),
        ("routing version", &versions.routing),
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
    AuthorityScope::new(
        observation.authority.tenant.clone(),
        observation.authority.application.clone(),
        observation.authority.environment.clone(),
    )?;
    validate_source(&observation.source)?;
    if observation.observed_time_source.is_empty() {
        return Err(EvidenceStoreError::InvalidWrite(
            "observation time source must not be empty".to_owned(),
        ));
    }
    validate_json("observation payload", &observation.payload_json)?;
    validate_versions(&observation.versions)
}

fn validate_source(source: &SourceKey) -> Result<(), EvidenceStoreError> {
    if source.instrumentation_scope.is_empty() || source.signal_name.is_empty() {
        return Err(EvidenceStoreError::InvalidWrite(
            "source instrumentation scope and signal name must not be empty".to_owned(),
        ));
    }
    let shape_is_valid = match source.signal {
        EvidenceSignal::Metric => {
            source
                .metric_kind
                .as_ref()
                .is_some_and(|value| !value.is_empty())
                && source.metric_unit.is_some()
                && source.parent_span_name.is_none()
        }
        EvidenceSignal::SpanEvent => {
            source.metric_kind.is_none()
                && source.metric_unit.is_none()
                && source
                    .parent_span_name
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
        }
        EvidenceSignal::Log | EvidenceSignal::Span => {
            source.metric_kind.is_none()
                && source.metric_unit.is_none()
                && source.parent_span_name.is_none()
        }
    };
    if !shape_is_valid {
        return Err(EvidenceStoreError::InvalidWrite(
            "source fields do not match the signal kind".to_owned(),
        ));
    }
    Ok(())
}

fn validate_diagnostic(diagnostic: &EvidenceDiagnostic) -> Result<(), EvidenceStoreError> {
    validate_digest("diagnostic ID", &diagnostic.diagnostic_id)?;
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

fn digest_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("digest component length must fit in u64")
                .to_be_bytes(),
        );
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}
