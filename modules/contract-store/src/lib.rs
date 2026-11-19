pub mod artifacts;
pub mod cursor;
pub mod timestamp;

use chrono::{DateTime, FixedOffset};
#[cfg(feature = "sqlite")]
use flaggo_contract::digests::{
    ExpressionCanonicalizer, canonicalize_expressions, contract_digest,
};
#[cfg(feature = "sqlite")]
use flaggo_contract::strict_json;
use flaggo_contract::{ContractError, DecisionContract};
#[cfg(feature = "sqlite")]
use flaggo_contract::{ContractResourceId, ResourceScope};
#[cfg(feature = "sqlite")]
use flaggo_decision::{Cancellation, ContractVersion, ContractVersionReader, StoreReadError};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlite")]
use sqlx::{
    Row, SqlitePool, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
};
#[cfg(feature = "sqlite")]
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

pub const SCHEMA_VERSION: i64 = 4;
#[cfg(feature = "sqlite")]
const COMPONENT: &str = "contract-store";
#[cfg(feature = "sqlite")]
const SCHEMA: &str = "
    INSERT INTO flaggo_schema_versions(component, version)
    VALUES ('contract-store', 4) ON CONFLICT(component) DO NOTHING;
    CREATE TABLE IF NOT EXISTS decision_contract_resources (
        resource_key TEXT PRIMARY KEY, contract_name TEXT NOT NULL,
        tenant TEXT NOT NULL, application TEXT NOT NULL, environment TEXT NOT NULL,
        UNIQUE(tenant, application, environment, contract_name)
    );
    CREATE TABLE IF NOT EXISTS decision_contract_versions (
        resource_key TEXT NOT NULL, contract_digest TEXT NOT NULL,
        accepted_at TEXT NOT NULL, contract_json BLOB NOT NULL,
        PRIMARY KEY(resource_key, contract_digest),
        FOREIGN KEY(resource_key) REFERENCES decision_contract_resources(resource_key)
    );
    CREATE INDEX IF NOT EXISTS ix_contract_versions_history
    ON decision_contract_versions(resource_key, accepted_at DESC, contract_digest DESC);
    CREATE UNIQUE INDEX IF NOT EXISTS ux_contract_versions_digest
    ON decision_contract_versions(contract_digest);
    CREATE TABLE IF NOT EXISTS decision_contract_current (
        resource_key TEXT NOT NULL, contract_digest TEXT NOT NULL,
        PRIMARY KEY(resource_key),
        FOREIGN KEY(resource_key, contract_digest)
            REFERENCES decision_contract_versions(resource_key, contract_digest)
    );
";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptedContractVersion {
    pub contract_digest: String,
    #[serde(with = "timestamp")]
    pub accepted_at: DateTime<FixedOffset>,
    pub contract: DecisionContract,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractVersionPage {
    pub versions: Vec<AcceptedContractVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContractStoreWriteResult {
    Created,
    Existing,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractStoreError {
    #[error("{message} (Parameter '{parameter}')")]
    InvalidArgument {
        parameter: &'static str,
        message: String,
    },
    #[error("Value cannot be null. (Parameter '{0}')")]
    NullArgument(&'static str),
    #[error(
        "Unsupported contract-store schema version {0}; expected 4. Migrate v3 explicitly before opening v4."
    )]
    UnsupportedSchema(i64),
    #[error("Contract version '{contract_name}' at '{contract_digest}' does not exist.")]
    NotFound {
        contract_name: String,
        contract_digest: String,
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

impl ContractStoreError {
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
pub use sqlite::SqliteContractVersionStore;

#[cfg(feature = "sqlite")]
mod sqlite {
    use super::*;
    use crate::artifacts::validate_scope;
    mod migration;

    #[derive(Clone)]
    pub struct SqliteContractVersionStore {
        pool: SqlitePool,
        canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
    }

    impl SqliteContractVersionStore {
        /// Compose logical stores over one SQLite transaction domain.
        pub fn from_pool(
            pool: SqlitePool,
            canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
        ) -> Self {
            Self {
                pool,
                canonicalizer,
            }
        }

        pub async fn open(
            path: impl AsRef<Path>,
            canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
        ) -> Result<Self, ContractStoreError> {
            let options = SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5));
            let pool = SqlitePoolOptions::new()
                .max_connections(4)
                .connect_with(options)
                .await?;
            Ok(Self {
                pool,
                canonicalizer,
            })
        }

        pub async fn close(&self) {
            self.pool.close().await;
        }

        pub async fn initialize(
            &self,
            cancellation: Cancellation,
        ) -> Result<(), ContractStoreError> {
            check(&cancellation)?;
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await?;
            check(&cancellation)?;
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
            component TEXT PRIMARY KEY, version INTEGER NOT NULL
        )",
            )
            .execute(&self.pool)
            .await?;
            let version: Option<i64> = sqlx::query_scalar(
                "SELECT version FROM flaggo_schema_versions WHERE component = ?",
            )
            .bind(COMPONENT)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(version) = version
                && version != SCHEMA_VERSION
            {
                return Err(ContractStoreError::UnsupportedSchema(version));
            }
            check(&cancellation)?;
            sqlx::raw_sql(SCHEMA).execute(&self.pool).await?;
            Ok(())
        }

        pub async fn put(
            &self,
            version: &AcceptedContractVersion,
            cancellation: Cancellation,
        ) -> Result<ContractStoreWriteResult, ContractStoreError> {
            check(&cancellation)?;
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let result = self
                .put_in(&mut transaction, version, cancellation.clone())
                .await?;
            check(&cancellation)?;
            transaction.commit().await?;
            Ok(result)
        }

        /// Adapter-only operation; the caller owns the surrounding transaction.
        pub async fn put_in(
            &self,
            connection: &mut sqlx::SqliteConnection,
            version: &AcceptedContractVersion,
            cancellation: Cancellation,
        ) -> Result<ContractStoreWriteResult, ContractStoreError> {
            validate_scope(&version.contract.authority)?;
            if version.contract.name != version.contract.name.trim() {
                return Err(argument(
                    "version",
                    "Contract name cannot contain surrounding whitespace.",
                ));
            }
            let canonicalizer = self
                .canonicalizer
                .as_deref()
                .map(|value| value as &dyn ExpressionCanonicalizer);
            let digest = contract_digest(&version.contract, canonicalizer)?;
            if digest != version.contract_digest {
                return Err(argument(
                    "version",
                    format!(
                        "Contract digest mismatch: expected {digest}, received {}.",
                        version.contract_digest,
                    ),
                ));
            }
            let stored = match canonicalizer {
                Some(canonicalizer) => canonicalize_expressions(&version.contract, canonicalizer)?,
                None => version.contract.clone(),
            };
            check(&cancellation)?;
            let scope = &version.contract.authority;
            let resource = ContractResourceId::try_from(&version.contract)?;
            let key = resource.resource_key()?;
            sqlx::query("INSERT INTO decision_contract_resources(resource_key, contract_name, tenant, application, environment)
            VALUES (?, ?, ?, ?, ?) ON CONFLICT(resource_key) DO NOTHING")
            .bind(&key).bind(&version.contract.name).bind(&scope.tenant).bind(&scope.application).bind(&scope.environment)
            .execute(&mut *connection).await?;
            check(&cancellation)?;
            let rows = sqlx::query(
                "INSERT INTO decision_contract_versions(
            resource_key, contract_digest, accepted_at, contract_json
        ) VALUES (?, ?, ?, ?) ON CONFLICT(resource_key, contract_digest) DO NOTHING",
            )
            .bind(&key)
            .bind(&version.contract_digest)
            .bind(timestamp::format_exact(version.accepted_at)?)
            .bind(strict_json::serialize(&stored)?)
            .execute(&mut *connection)
            .await?
            .rows_affected();
            check(&cancellation)?;
            Ok(if rows == 1 {
                ContractStoreWriteResult::Created
            } else {
                ContractStoreWriteResult::Existing
            })
        }

        pub async fn get(
            &self,
            resource: &ContractResourceId,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<Option<AcceptedContractVersion>, ContractStoreError> {
            check(&cancellation)?;
            let mut connection = self.pool.acquire().await?;
            self.get_in(&mut connection, resource, digest, cancellation)
                .await
        }

        /// Read and verify an immutable version inside the caller's transaction.
        pub async fn get_in(
            &self,
            connection: &mut sqlx::SqliteConnection,
            resource: &ContractResourceId,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<Option<AcceptedContractVersion>, ContractStoreError> {
            check(&cancellation)?;
            let key = resource.resource_key()?;
            let row = sqlx::query(
                "SELECT contract_digest, accepted_at, contract_json
            FROM decision_contract_versions WHERE resource_key = ? AND contract_digest = ?",
            )
            .bind(&key)
            .bind(digest)
            .fetch_optional(connection)
            .await?;
            check(&cancellation)?;
            row.as_ref().map(|row| read_version(row, &key)).transpose()
        }

        pub async fn get_current(
            &self,
            resource: &ContractResourceId,
            cancellation: Cancellation,
        ) -> Result<Option<AcceptedContractVersion>, ContractStoreError> {
            check(&cancellation)?;
            let key = resource.resource_key()?;
            let row = sqlx::query("SELECT versions.contract_digest, versions.accepted_at, versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
            ON versions.resource_key = current.resource_key AND versions.contract_digest = current.contract_digest
            WHERE current.resource_key = ?").bind(&key).fetch_optional(&self.pool).await?;
            check(&cancellation)?;
            row.as_ref().map(|row| read_version(row, &key)).transpose()
        }

        pub async fn list_all_current(
            &self,
            scope: &ResourceScope,
            cancellation: Cancellation,
        ) -> Result<Vec<AcceptedContractVersion>, ContractStoreError> {
            check(&cancellation)?;
            scope.validate()?;
            let rows = sqlx::query("SELECT versions.resource_key, versions.contract_digest,
            versions.accepted_at, versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
            ON versions.resource_key = current.resource_key AND versions.contract_digest = current.contract_digest
            JOIN decision_contract_resources AS resources ON resources.resource_key = versions.resource_key
            WHERE resources.tenant = ? AND resources.application = ? AND resources.environment = ?
            ORDER BY resources.contract_name ASC, versions.contract_digest ASC")
            .bind(&scope.tenant).bind(&scope.application).bind(&scope.environment).fetch_all(&self.pool).await?;
            check(&cancellation)?;
            rows.iter()
                .map(|row| {
                    check(&cancellation)?;
                    let version = read_version(row, row.try_get("resource_key")?)?;
                    if ResourceScope::try_from(&version.contract.authority)? != *scope {
                        return Err(ContractStoreError::Integrity(
                            "Stored resource namespace disagrees with catalog scope.".into(),
                        ));
                    }
                    Ok(version)
                })
                .collect()
        }

        pub async fn list(
            &self,
            resource: &ContractResourceId,
            page_size: i32,
            cursor: Option<&str>,
            cancellation: Cancellation,
        ) -> Result<ContractVersionPage, ContractStoreError> {
            if !(1..=200).contains(&page_size) {
                return Err(argument("pageSize", "Page size must be between 1 and 200."));
            }
            let boundary = cursor.map(decode_cursor).transpose()?;
            let key = resource.resource_key()?;
            check(&cancellation)?;
            let rows = sqlx::query(
                "SELECT contract_digest, accepted_at, contract_json
            FROM decision_contract_versions WHERE resource_key = ?
            AND (? IS NULL OR accepted_at < ? OR (accepted_at = ? AND contract_digest < ?))
            ORDER BY accepted_at DESC, contract_digest DESC LIMIT ?",
            )
            .bind(&key)
            .bind(boundary.as_ref().map(|boundary| &boundary.0))
            .bind(boundary.as_ref().map(|boundary| &boundary.0))
            .bind(boundary.as_ref().map(|boundary| &boundary.0))
            .bind(boundary.as_ref().map(|boundary| &boundary.1))
            .bind(page_size + 1)
            .fetch_all(&self.pool)
            .await?;
            check(&cancellation)?;
            let mut versions: Vec<_> = rows
                .iter()
                .map(|row| {
                    check(&cancellation)?;
                    read_version(row, &key)
                })
                .collect::<Result<_, ContractStoreError>>()?;
            let next_cursor = if versions.len() > page_size as usize {
                versions.pop();
                let last = versions.last().expect("positive page size");
                Some(encode_cursor(last.accepted_at, &last.contract_digest)?)
            } else {
                None
            };
            Ok(ContractVersionPage {
                versions,
                next_cursor,
            })
        }

        pub async fn set_current(
            &self,
            resource: &ContractResourceId,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<(), ContractStoreError> {
            check(&cancellation)?;
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            self.set_current_in(&mut transaction, resource, digest, cancellation.clone())
                .await?;
            check(&cancellation)?;
            transaction.commit().await?;
            Ok(())
        }

        /// Adapter-only publication; the caller owns the surrounding transaction.
        pub async fn set_current_in(
            &self,
            connection: &mut sqlx::SqliteConnection,
            resource: &ContractResourceId,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<(), ContractStoreError> {
            check(&cancellation)?;
            let key = resource.resource_key()?;
            let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM decision_contract_versions WHERE resource_key = ? AND contract_digest = ?",
        ).bind(&key).bind(digest).fetch_optional(&mut *connection).await?;
            if exists.is_none() {
                return Err(ContractStoreError::NotFound {
                    contract_name: resource.name.clone(),
                    contract_digest: digest.into(),
                });
            }
            check(&cancellation)?;
            sqlx::query("INSERT INTO decision_contract_current(resource_key, contract_digest)
            VALUES (?, ?) ON CONFLICT(resource_key) DO UPDATE SET contract_digest = excluded.contract_digest")
            .bind(&key).bind(digest).execute(&mut *connection).await?;
            check(&cancellation)?;
            Ok(())
        }

        pub async fn is_available(
            &self,
            cancellation: Cancellation,
        ) -> Result<bool, ContractStoreError> {
            check(&cancellation)?;
            let result = async {
            let row = sqlx::query("SELECT version FROM flaggo_schema_versions WHERE component = ?")
                .bind(COMPONENT).fetch_optional(&self.pool).await?;
            let Some(row) = row else { return Ok(false) };
            if row.try_get_raw("version")?.type_info().name() != "INTEGER"
                || row.try_get::<i64, _>("version")? != SCHEMA_VERSION { return Ok(false); }
            for query in [
                "SELECT resource_key, contract_name, tenant, application, environment FROM decision_contract_resources LIMIT 0",
                "SELECT resource_key, contract_digest, accepted_at, contract_json FROM decision_contract_versions LIMIT 0",
                "SELECT resource_key, contract_digest FROM decision_contract_current LIMIT 0",
            ] {
                check(&cancellation)?;
                sqlx::query(query).fetch_optional(&self.pool).await?;
            }
            Ok(true)
        }.await;
            match result {
                Err(ContractStoreError::Database(sqlx::Error::Database(_))) => Ok(false),
                result => result,
            }
        }
    }

    impl flaggo_decision::ReadinessProbe for SqliteContractVersionStore {
        async fn is_available(&self, cancellation: Cancellation) -> Result<bool, StoreReadError> {
            self.is_available(cancellation).await.map_err(read_error)
        }
    }

    impl ContractVersionReader for SqliteContractVersionStore {
        async fn get_contract(
            &self,
            resource: &ContractResourceId,
            digest: &str,
            cancellation: Cancellation,
        ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
            self.get(resource, digest, cancellation)
                .await
                .map(|version| {
                    version.map(|version| {
                        Arc::new(ContractVersion {
                            contract_digest: version.contract_digest,
                            contract: version.contract,
                        })
                    })
                })
                .map_err(read_error)
        }
    }

    fn read_error(error: ContractStoreError) -> StoreReadError {
        match error {
            ContractStoreError::Integrity(_) => StoreReadError::Integrity(Box::new(error)),
            ContractStoreError::Cancelled => StoreReadError::Cancelled,
            ContractStoreError::Database(sqlx::Error::PoolTimedOut) => {
                StoreReadError::Timeout(Box::new(error))
            }
            ContractStoreError::Database(
                sqlx::Error::Database(_) | sqlx::Error::Io(_) | sqlx::Error::PoolClosed,
            ) => StoreReadError::Dependency(Box::new(error)),
            ContractStoreError::InvalidArgument { .. }
            | ContractStoreError::NullArgument(_)
            | ContractStoreError::Contract(ContractError::Serialization(_)) => {
                StoreReadError::InvalidArgument(Box::new(error))
            }
            _ => StoreReadError::Native(Box::new(error)),
        }
    }

    fn read_version(
        row: &SqliteRow,
        key: &str,
    ) -> Result<AcceptedContractVersion, ContractStoreError> {
        let accepted_at = timestamp::parse_exact(row.try_get("accepted_at")?)?;
        let bytes: Vec<u8> = row.try_get("contract_json")?;
        let digest: String = row.try_get("contract_digest")?;
        let contract = artifacts::decode_persisted_contract(key, &digest, &bytes)?;
        Ok(AcceptedContractVersion {
            accepted_at,
            contract_digest: digest,
            contract,
        })
    }

    fn argument(parameter: &'static str, message: impl Into<String>) -> ContractStoreError {
        ContractStoreError::InvalidArgument {
            parameter,
            message: message.into(),
        }
    }

    fn check(cancellation: &Cancellation) -> Result<(), ContractStoreError> {
        if cancellation
            .as_ref()
            .is_some_and(|value| value.load(Ordering::Acquire))
        {
            Err(ContractStoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn encode_cursor(
        time: DateTime<FixedOffset>,
        digest: &str,
    ) -> Result<String, ContractStoreError> {
        Ok(cursor::encode(time, digest)?)
    }

    fn decode_cursor(cursor: &str) -> Result<(String, String), ContractStoreError> {
        self::cursor::decode(cursor)
            .map_err(|_| argument("cursor", "Invalid contract-version cursor."))
    }
}
