use flaggo_contract::digests::{contract_digest, executable_digest};
use flaggo_contract::strict_json;
use flaggo_contract::{
    ContractError, DecisionContract, DecisionExecutable, RuntimeInput, ValueSchema,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    id: String,
    domain_model: String,
    domain_action: String,
    document: String,
    #[serde(default)]
    validation_contract: Option<String>,
    #[serde(default)]
    validation_value: Option<String>,
}

pub fn evaluate(request: Request) -> Result<Value, Box<dyn std::error::Error>> {
    if matches!(request.domain_action.as_str(), "validate" | "validateValue") {
        return validate(request);
    }
    let bytes = request.document.as_bytes();
    let digest = match request.domain_action.as_str() {
        "digest" => true,
        "roundtrip" => false,
        _ => return Err(std::io::Error::other("unknown domain action").into()),
    };
    if digest && !matches!(request.domain_model.as_str(), "contract" | "executable") {
        return Err(std::io::Error::other("model does not have a digest").into());
    }

    fn validate(request: Request) -> Result<Value, Box<dyn std::error::Error>> {
        use flaggo_contract::{JsonValue, validation, value_schema};
        let bytes = request.document.as_bytes();
        let result = (|| -> Result<Value, ContractError> {
            let mut issues = Vec::new();
            match request.domain_model.as_str() {
                "contract" => {
                    return Ok(serde_json::to_value(validation::validate_contract(
                        &strict_json::deserialize(bytes)?,
                        None,
                    )?)?);
                }
                "schema" if request.domain_action == "validateValue" => {
                    let value = request.validation_value.as_ref().ok_or_else(|| {
                        ContractError::InvalidModel("missing validation value".into())
                    })?;
                    value_schema::validate_value(
                        &strict_json::deserialize(bytes)?,
                        &serde_json::from_str::<JsonValue>(value)?,
                        "/value",
                        &mut issues,
                    )?;
                }
                "schema" => value_schema::validate_schema(
                    &strict_json::deserialize(bytes)?,
                    "/schema",
                    &mut issues,
                ),
                "runtimeInput" | "executable" => {
                    let contract = request.validation_contract.as_ref().ok_or_else(|| {
                        ContractError::InvalidModel("missing validation contract".into())
                    })?;
                    let contract = strict_json::deserialize(contract.as_bytes())?;
                    if request.domain_model == "runtimeInput" {
                        validation::validate_runtime_input(
                            &contract,
                            &strict_json::deserialize(bytes)?,
                            &mut issues,
                        )?;
                    } else {
                        issues = validation::validate_executable(
                            &contract,
                            &strict_json::deserialize(bytes)?,
                            None,
                        )?;
                    }
                }
                _ => {
                    return Err(ContractError::InvalidModel(
                        "model cannot be validated".into(),
                    ));
                }
            }
            Ok(serde_json::to_value(issues)?)
        })();
        match result {
            Ok(value) => Ok(json!({"id":request.id, "status":"value", "value":value})),
            Err(ContractError::Json(error)) => {
                Ok(json!({"id":request.id, "status":"json-error", "message":error.to_string()}))
            }
            Err(error @ (ContractError::InvalidUnicode(_) | ContractError::InvalidModel(_))) => {
                Ok(json!({"id":request.id, "status":"runtime-fault", "message":error.to_string()}))
            }
            Err(error) => Err(error.into()),
        }
    }
    let result = (|| -> Result<(Value, Option<String>), ContractError> {
        match request.domain_model.as_str() {
            "contract" => {
                let model: DecisionContract = strict_json::deserialize(bytes)?;
                let digest = if digest {
                    Some(contract_digest(&model, None)?)
                } else {
                    None
                };
                Ok((serde_json::to_value(model)?, digest))
            }
            "executable" => {
                let model: DecisionExecutable = strict_json::deserialize(bytes)?;
                let digest = if digest {
                    Some(executable_digest(&model, None)?)
                } else {
                    None
                };
                Ok((serde_json::to_value(model)?, digest))
            }
            "runtimeInput" => Ok((
                serde_json::to_value(strict_json::deserialize::<RuntimeInput>(bytes)?)?,
                None,
            )),
            "schema" => Ok((
                serde_json::to_value(strict_json::deserialize::<ValueSchema>(bytes)?)?,
                None,
            )),
            "json" => Ok((strict_json::deserialize::<Value>(bytes)?, None)),
            _ => Err(ContractError::InvalidModel(
                "unknown domain model".to_owned(),
            )),
        }
    })();
    match result {
        Ok((document, digest)) => Ok(json!({
            "id": request.id, "status": "value", "value": {"document":document, "digest":digest}
        })),
        Err(ContractError::Json(error)) => Ok(json!({
            "id": request.id, "status": "json-error", "message": error.to_string()
        })),
        Err(ContractError::InvalidUnicode(error)) => Ok(json!({
            "id": request.id, "status": "runtime-fault", "message": error.to_string()
        })),
        Err(error) => Err(error.into()),
    }
}
