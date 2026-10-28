use crate::{CheckedExpression, CompilationError, CompiledExpression, ExpressionCompiler};
use flaggo_contract::digests::{executable_digest, is_sha256_digest};
use flaggo_contract::validation::{
    ExpressionPurpose, MAXIMUM_COLLECTION_SIZE, boolean_schema, is_decision_name,
    validate_executable,
};
use flaggo_contract::{
    ContractError, DecisionContract, DecisionExecutable, ExecutableRule, RuleReturn,
    ValidationIssue, ValueSchema,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckedDecisionExecutable {
    pub rules: Vec<Option<CheckedExecutableRule>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckedExecutableRule {
    pub name: String,
    pub predicate: CheckedExpression,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CheckedExpression>,
}

pub struct ExecutableCompilation {
    pub executable: DecisionExecutable,
    pub executable_digest: String,
    pub checked_executable: CheckedDecisionExecutable,
    pub compiled_executable: CompiledDecisionExecutable,
}

pub struct CompiledDecisionExecutable {
    executable: DecisionExecutable,
    executable_digest: String,
    rules: Vec<CompiledExecutableRule>,
}

impl CompiledDecisionExecutable {
    pub fn executable(&self) -> &DecisionExecutable {
        &self.executable
    }

    pub fn executable_digest(&self) -> &str {
        &self.executable_digest
    }

    pub fn rules(&self) -> &[CompiledExecutableRule] {
        &self.rules
    }
}

pub struct CompiledExecutableRule {
    name: String,
    predicate: CompiledExpression,
    result: RuleReturn,
    result_expression: Option<CompiledExpression>,
    referenced_attributes: HashSet<String>,
}

impl CompiledExecutableRule {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn predicate(&self) -> &CompiledExpression {
        &self.predicate
    }

    pub fn result(&self) -> &RuleReturn {
        &self.result
    }

    pub fn result_expression(&self) -> Option<&CompiledExpression> {
        self.result_expression.as_ref()
    }

    pub fn referenced_attributes(&self) -> &HashSet<String> {
        &self.referenced_attributes
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutableCompilationError {
    #[error("{}", .0.iter().filter(|issue| issue.severity == "error")
        .map(|issue| format!("{}: {}", issue.path, issue.message)).collect::<Vec<_>>().join("; "))]
    Validation(Vec<ValidationIssue>),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Expression(#[from] CompilationError),
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutableMaterializationError {
    #[error("{message}")]
    Invalid {
        message: String,
        #[source]
        source: Option<CompilationError>,
    },
    #[error(transparent)]
    Contract(#[from] ContractError),
}

fn invalid(message: impl Into<String>) -> ExecutableMaterializationError {
    ExecutableMaterializationError::Invalid {
        message: message.into(),
        source: None,
    }
}

#[derive(Default)]
pub struct ExecutableCompiler {
    expressions: ExpressionCompiler,
}

impl ExecutableCompiler {
    pub fn compile(
        &self,
        contract: &DecisionContract,
        executable: &DecisionExecutable,
    ) -> Result<ExecutableCompilation, ExecutableCompilationError> {
        let issues = validate_executable(contract, executable, Some(&self.expressions))?;
        if issues.iter().any(|issue| issue.severity == "error") {
            return Err(ExecutableCompilationError::Validation(issues));
        }
        let attributes = attributes(contract)?;
        let mut normalized = executable.clone();
        let mut compiled_rules = Vec::with_capacity(executable.rules.len());
        let mut checked_rules = Vec::with_capacity(executable.rules.len());
        for rule in &mut normalized.rules {
            let rule = rule.as_mut().ok_or_else(null_rule)?;
            let predicate = self.expressions.compile(
                predicate_source(rule)?,
                &attributes,
                &boolean_schema(),
                ExpressionPurpose::Predicate,
            )?;
            let result = match &rule.result {
                RuleReturn::Expression(expression) => Some(self.expressions.compile(
                    expression,
                    &attributes,
                    &contract.result.schema,
                    ExpressionPurpose::Result,
                )?),
                RuleReturn::Value(_) => None,
            };
            rule.when.expression = Some(predicate.checked().canonical_expression.clone());
            if let Some(result) = &result {
                rule.result = RuleReturn::Expression(result.checked().canonical_expression.clone());
            }
            checked_rules.push(Some(CheckedExecutableRule {
                name: rule.name.clone(),
                predicate: predicate.checked().clone(),
                result: result.as_ref().map(|result| result.checked().clone()),
            }));
            compiled_rules.push(compiled_rule(rule, predicate, result));
        }
        let digest = executable_digest(&normalized, None)?;
        Ok(ExecutableCompilation {
            compiled_executable: CompiledDecisionExecutable {
                executable: normalized.clone(),
                executable_digest: digest.clone(),
                rules: compiled_rules,
            },
            executable: normalized,
            executable_digest: digest,
            checked_executable: CheckedDecisionExecutable {
                rules: checked_rules,
            },
        })
    }

    pub fn materialize(
        &self,
        contract: &DecisionContract,
        contract_digest: &str,
        executable: &DecisionExecutable,
        digest: &str,
        checked: &CheckedDecisionExecutable,
    ) -> Result<CompiledDecisionExecutable, ExecutableMaterializationError> {
        validate_stored_shape(contract, contract_digest, executable, digest, checked)?;
        let attributes = attributes(contract)?;
        let boolean = boolean_schema();
        let mut rules = Vec::with_capacity(executable.rules.len());
        for (rule, checked) in executable.rules.iter().zip(&checked.rules) {
            let rule = rule.as_ref().ok_or_else(null_rule)?;
            let checked = checked.as_ref().ok_or_else(null_rule)?;
            if predicate_source(rule)? != checked.predicate.canonical_expression {
                return Err(invalid(format!(
                    "Rule '{}' predicate is not its stored canonical expression.",
                    rule.name
                )));
            }
            let materialize = |checked, schema, purpose| {
                self.expressions
                    .materialize(checked, &attributes, schema, purpose)
                    .map_err(|source| ExecutableMaterializationError::Invalid {
                        message: format!(
                            "Rule '{}' contains an invalid checked expression.",
                            rule.name
                        ),
                        source: Some(source),
                    })
            };
            let predicate =
                materialize(&checked.predicate, &boolean, ExpressionPurpose::Predicate)?;
            let result = match &rule.result {
                RuleReturn::Expression(expression) => {
                    let checked = checked
                        .result
                        .as_ref()
                        .filter(|checked| checked.canonical_expression == *expression)
                        .ok_or_else(|| {
                            invalid(format!(
                                "Rule '{}' return is not its stored canonical expression.",
                                rule.name
                            ))
                        })?;
                    Some(materialize(
                        checked,
                        &contract.result.schema,
                        ExpressionPurpose::Result,
                    )?)
                }
                RuleReturn::Value(_) => {
                    if checked.result.is_some() {
                        return Err(invalid(format!(
                            "Literal rule '{}' unexpectedly has a checked return expression.",
                            rule.name
                        )));
                    }
                    None
                }
            };
            rules.push(compiled_rule(rule, predicate, result));
        }
        Ok(CompiledDecisionExecutable {
            executable: executable.clone(),
            executable_digest: digest.to_owned(),
            rules,
        })
    }
}

fn validate_stored_shape(
    contract: &DecisionContract,
    contract_digest: &str,
    executable: &DecisionExecutable,
    digest: &str,
    checked: &CheckedDecisionExecutable,
) -> Result<(), ExecutableMaterializationError> {
    if !is_sha256_digest(contract_digest) || executable.contract_digest != contract_digest {
        return Err(invalid(
            "Executable contract identity does not match the accepted contract.",
        ));
    }
    if executable.kind != "rules" {
        return Err(invalid(format!(
            "Executable kind '{}' is not supported.",
            executable.kind
        )));
    }
    if executable.rules.len() > MAXIMUM_COLLECTION_SIZE {
        return Err(invalid(format!(
            "Executable exceeds the {MAXIMUM_COLLECTION_SIZE}-rule limit."
        )));
    }
    if executable_digest(executable, None)? != digest {
        return Err(invalid(
            "Executable digest does not match its canonical stored content.",
        ));
    }
    if checked.rules.len() != executable.rules.len() {
        return Err(invalid("Checked rule count does not match the executable."));
    }
    let mut names = HashSet::new();
    let mut issues = Vec::new();
    for (index, (rule, checked)) in executable.rules.iter().zip(&checked.rules).enumerate() {
        let rule = rule.as_ref().ok_or_else(null_rule)?;
        let checked = checked.as_ref().ok_or_else(null_rule)?;
        if !is_decision_name(&rule.name) {
            return Err(invalid(format!(
                "Executable rule name '{}' is invalid.",
                rule.name
            )));
        }
        if !names.insert(&rule.name) {
            return Err(invalid(format!(
                "Executable rule name '{}' is duplicated.",
                rule.name
            )));
        }
        if rule.name != checked.name {
            return Err(invalid(format!(
                "Checked rule at index {index} does not match executable rule '{}'.",
                rule.name
            )));
        }
        if rule.when.expression.is_none() {
            return Err(invalid(format!(
                "Executable rule '{}' has no deterministic predicate.",
                rule.name
            )));
        }
        if let RuleReturn::Value(value) = &rule.result {
            flaggo_contract::value_schema::validate_value(
                &contract.result.schema,
                value,
                &format!("/rules/{index}/return/value"),
                &mut issues,
            )?;
        }
    }
    if !issues.is_empty() {
        return Err(invalid(
            "Executable contains a literal result that violates the contract.",
        ));
    }
    Ok(())
}

fn attributes(contract: &DecisionContract) -> Result<IndexMap<String, ValueSchema>, ContractError> {
    let mut attributes = IndexMap::new();
    for attribute in &contract.attributes {
        let attribute = attribute
            .as_ref()
            .ok_or_else(|| ContractError::InvalidModel("null accepted attribute".into()))?;
        if attributes
            .insert(attribute.name.clone(), attribute.schema.clone())
            .is_some()
        {
            return Err(ContractError::InvalidModel(
                "duplicate accepted attribute".into(),
            ));
        }
    }
    Ok(attributes)
}

fn predicate_source(rule: &ExecutableRule) -> Result<&str, ContractError> {
    rule.when
        .expression
        .as_deref()
        .ok_or_else(|| ContractError::InvalidModel("missing predicate expression".into()))
}

fn null_rule() -> ContractError {
    ContractError::InvalidModel("null executable or checked rule".into())
}

fn compiled_rule(
    rule: &ExecutableRule,
    predicate: CompiledExpression,
    result_expression: Option<CompiledExpression>,
) -> CompiledExecutableRule {
    let mut referenced_attributes: HashSet<_> = predicate
        .checked()
        .referenced_attributes
        .iter()
        .cloned()
        .collect();
    if let Some(result) = &result_expression {
        referenced_attributes.extend(result.checked().referenced_attributes.iter().cloned());
    }
    CompiledExecutableRule {
        name: rule.name.clone(),
        predicate,
        result: rule.result.clone(),
        result_expression,
        referenced_attributes,
    }
}
