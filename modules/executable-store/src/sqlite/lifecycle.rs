use super::*;
use crate::policy::{
    LifecycleSnapshot, ResolutionPlan, ResolutionSnapshot, activation_conflict, candidate_conflict,
    next_version,
};

impl SqliteExecutableStore {
    pub async fn activate(
        &self,
        contract: &str,
        digest: &str,
        expected: Option<&str>,
        cancellation: Cancellation,
    ) -> Result<ActivationResult> {
        self.activate_core(contract, digest, expected.map(Some), cancellation)
            .await
    }

    pub async fn activate_if_none(
        &self,
        contract: &str,
        digest: &str,
        cancellation: Cancellation,
    ) -> Result<ActivationResult> {
        self.activate_core(contract, digest, Some(None), cancellation)
            .await
    }

    async fn activate_core(
        &self,
        contract: &str,
        digest: &str,
        expected: Option<Option<&str>>,
        cancellation: Cancellation,
    ) -> Result<ActivationResult> {
        check(&cancellation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = self
            .activate_in(&mut tx, contract, digest, expected, cancellation.clone())
            .await?;
        check(&cancellation)?;
        tx.commit().await?;
        Ok(result)
    }

    pub(super) async fn activate_in(
        &self,
        connection: &mut SqliteConnection,
        contract: &str,
        digest: &str,
        expected: Option<Option<&str>>,
        cancellation: Cancellation,
    ) -> Result<ActivationResult> {
        check(&cancellation)?;
        let selected: Option<String> = sqlx::query_scalar(
            "SELECT contract_digest FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_optional(&mut *connection)
        .await?;
        let selected = selected.ok_or_else(|| ExecutableStoreError::NotFound(digest.into()))?;
        let active = active_identity(connection, contract).await?;
        let plan = policy::plan_activation(contract, digest, &selected, active.as_ref(), expected)?;
        if !plan.changed {
            check(&cancellation)?;
            return Ok(plan);
        }
        sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = CASE WHEN state_version < 9223372036854775807 THEN state_version + 1 ELSE NULL END
            WHERE contract_digest = ? AND lifecycle_state = 'active'")
            .bind(contract).execute(&mut *connection).await?;
        let rows = sqlx::query("UPDATE decision_executables SET lifecycle_state = 'active', state_version = ?, activated_at = ?
            WHERE executable_digest = ?").bind(plan.state_version).bind(timestamp::format_exact((self.clock)())?)
            .bind(digest).execute(&mut *connection).await?.rows_affected();
        if rows != 1 {
            return Err(ExecutableStoreError::NotFound(digest.into()));
        }
        check(&cancellation)?;
        Ok(plan)
    }

    pub async fn activate_candidate(
        &self,
        command: &CandidateActivationCommand,
        cancellation: Cancellation,
    ) -> Result<CandidateActivationResult> {
        policy::validate_candidate_activation(command)?;
        let resource_key = command.resource.resource_key()?;
        check(&cancellation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check(&cancellation)?;
        let row = sqlx::query(
            "SELECT contract_digest, lifecycle_state, state_version FROM decision_executables
            WHERE executable_digest = ?",
        )
        .bind(&command.executable_digest)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ExecutableStoreError::NotFound(command.executable_digest.clone()))?;
        let candidate = LifecycleSnapshot {
            state: parse_state(row.try_get("lifecycle_state")?)?,
            state_version: row.try_get("state_version")?,
        };
        let current = current_contract(&mut tx, &resource_key).await?;
        let active = active_identity(&mut tx, &command.contract_digest).await?;
        let plan = policy::plan_candidate_activation(
            command,
            row.try_get("contract_digest")?,
            candidate,
            current.as_deref(),
            active.as_ref(),
        )?;
        if !deactivate_expected(&mut tx, &command.contract_digest, &plan.previous).await? {
            return Err(activation_conflict(
                Some(&command.expected_active_executable.executable_digest),
                Some(&plan.previous),
            ));
        }
        if !self
            .activate_expected(
                &mut tx,
                &command.executable_digest,
                command.expected_candidate_state_version,
                plan.state_version,
            )
            .await?
        {
            return Err(candidate_conflict(command, candidate));
        }
        let superseded =
            supersede_others(&mut tx, &resource_key, &command.executable_digest).await?;
        check(&cancellation)?;
        tx.commit().await?;
        Ok(CandidateActivationResult {
            executable_digest: command.executable_digest.clone(),
            previous_executable_digest: plan.previous.executable_digest,
            state_version: plan.state_version,
            superseded_candidate_count: superseded,
        })
    }

    pub async fn list_pending(
        &self,
        page_size: i32,
        after_resource_key: Option<&str>,
        cancellation: Cancellation,
    ) -> Result<AnalysisCandidateActivationPage> {
        page_limit(page_size, 1000)?;
        check(&cancellation)?;
        let rows = sqlx::query("
            WITH ranked_candidates AS (
                SELECT admission.resource_key, admission.workspace_id, admission.cycle_id, admission.attempt_id, admission.contract_name,
                       admission.contract_digest, admission.executable_digest, admission.analysis_manifest_digest,
                       admission.created_at, executable.state_version,
                       ROW_NUMBER() OVER (
                           PARTITION BY admission.resource_key ORDER BY
                               CASE WHEN admission.contract_digest = current.contract_digest THEN 0 ELSE 1 END,
                               admission.created_at DESC, admission.executable_digest DESC,
                               admission.workspace_id DESC, admission.cycle_id DESC
                       ) AS candidate_rank
                FROM decision_executables AS executable
                CROSS JOIN analysis_candidate_admissions AS admission
                LEFT JOIN decision_contract_current AS current ON current.resource_key = admission.resource_key
                WHERE executable.lifecycle_state = 'candidate'
                  AND admission.executable_digest = executable.executable_digest
                  AND (? IS NULL OR admission.resource_key > ?)
            )
            SELECT candidate.resource_key, candidate.workspace_id, candidate.cycle_id, candidate.attempt_id, candidate.contract_name,
                   candidate.contract_digest, candidate.executable_digest, candidate.analysis_manifest_digest,
                   candidate.created_at, candidate.state_version,
                   active.executable_digest AS active_digest, active.state_version AS active_version
            FROM ranked_candidates AS candidate LEFT JOIN decision_executables AS active
              ON active.contract_digest = candidate.contract_digest AND active.lifecycle_state = 'active'
            WHERE candidate.candidate_rank = 1 ORDER BY candidate.resource_key ASC LIMIT ?")
            .bind(after_resource_key).bind(after_resource_key).bind(page_size + 1).fetch_all(&self.pool).await?;
        check(&cancellation)?;
        let mut candidates = rows
            .iter()
            .map(|row| {
                Ok(AnalysisCandidateActivationSnapshot {
                    admission: read_admission(row, true)?,
                    candidate_state_version: row.try_get("state_version")?,
                    active_executable: row
                        .try_get::<Option<String>, _>("active_digest")?
                        .map(|digest| {
                            Ok::<_, ExecutableStoreError>(ExecutableLifecycleToken {
                                executable_digest: digest,
                                state_version: row.try_get("active_version")?,
                            })
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next_resource_key = if candidates.len() > page_size as usize {
            candidates.pop();
            Some(
                candidates
                    .last()
                    .expect("bounded pending page")
                    .admission
                    .resource
                    .resource_key()?,
            )
        } else {
            None
        };
        Ok(AnalysisCandidateActivationPage {
            candidates,
            next_resource_key,
        })
    }

    pub async fn resolve(
        &self,
        command: &AnalysisCandidateResolutionCommand,
        cancellation: Cancellation,
    ) -> Result<AnalysisCandidateResolutionResult> {
        policy::validate_resolution(command)?;
        let admission = &command.admission;
        let resource_key = admission.resource.resource_key()?;
        check(&cancellation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check(&cancellation)?;
        let row = sqlx::query("SELECT executable.lifecycle_state, executable.state_version, candidate.created_at
            FROM analysis_candidate_admissions AS candidate JOIN decision_executables AS executable
              ON executable.executable_digest = candidate.executable_digest
            WHERE candidate.workspace_id = ? AND candidate.cycle_id = ? AND candidate.resource_key = ?
              AND candidate.contract_digest = ? AND candidate.executable_digest = ?")
            .bind(&admission.workspace_id).bind(&admission.cycle_id).bind(&resource_key)
            .bind(&admission.contract_digest).bind(&admission.executable_digest).fetch_optional(&mut *tx).await?
            .ok_or_else(|| integrity(format!("Analysis admission for Candidate '{}' does not exist.", admission.executable_digest)))?;
        let current = current_contract(&mut tx, &resource_key).await?;
        let newest: Option<String> = sqlx::query_scalar("SELECT candidate.executable_digest
                    FROM analysis_candidate_admissions AS candidate JOIN decision_executables AS executable
                      ON executable.executable_digest = candidate.executable_digest
                    WHERE candidate.resource_key = ? AND candidate.contract_digest = ? AND executable.lifecycle_state = 'candidate'
                    ORDER BY candidate.created_at DESC, candidate.executable_digest DESC,
                             candidate.workspace_id DESC, candidate.cycle_id DESC LIMIT 1")
                    .bind(&resource_key).bind(&admission.contract_digest).fetch_optional(&mut *tx).await?;
        let active = active_identity(&mut tx, &admission.contract_digest).await?;
        let plan = policy::plan_resolution(
            command,
            &ResolutionSnapshot {
                candidate: LifecycleSnapshot {
                    state: parse_state(row.try_get("lifecycle_state")?)?,
                    state_version: row.try_get("state_version")?,
                },
                created_at: timestamp::parse_exact(row.try_get("created_at")?)?,
                current_contract: current.as_deref(),
                newest_candidate: newest.as_deref(),
                active: active.as_ref(),
            },
        )?;
        let superseded = match &plan {
            ResolutionPlan::Yielded | ResolutionPlan::AlreadyResolved { .. } => 0,
            ResolutionPlan::SupersedeStale { .. } => {
                supersede_stale(&mut tx, &resource_key, current.as_deref()).await?
            }
            ResolutionPlan::Reject { state_version } => {
                let rows = sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = ?
                            WHERE executable_digest = ? AND lifecycle_state = 'candidate' AND state_version = ?")
                            .bind(state_version).bind(&admission.executable_digest).bind(command.expected_candidate_state_version).execute(&mut *tx).await?.rows_affected();
                if rows != 1 {
                    tx.rollback().await?;
                    return Ok(ResolutionPlan::Yielded.result(&admission.executable_digest, 0));
                }
                supersede_others(&mut tx, &resource_key, &admission.executable_digest).await?
            }
            ResolutionPlan::Activate {
                previous,
                state_version,
            } => {
                if let Some(previous) = previous
                    && !deactivate_expected(&mut tx, &admission.contract_digest, previous).await?
                {
                    tx.rollback().await?;
                    return Ok(ResolutionPlan::Yielded.result(&admission.executable_digest, 0));
                }
                if !self
                    .activate_expected(
                        &mut tx,
                        &admission.executable_digest,
                        command.expected_candidate_state_version,
                        *state_version,
                    )
                    .await?
                {
                    tx.rollback().await?;
                    return Ok(ResolutionPlan::Yielded.result(&admission.executable_digest, 0));
                }
                supersede_others(&mut tx, &resource_key, &admission.executable_digest).await?
            }
        };
        check(&cancellation)?;
        tx.commit().await?;
        Ok(plan.result(&admission.executable_digest, superseded))
    }

    async fn activate_expected(
        &self,
        connection: &mut SqliteConnection,
        digest: &str,
        expected: i64,
        next: i64,
    ) -> Result<bool> {
        Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'active', state_version = ?, activated_at = ?
            WHERE executable_digest = ? AND lifecycle_state = 'candidate' AND state_version = ?")
            .bind(next).bind(timestamp::format_exact((self.clock)())?).bind(digest).bind(expected)
            .execute(connection).await?.rows_affected() == 1)
    }
}

async fn active_identity(
    connection: &mut SqliteConnection,
    contract: &str,
) -> Result<Option<ExecutableLifecycleToken>> {
    sqlx::query(
        "SELECT executable_digest, state_version FROM decision_executables
        WHERE contract_digest = ? AND lifecycle_state = 'active'",
    )
    .bind(contract)
    .fetch_optional(connection)
    .await?
    .map(|row| {
        Ok(ExecutableLifecycleToken {
            executable_digest: row.try_get("executable_digest")?,
            state_version: row.try_get("state_version")?,
        })
    })
    .transpose()
}

async fn current_contract(connection: &mut SqliteConnection, key: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT contract_digest FROM decision_contract_current WHERE resource_key = ?",
    )
    .bind(key)
    .fetch_optional(connection)
    .await?)
}

async fn deactivate_expected(
    connection: &mut SqliteConnection,
    contract: &str,
    token: &ExecutableLifecycleToken,
) -> Result<bool> {
    let next = next_version(token.state_version)?;
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = ?
        WHERE contract_digest = ? AND executable_digest = ? AND lifecycle_state = 'active' AND state_version = ?")
        .bind(next).bind(contract).bind(&token.executable_digest).bind(token.state_version).execute(connection).await?.rows_affected() == 1)
}

async fn supersede_stale(
    connection: &mut SqliteConnection,
    key: &str,
    current: Option<&str>,
) -> Result<u64> {
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = CASE WHEN state_version < 9223372036854775807 THEN state_version + 1 ELSE NULL END
        WHERE lifecycle_state = 'candidate' AND EXISTS (
            SELECT 1 FROM analysis_candidate_admissions AS candidate
            WHERE candidate.executable_digest = decision_executables.executable_digest
              AND candidate.resource_key = ? AND (? IS NULL OR candidate.contract_digest <> ?)
        )").bind(key).bind(current).bind(current).execute(connection).await?.rows_affected())
}

async fn supersede_others(
    connection: &mut SqliteConnection,
    key: &str,
    selected: &str,
) -> Result<u64> {
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = CASE WHEN state_version < 9223372036854775807 THEN state_version + 1 ELSE NULL END
        WHERE executable_digest <> ? AND lifecycle_state = 'candidate' AND EXISTS (
            SELECT 1 FROM analysis_candidate_admissions AS candidate
            WHERE candidate.executable_digest = decision_executables.executable_digest AND candidate.resource_key = ?
        )").bind(selected).bind(key).execute(connection).await?.rows_affected())
}
