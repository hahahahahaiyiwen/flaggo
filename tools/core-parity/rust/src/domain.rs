use flaggo_contract::digests::{ExpressionCanonicalizer, contract_digest, executable_digest};
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
    #[serde(default)]
    with_expressions: bool,
}

pub fn evaluate(request: Request) -> Result<Value, Box<dyn std::error::Error>> {
    if matches!(request.domain_action.as_str(), "validate" | "validateValue") {
        return validate(request);
    }
    let bytes = request.document.as_bytes();
    if request.domain_action == "rawRoundtrip" && request.domain_model == "json" {
        let result = strict_json::deserialize::<flaggo_contract::JsonValue>(bytes)
            .and_then(|value| strict_json::serialize(&value))
            .map(|bytes| String::from_utf8(bytes).expect("JSON serialization emits UTF-8"));
        return Ok(match result {
            Ok(value) => json!({"id":request.id, "status":"value", "value":value}),
            Err(error) => fault(&request.id, error),
        });
    }
    let digest = match request.domain_action.as_str() {
        "digest" => true,
        "roundtrip" | "canonicalize" => false,
        _ => return Err(std::io::Error::other("unknown domain action").into()),
    };
    if digest && !matches!(request.domain_model.as_str(), "contract" | "executable") {
        return Err(std::io::Error::other("model does not have a digest").into());
    }

    fn validate(request: Request) -> Result<Value, Box<dyn std::error::Error>> {
        use flaggo_contract::{JsonValue, validation, value_schema};
        let bytes = request.document.as_bytes();
        let compiler = request
            .with_expressions
            .then(flaggo_expression::ExpressionCompiler::default);
        let validator = compiler
            .as_ref()
            .map(|compiler| compiler as &dyn validation::ContractExpressionValidator);
        let result = (|| -> Result<Value, ContractError> {
            let mut issues = Vec::new();
            match request.domain_model.as_str() {
                "contract" => {
                    return Ok(serde_json::to_value(validation::validate_contract(
                        &strict_json::deserialize(bytes)?,
                        validator,
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
                            validator,
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
            Err(error) => Ok(fault(&request.id, error)),
        }
    }
    let compiler = request
        .with_expressions
        .then(flaggo_expression::ExpressionCompiler::default);
    let canonicalizer = compiler
        .as_ref()
        .map(|compiler| compiler as &dyn ExpressionCanonicalizer);
    let result = (|| -> Result<(Value, Option<String>), ContractError> {
        match request.domain_model.as_str() {
            "contract" => {
                let mut model: DecisionContract = strict_json::deserialize(bytes)?;
                if request.domain_action == "canonicalize" {
                    let compiler = compiler.as_ref().ok_or_else(|| {
                        ContractError::InvalidModel(
                            "canonicalization requires an expression compiler".to_owned(),
                        )
                    })?;
                    model = flaggo_contract::digests::canonicalize_expressions(&model, compiler)?;
                }
                let digest = if digest {
                    Some(contract_digest(&model, canonicalizer)?)
                } else {
                    None
                };
                Ok((strict_json::to_value(&model)?, digest))
            }
            "executable" => {
                let model: DecisionExecutable = strict_json::deserialize(bytes)?;
                let digest = if digest {
                    Some(executable_digest(&model, canonicalizer)?)
                } else {
                    None
                };
                Ok((strict_json::to_value(&model)?, digest))
            }
            "runtimeInput" => Ok((
                strict_json::to_value(&strict_json::deserialize::<RuntimeInput>(bytes)?)?,
                None,
            )),
            "runtimeDecision" => Ok((
                strict_json::to_value(&strict_json::deserialize::<
                    flaggo_contract::RuntimeDecision,
                >(bytes)?)?,
                None,
            )),
            "schema" => Ok((
                strict_json::to_value(&strict_json::deserialize::<ValueSchema>(bytes)?)?,
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
        Err(error) => Ok(fault(&request.id, error)),
    }
}

fn fault(id: &str, error: ContractError) -> Value {
    let (status, exception_type) = match &error {
        ContractError::Json(_) => ("json-error", None),
        ContractError::Expression(_) => ("compile-error", None),
        ContractError::MissingTypeDiscriminator => {
            ("runtime-fault", Some("System.NotSupportedException"))
        }
        ContractError::Serialization(_) => ("runtime-fault", Some("System.ArgumentException")),
        ContractError::InvalidUnicode(_) => {
            ("runtime-fault", Some("System.InvalidOperationException"))
        }
        ContractError::InvalidModel(_) => ("runtime-fault", None),
    };
    let mut response = json!({"id":id, "status":status, "message":error.to_string()});
    if let Some(exception_type) = exception_type {
        response["exceptionType"] = json!(exception_type);
    }
    response
}
