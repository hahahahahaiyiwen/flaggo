pub mod cursor;
pub mod timestamp;

use chrono::{DateTime, FixedOffset};
use flaggo_contract::digests::{
    ExpressionCanonicalizer, canonicalize_expressions, contract_digest,
};
use flaggo_contract::{AuthorityScope, ContractError, DecisionContract, strict_json};
use flaggo_decision::{Cancellation, ContractVersion, ContractVersionReader, StoreReadError};
use serde::{Deserialize, Serialize};
use sqlx::{
    Row, SqlitePool, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

pub const SCHEMA_VERSION: i64 = 3;
const COMPONENT: &str = "contract-store";
const SCHEMA: &str = "
    INSERT INTO flaggo_schema_versions(component, version)
    VALUES ('contract-store', 3) ON CONFLICT(component) DO NOTHING;
    CREATE TABLE IF NOT EXISTS decision_contract_resources (
        contract_name TEXT PRIMARY KEY,
        tenant TEXT NOT NULL, application TEXT NOT NULL, environment TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS decision_contract_versions (
        contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL,
        accepted_at TEXT NOT NULL, contract_json BLOB NOT NULL,
        PRIMARY KEY(contract_name, contract_digest),
        FOREIGN KEY(contract_name) REFERENCES decision_contract_resources(contract_name)
    );
    CREATE INDEX IF NOT EXISTS ix_contract_versions_history
    ON decision_contract_versions(contract_name, accepted_at DESC, contract_digest DESC);
    CREATE TABLE IF NOT EXISTS decision_contract_current (
        contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL,
        PRIMARY KEY(contract_name),
        FOREIGN KEY(contract_name, contract_digest)
            REFERENCES decision_contract_versions(contract_name, contract_digest)
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
    #[error("Unsupported contract-store schema version {0}; expected 3.")]
    UnsupportedSchema(i64),
    #[error("Contract version '{contract_name}' at '{contract_digest}' does not exist.")]
    NotFound {
        contract_name: String,
        contract_digest: String,
    },
    #[error("Contract name '{contract_name}' belongs to authority '{}', not '{}'.",
        authority_display(.existing_authority), authority_display(.requested_authority))]
    AuthorityConflict {
        contract_name: String,
        existing_authority: Box<AuthorityScope>,
        requested_authority: AuthorityScope,
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
    Database(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct SqliteContractVersionStore {
    pool: SqlitePool,
    canonicalizer: Option<Arc<dyn ExpressionCanonicalizer + Send + Sync>>,
}

impl SqliteContractVersionStore {
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

    pub async fn initialize(&self, cancellation: Cancellation) -> Result<(), ContractStoreError> {
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
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM flaggo_schema_versions WHERE component = ?")
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
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let scope = &version.contract.authority;
        sqlx::query("INSERT INTO decision_contract_resources(contract_name, tenant, application, environment)
            VALUES (?, ?, ?, ?) ON CONFLICT(contract_name) DO NOTHING")
            .bind(&version.contract.name).bind(&scope.tenant).bind(&scope.application).bind(&scope.environment)
            .execute(&mut *transaction).await?;
        check(&cancellation)?;
        let row = sqlx::query(
            "SELECT tenant, application, environment
            FROM decision_contract_resources WHERE contract_name = ?",
        )
        .bind(&version.contract.name)
        .fetch_one(&mut *transaction)
        .await?;
        let existing = AuthorityScope {
            tenant: Some(row.try_get("tenant")?),
            application: Some(row.try_get("application")?),
            environment: Some(row.try_get("environment")?),
        };
        if existing != *scope {
            return Err(ContractStoreError::AuthorityConflict {
                contract_name: version.contract.name.clone(),
                existing_authority: Box::new(existing),
                requested_authority: scope.clone(),
            });
        }
        check(&cancellation)?;
        let rows = sqlx::query(
            "INSERT INTO decision_contract_versions(
            contract_name, contract_digest, accepted_at, contract_json
        ) VALUES (?, ?, ?, ?) ON CONFLICT(contract_name, contract_digest) DO NOTHING",
        )
        .bind(&version.contract.name)
        .bind(&version.contract_digest)
        .bind(timestamp::format_exact(version.accepted_at)?)
        .bind(strict_json::serialize(&stored)?)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        check(&cancellation)?;
        transaction.commit().await?;
        Ok(if rows == 1 {
            ContractStoreWriteResult::Created
        } else {
            ContractStoreWriteResult::Existing
        })
    }

    pub async fn get(
        &self,
        name: &str,
        digest: &str,
        cancellation: Cancellation,
    ) -> Result<Option<AcceptedContractVersion>, ContractStoreError> {
        check(&cancellation)?;
        let row = sqlx::query(
            "SELECT contract_digest, accepted_at, contract_json
            FROM decision_contract_versions WHERE contract_name = ? AND contract_digest = ?",
        )
        .bind(name)
        .bind(digest)
        .fetch_optional(&self.pool)
        .await?;
        check(&cancellation)?;
        row.as_ref().map(|row| read_version(row, name)).transpose()
    }

    pub async fn get_current(
        &self,
        name: &str,
        cancellation: Cancellation,
    ) -> Result<Option<AcceptedContractVersion>, ContractStoreError> {
        check(&cancellation)?;
        let row = sqlx::query("SELECT versions.contract_digest, versions.accepted_at, versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
            ON versions.contract_name = current.contract_name AND versions.contract_digest = current.contract_digest
            WHERE current.contract_name = ?").bind(name).fetch_optional(&self.pool).await?;
        check(&cancellation)?;
        row.as_ref().map(|row| read_version(row, name)).transpose()
    }

    pub async fn list_all_current(
        &self,
        cancellation: Cancellation,
    ) -> Result<Vec<AcceptedContractVersion>, ContractStoreError> {
        check(&cancellation)?;
        let rows = sqlx::query("SELECT versions.contract_name, versions.contract_digest,
            versions.accepted_at, versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
            ON versions.contract_name = current.contract_name AND versions.contract_digest = current.contract_digest
            JOIN decision_contract_resources AS resources ON resources.contract_name = versions.contract_name
            ORDER BY resources.tenant ASC, resources.application ASC, resources.environment ASC,
                versions.contract_name ASC, versions.contract_digest ASC").fetch_all(&self.pool).await?;
        check(&cancellation)?;
        rows.iter()
            .map(|row| {
                check(&cancellation)?;
                read_version(row, row.try_get("contract_name")?)
            })
            .collect()
    }

    pub async fn list(
        &self,
        name: &str,
        page_size: i32,
        cursor: Option<&str>,
        cancellation: Cancellation,
    ) -> Result<ContractVersionPage, ContractStoreError> {
        if !(1..=200).contains(&page_size) {
            return Err(argument("pageSize", "Page size must be between 1 and 200."));
        }
        let boundary = cursor.map(decode_cursor).transpose()?;
        check(&cancellation)?;
        let rows = sqlx::query(
            "SELECT contract_digest, accepted_at, contract_json
            FROM decision_contract_versions WHERE contract_name = ?
            AND (? IS NULL OR accepted_at < ? OR (accepted_at = ? AND contract_digest < ?))
            ORDER BY accepted_at DESC, contract_digest DESC LIMIT ?",
        )
        .bind(name)
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
                read_version(row, name)
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
        name: &str,
        digest: &str,
        cancellation: Cancellation,
    ) -> Result<(), ContractStoreError> {
        check(&cancellation)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM decision_contract_versions WHERE contract_name = ? AND contract_digest = ?",
        ).bind(name).bind(digest).fetch_optional(&mut *transaction).await?;
        if exists.is_none() {
            return Err(ContractStoreError::NotFound {
                contract_name: name.into(),
                contract_digest: digest.into(),
            });
        }
        check(&cancellation)?;
        sqlx::query("INSERT INTO decision_contract_current(contract_name, contract_digest)
            VALUES (?, ?) ON CONFLICT(contract_name) DO UPDATE SET contract_digest = excluded.contract_digest")
            .bind(name).bind(digest).execute(&mut *transaction).await?;
        check(&cancellation)?;
        transaction.commit().await?;
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
                "SELECT contract_name, tenant, application, environment FROM decision_contract_resources LIMIT 0",
                "SELECT contract_name, contract_digest, accepted_at, contract_json FROM decision_contract_versions LIMIT 0",
                "SELECT contract_name, contract_digest FROM decision_contract_current LIMIT 0",
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

impl ContractVersionReader for SqliteContractVersionStore {
    async fn get_contract(
        &self,
        name: &str,
        digest: &str,
        cancellation: Cancellation,
    ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        self.get(name, digest, cancellation)
            .await
            .map(|version| {
                version.map(|version| {
                    Arc::new(ContractVersion {
                        contract_digest: version.contract_digest,
                        contract: version.contract,
                    })
                })
            })
            .map_err(|error| match error {
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
            })
    }
}

fn read_version(
    row: &SqliteRow,
    name: &str,
) -> Result<AcceptedContractVersion, ContractStoreError> {
    let accepted_at = timestamp::parse_exact(row.try_get("accepted_at")?)?;
    let bytes: Vec<u8> = row.try_get("contract_json")?;
    let contract: DecisionContract = strict_json::deserialize(&bytes)?;
    if contract.name != name {
        return Err(ContractStoreError::Integrity(format!(
            "Stored contract name '{}' does not match key '{name}'.",
            contract.name,
        )));
    }
    validate_scope(&contract.authority)?;
    let digest: String = row.try_get("contract_digest")?;
    let computed = contract_digest(&contract, None)?;
    if computed != digest {
        return Err(ContractStoreError::Integrity(format!(
            "Stored contract digest '{digest}' does not match payload '{computed}'.",
        )));
    }
    Ok(AcceptedContractVersion {
        accepted_at,
        contract_digest: digest,
        contract,
    })
}

fn validate_scope(scope: &AuthorityScope) -> Result<(), ContractStoreError> {
    for (parameter, value) in [
        ("scope.Tenant", &scope.tenant),
        ("scope.Application", &scope.application),
        ("scope.Environment", &scope.environment),
    ] {
        let Some(value) = value else {
            return Err(ContractStoreError::NullArgument(parameter));
        };
        if value.trim().is_empty() {
            return Err(argument(
                parameter,
                "The value cannot be an empty string or composed entirely of whitespace.",
            ));
        }
    }
    Ok(())
}

fn argument(parameter: &'static str, message: impl Into<String>) -> ContractStoreError {
    ContractStoreError::InvalidArgument {
        parameter,
        message: message.into(),
    }
}

fn authority_display(scope: &AuthorityScope) -> String {
    format!(
        "{}/{}/{}",
        scope.tenant.as_deref().unwrap_or(""),
        scope.application.as_deref().unwrap_or(""),
        scope.environment.as_deref().unwrap_or("")
    )
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

fn encode_cursor(time: DateTime<FixedOffset>, digest: &str) -> Result<String, ContractStoreError> {
    Ok(cursor::encode(time, digest)?)
}

fn decode_cursor(cursor: &str) -> Result<(String, String), ContractStoreError> {
    self::cursor::decode(cursor).map_err(|_| argument("cursor", "Invalid contract-version cursor."))
}
