use flaggo_contract::{
    ContractError, DecisionContract, DecisionExecutable, RuntimeInput, strict_json,
};
use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, ContractVersion, ContractVersionReader,
    DecisionError, DecisionRuntime, StoreReadError,
};
use flaggo_expression::{
    CheckedDecisionExecutable, ExecutableCompilationError, ExecutableCompiler,
    ExecutableMaterializationError, ExpressionCompiler,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, atomic::AtomicBool};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    id: String,
    decision_action: String,
    contract: String,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    artifact: Option<Artifact>,
    #[serde(default)]
    evaluation_contract: Option<String>,
    #[serde(default)]
    cancelled: bool,
    #[serde(default)]
    contract_fault: Option<String>,
    #[serde(default)]
    active_fault: Option<String>,
    #[serde(default)]
    contract_name: Option<String>,
    #[serde(default)]
    contract_digest: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    executable: DecisionExecutable,
    executable_digest: String,
    checked_executable: CheckedDecisionExecutable,
}

pub async fn evaluate(request: Request) -> Result<Value, Box<dyn std::error::Error>> {
    let mut artifact = None;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let result = run(&request, &mut artifact, Arc::clone(&calls)).await;
    let mut response = match result {
        Ok(value) => json!({"id":request.id,"status":"value","value":value}),
        Err(ProbeError::Compile(ExecutableCompilationError::Validation(issues))) => {
            json!({"id":request.id,"status":"value","value":{
                "outcome":"compile-invalid",
                "message":ExecutableCompilationError::Validation(issues.clone()).to_string(),
                "issues":issues
            }})
        }
        Err(ProbeError::Materialize(ExecutableMaterializationError::Invalid {
            message, ..
        })) => {
            json!({"id":request.id,"status":"value","value":{
                "outcome":"materialize-invalid","message":message
            }})
        }
        Err(ProbeError::Decision(error)) => {
            let (outcome, category) = error.telemetry_classification();
            let mut value = json!({
                "outcome":"error", "calls":*calls.lock().expect("calls poisoned"),
                "telemetry":{"outcome":outcome,"category":category}
            });
            let exception = match &error {
                DecisionError::InvalidIdentity(_) | DecisionError::NonFiniteResult(_) => {
                    "System.ArgumentException"
                }
                DecisionError::InputValidation(issues) => {
                    value["issues"] = json!(issues);
                    "Flaggo.Decision.RuntimeInputValidationException"
                }
                DecisionError::ResultValidation { rule, issues } => {
                    value["rule"] = json!(rule);
                    value["issues"] = json!(issues);
                    "Flaggo.Decision.DecisionResultValidationException"
                }
                DecisionError::Evaluation { rule, role, .. } => {
                    value["rule"] = json!(rule);
                    value["role"] = json!(role);
                    "Flaggo.Decision.DecisionEvaluationException"
                }
                DecisionError::NonBooleanPredicate { rule } => {
                    value["rule"] = json!(rule);
                    value["role"] = json!("predicate");
                    "Flaggo.Decision.DecisionEvaluationException"
                }
                DecisionError::ContractNotFound { .. } => {
                    "Flaggo.Decision.DecisionContractVersionNotFoundException"
                }
                DecisionError::ActiveExecutableNotFound(_) => {
                    "Flaggo.Decision.ActiveExecutableNotFoundException"
                }
                DecisionError::Integrity { .. } => "Flaggo.Decision.RuntimeIntegrityException",
                DecisionError::Cancelled | DecisionError::Store(StoreReadError::Cancelled) => {
                    "System.OperationCanceledException"
                }
                DecisionError::Store(StoreReadError::Dependency(_)) => "System.IO.IOException",
                DecisionError::Store(StoreReadError::Timeout(_)) => "System.TimeoutException",
                DecisionError::Contract(error) => return Ok(contract_fault(&request.id, error)),
                DecisionError::MaterializationNative(error) => {
                    return Ok(json!({"id":request.id,"status":"runtime-fault",
                        "message":error.to_string()}));
                }
                DecisionError::Store(StoreReadError::Integrity(_)) => {
                    return Err(std::io::Error::other("unmapped store integrity fault").into());
                }
                DecisionError::Store(StoreReadError::InvalidArgument(_)) => {
                    "System.ArgumentException"
                }
                DecisionError::Store(StoreReadError::Native(error)) => {
                    return Err(std::io::Error::other(error.to_string()).into());
                }
            };
            value["exceptionType"] = json!(exception);
            if !matches!(
                exception,
                "System.ArgumentException" | "System.OperationCanceledException"
            ) {
                value["message"] = json!(error.to_string());
            }
            json!({"id":request.id,"status":"value","value":value})
        }
        Err(ProbeError::Contract(error))
        | Err(ProbeError::Compile(ExecutableCompilationError::Contract(error)))
        | Err(ProbeError::Materialize(ExecutableMaterializationError::Contract(error))) => {
            contract_fault(&request.id, &error)
        }
        Err(ProbeError::Compile(error)) => return Err(error.into()),
        Err(ProbeError::Protocol(error)) => return Err(std::io::Error::other(error).into()),
    };
    if let Some(artifact) = artifact {
        response["artifact"] = strict_json::to_value(&artifact)?;
    }
    Ok(response)
}

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Compile(#[from] ExecutableCompilationError),
    #[error(transparent)]
    Materialize(#[from] ExecutableMaterializationError),
    #[error(transparent)]
    Decision(#[from] DecisionError),
    #[error("{0}")]
    Protocol(&'static str),
}

async fn run(
    request: &Request,
    artifact_out: &mut Option<Artifact>,
    calls: Arc<Mutex<Vec<&'static str>>>,
) -> Result<Value, ProbeError> {
    let contract: DecisionContract = strict_json::deserialize(request.contract.as_bytes())?;
    let digest =
        flaggo_contract::digests::contract_digest(&contract, Some(&ExpressionCompiler::default()))?;
    let compiler = ExecutableCompiler::default();
    let mut compiled = None;
    let artifact = if let Some(artifact) = &request.artifact {
        strict_json::deserialize(&strict_json::serialize(artifact)?)?
    } else {
        let mut executable: DecisionExecutable = strict_json::deserialize(
            request
                .executable
                .as_ref()
                .ok_or(ProbeError::Protocol("missing executable"))?
                .as_bytes(),
        )?;
        executable.contract_digest = digest.clone();
        let compilation = compiler.compile(&contract, &executable)?;
        compiled = Some(compilation.compiled_executable);
        Artifact {
            executable: compilation.executable,
            executable_digest: compilation.executable_digest,
            checked_executable: compilation.checked_executable,
        }
    };
    *artifact_out = Some(artifact);
    let artifact = artifact_out.as_ref().expect("assigned artifact");
    if request.decision_action == "compile" {
        return Ok(
            json!({"outcome":"compiled","executable":artifact.executable,
            "executableDigest":artifact.executable_digest}),
        );
    }
    let input: RuntimeInput = strict_json::deserialize(
        request
            .input
            .as_ref()
            .ok_or(ProbeError::Protocol("missing input"))?
            .as_bytes(),
    )?;
    let cancellation = request.cancelled.then(|| Arc::new(AtomicBool::new(true)));
    let decision = match request.decision_action.as_str() {
        "evaluate" => {
            let compiled = match compiled {
                Some(compiled) => compiled,
                None => compiler.materialize(
                    &contract,
                    &digest,
                    &artifact.executable,
                    &artifact.executable_digest,
                    &artifact.checked_executable,
                )?,
            };
            let contract = match &request.evaluation_contract {
                Some(contract) => strict_json::deserialize(contract.as_bytes())?,
                None => contract,
            };
            flaggo_decision::evaluate(&contract, &compiled, &input, cancellation)?
        }
        "runtime" => {
            let name = request.contract_name.as_deref().unwrap_or(&contract.name);
            let requested_digest = request.contract_digest.as_deref().unwrap_or(&digest);
            let reader = Reader {
                version: Arc::new(ContractVersion {
                    contract_digest: digest.clone(),
                    contract: contract.clone(),
                }),
                active: Arc::new(ActiveExecutable {
                    executable: artifact.executable.clone(),
                    executable_digest: artifact.executable_digest.clone(),
                    checked_executable: artifact.checked_executable.clone(),
                }),
                contract_fault: request.contract_fault.clone(),
                active_fault: request.active_fault.clone(),
                calls: Arc::clone(&calls),
            };
            DecisionRuntime::new(reader.clone(), reader)
                .decide(name, requested_digest, &input, cancellation)
                .await?
        }
        _ => return Err(ProbeError::Protocol("unknown decision action")),
    };
    Ok(
        json!({"outcome":"decision","source":decision.evaluation.source(),"decision":decision,
        "calls":*calls.lock().expect("calls poisoned")}),
    )
}

fn contract_fault(id: &str, error: &ContractError) -> Value {
    match error {
        ContractError::Json(_) => {
            json!({"id":id,"status":"json-error","message":error.to_string()})
        }
        _ => json!({"id":id,"status":"runtime-fault","message":error.to_string()}),
    }
}

#[derive(Clone)]
struct Reader {
    version: Arc<ContractVersion>,
    active: Arc<ActiveExecutable>,
    contract_fault: Option<String>,
    active_fault: Option<String>,
    calls: Arc<Mutex<Vec<&'static str>>>,
}

fn fail(fault: Option<&str>) -> Result<(), StoreReadError> {
    match fault {
        None | Some("missing") => Ok(()),
        Some("integrity") => Err(StoreReadError::Integrity(
            std::io::Error::other("injected integrity failure").into(),
        )),
        Some("dependency") => Err(StoreReadError::Dependency(
            std::io::Error::other("injected dependency failure").into(),
        )),
        Some("timeout") => Err(StoreReadError::Timeout(
            std::io::Error::other("injected timeout").into(),
        )),
        Some("cancelled") => Err(StoreReadError::Cancelled),
        Some(_) => panic!("unknown injected store fault"),
    }
}

impl ContractVersionReader for Reader {
    async fn get_contract(
        &self,
        name: &str,
        digest: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        self.calls.lock().expect("calls poisoned").push("contract");
        fail(self.contract_fault.as_deref())?;
        Ok((self.contract_fault.as_deref() != Some("missing")
            && name == self.version.contract.name
            && digest == self.version.contract_digest)
            .then(|| Arc::clone(&self.version)))
    }
}

impl ActiveExecutableReader for Reader {
    async fn get_active(
        &self,
        _: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        self.calls.lock().expect("calls poisoned").push("active");
        fail(self.active_fault.as_deref())?;
        Ok((self.active_fault.as_deref() != Some("missing")).then(|| Arc::clone(&self.active)))
    }
}
