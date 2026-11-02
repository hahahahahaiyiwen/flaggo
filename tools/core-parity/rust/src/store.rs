use flaggo_contract::{ContractError, DecisionContract, digests, strict_json};
use flaggo_contract_store::{
    AcceptedContractVersion, ContractStoreError, ContractStoreWriteResult,
    SqliteContractVersionStore, timestamp,
};
use flaggo_expression::ExpressionCompiler;
use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

pub async fn evaluate(request: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let database = text(&request, "database")?;
    let compiler = request["withExpressions"]
        .as_bool()
        .ok_or("missing withExpressions")?
        .then(|| Arc::new(ExpressionCompiler::default()));
    let store = SqliteContractVersionStore::open(
        database,
        compiler.as_ref().map(|compiler| {
            Arc::clone(compiler) as Arc<dyn digests::ExpressionCanonicalizer + Send + Sync>
        }),
    )
    .await?;
    let mut versions = HashMap::<String, AcceptedContractVersion>::new();
    let mut cursors = HashMap::<String, Option<String>>::new();
    let mut replies = Vec::new();
    for operation in request["operations"]
        .as_array()
        .ok_or("missing operations")?
    {
        let id = text(operation, "id")?;
        let cancellation = operation["cancelled"]
            .as_bool()
            .unwrap_or(false)
            .then(|| Arc::new(AtomicBool::new(true)));
        let result: Result<Value, ContractStoreError> = async {
            match operation["action"]
                .as_str()
                .ok_or_else(|| protocol("missing action"))?
            {
                "initialize" => {
                    store.initialize(cancellation).await?;
                    Ok(Value::Null)
                }
                "put" => {
                    let contract: DecisionContract = strict_json::deserialize(
                        operation["document"]
                            .as_str()
                            .ok_or_else(|| protocol("missing document"))?
                            .as_bytes(),
                    )?;
                    let version = AcceptedContractVersion {
                        contract_digest: match operation["contractDigest"].as_str() {
                            Some(digest) => digest.into(),
                            None => digests::contract_digest(
                                &contract,
                                compiler.as_ref().map(|compiler| {
                                    compiler.as_ref() as &dyn digests::ExpressionCanonicalizer
                                }),
                            )?,
                        },
                        accepted_at: timestamp::parse_exact(
                            operation["acceptedAt"]
                                .as_str()
                                .ok_or_else(|| protocol("missing acceptedAt"))?,
                        )?,
                        contract,
                    };
                    versions.insert(id.into(), version.clone());
                    Ok(json!(match store.put(&version, cancellation).await? {
                        ContractStoreWriteResult::Created => "created",
                        ContractStoreWriteResult::Existing => "existing",
                    }))
                }
                "get" => strict_json::to_value(
                    &store
                        .get(
                            name(operation, &versions)?,
                            digest(operation, &versions)?,
                            cancellation,
                        )
                        .await?,
                )
                .map_err(Into::into),
                "current" => strict_json::to_value(
                    &store
                        .get_current(name(operation, &versions)?, cancellation)
                        .await?,
                )
                .map_err(Into::into),
                "catalog" => strict_json::to_value(&store.list_all_current(cancellation).await?)
                    .map_err(Into::into),
                "setCurrent" => {
                    store
                        .set_current(
                            name(operation, &versions)?,
                            digest(operation, &versions)?,
                            cancellation,
                        )
                        .await?;
                    Ok(Value::Null)
                }
                "list" => {
                    let cursor = match operation["cursorFrom"].as_str() {
                        Some(id) => cursors
                            .get(id)
                            .ok_or_else(|| protocol("unknown cursor alias"))?
                            .as_deref(),
                        None => operation["cursor"].as_str(),
                    };
                    let size = operation["pageSize"]
                        .as_i64()
                        .and_then(|value| i32::try_from(value).ok())
                        .ok_or_else(|| protocol("missing pageSize"))?;
                    let page = store
                        .list(name(operation, &versions)?, size, cursor, cancellation)
                        .await?;
                    cursors.insert(id.into(), page.next_cursor.clone());
                    strict_json::to_value(&page).map_err(Into::into)
                }
                "health" => Ok(json!(store.is_available(cancellation).await?)),
                "sql" => {
                    let pool = sqlx::SqlitePool::connect_with(
                        SqliteConnectOptions::new()
                            .filename(database)
                            .create_if_missing(true),
                    )
                    .await?;
                    // Only the temporary parity corpus supplies SQL for isolated fixture corruption.
                    let result = sqlx::raw_sql(sqlx::AssertSqlSafe(
                        operation["query"]
                            .as_str()
                            .ok_or_else(|| protocol("missing query"))?
                            .to_owned(),
                    ))
                    .execute(&pool)
                    .await;
                    pool.close().await;
                    result?;
                    Ok(Value::Null)
                }
                _ => Err(protocol("unknown store action")),
            }
        }
        .await;
        replies.push(match result {
            Ok(value) => json!({"id":id,"outcome":"ok","value":value}),
            Err(error) => fault(id, error),
        });
    }
    store.close().await;
    Ok(json!({"id":request["id"],"status":"value","value":replies}))
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("missing {field}").into())
}

fn protocol(message: &str) -> ContractStoreError {
    ContractStoreError::Integrity(format!("invalid store probe protocol: {message}"))
}

fn name<'a>(
    operation: &'a Value,
    versions: &'a HashMap<String, AcceptedContractVersion>,
) -> Result<&'a str, ContractStoreError> {
    match operation["use"].as_str() {
        Some(name) => versions
            .get(name)
            .map(|version| version.contract.name.as_str())
            .ok_or_else(|| protocol("unknown version alias")),
        None => operation["contractName"]
            .as_str()
            .ok_or_else(|| protocol("missing contractName")),
    }
}

fn digest<'a>(
    operation: &'a Value,
    versions: &'a HashMap<String, AcceptedContractVersion>,
) -> Result<&'a str, ContractStoreError> {
    operation["contractDigest"]
        .as_str()
        .or_else(|| {
            operation["use"]
                .as_str()
                .and_then(|name| versions.get(name))
                .map(|version| version.contract_digest.as_str())
        })
        .ok_or_else(|| protocol("missing digest/alias"))
}

pub(super) fn fault(id: &str, error: ContractStoreError) -> Value {
    let mut value = json!({"id":id,"outcome":"error"});
    let exception = match &error {
        ContractStoreError::InvalidArgument { parameter, .. } => {
            value["parameter"] = json!(parameter);
            if *parameter == "pageSize" {
                "System.ArgumentOutOfRangeException"
            } else {
                "System.ArgumentException"
            }
        }
        ContractStoreError::NullArgument(parameter) => {
            value["parameter"] = json!(parameter);
            "System.ArgumentNullException"
        }
        ContractStoreError::UnsupportedSchema(_) => "System.InvalidOperationException",
        ContractStoreError::NotFound { .. } => {
            value["message"] = json!(error.to_string());
            "Flaggo.ContractStore.ContractVersionNotFoundException"
        }
        ContractStoreError::AuthorityConflict {
            contract_name,
            existing_authority,
            requested_authority,
        } => {
            value["contractName"] = json!(contract_name);
            value["existingAuthority"] = json!(existing_authority);
            value["requestedAuthority"] = json!(requested_authority);
            "Flaggo.ContractStore.ContractNameAuthorityConflictException"
        }
        ContractStoreError::Integrity(_) => {
            value["message"] = json!(error.to_string());
            "System.IO.InvalidDataException"
        }
        ContractStoreError::Cancelled => "System.OperationCanceledException",
        ContractStoreError::Timestamp(_) => "System.FormatException",
        ContractStoreError::Contract(error) => match error {
            ContractError::Json(_) => "System.Text.Json.JsonException",
            ContractError::Expression(_) => {
                value["message"] = json!(error.to_string());
                "Flaggo.Expressions.ExpressionCompilationException"
            }
            ContractError::Serialization(_) => {
                value["parameter"] = Value::Null;
                "System.ArgumentException"
            }
            ContractError::InvalidUnicode(_) => "System.InvalidOperationException",
            ContractError::MissingTypeDiscriminator => "System.NotSupportedException",
            ContractError::InvalidModel(_) => "System.NullReferenceException",
        },
        ContractStoreError::Database(sqlx::Error::Database(error)) => {
            let code = error.code().and_then(|code| code.parse::<i32>().ok());
            value["sqliteCode"] = json!(code.map(|code| code & 255));
            "Microsoft.Data.Sqlite.SqliteException"
        }
        ContractStoreError::Database(_) => "System.InvalidCastException",
    };
    value["exceptionType"] = json!(exception);
    value
}
