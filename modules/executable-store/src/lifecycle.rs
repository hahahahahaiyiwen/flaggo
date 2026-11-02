use super::*;
use AnalysisCandidateResolutionOutcome as Outcome;
use ExecutableLifecycleState as State;

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
        check(&cancellation)?;
        let selected: Option<String> = sqlx::query_scalar(
            "SELECT contract_digest FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_optional(&mut *tx)
        .await?;
        let selected = selected.ok_or_else(|| ExecutableStoreError::NotFound(digest.into()))?;
        binding(digest, contract, &selected)?;
        let active = active_identity(&mut tx, contract).await?;
        if let Some(expected) = expected
            && active
                .as_ref()
                .map(|value| value.executable_digest.as_str())
                != expected
        {
            return Err(activation_conflict(expected, &active));
        }
        if let Some(active) = &active
            && active.executable_digest == digest
        {
            check(&cancellation)?;
            tx.commit().await?;
            return Ok(ActivationResult {
                executable_digest: digest.into(),
                previous_executable_digest: Some(active.executable_digest.clone()),
                state_version: active.state_version,
                changed: false,
            });
        }
        sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = state_version + 1
            WHERE contract_digest = ? AND lifecycle_state = 'active'")
            .bind(contract).execute(&mut *tx).await?;
        let version = active
            .as_ref()
            .map_or(0, |value| value.state_version)
            .wrapping_add(1);
        let rows = sqlx::query("UPDATE decision_executables SET lifecycle_state = 'active', state_version = ?, activated_at = ?
            WHERE executable_digest = ?").bind(version).bind(timestamp::format_exact((self.clock)())?)
            .bind(digest).execute(&mut *tx).await?.rows_affected();
        if rows != 1 {
            return Err(ExecutableStoreError::NotFound(digest.into()));
        }
        check(&cancellation)?;
        tx.commit().await?;
        Ok(ActivationResult {
            executable_digest: digest.into(),
            previous_executable_digest: active.map(|value| value.executable_digest),
            state_version: version,
            changed: true,
        })
    }

    pub async fn activate_candidate(
        &self,
        command: &CandidateActivationCommand,
        cancellation: Cancellation,
    ) -> Result<CandidateActivationResult> {
        for (parameter, value) in [
            ("command.ContractName", &command.contract_name),
            ("command.ContractDigest", &command.contract_digest),
            ("command.ExecutableDigest", &command.executable_digest),
        ] {
            if value.trim().is_empty() {
                return Err(argument(
                    parameter,
                    "The value cannot be an empty string or composed entirely of whitespace.",
                ));
            }
        }
        if command.expected_candidate_state_version < 0 {
            return Err(range(
                "command",
                "Expected Candidate state version cannot be negative.",
            ));
        }
        if command.expected_active_executable.state_version < 0 {
            return Err(range(
                "command",
                "Expected active state version cannot be negative.",
            ));
        }
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
        binding(
            &command.executable_digest,
            &command.contract_digest,
            row.try_get("contract_digest")?,
        )?;
        let state = parse_state(row.try_get("lifecycle_state")?)?;
        let version = row.try_get("state_version")?;
        if state != State::Candidate || version != command.expected_candidate_state_version {
            return Err(candidate_conflict(command, state, version));
        }
        let current = current_contract(&mut tx, &command.contract_name).await?;
        if current.as_deref() != Some(&command.contract_digest) {
            return Err(ExecutableStoreError::CurrentContractConflict {
                contract_name: command.contract_name.clone(),
                expected: command.contract_digest.clone(),
                actual: current,
            });
        }
        let active = active_identity(&mut tx, &command.contract_digest).await?;
        if active.as_ref() != Some(&command.expected_active_executable) {
            return Err(activation_conflict(
                Some(&command.expected_active_executable.executable_digest),
                &active,
            ));
        }
        let active = active.expect("matching non-null lifecycle token");
        if !deactivate_expected(&mut tx, &command.contract_digest, &active).await? {
            return Err(activation_conflict(
                Some(&command.expected_active_executable.executable_digest),
                &Some(active),
            ));
        }
        let next = active
            .state_version
            .max(command.expected_candidate_state_version)
            .wrapping_add(1);
        if !self
            .activate_expected(
                &mut tx,
                &command.executable_digest,
                command.expected_candidate_state_version,
                next,
            )
            .await?
        {
            return Err(candidate_conflict(command, state, version));
        }
        let superseded =
            supersede_others(&mut tx, &command.contract_name, &command.executable_digest).await?;
        check(&cancellation)?;
        tx.commit().await?;
        Ok(CandidateActivationResult {
            executable_digest: command.executable_digest.clone(),
            previous_executable_digest: active.executable_digest,
            state_version: next,
            superseded_candidate_count: superseded,
        })
    }

    pub async fn list_pending(
        &self,
        page_size: i32,
        after_contract_name: Option<&str>,
        cancellation: Cancellation,
    ) -> Result<AnalysisCandidateActivationPage> {
        page_limit(page_size, 1000)?;
        check(&cancellation)?;
        let rows = sqlx::query("
            WITH ranked_candidates AS (
                SELECT admission.workspace_id, admission.cycle_id, admission.attempt_id, admission.contract_name,
                       admission.contract_digest, admission.executable_digest, admission.analysis_manifest_digest,
                       admission.created_at, executable.state_version,
                       ROW_NUMBER() OVER (
                           PARTITION BY admission.contract_name ORDER BY
                               CASE WHEN admission.contract_digest = current.contract_digest THEN 0 ELSE 1 END,
                               admission.created_at DESC, admission.executable_digest DESC,
                               admission.workspace_id DESC, admission.cycle_id DESC
                       ) AS candidate_rank
                FROM decision_executables AS executable
                CROSS JOIN analysis_candidate_admissions AS admission
                LEFT JOIN decision_contract_current AS current ON current.contract_name = admission.contract_name
                WHERE executable.lifecycle_state = 'candidate'
                  AND admission.executable_digest = executable.executable_digest
                  AND (? IS NULL OR admission.contract_name > ?)
            )
            SELECT candidate.workspace_id, candidate.cycle_id, candidate.attempt_id, candidate.contract_name,
                   candidate.contract_digest, candidate.executable_digest, candidate.analysis_manifest_digest,
                   candidate.created_at, candidate.state_version,
                   active.executable_digest AS active_digest, active.state_version AS active_version
            FROM ranked_candidates AS candidate LEFT JOIN decision_executables AS active
              ON active.contract_digest = candidate.contract_digest AND active.lifecycle_state = 'active'
            WHERE candidate.candidate_rank = 1 ORDER BY candidate.contract_name ASC LIMIT ?")
            .bind(after_contract_name).bind(after_contract_name).bind(page_size + 1).fetch_all(&self.pool).await?;
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
        let next_contract_name = if candidates.len() > page_size as usize {
            candidates.pop();
            Some(
                candidates
                    .last()
                    .expect("bounded pending page")
                    .admission
                    .contract_name
                    .clone(),
            )
        } else {
            None
        };
        Ok(AnalysisCandidateActivationPage {
            candidates,
            next_contract_name,
        })
    }

    pub async fn resolve(
        &self,
        command: &AnalysisCandidateResolutionCommand,
        cancellation: Cancellation,
    ) -> Result<AnalysisCandidateResolutionResult> {
        if command.expected_candidate_state_version < 0 {
            return Err(range(
                "command",
                "Expected Candidate state version cannot be negative.",
            ));
        }
        if command
            .expected_active_executable
            .as_ref()
            .is_some_and(|value| value.state_version < 0)
        {
            return Err(range(
                "command",
                "Expected active executable state version cannot be negative.",
            ));
        }
        let admission = &command.admission;
        if [
            &admission.workspace_id,
            &admission.cycle_id,
            &admission.contract_name,
            &admission.contract_digest,
            &admission.executable_digest,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(argument(
                "command",
                "Candidate resolution requires complete admission identity.",
            ));
        }
        check(&cancellation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check(&cancellation)?;
        let row = sqlx::query("SELECT executable.lifecycle_state, executable.state_version, candidate.created_at
            FROM analysis_candidate_admissions AS candidate JOIN decision_executables AS executable
              ON executable.executable_digest = candidate.executable_digest
            WHERE candidate.workspace_id = ? AND candidate.cycle_id = ? AND candidate.contract_name = ?
              AND candidate.contract_digest = ? AND candidate.executable_digest = ?")
            .bind(&admission.workspace_id).bind(&admission.cycle_id).bind(&admission.contract_name)
            .bind(&admission.contract_digest).bind(&admission.executable_digest).fetch_optional(&mut *tx).await?
            .ok_or_else(|| integrity(format!("Analysis admission for Candidate '{}' does not exist.", admission.executable_digest)))?;
        let state = parse_state(row.try_get("lifecycle_state")?)?;
        let version: i64 = row.try_get("state_version")?;
        let created_at = timestamp::parse_exact(row.try_get("created_at")?)?;
        let mut result = AnalysisCandidateResolutionResult {
            outcome: Outcome::Yielded,
            executable_digest: admission.executable_digest.clone(),
            previous_executable_digest: None,
            state_version: None,
            superseded_candidate_count: 0,
        };
        if state != State::Candidate {
            result.outcome = Outcome::AlreadyResolved;
            result.state_version = Some(version);
        } else if version == command.expected_candidate_state_version
            && created_at == admission.created_at
        {
            let current = current_contract(&mut tx, &admission.contract_name).await?;
            if current.as_deref() != Some(&admission.contract_digest) {
                result.outcome = Outcome::Superseded;
                result.state_version =
                    Some(command.expected_candidate_state_version.wrapping_add(1));
                result.superseded_candidate_count =
                    supersede_stale(&mut tx, &admission.contract_name, current.as_deref()).await?;
            } else {
                let newest: Option<String> = sqlx::query_scalar("SELECT candidate.executable_digest
                    FROM analysis_candidate_admissions AS candidate JOIN decision_executables AS executable
                      ON executable.executable_digest = candidate.executable_digest
                    WHERE candidate.contract_name = ? AND candidate.contract_digest = ? AND executable.lifecycle_state = 'candidate'
                    ORDER BY candidate.created_at DESC, candidate.executable_digest DESC,
                             candidate.workspace_id DESC, candidate.cycle_id DESC LIMIT 1")
                    .bind(&admission.contract_name).bind(&admission.contract_digest).fetch_optional(&mut *tx).await?;
                if newest.as_deref() == Some(&admission.executable_digest) {
                    if matches!(command.action, AnalysisCandidateResolutionAction::Reject) {
                        let rows = sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = state_version + 1
                            WHERE executable_digest = ? AND lifecycle_state = 'candidate' AND state_version = ?")
                            .bind(&admission.executable_digest).bind(command.expected_candidate_state_version).execute(&mut *tx).await?.rows_affected();
                        if rows != 1 {
                            tx.rollback().await?;
                            return Ok(result);
                        }
                        result.outcome = Outcome::Rejected;
                        result.state_version =
                            Some(command.expected_candidate_state_version.wrapping_add(1));
                        result.superseded_candidate_count = supersede_others(
                            &mut tx,
                            &admission.contract_name,
                            &admission.executable_digest,
                        )
                        .await?;
                    } else {
                        let active = active_identity(&mut tx, &admission.contract_digest).await?;
                        if active == command.expected_active_executable {
                            if let Some(previous) = &active
                                && !deactivate_expected(
                                    &mut tx,
                                    &admission.contract_digest,
                                    previous,
                                )
                                .await?
                            {
                                tx.rollback().await?;
                                return Ok(result);
                            }
                            let next = active
                                .as_ref()
                                .map_or(command.expected_candidate_state_version, |value| {
                                    value.state_version
                                })
                                .wrapping_add(1);
                            if !self
                                .activate_expected(
                                    &mut tx,
                                    &admission.executable_digest,
                                    command.expected_candidate_state_version,
                                    next,
                                )
                                .await?
                            {
                                tx.rollback().await?;
                                return Ok(result);
                            }
                            result.outcome = Outcome::Activated;
                            result.previous_executable_digest =
                                active.map(|value| value.executable_digest);
                            result.state_version = Some(next);
                            result.superseded_candidate_count = supersede_others(
                                &mut tx,
                                &admission.contract_name,
                                &admission.executable_digest,
                            )
                            .await?;
                        }
                    }
                }
            }
        }
        check(&cancellation)?;
        tx.commit().await?;
        Ok(result)
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

fn binding(digest: &str, expected: &str, actual: &str) -> Result<()> {
    if expected != actual {
        return Err(ExecutableStoreError::ContractMismatch {
            executable_digest: digest.into(),
            expected_contract_digest: expected.into(),
            actual_contract_digest: actual.into(),
        });
    }
    Ok(())
}

fn candidate_conflict(
    command: &CandidateActivationCommand,
    state: State,
    actual: i64,
) -> ExecutableStoreError {
    ExecutableStoreError::CandidateConflict {
        executable_digest: command.executable_digest.clone(),
        expected: command.expected_candidate_state_version,
        state,
        actual,
    }
}

fn activation_conflict(
    expected: Option<&str>,
    active: &Option<ExecutableLifecycleToken>,
) -> ExecutableStoreError {
    ExecutableStoreError::ActivationConflict {
        expected: expected.map(Into::into),
        actual: active.as_ref().map(|value| value.executable_digest.clone()),
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

async fn current_contract(connection: &mut SqliteConnection, name: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT contract_digest FROM decision_contract_current WHERE contract_name = ?",
    )
    .bind(name)
    .fetch_optional(connection)
    .await?)
}

async fn deactivate_expected(
    connection: &mut SqliteConnection,
    contract: &str,
    token: &ExecutableLifecycleToken,
) -> Result<bool> {
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = state_version + 1
        WHERE contract_digest = ? AND executable_digest = ? AND lifecycle_state = 'active' AND state_version = ?")
        .bind(contract).bind(&token.executable_digest).bind(token.state_version).execute(connection).await?.rows_affected() == 1)
}

async fn supersede_stale(
    connection: &mut SqliteConnection,
    name: &str,
    current: Option<&str>,
) -> Result<u64> {
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = state_version + 1
        WHERE lifecycle_state = 'candidate' AND EXISTS (
            SELECT 1 FROM analysis_candidate_admissions AS candidate
            WHERE candidate.executable_digest = decision_executables.executable_digest
              AND candidate.contract_name = ? AND (? IS NULL OR candidate.contract_digest <> ?)
        )").bind(name).bind(current).bind(current).execute(connection).await?.rows_affected())
}

async fn supersede_others(
    connection: &mut SqliteConnection,
    name: &str,
    selected: &str,
) -> Result<u64> {
    Ok(sqlx::query("UPDATE decision_executables SET lifecycle_state = 'inactive', state_version = state_version + 1
        WHERE executable_digest <> ? AND lifecycle_state = 'candidate' AND EXISTS (
            SELECT 1 FROM analysis_candidate_admissions AS candidate
            WHERE candidate.executable_digest = decision_executables.executable_digest AND candidate.contract_name = ?
        )").bind(selected).bind(name).execute(connection).await?.rows_affected())
}
