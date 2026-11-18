use std::collections::BTreeMap;

use sqlx::SqlSafeStr;

use super::*;

impl SqliteContractVersionStore {
    /// Explicit local v3-to-v4 migration. Payload bytes, digests and acceptance
    /// timestamps are copied unchanged; executable/provenance tables are untouched.
    pub async fn migrate_v3_to_v4(&self) -> Result<bool, ContractStoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed = self.migrate_v3_to_v4_in(&mut tx).await?;
        tx.commit().await?;
        Ok(changed)
    }

    /// Adapter-only migration inside the caller's authority transaction.
    pub async fn migrate_v3_to_v4_in(
        &self,
        tx: &mut sqlx::SqliteConnection,
    ) -> Result<bool, ContractStoreError> {
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT version FROM flaggo_schema_versions WHERE component = 'contract-store'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if version == Some(SCHEMA_VERSION) {
            return Ok(false);
        }
        if version != Some(3) {
            return Err(ContractStoreError::UnsupportedSchema(
                version.unwrap_or_default(),
            ));
        }
        if sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
        {
            return Err(ContractStoreError::Integrity(
                "Cannot migrate a database with foreign-key violations.".into(),
            ));
        }
        let resources = sqlx::query(
            "SELECT contract_name, tenant, application, environment FROM decision_contract_resources",
        ).fetch_all(&mut *tx).await?;
        let versions = sqlx::query(
            "SELECT contract_name, contract_digest, accepted_at, contract_json FROM decision_contract_versions",
        ).fetch_all(&mut *tx).await?;
        let current =
            sqlx::query("SELECT contract_name, contract_digest FROM decision_contract_current")
                .fetch_all(&mut *tx)
                .await?;
        let mut keys = BTreeMap::new();
        for row in &resources {
            let name: String = row.try_get("contract_name")?;
            let resource = ContractResourceId::new(
                ResourceScope::new(
                    row.try_get("tenant")?,
                    row.try_get("application")?,
                    row.try_get("environment")?,
                )?,
                name.clone(),
            )?;
            if keys.insert(name, resource.resource_key()?).is_some() {
                return Err(ContractStoreError::Integrity(
                    "Duplicate legacy resource ownership.".into(),
                ));
            }
        }
        for row in &versions {
            let name: String = row.try_get("contract_name")?;
            let key = keys.get(&name).ok_or_else(|| {
                ContractStoreError::Integrity("Legacy version has no resource owner.".into())
            })?;
            read_version(row, key)?;
        }
        let staging = SCHEMA
            .replace(
                "decision_contract_resources",
                "decision_contract_resources_v4",
            )
            .replace(
                "decision_contract_versions",
                "decision_contract_versions_v4",
            )
            .replace("decision_contract_current", "decision_contract_current_v4");
        sqlx::raw_sql(sqlx::AssertSqlSafe(staging).into_sql_str())
            .execute(&mut *tx)
            .await?;
        for row in &resources {
            let name: String = row.try_get("contract_name")?;
            sqlx::query(
                "INSERT INTO decision_contract_resources_v4(resource_key, contract_name, tenant, application, environment) VALUES (?, ?, ?, ?, ?)",
            ).bind(&keys[&name]).bind(name).bind(row.try_get::<String, _>("tenant")?)
                .bind(row.try_get::<String, _>("application")?).bind(row.try_get::<String, _>("environment")?)
                .execute(&mut *tx).await?;
        }
        for row in &versions {
            let name: String = row.try_get("contract_name")?;
            sqlx::query(
                "INSERT INTO decision_contract_versions_v4(resource_key, contract_digest, accepted_at, contract_json) VALUES (?, ?, ?, ?)",
            ).bind(&keys[&name]).bind(row.try_get::<String, _>("contract_digest")?)
                .bind(row.try_get::<String, _>("accepted_at")?).bind(row.try_get::<Vec<u8>, _>("contract_json")?)
                .execute(&mut *tx).await?;
        }
        for row in &current {
            let name: String = row.try_get("contract_name")?;
            let key = keys.get(&name).ok_or_else(|| {
                ContractStoreError::Integrity(
                    "Legacy current pointer has no resource owner.".into(),
                )
            })?;
            sqlx::query(
                "INSERT INTO decision_contract_current_v4(resource_key, contract_digest) VALUES (?, ?)",
            ).bind(key).bind(row.try_get::<String, _>("contract_digest")?)
                .execute(&mut *tx).await?;
        }
        sqlx::raw_sql(
            "DROP TABLE decision_contract_current;
             DROP TABLE decision_contract_versions;
             DROP TABLE decision_contract_resources;
             ALTER TABLE decision_contract_resources_v4 RENAME TO decision_contract_resources;
             ALTER TABLE decision_contract_versions_v4 RENAME TO decision_contract_versions;
             ALTER TABLE decision_contract_current_v4 RENAME TO decision_contract_current;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(SCHEMA).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE flaggo_schema_versions SET version = 4 WHERE component = 'contract-store'",
        )
        .execute(&mut *tx)
        .await?;
        if sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
        {
            return Err(ContractStoreError::Integrity(
                "Migration produced invalid authority references.".into(),
            ));
        }
        Ok(true)
    }
}
