mod runtime;

pub use runtime::{
    ActiveExecutable, ActiveExecutableReader, ContractVersion, ContractVersionReader,
    DecisionRuntime, MAXIMUM_COMPILED_EXECUTABLES, StoreReadError,
};

use flaggo_contract::{
    ContractError, DecisionContract, EvaluationProvenance, JsonValue, RuleReturn, RuntimeDecision,
    RuntimeInput, ValidationIssue,
};
use flaggo_expression::{CompiledDecisionExecutable, EvaluationError};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Cancellation = Option<Arc<AtomicBool>>;

#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    #[error("{0}")]
    InvalidIdentity(&'static str),
    #[error("Contract version '{contract_name}' at '{contract_digest}' was not found.")]
    ContractNotFound {
        contract_name: String,
        contract_digest: String,
    },
    #[error("Runtime input does not satisfy the decision contract.")]
    InputValidation(Vec<ValidationIssue>),
    #[error("No executable is active for contract '{0}'.")]
    ActiveExecutableNotFound(String),
    #[error("{message}")]
    Integrity {
        message: &'static str,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    #[error("Rule '{rule}' {role} evaluation failed.")]
    Evaluation {
        rule: String,
        role: &'static str,
        #[source]
        source: EvaluationError,
    },
    #[error(
        "Rule '{rule}' predicate evaluation failed: Predicate did not produce a boolean value."
    )]
    NonBooleanPredicate { rule: String },
    #[error("{}", match rule {
        Some(rule) => format!("Rule '{rule}' produced a result that violates the result schema."),
        None => "The contract default violates the result schema.".into(),
    })]
    ResultValidation {
        rule: Option<String>,
        issues: Vec<ValidationIssue>,
    },
    #[error("The operation was canceled.")]
    Cancelled,
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    NonFiniteResult(EvaluationError),
    #[error(transparent)]
    MaterializationNative(Arc<flaggo_expression::ExecutableMaterializationError>),
    #[error(transparent)]
    Store(#[from] StoreReadError),
}

impl DecisionError {
    pub fn telemetry_classification(&self) -> (&'static str, &'static str) {
        match self {
            Self::InvalidIdentity(_)
            | Self::InputValidation(_)
            | Self::NonFiniteResult(_)
            | Self::Contract(ContractError::Serialization(_)) => ("invalid", "validation"),
            Self::Store(StoreReadError::InvalidArgument(_)) => ("invalid", "validation"),
            Self::ContractNotFound { .. } => ("not_found", "not_found"),
            Self::ActiveExecutableNotFound(_) | Self::Store(StoreReadError::Dependency(_)) => {
                ("unavailable", "dependency")
            }
            Self::Store(StoreReadError::Timeout(_)) => ("unavailable", "timeout"),
            Self::Cancelled | Self::Store(StoreReadError::Cancelled) => ("cancelled", "cancelled"),
            Self::Integrity { .. } | Self::ResultValidation { .. } => ("failure", "integrity"),
            Self::MaterializationNative(error)
                if matches!(
                    error.as_ref(),
                    flaggo_expression::ExecutableMaterializationError::Contract(
                        ContractError::Serialization(_)
                    )
                ) =>
            {
                ("invalid", "validation")
            }
            _ => ("failure", "internal"),
        }
    }
}

pub fn evaluate(
    contract: &DecisionContract,
    executable: &CompiledDecisionExecutable,
    input: &RuntimeInput,
    cancellation: Cancellation,
) -> Result<RuntimeDecision, DecisionError> {
    for rule in executable.rules() {
        if cancellation
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
        {
            return Err(DecisionError::Cancelled);
        }
        if rule
            .referenced_attributes()
            .iter()
            .any(|name| !input.attributes.contains_key(name))
        {
            continue;
        }
        let run = |expression: &flaggo_expression::CompiledExpression, role| {
            expression
                .evaluate(&input.attributes, cancellation.clone())
                .map_err(|source| match source {
                    EvaluationError::NonFiniteResult => DecisionError::NonFiniteResult(source),
                    source => DecisionError::Evaluation {
                        rule: rule.name().into(),
                        role,
                        source,
                    },
                })
        };
        let predicate = run(rule.predicate(), "predicate")?;
        let JsonValue::Bool(matches) = predicate else {
            return Err(DecisionError::NonBooleanPredicate {
                rule: rule.name().into(),
            });
        };
        if !matches {
            continue;
        }
        let result = match rule.result() {
            RuleReturn::Value(value) => value.clone(),
            RuleReturn::Expression(_) => {
                let expression = rule.result_expression().ok_or(DecisionError::Integrity {
                    message: "Compiled rule has no executable return.",
                    source: None,
                })?;
                run(expression, "return")?
            }
        };
        validate_result(contract, &result, Some(rule.name()))?;
        return Ok(decision(
            executable,
            result,
            EvaluationProvenance::Rule {
                rule: rule.name().into(),
            },
        ));
    }
    let result = contract.result.default.clone();
    validate_result(contract, &result, None)?;
    Ok(decision(
        executable,
        result,
        EvaluationProvenance::Default {},
    ))
}

fn validate_result(
    contract: &DecisionContract,
    result: &JsonValue,
    rule: Option<&str>,
) -> Result<(), DecisionError> {
    let mut issues = Vec::new();
    flaggo_contract::value_schema::validate_value(
        &contract.result.schema,
        result,
        "/result",
        &mut issues,
    )?;
    if issues.is_empty() {
        Ok(())
    } else {
        Err(DecisionError::ResultValidation {
            rule: rule.map(str::to_owned),
            issues,
        })
    }
}

fn decision(
    executable: &CompiledDecisionExecutable,
    result: JsonValue,
    evaluation: EvaluationProvenance,
) -> RuntimeDecision {
    RuntimeDecision {
        contract_digest: executable.executable().contract_digest.clone(),
        executable_digest: executable.executable_digest().into(),
        result,
        evaluation,
    }
}
