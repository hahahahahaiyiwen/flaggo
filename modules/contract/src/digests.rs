use crate::{
    ContractError, DecisionContract, DecisionExecutable, RuleReturn, RuleWhen, ValueSchema,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

pub trait ExpressionCanonicalizer {
    fn canonicalize(&self, expression: &str) -> Result<String, String>;
}

pub fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

pub fn contract_digest(
    contract: &DecisionContract,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<String, ContractError> {
    let mut contract = contract.clone();
    for attribute in &mut contract.attributes {
        let attribute = attribute
            .as_mut()
            .ok_or_else(|| invalid("null attribute"))?;
        normalize_schema(&mut attribute.schema)?;
    }
    normalize_schema(&mut contract.result.schema)?;
    if let Some(authored) = &mut contract.authored_executable {
        for rule in &mut authored.rules {
            let rule = rule.as_mut().ok_or_else(|| invalid("null authored rule"))?;
            rule.description = None;
            if let RuleWhen::Expression(expression) = &mut rule.when {
                canonicalize(expression, canonicalizer)?;
            }
            canonicalize_return(&mut rule.result, canonicalizer)?;
        }
    }
    if let Some(learning) = &mut contract.learning {
        for evidence in &mut learning.evidence {
            let evidence = evidence.as_mut().ok_or_else(|| invalid("null evidence"))?;
            evidence.description = None;
            evidence
                .correlate_by
                .sort_by(|a, b| ordinal(a.as_deref(), b.as_deref()));
        }
        if let Some(guardrails) = &mut learning.objective.guardrails {
            for guardrail in guardrails {
                let guardrail = guardrail
                    .as_mut()
                    .ok_or_else(|| invalid("null guardrail"))?;
                guardrail.description = None;
                canonicalize(&mut guardrail.expression, canonicalizer)?;
            }
        }
    }
    let mut value = serde_json::to_value(contract)?;
    sort_named(&mut value["attributes"])?;
    if let Some(learning) = value.get_mut("learning") {
        sort_named(&mut learning["evidence"])?;
        if let Some(guardrails) = learning["objective"].get_mut("guardrails") {
            sort_named(guardrails)?;
        }
    }
    digest(&value)
}

pub fn executable_digest(
    executable: &DecisionExecutable,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<String, ContractError> {
    let mut executable = executable.clone();
    for rule in executable.rules.iter_mut().flatten() {
        if let Some(expression) = &mut rule.when.expression {
            canonicalize(expression, canonicalizer)?;
        }
        canonicalize_return(&mut rule.result, canonicalizer)?;
    }
    digest(&serde_json::to_value(executable)?)
}

pub fn canonicalize_expressions(
    contract: &DecisionContract,
    canonicalizer: &dyn ExpressionCanonicalizer,
) -> Result<DecisionContract, ContractError> {
    let mut contract = contract.clone();
    if let Some(authored) = &mut contract.authored_executable {
        for rule in &mut authored.rules {
            let rule = rule.as_mut().ok_or_else(|| invalid("null authored rule"))?;
            if let RuleWhen::Expression(expression) = &mut rule.when {
                canonicalize(expression, Some(canonicalizer))?;
            }
            canonicalize_return(&mut rule.result, Some(canonicalizer))?;
        }
    }
    if let Some(guardrails) = contract
        .learning
        .as_mut()
        .and_then(|learning| learning.objective.guardrails.as_mut())
    {
        for guardrail in guardrails {
            let guardrail = guardrail
                .as_mut()
                .ok_or_else(|| invalid("null guardrail"))?;
            canonicalize(&mut guardrail.expression, Some(canonicalizer))?;
        }
    }
    Ok(contract)
}

fn invalid(message: &str) -> ContractError {
    ContractError::InvalidModel(message.to_owned())
}

fn canonicalize(
    expression: &mut String,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<(), ContractError> {
    if let Some(canonicalizer) = canonicalizer {
        *expression = canonicalizer
            .canonicalize(expression)
            .map_err(ContractError::Expression)?;
    }
    Ok(())
}

fn canonicalize_return(
    result: &mut RuleReturn,
    canonicalizer: Option<&dyn ExpressionCanonicalizer>,
) -> Result<(), ContractError> {
    if let RuleReturn::Expression(expression) = result {
        canonicalize(expression, canonicalizer)?;
    }
    Ok(())
}

fn normalize_schema(schema: &mut ValueSchema) -> Result<(), ContractError> {
    schema.description = None;
    if let Some(required) = &mut schema.required {
        required.sort_by(|a, b| ordinal(a.as_deref(), b.as_deref()));
    }
    if let Some(items) = &mut schema.items {
        normalize_schema(items)?;
    }
    if let Some(properties) = &mut schema.properties {
        for value in properties.values_mut() {
            normalize_schema(
                value
                    .as_mut()
                    .ok_or_else(|| invalid("null property schema"))?,
            )?;
        }
    }
    Ok(())
}

fn ordinal(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => left.encode_utf16().cmp(right.encode_utf16()),
    }
}

fn sort_named(value: &mut Value) -> Result<(), ContractError> {
    let values = value
        .as_array_mut()
        .ok_or_else(|| invalid("expected a named collection"))?;
    values.sort_by(|left, right| ordinal(left["name"].as_str(), right["name"].as_str()));
    Ok(())
}

fn digest(value: &Value) -> Result<String, ContractError> {
    let bytes = serde_jcs::to_vec(value)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}
