use flaggo_contract::{
    ContractError, DecisionContract, DecisionExecutable, RuntimeInput, digests, strict_json,
};
use flaggo_contract_store::{
    AcceptedContractVersion, ContractStoreError, SqliteContractVersionStore, timestamp,
};
use flaggo_decision::{DecisionError, DecisionRuntime, StoreReadError};
use flaggo_executable_store::*;
use flaggo_expression::{ExecutableCompilationError, ExecutableCompiler, ExpressionCompiler};
use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Store(#[from] ExecutableStoreError),
    #[error(transparent)]
    ContractStore(#[from] ContractStoreError),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Time(#[from] timestamp::TimestampError),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Compile(#[from] ExecutableCompilationError),
}

fn text<'a>(op: &'a Value, key: &str) -> &'a str {
    op[key]
        .as_str()
        .expect("required executable-store probe field")
}

pub async fn evaluate(request: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let database = text(&request, "database");
    let now = timestamp::parse_exact(text(&request, "now"))?;
    let compiler = request["withExpressions"]
        .as_bool()
        .expect("withExpressions")
        .then(|| Arc::new(ExpressionCompiler::default()));
    let canonicalizer = compiler
        .as_ref()
        .map(|value| Arc::clone(value) as Arc<dyn digests::ExpressionCanonicalizer + Send + Sync>);
    let store = SqliteExecutableStore::open(database, canonicalizer.clone())
        .await?
        .with_clock(Arc::new(move || now));
    let contracts = SqliteContractVersionStore::open(database, canonicalizer).await?;
    let mut versions = HashMap::<String, AcceptedContractVersion>::new();
    let mut records = HashMap::<String, StoredExecutable>::new();
    let mut pages = HashMap::<String, AnalysisCandidateActivationPage>::new();
    let mut cursors = HashMap::<String, Option<String>>::new();
    let mut replies = Vec::new();
    for op in request["operations"].as_array().expect("operations") {
        let id = text(op, "id");
        let cancellation = op["cancelled"]
            .as_bool()
            .unwrap_or(false)
            .then(|| Arc::new(AtomicBool::new(true)));
        let result: Result<Value, Error> = async {
            let digest = || {
                op["executableDigest"]
                    .as_str()
                    .unwrap_or_else(|| &records[text(op, "use")].executable_digest)
            };
            let contract_digest = || {
                op["contractDigest"]
                    .as_str()
                    .unwrap_or_else(|| &records[text(op, "use")].executable.contract_digest)
            };
            let expected = || {
                op["expected"].as_str().or_else(|| {
                    op["expectedFrom"]
                        .as_str()
                        .map(|alias| records[alias].executable_digest.as_str())
                })
            };
            let expected_token = || {
                if let Some(alias) = op["expectedFrom"].as_str() {
                    Some(ExecutableLifecycleToken {
                        executable_digest: records[alias].executable_digest.clone(),
                        state_version: op["activeVersion"].as_i64().unwrap_or(1),
                    })
                } else {
                    op.get("expectedActive")
                        .filter(|value| !value.is_null())
                        .map(|value| {
                            serde_json::from_value(value.clone()).expect("expectedActive token")
                        })
                }
            };
            match text(op, "action") {
                "initialize" => {
                    store.initialize(cancellation).await?;
                    Ok(Value::Null)
                }
                "initializeContract" => {
                    contracts.initialize(cancellation).await?;
                    Ok(Value::Null)
                }
                "accept" => {
                    let contract: DecisionContract =
                        strict_json::deserialize(text(op, "document").as_bytes())?;
                    let version = AcceptedContractVersion {
                        contract_digest: digests::contract_digest(
                            &contract,
                            compiler.as_ref().map(|compiler| {
                                compiler.as_ref() as &dyn digests::ExpressionCanonicalizer
                            }),
                        )?,
                        accepted_at: timestamp::parse_exact(text(op, "createdAt"))?,
                        contract,
                    };
                    contracts.put(&version, cancellation.clone()).await?;
                    contracts
                        .set_current(
                            &version.contract.name,
                            &version.contract_digest,
                            cancellation,
                        )
                        .await?;
                    versions.insert(id.into(), version);
                    Ok(Value::Null)
                }
                "put" | "admit" => {
                    let accepted = &versions[text(op, "contract")];
                    let mut authored: DecisionExecutable =
                        strict_json::deserialize(text(op, "document").as_bytes())?;
                    authored.contract_digest = accepted.contract_digest.clone();
                    let compiled =
                        ExecutableCompiler::default().compile(&accepted.contract, &authored)?;
                    let stored = StoredExecutable {
                        executable_digest: op["executableDigest"]
                            .as_str()
                            .map(Into::into)
                            .unwrap_or(compiled.executable_digest),
                        executable: if op["raw"] == true {
                            authored
                        } else {
                            compiled.executable
                        },
                        checked_executable: op
                            .get("checked")
                            .map(|checked| strict_json::deserialize(checked.to_string().as_bytes()))
                            .transpose()?
                            .unwrap_or(compiled.checked_executable),
                        state: match op["state"].as_i64().unwrap_or(0) {
                            0 => ExecutableLifecycleState::Candidate,
                            1 => ExecutableLifecycleState::Active,
                            2 => ExecutableLifecycleState::Inactive,
                            _ => {
                                return Err(ExecutableStoreError::InvalidArgument {
                                    parameter: "stored",
                                    message: "Invalid candidate state.".into(),
                                    out_of_range: false,
                                }
                                .into());
                            }
                        },
                        state_version: op["stateVersion"].as_i64().unwrap_or(0),
                        created_at: timestamp::parse_exact(text(op, "createdAt"))?,
                        activated_at: op["activatedAt"]
                            .as_str()
                            .map(timestamp::parse_exact)
                            .transpose()?,
                        provenance: op["provenance"]
                            .as_str()
                            .map(|value| strict_json::deserialize(value.as_bytes()))
                            .transpose()?,
                    };
                    records.insert(id.into(), stored.clone());
                    if op["action"] == "put" {
                        Ok(json!(
                            match store.put_candidate(&stored, cancellation).await? {
                                ExecutableStoreWriteResult::Created => "created",
                                ExecutableStoreWriteResult::Existing => "existing",
                            }
                        ))
                    } else {
                        let admission = AnalysisCandidateAdmission {
                            workspace_id: op["workspaceId"].as_str().unwrap_or("workspace").into(),
                            cycle_id: op["cycleId"].as_str().unwrap_or(id).into(),
                            attempt_id: op["attemptId"].as_str().unwrap_or("attempt").into(),
                            contract_name: op["contractName"]
                                .as_str()
                                .unwrap_or(&accepted.contract.name)
                                .into(),
                            contract_digest: op["admissionContractDigest"]
                                .as_str()
                                .unwrap_or(&stored.executable.contract_digest)
                                .into(),
                            executable_digest: op["admissionExecutableDigest"]
                                .as_str()
                                .unwrap_or(&stored.executable_digest)
                                .into(),
                            analysis_manifest_digest: op["manifestDigest"]
                                .as_str()
                                .map(Into::into)
                                .unwrap_or_else(|| format!("sha256:{}", "a".repeat(64))),
                            created_at: op["admissionTime"]
                                .as_str()
                                .map(timestamp::parse_exact)
                                .transpose()?
                                .unwrap_or(stored.created_at),
                        };
                        Ok(strict_json::to_value(
                            &store
                                .put_analysis_candidate(&stored, &admission, cancellation)
                                .await?,
                        )?)
                    }
                }
                "get" => Ok(strict_json::to_value(
                    &store.get(digest(), cancellation).await?,
                )?),
                "active" => Ok(strict_json::to_value(
                    &store.get_active(contract_digest(), cancellation).await?,
                )?),
                "activate" => Ok(strict_json::to_value(
                    &store
                        .activate(contract_digest(), digest(), expected(), cancellation)
                        .await?,
                )?),
                "activateIfNone" => Ok(strict_json::to_value(
                    &store
                        .activate_if_none(contract_digest(), digest(), cancellation)
                        .await?,
                )?),
                "activateCandidate" => {
                    let command = CandidateActivationCommand {
                        contract_name: op["contractName"]
                            .as_str()
                            .unwrap_or_else(|| &versions[text(op, "contract")].contract.name)
                            .into(),
                        contract_digest: contract_digest().into(),
                        executable_digest: digest().into(),
                        expected_candidate_state_version: op["expectedVersion"]
                            .as_i64()
                            .expect("expectedVersion"),
                        expected_active_executable: expected_token()
                            .expect("manual activation token"),
                    };
                    Ok(strict_json::to_value(
                        &store.activate_candidate(&command, cancellation).await?,
                    )?)
                }
                "list" | "candidates" => {
                    let cursor = op["cursorFrom"]
                        .as_str()
                        .map_or_else(|| op["cursor"].as_str(), |from| cursors[from].as_deref());
                    let size = i32::try_from(op["pageSize"].as_i64().expect("pageSize"))
                        .expect("i32 pageSize");
                    let page = if op["action"] == "list" {
                        store
                            .list_executables(contract_digest(), size, cursor, cancellation)
                            .await?
                    } else {
                        store
                            .list_candidates(contract_digest(), size, cursor, cancellation)
                            .await?
                    };
                    cursors.insert(id.into(), page.next_cursor.clone());
                    Ok(strict_json::to_value(&page)?)
                }
                "pending" => {
                    let page = store
                        .list_pending(
                            i32::try_from(op["pageSize"].as_i64().expect("pageSize"))
                                .expect("i32 pageSize"),
                            op["afterContractName"].as_str(),
                            cancellation,
                        )
                        .await?;
                    let value = strict_json::to_value(&page)?;
                    pages.insert(id.into(), page);
                    Ok(value)
                }
                "resolve" => {
                    let snapshot = &pages[text(op, "snapshotFrom")].candidates
                        [op["index"].as_u64().unwrap_or(0) as usize];
                    let mut admission = snapshot.admission.clone();
                    if let Some(time) = op["admissionTime"].as_str() {
                        admission.created_at = timestamp::parse_exact(time)?;
                    }
                    let command = AnalysisCandidateResolutionCommand {
                        admission,
                        expected_candidate_state_version: op["expectedVersion"]
                            .as_i64()
                            .unwrap_or(snapshot.candidate_state_version),
                        expected_active_executable: if op.get("expectedActive").is_some()
                            || op.get("expectedFrom").is_some()
                        {
                            expected_token()
                        } else {
                            snapshot.active_executable.clone()
                        },
                        action: match op["resolution"].as_i64().expect("resolution") {
                            0 => AnalysisCandidateResolutionAction::Activate,
                            1 => AnalysisCandidateResolutionAction::Reject,
                            _ => {
                                return Err(ExecutableStoreError::InvalidArgument {
                                    parameter: "command",
                                    message: "Candidate resolution action is not supported.".into(),
                                    out_of_range: true,
                                }
                                .into());
                            }
                        },
                    };
                    Ok(strict_json::to_value(
                        &store.resolve(&command, cancellation).await?,
                    )?)
                }
                "runtime" => {
                    let version = &versions[text(op, "contract")];
                    let input: RuntimeInput =
                        strict_json::deserialize(text(op, "input").as_bytes())?;
                    let runtime = DecisionRuntime::new(contracts.clone(), store.clone());
                    Ok(
                        match runtime
                            .decide(
                                &version.contract.name,
                                &version.contract_digest,
                                &input,
                                cancellation,
                            )
                            .await
                        {
                            Ok(decision) => strict_json::to_value(&decision)?,
                            Err(error) => return Ok(runtime_fault(id, error)),
                        },
                    )
                }
                "health" => Ok(json!(store.is_available(cancellation).await?)),
                "sql" => {
                    let pool = sqlx::SqlitePool::connect_with(
                        SqliteConnectOptions::new()
                            .filename(database)
                            .create_if_missing(true),
                    )
                    .await?;
                    // Only trusted test fixtures execute dynamic SQL against isolated parity databases.
                    let result = sqlx::raw_sql(sqlx::AssertSqlSafe(text(op, "query").to_owned()))
                        .execute(&pool)
                        .await;
                    pool.close().await;
                    result?;
                    Ok(Value::Null)
                }
                _ => panic!("unknown executable-store probe action"),
            }
        }
        .await;
        replies.push(match result {
            Ok(value) if op["action"] == "runtime" && value["outcome"] == "error" => value,
            Ok(value) => json!({"id":id,"outcome":"ok","value":value}),
            Err(error) => fault(id, error),
        });
    }
    store.close().await;
    contracts.close().await;
    Ok(json!({"id":request["id"],"status":"value","value":replies}))
}

fn fault(id: &str, error: Error) -> Value {
    let error = match error {
        Error::ContractStore(error) => return super::store::fault(id, error),
        Error::Contract(error) => return super::store::fault(id, error.into()),
        Error::Time(error) => return super::store::fault(id, error.into()),
        Error::Sql(error) => return super::store::fault(id, error.into()),
        Error::Compile(_) => {
            return json!({"id":id,"outcome":"error","exceptionType":"Flaggo.Expressions.ExecutableCompilationException"});
        }
        Error::Store(error) => error,
    };
    let mut value = json!({"id":id,"outcome":"error"});
    let kind = match error {
        ExecutableStoreError::InvalidArgument {
            parameter,
            out_of_range,
            ..
        } => {
            value["parameter"] = json!(parameter);
            if out_of_range {
                "System.ArgumentOutOfRangeException"
            } else {
                "System.ArgumentException"
            }
        }
        ExecutableStoreError::UnsupportedSchema(_) => "System.InvalidOperationException",
        ExecutableStoreError::Cancelled => "System.OperationCanceledException",
        ExecutableStoreError::Contract(error) => return super::store::fault(id, error.into()),
        ExecutableStoreError::Timestamp(error) => return super::store::fault(id, error.into()),
        ExecutableStoreError::Database(error) => return super::store::fault(id, error.into()),
        error => {
            value["message"] = json!(error.to_string());
            match error {
                ExecutableStoreError::NotFound(_) => {
                    "Flaggo.ExecutableStore.ExecutableNotFoundException"
                }
                ExecutableStoreError::ContractMismatch { .. } => {
                    "Flaggo.ExecutableStore.ExecutableContractMismatchException"
                }
                ExecutableStoreError::ActivationConflict { .. } => {
                    "Flaggo.ExecutableStore.ActivationConflictException"
                }
                ExecutableStoreError::CandidateConflict { .. } => {
                    "Flaggo.ExecutableStore.CandidateActivationConflictException"
                }
                ExecutableStoreError::CurrentContractConflict { .. } => {
                    "Flaggo.ExecutableStore.ActivationContractConflictException"
                }
                ExecutableStoreError::AdmissionConflict { .. } => {
                    "Flaggo.ExecutableStore.CandidateAdmissionConflictException"
                }
                ExecutableStoreError::LifecycleConflict { .. } => {
                    "Flaggo.ExecutableStore.CandidateLifecycleConflictException"
                }
                ExecutableStoreError::Integrity(_) => "System.IO.InvalidDataException",
                _ => unreachable!("forwarded native store error"),
            }
        }
    };
    value["exceptionType"] = json!(kind);
    value
}

fn runtime_fault(id: &str, error: DecisionError) -> Value {
    let (outcome, category) = error.telemetry_classification();
    let mut result = match &error {
        DecisionError::Integrity { message, .. } => {
            json!({"id":id,"outcome":"error","exceptionType":"Flaggo.Decision.RuntimeIntegrityException","message":message})
        }
        DecisionError::Store(
            StoreReadError::Native(source)
            | StoreReadError::InvalidArgument(source)
            | StoreReadError::Dependency(source)
            | StoreReadError::Timeout(source),
        ) => {
            if let Some(source) = source.downcast_ref::<ExecutableStoreError>() {
                match source {
                    ExecutableStoreError::Contract(ContractError::Json(_)) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.Text.Json.JsonException"})
                    }
                    ExecutableStoreError::Timestamp(_) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.FormatException"})
                    }
                    ExecutableStoreError::Contract(ContractError::Serialization(_)) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.ArgumentException","parameter":null})
                    }
                    ExecutableStoreError::Contract(ContractError::InvalidUnicode(_)) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.InvalidOperationException"})
                    }
                    ExecutableStoreError::Contract(ContractError::MissingTypeDiscriminator) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.NotSupportedException"})
                    }
                    ExecutableStoreError::Contract(ContractError::InvalidModel(_)) => {
                        json!({"id":id,"outcome":"error","exceptionType":"System.NullReferenceException"})
                    }
                    ExecutableStoreError::Database(sqlx::Error::Database(error)) => {
                        let code = error.code().and_then(|code| code.parse::<i32>().ok());
                        json!({"id":id,"outcome":"error","exceptionType":"Microsoft.Data.Sqlite.SqliteException",
                            "sqliteCode":code.map(|code| code & 255)})
                    }
                    _ => {
                        panic!(
                            "Unhandled runtime store fault in executable parity protocol: {source}"
                        )
                    }
                }
            } else {
                panic!("Unhandled runtime reader fault in executable parity protocol: {source}")
            }
        }
        DecisionError::Store(StoreReadError::Cancelled) => {
            json!({"id":id,"outcome":"error","exceptionType":"System.OperationCanceledException"})
        }
        DecisionError::ActiveExecutableNotFound(_) => {
            json!({"id":id,"outcome":"error","exceptionType":"Flaggo.Decision.ActiveExecutableNotFoundException"})
        }
        _ => {
            panic!("Unhandled runtime fault in executable parity protocol: {error}")
        }
    };
    result["telemetry"] = json!({"outcome":outcome,"category":category});
    result
}
