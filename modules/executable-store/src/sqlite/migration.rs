use super::*;

impl SqliteExecutableStore {
    pub(super) async fn migrate_scoped_resources_in(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<bool> {
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT version FROM flaggo_schema_versions WHERE component = 'executable-store'",
        )
        .fetch_optional(&mut *connection)
        .await?;
        if version == Some(SCHEMA_VERSION) {
            return Ok(false);
        }
        if !matches!(version, Some(4 | 5)) {
            return Err(ExecutableStoreError::UnsupportedSchema(
                version.unwrap_or_default(),
            ));
        }
        let contract_version: Option<i64> = sqlx::query_scalar(
            "SELECT version FROM flaggo_schema_versions WHERE component = 'contract-store'",
        )
        .fetch_optional(&mut *connection)
        .await?;
        if contract_version != Some(flaggo_contract_store::SCHEMA_VERSION) {
            return Err(integrity(
                "Scoped contract migration must precede executable migration in the same authority transaction.",
            ));
        }
        if version == Some(4) {
            sqlx::raw_sql(schema::MIGRATE_FOUR)
                .execute(&mut *connection)
                .await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analysis_candidate_admissions")
            .fetch_one(&mut *connection)
            .await?;
        sqlx::raw_sql(
            "CREATE TABLE analysis_candidate_admissions_v6 (
                resource_key TEXT NOT NULL,
                workspace_id TEXT NOT NULL, cycle_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
                contract_name TEXT NOT NULL, contract_digest TEXT NOT NULL, executable_digest TEXT NOT NULL,
                analysis_manifest_digest TEXT NOT NULL, created_at TEXT NOT NULL,
                PRIMARY KEY(resource_key, workspace_id, cycle_id),
                FOREIGN KEY(executable_digest) REFERENCES decision_executables(executable_digest)
            );
            INSERT INTO analysis_candidate_admissions_v6
            SELECT resource.resource_key, admission.workspace_id, admission.cycle_id, admission.attempt_id,
                   admission.contract_name, admission.contract_digest, admission.executable_digest,
                   admission.analysis_manifest_digest, admission.created_at
            FROM analysis_candidate_admissions AS admission
            JOIN decision_contract_versions AS version ON version.contract_digest = admission.contract_digest
            JOIN decision_contract_resources AS resource ON resource.resource_key = version.resource_key
                AND resource.contract_name = admission.contract_name;",
        ).execute(&mut *connection).await?;
        let copied: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analysis_candidate_admissions_v6")
                .fetch_one(&mut *connection)
                .await?;
        if copied != count {
            return Err(integrity(
                "Cannot migrate admissions with missing or ambiguous accepted resource identity.",
            ));
        }
        sqlx::raw_sql(
            "DROP TABLE analysis_candidate_admissions;
             ALTER TABLE analysis_candidate_admissions_v6 RENAME TO analysis_candidate_admissions;",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::raw_sql(schema::SCHEMA)
            .execute(&mut *connection)
            .await?;
        if sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *connection)
            .await?
            .is_some()
        {
            return Err(integrity(
                "Scoped migration produced invalid executable references.",
            ));
        }
        Ok(true)
    }
}
