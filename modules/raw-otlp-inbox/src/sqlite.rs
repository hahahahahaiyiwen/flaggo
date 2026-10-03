use std::{
    num::{NonZeroU16, NonZeroU64},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow, SqliteSynchronous},
};

use crate::{
    InboxAppendReceipt, InboxBatchId, InboxClock, NewRawOtlpBatch, OtlpProfileVersion, OtlpSignal,
    OtlpTransportCompression, OtlpWireEncoding, PayloadSha256, RawOtlpInbox, RawOtlpInboxBatch,
    RawOtlpInboxError, RawOtlpInboxHealth, RawOtlpInboxLimits, SystemInboxClock,
};

const COMPONENT_NAME: &str = "raw-otlp-inbox";
const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const CREATE_SCHEMA_VERSIONS: &str = "
    CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
        component TEXT PRIMARY KEY,
        version INTEGER NOT NULL
    )
";
const CREATE_BATCHES: &str = "
    CREATE TABLE IF NOT EXISTS raw_otlp_inbox_batches (
        inbox_batch_id INTEGER PRIMARY KEY AUTOINCREMENT,
        signal_type TEXT NOT NULL
            CHECK(signal_type IN ('logs', 'metrics', 'traces')),
        wire_encoding TEXT NOT NULL
            CHECK(wire_encoding IN ('protobuf', 'protobuf-json')),
        media_type TEXT NOT NULL
            CHECK(media_type IN ('application/x-protobuf', 'application/json')),
        transport_compression TEXT NOT NULL
            CHECK(transport_compression IN ('identity', 'gzip')),
        otlp_profile_version TEXT NOT NULL
            CHECK(length(otlp_profile_version) BETWEEN 1 AND 64),
        received_at_unix_ms INTEGER NOT NULL,
        payload_length INTEGER NOT NULL
            CHECK(payload_length >= 0),
        payload_sha256 BLOB NOT NULL
            CHECK(length(payload_sha256) = 32),
        payload BLOB NOT NULL,
        CHECK(payload_length = length(payload)),
        CHECK(
            (wire_encoding = 'protobuf' AND media_type = 'application/x-protobuf')
            OR
            (wire_encoding = 'protobuf-json' AND media_type = 'application/json')
        )
    )
";
const CREATE_RECEIPT_INDEX: &str = "
    CREATE INDEX IF NOT EXISTS ix_raw_otlp_inbox_receipt
    ON raw_otlp_inbox_batches(received_at_unix_ms, inbox_batch_id)
";
const CREATE_STATE: &str = "
    CREATE TABLE IF NOT EXISTS raw_otlp_inbox_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        write_revision INTEGER NOT NULL CHECK(write_revision >= 0),
        retained_batch_count INTEGER NOT NULL CHECK(retained_batch_count >= 0),
        retained_payload_bytes INTEGER NOT NULL CHECK(retained_payload_bytes >= 0),
        expired_batch_count INTEGER NOT NULL CHECK(expired_batch_count >= 0),
        expired_payload_bytes INTEGER NOT NULL CHECK(expired_payload_bytes >= 0),
        earliest_replay_at_unix_ms INTEGER NULL,
        last_received_at_unix_ms INTEGER NULL
    )
";

#[derive(Clone)]
pub struct SqliteRawOtlpInbox {
    pool: SqlitePool,
    limits: RawOtlpInboxLimits,
    clock: Arc<dyn InboxClock>,
}

impl SqliteRawOtlpInbox {
    pub async fn connect(
        database_url: &str,
        limits: RawOtlpInboxLimits,
    ) -> Result<Self, RawOtlpInboxError> {
        Self::connect_with_clock(database_url, limits, Arc::new(SystemInboxClock)).await
    }

    pub async fn connect_with_clock(
        database_url: &str,
        limits: RawOtlpInboxLimits,
        clock: Arc<dyn InboxClock>,
    ) -> Result<Self, RawOtlpInboxError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(RawOtlpInboxError::unavailable)?
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(BUSY_TIMEOUT)
            .connect_with(options)
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        let inbox = Self {
            pool,
            limits,
            clock,
        };
        if let Err(error) = inbox.initialize().await {
            inbox.pool.close().await;
            return Err(error);
        }
        Ok(inbox)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn initialize(&self) -> Result<(), RawOtlpInboxError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        sqlx::query(CREATE_SCHEMA_VERSIONS)
            .execute(&mut *transaction)
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        sqlx::query(
            "INSERT INTO flaggo_schema_versions(component, version)
             VALUES (?, ?)
             ON CONFLICT(component) DO NOTHING",
        )
        .bind(COMPONENT_NAME)
        .bind(SCHEMA_VERSION)
        .execute(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        let found_version: i64 =
            sqlx::query_scalar("SELECT version FROM flaggo_schema_versions WHERE component = ?")
                .bind(COMPONENT_NAME)
                .fetch_one(&mut *transaction)
                .await
                .map_err(RawOtlpInboxError::unavailable)?;
        if found_version != SCHEMA_VERSION {
            transaction
                .rollback()
                .await
                .map_err(RawOtlpInboxError::unavailable)?;
            return Err(RawOtlpInboxError::UnsupportedSchemaVersion {
                component: COMPONENT_NAME,
                found: found_version,
                expected: SCHEMA_VERSION,
            });
        }

        for statement in [CREATE_BATCHES, CREATE_RECEIPT_INDEX, CREATE_STATE] {
            sqlx::query(statement)
                .execute(&mut *transaction)
                .await
                .map_err(RawOtlpInboxError::unavailable)?;
        }
        sqlx::query(
            "INSERT INTO raw_otlp_inbox_state(
                singleton,
                write_revision,
                retained_batch_count,
                retained_payload_bytes,
                expired_batch_count,
                expired_payload_bytes,
                earliest_replay_at_unix_ms,
                last_received_at_unix_ms
             )
             VALUES (1, 0, 0, 0, 0, 0, NULL, NULL)
             ON CONFLICT(singleton) DO NOTHING",
        )
        .execute(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(RawOtlpInboxError::unavailable)
    }

    async fn expire_before(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        cutoff_unix_ms: i64,
    ) -> Result<(u64, u64), RawOtlpInboxError> {
        let (expired_batch_count, expired_payload_bytes): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(payload_length), 0)
             FROM raw_otlp_inbox_batches
             WHERE received_at_unix_ms < ?",
        )
        .bind(cutoff_unix_ms)
        .fetch_one(&mut **transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;

        if expired_batch_count == 0 {
            return Ok((0, 0));
        }
        let expired_batch_count_u64 = nonnegative("expired batch count", expired_batch_count)?;
        let expired_payload_bytes_u64 =
            nonnegative("expired payload bytes", expired_payload_bytes)?;

        sqlx::query(
            "DELETE FROM raw_otlp_inbox_batches
             WHERE received_at_unix_ms < ?",
        )
        .bind(cutoff_unix_ms)
        .execute(&mut **transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        sqlx::query(
            "UPDATE raw_otlp_inbox_state
             SET retained_batch_count = retained_batch_count - ?,
                 retained_payload_bytes = retained_payload_bytes - ?,
                 expired_batch_count = expired_batch_count + ?,
                 expired_payload_bytes = expired_payload_bytes + ?,
                 earliest_replay_at_unix_ms = CASE
                     WHEN earliest_replay_at_unix_ms IS NULL
                          OR earliest_replay_at_unix_ms < ?
                     THEN ?
                     ELSE earliest_replay_at_unix_ms
                 END
             WHERE singleton = 1",
        )
        .bind(expired_batch_count)
        .bind(expired_payload_bytes)
        .bind(expired_batch_count)
        .bind(expired_payload_bytes)
        .bind(cutoff_unix_ms)
        .bind(cutoff_unix_ms)
        .execute(&mut **transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        Ok((expired_batch_count_u64, expired_payload_bytes_u64))
    }

    async fn expire_due(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<(), RawOtlpInboxError> {
        let lock_result = sqlx::query(
            "UPDATE raw_otlp_inbox_state
             SET write_revision = write_revision + 1
             WHERE singleton = 1",
        )
        .execute(&mut **transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        if lock_result.rows_affected() != 1 {
            return Err(corrupt("singleton state row is missing"));
        }

        let (last_received_at_unix_ms, earliest_replay_at_unix_ms): (Option<i64>, Option<i64>) =
            sqlx::query_as(
                "SELECT last_received_at_unix_ms, earliest_replay_at_unix_ms
                 FROM raw_otlp_inbox_state
                 WHERE singleton = 1",
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        let retention_reference_unix_ms = last_received_at_unix_ms
            .into_iter()
            .chain(earliest_replay_at_unix_ms)
            .fold(self.clock.now().timestamp_millis(), i64::max);
        let cutoff_unix_ms =
            retention_reference_unix_ms.saturating_sub(self.limits.hard_retention_millis());
        Self::expire_before(transaction, cutoff_unix_ms).await?;
        Ok(())
    }

    fn decode_row(row: &SqliteRow) -> Result<RawOtlpInboxBatch, RawOtlpInboxError> {
        let raw_batch_id: i64 = row
            .try_get("inbox_batch_id")
            .map_err(RawOtlpInboxError::unavailable)?;
        let batch_id = positive_batch_id(raw_batch_id)?;
        let signal_value: String = row
            .try_get("signal_type")
            .map_err(RawOtlpInboxError::unavailable)?;
        let signal = OtlpSignal::from_storage(&signal_value)
            .ok_or_else(|| corrupt(format!("unknown signal type '{signal_value}'")))?;
        let encoding_value: String = row
            .try_get("wire_encoding")
            .map_err(RawOtlpInboxError::unavailable)?;
        let wire_encoding = OtlpWireEncoding::from_storage(&encoding_value)
            .ok_or_else(|| corrupt(format!("unknown wire encoding '{encoding_value}'")))?;
        let media_type: String = row
            .try_get("media_type")
            .map_err(RawOtlpInboxError::unavailable)?;
        if media_type != wire_encoding.media_type() {
            return Err(corrupt(format!(
                "media type '{media_type}' does not match wire encoding '{encoding_value}'"
            )));
        }
        let compression_value: String = row
            .try_get("transport_compression")
            .map_err(RawOtlpInboxError::unavailable)?;
        let transport_compression = OtlpTransportCompression::from_storage(&compression_value)
            .ok_or_else(|| {
                corrupt(format!(
                    "unknown transport compression '{compression_value}'"
                ))
            })?;
        let profile_value: String = row
            .try_get("otlp_profile_version")
            .map_err(RawOtlpInboxError::unavailable)?;
        let otlp_profile_version = profile_value
            .parse::<OtlpProfileVersion>()
            .map_err(|_| corrupt(format!("invalid OTLP profile version '{profile_value}'")))?;
        let received_at_unix_ms: i64 = row
            .try_get("received_at_unix_ms")
            .map_err(RawOtlpInboxError::unavailable)?;
        let received_at = DateTime::<Utc>::from_timestamp_millis(received_at_unix_ms)
            .ok_or_else(|| corrupt(format!("invalid receive time {received_at_unix_ms}")))?;
        let raw_payload_length: i64 = row
            .try_get("payload_length")
            .map_err(RawOtlpInboxError::unavailable)?;
        let payload_length = nonnegative("payload length", raw_payload_length)?;
        let raw_digest: Vec<u8> = row
            .try_get("payload_sha256")
            .map_err(RawOtlpInboxError::unavailable)?;
        let digest_bytes: [u8; 32] = raw_digest
            .try_into()
            .map_err(|value: Vec<u8>| corrupt(format!("invalid digest length {}", value.len())))?;
        let payload: Vec<u8> = row
            .try_get("payload")
            .map_err(RawOtlpInboxError::unavailable)?;
        if payload_length != u64::try_from(payload.len()).expect("payload length must fit u64") {
            return Err(corrupt(format!(
                "payload length {payload_length} does not match stored bytes {}",
                payload.len()
            )));
        }
        let computed_digest: [u8; 32] = Sha256::digest(&payload).into();
        if computed_digest != digest_bytes {
            return Err(corrupt("payload SHA-256 does not match stored bytes"));
        }

        Ok(RawOtlpInboxBatch {
            inbox_batch_id: batch_id,
            signal,
            wire_encoding,
            media_type,
            transport_compression,
            otlp_profile_version,
            received_at,
            payload_length,
            payload_sha256: PayloadSha256::new(digest_bytes),
            payload,
        })
    }
}

#[async_trait]
impl RawOtlpInbox for SqliteRawOtlpInbox {
    async fn append(
        &self,
        batch: NewRawOtlpBatch,
    ) -> Result<InboxAppendReceipt, RawOtlpInboxError> {
        let NewRawOtlpBatch {
            signal,
            wire_encoding,
            transport_compression,
            otlp_profile_version,
            payload,
        } = batch;
        let payload_length =
            u64::try_from(payload.len()).expect("in-memory payload length must fit u64");
        let payload_length_i64 =
            i64::try_from(payload_length).map_err(|_| RawOtlpInboxError::CapacityExceeded {
                retained_payload_bytes: 0,
                incoming_payload_bytes: payload_length,
                max_retained_payload_bytes: self.limits.max_retained_payload_bytes(),
            })?;
        let digest_bytes: [u8; 32] = Sha256::digest(&payload).into();
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        let lock_result = sqlx::query(
            "UPDATE raw_otlp_inbox_state
             SET write_revision = write_revision + 1
             WHERE singleton = 1",
        )
        .execute(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        if lock_result.rows_affected() != 1 {
            return Err(corrupt("singleton state row is missing"));
        }

        let (
            last_received_at_unix_ms,
            earliest_replay_at_unix_ms,
            retained_batch_count_i64,
            retained_payload_bytes_i64,
        ): (Option<i64>, Option<i64>, i64, i64) = sqlx::query_as(
            "SELECT
                last_received_at_unix_ms,
                earliest_replay_at_unix_ms,
                retained_batch_count,
                retained_payload_bytes
             FROM raw_otlp_inbox_state
             WHERE singleton = 1",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        let clock_unix_ms = self.clock.now().timestamp_millis();
        let received_at_unix_ms = last_received_at_unix_ms
            .into_iter()
            .chain(earliest_replay_at_unix_ms)
            .fold(clock_unix_ms, i64::max);
        let received_at = DateTime::<Utc>::from_timestamp_millis(received_at_unix_ms)
            .expect("clock-generated timestamp must remain representable");
        let cutoff_unix_ms =
            received_at_unix_ms.saturating_sub(self.limits.hard_retention_millis());
        let (expired_batch_count, expired_payload_bytes) =
            Self::expire_before(&mut transaction, cutoff_unix_ms).await?;
        let retained_batch_count = nonnegative("retained batch count", retained_batch_count_i64)?
            .checked_sub(expired_batch_count)
            .ok_or_else(|| corrupt("retained batch count is inconsistent with expiry"))?;
        let retained_payload_bytes =
            nonnegative("retained payload bytes", retained_payload_bytes_i64)?
                .checked_sub(expired_payload_bytes)
                .ok_or_else(|| corrupt("retained payload bytes are inconsistent with expiry"))?;
        let next_retained_payload_bytes = retained_payload_bytes
            .checked_add(payload_length)
            .filter(|required| *required <= self.limits.max_retained_payload_bytes());
        let Some(next_retained_payload_bytes) = next_retained_payload_bytes else {
            transaction
                .commit()
                .await
                .map_err(RawOtlpInboxError::unavailable)?;
            return Err(RawOtlpInboxError::CapacityExceeded {
                retained_payload_bytes,
                incoming_payload_bytes: payload_length,
                max_retained_payload_bytes: self.limits.max_retained_payload_bytes(),
            });
        };
        let next_retained_batch_count = retained_batch_count
            .checked_add(1)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or_else(|| corrupt("retained batch count exceeds SQLite integer range"))?;
        let next_retained_payload_bytes = i64::try_from(next_retained_payload_bytes)
            .expect("admitted payload total must fit SQLite integer");

        sqlx::query(
            "UPDATE raw_otlp_inbox_state
             SET last_received_at_unix_ms = ?,
                 retained_batch_count = ?,
                 retained_payload_bytes = ?
             WHERE singleton = 1",
        )
        .bind(received_at_unix_ms)
        .bind(next_retained_batch_count)
        .bind(next_retained_payload_bytes)
        .execute(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        let insert_result = sqlx::query(
            "INSERT INTO raw_otlp_inbox_batches(
                signal_type,
                wire_encoding,
                media_type,
                transport_compression,
                otlp_profile_version,
                received_at_unix_ms,
                payload_length,
                payload_sha256,
                payload
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(signal.as_str())
        .bind(wire_encoding.as_str())
        .bind(wire_encoding.media_type())
        .bind(transport_compression.as_str())
        .bind(otlp_profile_version.as_str())
        .bind(received_at_unix_ms)
        .bind(payload_length_i64)
        .bind(digest_bytes.as_slice())
        .bind(payload)
        .execute(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        let inbox_batch_id = positive_batch_id(insert_result.last_insert_rowid())?;
        transaction
            .commit()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;

        Ok(InboxAppendReceipt {
            inbox_batch_id,
            received_at,
            payload_length,
            payload_sha256: PayloadSha256::new(digest_bytes),
        })
    }

    async fn read_after(
        &self,
        after: Option<InboxBatchId>,
        limit: NonZeroU16,
    ) -> Result<Vec<RawOtlpInboxBatch>, RawOtlpInboxError> {
        let after = after.map_or(0, InboxBatchId::get);
        let after = i64::try_from(after)
            .map_err(|_| corrupt(format!("batch cursor {after} exceeds SQLite integer range")))?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        self.expire_due(&mut transaction).await?;
        let rows = sqlx::query(
            "SELECT
                inbox_batch_id,
                signal_type,
                wire_encoding,
                media_type,
                transport_compression,
                otlp_profile_version,
                received_at_unix_ms,
                payload_length,
                payload_sha256,
                payload
             FROM raw_otlp_inbox_batches
             WHERE inbox_batch_id > ?
             ORDER BY inbox_batch_id
             LIMIT ?",
        )
        .bind(after)
        .bind(i64::from(limit.get()))
        .fetch_all(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        rows.iter().map(Self::decode_row).collect()
    }

    async fn inspect(&self) -> Result<RawOtlpInboxHealth, RawOtlpInboxError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;
        self.expire_due(&mut transaction).await?;
        let state = sqlx::query(
            "SELECT
                retained_batch_count,
                retained_payload_bytes,
                expired_batch_count,
                expired_payload_bytes,
                earliest_replay_at_unix_ms
             FROM raw_otlp_inbox_state
             WHERE singleton = 1",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        let retained_batch_count = nonnegative(
            "retained batch count",
            state
                .try_get("retained_batch_count")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let retained_payload_bytes = nonnegative(
            "retained payload bytes",
            state
                .try_get("retained_payload_bytes")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let expired_batch_count = nonnegative(
            "expired batch count",
            state
                .try_get("expired_batch_count")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let expired_payload_bytes = nonnegative(
            "expired payload bytes",
            state
                .try_get("expired_payload_bytes")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let replay_boundary_unix_ms: Option<i64> = state
            .try_get("earliest_replay_at_unix_ms")
            .map_err(RawOtlpInboxError::unavailable)?;
        let statistics = sqlx::query(
            "SELECT
                MIN(received_at_unix_ms) AS oldest_retained_at_unix_ms,
                MAX(received_at_unix_ms) AS newest_retained_at_unix_ms
             FROM raw_otlp_inbox_batches",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(RawOtlpInboxError::unavailable)?;
        transaction
            .commit()
            .await
            .map_err(RawOtlpInboxError::unavailable)?;

        let oldest_retained_at = optional_timestamp(
            "oldest retained time",
            statistics
                .try_get("oldest_retained_at_unix_ms")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let newest_retained_at = optional_timestamp(
            "newest retained time",
            statistics
                .try_get("newest_retained_at_unix_ms")
                .map_err(RawOtlpInboxError::unavailable)?,
        )?;
        let replay_boundary =
            optional_timestamp("earliest replay boundary", replay_boundary_unix_ms)?;
        let earliest_replay_at = replay_boundary.or(oldest_retained_at);

        Ok(RawOtlpInboxHealth {
            retained_batch_count,
            retained_payload_bytes,
            oldest_retained_at,
            newest_retained_at,
            earliest_replay_at,
            expired_batch_count,
            expired_payload_bytes,
            limits: self.limits,
        })
    }
}

fn positive_batch_id(value: i64) -> Result<InboxBatchId, RawOtlpInboxError> {
    let value = u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(|| corrupt(format!("invalid batch ID {value}")))?;
    Ok(InboxBatchId::new(value))
}

fn nonnegative(name: &str, value: i64) -> Result<u64, RawOtlpInboxError> {
    u64::try_from(value).map_err(|_| corrupt(format!("{name} cannot be negative: {value}")))
}

fn optional_timestamp(
    name: &str,
    value: Option<i64>,
) -> Result<Option<DateTime<Utc>>, RawOtlpInboxError> {
    value
        .map(|timestamp| {
            DateTime::<Utc>::from_timestamp_millis(timestamp)
                .ok_or_else(|| corrupt(format!("{name} is out of range: {timestamp}")))
        })
        .transpose()
}

fn corrupt(message: impl Into<String>) -> RawOtlpInboxError {
    RawOtlpInboxError::CorruptData(message.into())
}
