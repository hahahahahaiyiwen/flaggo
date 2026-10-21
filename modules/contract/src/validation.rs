use crate::digests::{ExpressionCanonicalizer, contract_digest};
use crate::value_schema::{validate_schema, validate_value};
use crate::{
    ContractError, DecisionContract, DecisionContractValidationResult, DecisionExecutable,
    EvidenceSourceKind, JsonValue, RuleReturn, RuleWhen, RuntimeInput, ValidationIssue,
    ValueSchema,
};
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const MAXIMUM_ATTRIBUTES: usize = 128;
pub const MAXIMUM_COLLECTION_SIZE: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionPurpose {
    Predicate,
    Result,
    Guardrail,
}

pub struct ExpressionValidation {
    pub is_valid: bool,
    pub canonical_expression: Option<String>,
    pub referenced_attributes: HashSet<String>,
    pub error: Option<String>,
}

pub trait ContractExpressionValidator: ExpressionCanonicalizer {
    fn validate(
        &self,
        expression: &str,
        attributes: &IndexMap<String, ValueSchema>,
        expected_result: &ValueSchema,
        purpose: ExpressionPurpose,
    ) -> ExpressionValidation;
    fn validate_syntax(&self, expression: &str) -> ExpressionValidation;
}

pub fn is_decision_name(value: &str) -> bool {
    identifier(value, true)
}

fn identifier(value: &str, punctuation: bool) -> bool {
    if !(1..=128).contains(&value.encode_utf16().count()) {
        return false;
    }
    // .NET's non-multiline '$' also matches immediately before a final LF.
    let value = value.strip_suffix('\n').unwrap_or(value);
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|value| value.is_ascii_alphabetic())
        && chars.all(|value| {
            value.is_ascii_alphanumeric()
                || value == '_'
                || punctuation && matches!(value, '.' | '-')
        })
}

pub fn validate_contract(
    contract: &DecisionContract,
    validator: Option<&dyn ContractExpressionValidator>,
) -> Result<DecisionContractValidationResult, ContractError> {
    let mut issues = Vec::new();
    for (name, value) in [
        ("tenant", &contract.authority.tenant),
        ("application", &contract.authority.application),
        ("environment", &contract.authority.environment),
    ] {
        if !value.as_deref().is_some_and(is_decision_name) {
            error(
                &mut issues,
                "invalid-authority-identifier",
                &format!("/authority/{name}"),
                "Authority identifiers must match ^[A-Za-z][A-Za-z0-9._-]*$.",
            );
        }
    }
    if !is_decision_name(&contract.name) {
        error(
            &mut issues,
            "invalid-decision-name",
            "/name",
            "Decision name must match ^[A-Za-z][A-Za-z0-9._-]*$.",
        );
    }
    if contract.expression_syntax != "flaggo.cel/v1" {
        error(
            &mut issues,
            "unsupported-expression-syntax",
            "/expression_syntax",
            "expression_syntax must be flaggo.cel/v1.",
        );
    }
    if contract.attributes.len() > MAXIMUM_ATTRIBUTES {
        error(
            &mut issues,
            "too-many-attributes",
            "/attributes",
            format!("A contract may declare at most {MAXIMUM_ATTRIBUTES} attributes."),
        );
    }
    let mut attributes = IndexMap::new();
    for (index, attribute) in contract.attributes.iter().enumerate() {
        let path = format!("/attributes/{index}");
        let Some(attribute) = attribute else {
            error(
                &mut issues,
                "null-attribute",
                &path,
                "Attribute declarations must not be null.",
            );
            continue;
        };
        if !identifier(&attribute.name, false) {
            error(
                &mut issues,
                "invalid-attribute-name",
                &format!("{path}/name"),
                "Attribute name must match ^[A-Za-z][A-Za-z0-9_]*$.",
            );
        }
        if attributes.contains_key(&attribute.name) {
            error(
                &mut issues,
                "duplicate-attribute",
                &format!("{path}/name"),
                format!("Attribute '{}' is declared more than once.", attribute.name),
            );
        } else {
            attributes.insert(attribute.name.clone(), attribute.schema.clone());
        }
        validate_schema(&attribute.schema, &format!("{path}/schema"), &mut issues);
    }
    validate_schema(&contract.result.schema, "/result/schema", &mut issues);
    let mut default_issues = Vec::new();
    validate_value(
        &contract.result.schema,
        &contract.result.default,
        "/result/default",
        &mut default_issues,
    )?;
    if !default_issues.is_empty() {
        error(
            &mut issues,
            "default-out-of-schema",
            "/result/default",
            "The default value does not satisfy result.schema.",
        );
    }
    validate_authored(contract, &attributes, validator, &mut issues)?;
    validate_learning(contract, &attributes, validator, &mut issues);
    let valid = !issues.iter().any(|issue| issue.severity == "error");
    Ok(DecisionContractValidationResult {
        status: if valid { "valid" } else { "invalid" }.into(),
        contract_digest: if valid {
            Some(contract_digest(
                contract,
                validator.map(|value| value as &dyn ExpressionCanonicalizer),
            )?)
        } else {
            None
        },
        issues,
    })
}

pub fn validate_runtime_input(
    contract: &DecisionContract,
    input: &RuntimeInput,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), ContractError> {
    if input.attributes.len() > MAXIMUM_ATTRIBUTES + 1 {
        error(
            issues,
            "too-many-attributes",
            "/attributes",
            format!(
                "Runtime input may contain at most {} attributes.",
                MAXIMUM_ATTRIBUTES + 1
            ),
        );
    }
    if !input
        .attributes
        .get("_random")
        .and_then(JsonValue::as_f64)
        .is_some_and(|value| value.is_finite() && (0.0..1.0).contains(&value))
    {
        error(
            issues,
            "invalid-internal-attribute",
            "/attributes/_random",
            "_random is required and must be a finite number in [0, 1).",
        );
    }
    let declarations = declarations(contract)?;
    for (name, value) in &input.attributes {
        if name == "_random" {
            continue;
        }
        let path = format!("/attributes/{}", escape_pointer(name));
        if name.starts_with('_') {
            error(
                issues,
                "unknown-internal-attribute",
                &path,
                format!("Internal attribute '{name}' is not defined by this contract profile."),
            );
        } else if let Some(schema) = declarations.get(name) {
            validate_value(schema, value, &path, issues)?;
        } else {
            error(
                issues,
                "undeclared-attribute",
                &path,
                format!("Attribute '{name}' is not declared by the contract."),
            );
        }
    }
    if let Some(exposure) = &input.current_exposure
        && !(1..=256).contains(&exposure.exposure_id.len())
    {
        error(
            issues,
            "invalid-current-exposure",
            "/currentExposure/exposureId",
            "exposureId must contain between 1 and 256 UTF-8 bytes.",
        );
    }
    Ok(())
}

pub fn validate_executable(
    contract: &DecisionContract,
    executable: &DecisionExecutable,
    validator: Option<&dyn ContractExpressionValidator>,
) -> Result<Vec<ValidationIssue>, ContractError> {
    let validation = validate_contract(contract, validator)?;
    let mut issues = validation.issues;
    if validation.status != "valid" {
        return Ok(issues);
    }
    if Some(&executable.contract_digest) != validation.contract_digest.as_ref() {
        error(
            &mut issues,
            "contract-digest-mismatch",
            "/contractDigest",
            "Executable contractDigest does not match the supplied contract.",
        );
    }
    if executable.kind != "rules" {
        error(
            &mut issues,
            "unsupported-executable-kind",
            "/kind",
            "Executable kind must be rules.",
        );
    }
    if executable.rules.len() > MAXIMUM_COLLECTION_SIZE {
        error(
            &mut issues,
            "too-many-rules",
            "/rules",
            format!("An executable may contain at most {MAXIMUM_COLLECTION_SIZE} rules."),
        );
    }
    let attributes = declarations(contract)?;
    let mut names = HashSet::new();
    for (index, rule) in executable.rules.iter().enumerate() {
        let rule = rule
            .as_ref()
            .ok_or_else(|| ContractError::InvalidModel("null executable rule".into()))?;
        let path = format!("/rules/{index}");
        validate_member(&rule.name, &format!("{path}/name"), &mut issues);
        if !names.insert(&rule.name) {
            error(
                &mut issues,
                "duplicate-rule",
                &format!("{path}/name"),
                format!("Rule '{}' is declared more than once.", rule.name),
            );
        }
        validate_expression(
            rule.when.expression.as_deref(),
            &attributes,
            &boolean_schema(),
            ExpressionPurpose::Predicate,
            &format!("{path}/when/expression"),
            validator,
            &mut issues,
        );
        validate_return(
            &rule.result,
            contract,
            &attributes,
            &path,
            validator,
            &mut issues,
        )?;
    }
    Ok(issues)
}

fn declarations(
    contract: &DecisionContract,
) -> Result<IndexMap<String, ValueSchema>, ContractError> {
    let mut result = IndexMap::new();
    for attribute in &contract.attributes {
        let attribute = attribute.as_ref().ok_or_else(|| {
            ContractError::InvalidModel("null attribute in accepted contract".into())
        })?;
        if result
            .insert(attribute.name.clone(), attribute.schema.clone())
            .is_some()
        {
            return Err(ContractError::InvalidModel(
                "duplicate attribute in accepted contract".into(),
            ));
        }
    }
    Ok(result)
}

fn validate_authored(
    contract: &DecisionContract,
    attributes: &IndexMap<String, ValueSchema>,
    validator: Option<&dyn ContractExpressionValidator>,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), ContractError> {
    let Some(authored) = &contract.authored_executable else {
        return Ok(());
    };
    if authored.rules.is_empty() {
        error(
            issues,
            "empty-authored-executable",
            "/authoredExecutable/rules",
            "An authored executable must contain at least one rule.",
        );
    }
    if authored.rules.len() > MAXIMUM_COLLECTION_SIZE {
        error(
            issues,
            "too-many-rules",
            "/authoredExecutable/rules",
            format!("An authored executable may contain at most {MAXIMUM_COLLECTION_SIZE} rules."),
        );
    }
    let mut names = HashSet::new();
    for (index, rule) in authored.rules.iter().enumerate() {
        let path = format!("/authoredExecutable/rules/{index}");
        let Some(rule) = rule else {
            error(
                issues,
                "null-rule",
                &path,
                "Authored rules must not be null.",
            );
            continue;
        };
        validate_member(&rule.name, &format!("{path}/name"), issues);
        if !names.insert(&rule.name) {
            error(
                issues,
                "duplicate-rule",
                &format!("{path}/name"),
                format!("Rule '{}' is declared more than once.", rule.name),
            );
        }
        match &rule.when {
            RuleWhen::Expression(expression) => validate_expression(
                Some(expression),
                attributes,
                &boolean_schema(),
                ExpressionPurpose::Predicate,
                &format!("{path}/when/expression"),
                validator,
                issues,
            ),
            RuleWhen::Condition(condition) if condition.trim().is_empty() => error(
                issues,
                "invalid-authored-condition",
                &format!("{path}/when/condition"),
                "Authored rule conditions must not be empty.",
            ),
            _ => {}
        }
        validate_return(&rule.result, contract, attributes, &path, validator, issues)?;
    }
    Ok(())
}

fn validate_return(
    result: &RuleReturn,
    contract: &DecisionContract,
    attributes: &IndexMap<String, ValueSchema>,
    path: &str,
    validator: Option<&dyn ContractExpressionValidator>,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), ContractError> {
    match result {
        RuleReturn::Value(value) => validate_value(
            &contract.result.schema,
            value,
            &format!("{path}/return/value"),
            issues,
        )?,
        RuleReturn::Expression(expression) => validate_expression(
            Some(expression),
            attributes,
            &contract.result.schema,
            ExpressionPurpose::Result,
            &format!("{path}/return/expression"),
            validator,
            issues,
        ),
    }
    Ok(())
}

static INTERVAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"\AP(?P<date>(?:\d+Y)?(?:\d+M)?(?:\d+W)?(?:\d+D)?)(?:T(?P<time>(?:\d+H)?(?:\d+M)?(?:\d+(?:\.\d+)?S)?))?\n?\z"
).expect("constant duration expression is valid")
});

fn valid_interval(value: &str) -> bool {
    // The reference regex classifies UTF-16 code units, not supplementary digits.
    if value.chars().any(|value| value.len_utf16() != 1) {
        return false;
    }
    let Some(groups) = INTERVAL.captures(value) else {
        return false;
    };
    let date = groups
        .name("date")
        .expect("date group is always present")
        .as_str();
    let time = groups.name("time").map(|value| value.as_str());
    (!date.is_empty() || time.is_some_and(|value| !value.is_empty()))
        && time != Some("")
        && value.bytes().any(|value| (b'1'..=b'9').contains(&value))
}

fn validate_learning(
    contract: &DecisionContract,
    attributes: &IndexMap<String, ValueSchema>,
    validator: Option<&dyn ContractExpressionValidator>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(learning) = &contract.learning else {
        return;
    };
    if learning.policy.mode != "auto-activation" {
        error(
            issues,
            "unsupported-learning-policy",
            "/learning/policy/mode",
            "The initial learning policy mode is auto-activation.",
        );
    }
    if !valid_interval(&learning.policy.evaluate.interval) {
        error(
            issues,
            "invalid-learning-interval",
            "/learning/policy/evaluate/interval",
            "The learning interval must be a positive ISO 8601 duration.",
        );
    }
    if learning.evidence.is_empty() {
        error(
            issues,
            "empty-evidence",
            "/learning/evidence",
            "A learning declaration must contain at least one evidence entry.",
        );
    }
    let mut names = HashSet::new();
    let mut sources = HashSet::new();
    for (index, evidence) in learning.evidence.iter().enumerate() {
        let path = format!("/learning/evidence/{index}");
        let Some(evidence) = evidence else {
            error(
                issues,
                "null-evidence",
                &path,
                "Evidence declarations must not be null.",
            );
            continue;
        };
        validate_member(&evidence.name, &format!("{path}/name"), issues);
        if !names.insert(&evidence.name) {
            error(
                issues,
                "duplicate-evidence",
                &format!("{path}/name"),
                format!("Evidence '{}' is declared more than once.", evidence.name),
            );
        }
        if evidence.attribute.is_empty() || !attributes.contains_key(&evidence.attribute) {
            error(
                issues,
                "unknown-evidence-attribute",
                &format!("{path}/attribute"),
                format!(
                    "Evidence attribute '{}' is not declared.",
                    evidence.attribute
                ),
            );
        }
        let mut correlations = IndexSet::new();
        for (index, correlation) in evidence.correlate_by.iter().enumerate() {
            let correlation_path = format!("{path}/correlateBy/{index}");
            let Some(correlation) = correlation.as_deref().filter(|value| !value.is_empty()) else {
                error(
                    issues,
                    "invalid-correlation-attribute",
                    &correlation_path,
                    "Correlation attribute names must not be null or empty.",
                );
                continue;
            };
            if !correlations.insert(correlation) {
                error(
                    issues,
                    "duplicate-correlation-attribute",
                    &correlation_path,
                    format!("Correlation attribute '{correlation}' is repeated."),
                );
            }
            if !attributes.contains_key(correlation) {
                error(
                    issues,
                    "unknown-correlation-attribute",
                    &correlation_path,
                    format!("Correlation attribute '{correlation}' is not declared."),
                );
            }
        }
        let source = &evidence.source.0;
        validate_source(source, &correlations, &format!("{path}/source"), issues);
        if !sources.insert(source_identity(source)) {
            error(
                issues,
                "duplicate-evidence-source",
                &format!("{path}/source"),
                "Each evidence entry in a contract must select a distinct OpenTelemetry source.",
            );
        }
    }
    if !names.contains(&learning.objective.primary.evidence) {
        error(
            issues,
            "unknown-primary-evidence",
            "/learning/objective/primary/evidence",
            "The primary objective must reference declared evidence.",
        );
    }
    if !matches!(
        learning.objective.primary.direction.as_str(),
        "minimize" | "maximize"
    ) {
        error(
            issues,
            "invalid-objective-direction",
            "/learning/objective/primary/direction",
            "Objective direction must be minimize or maximize.",
        );
    }
    let mut names = HashSet::new();
    for (index, guardrail) in learning
        .objective
        .guardrails
        .as_deref()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let path = format!("/learning/objective/guardrails/{index}");
        let Some(guardrail) = guardrail else {
            error(
                issues,
                "null-guardrail",
                &path,
                "Guardrails must not be null.",
            );
            continue;
        };
        validate_member(&guardrail.name, &format!("{path}/name"), issues);
        if !names.insert(&guardrail.name) {
            error(
                issues,
                "duplicate-guardrail",
                &format!("{path}/name"),
                format!("Guardrail '{}' is declared more than once.", guardrail.name),
            );
        }
        if guardrail.expression.trim().is_empty() {
            error(
                issues,
                "invalid-guardrail-expression",
                &format!("{path}/expression"),
                "Guardrail expression must not be empty.",
            );
        } else if let Some(validator) = validator {
            let result = validator.validate_syntax(&guardrail.expression);
            if !result.is_valid {
                error(
                    issues,
                    "invalid-guardrail-expression",
                    &format!("{path}/expression"),
                    result
                        .error
                        .as_deref()
                        .unwrap_or("Guardrail expression is invalid."),
                );
            }
        }
    }
}

fn validate_source(
    source: &EvidenceSourceKind,
    correlations: &IndexSet<&str>,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    use EvidenceSourceKind::*;
    let (scope, name, mapping) = match source {
        Metric {
            scope,
            name,
            correlation,
            ..
        }
        | Log {
            scope,
            name,
            correlation,
        }
        | Span {
            scope,
            name,
            correlation,
        }
        | SpanEvent {
            scope,
            name,
            correlation,
            ..
        } => (scope, name, correlation),
    };
    validate_otel_name(
        scope,
        &format!("{path}/scope"),
        "instrumentation scope",
        issues,
    );
    validate_otel_name(name, &format!("{path}/name"), "signal", issues);
    match source {
        Metric {
            metric_kind, unit, ..
        } => {
            if !matches!(
                metric_kind.as_str(),
                "gauge" | "sum" | "histogram" | "exponentialHistogram" | "summary"
            ) {
                error(
                    issues,
                    "invalid-metric-kind",
                    &format!("{path}/metricKind"),
                    "Metric source kind must be gauge, sum, histogram, exponentialHistogram, or summary.",
                );
            }
            if unit.encode_utf16().count() > 128 {
                error(
                    issues,
                    "invalid-metric-unit",
                    &format!("{path}/unit"),
                    "Metric source unit must contain at most 128 characters.",
                );
            }
        }
        SpanEvent { span_name, .. } => validate_otel_name(
            span_name,
            &format!("{path}/spanName"),
            "parent span",
            issues,
        ),
        _ => {}
    }
    for correlation in correlations {
        if !mapping.contains_key(*correlation) {
            error(
                issues,
                "missing-evidence-correlation",
                &format!("{path}/correlation"),
                format!("Evidence source does not map correlation attribute '{correlation}'."),
            );
        }
    }
    for (correlation, attribute) in mapping {
        let path = format!("{path}/correlation/{}", escape_pointer(correlation));
        if !correlations.contains(correlation.as_str()) {
            error(
                issues,
                "unexpected-evidence-correlation",
                &path,
                format!("Evidence source maps undeclared correlation attribute '{correlation}'."),
            );
        }
        let Some(attribute) = attribute else {
            error(
                issues,
                "null-evidence-correlation",
                &path,
                "Evidence correlation mappings must not be null.",
            );
            continue;
        };
        if !matches!(
            attribute.location.as_str(),
            "resource" | "scope" | "signal" | "parentSpan"
        ) {
            error(
                issues,
                "invalid-evidence-correlation-location",
                &format!("{path}/location"),
                "Evidence correlation location must be resource, scope, signal, or parentSpan.",
            );
        } else if attribute.location == "parentSpan" && !matches!(source, SpanEvent { .. }) {
            error(
                issues,
                "invalid-evidence-correlation-location",
                &format!("{path}/location"),
                "parentSpan correlation is valid only for a spanEvent source.",
            );
        }
        validate_otel_name(
            &attribute.attribute,
            &format!("{path}/attribute"),
            "correlation attribute",
            issues,
        );
    }
}

fn source_identity(
    source: &EvidenceSourceKind,
) -> (&str, &str, Option<&str>, &str, Option<&str>, Option<&str>) {
    use EvidenceSourceKind::*;
    match source {
        Metric {
            scope,
            name,
            metric_kind,
            unit,
            ..
        } => ("metric", scope, None, name, Some(metric_kind), Some(unit)),
        Log { scope, name, .. } => ("log", scope, None, name, None, None),
        Span { scope, name, .. } => ("span", scope, None, name, None, None),
        SpanEvent {
            scope,
            name,
            span_name,
            ..
        } => ("spanEvent", scope, Some(span_name), name, None, None),
    }
}

fn validate_otel_name(value: &str, path: &str, subject: &str, issues: &mut Vec<ValidationIssue>) {
    if !(1..=256).contains(&value.encode_utf16().count()) {
        error(
            issues,
            "invalid-otel-name",
            path,
            format!("OpenTelemetry {subject} name must contain between 1 and 256 characters."),
        );
    }
}

fn validate_member(value: &str, path: &str, issues: &mut Vec<ValidationIssue>) {
    if !is_decision_name(value) {
        error(
            issues,
            "invalid-member-name",
            path,
            "Member name must match ^[A-Za-z][A-Za-z0-9._-]*$.",
        );
    }
}

fn validate_expression(
    expression: Option<&str>,
    attributes: &IndexMap<String, ValueSchema>,
    result_schema: &ValueSchema,
    purpose: ExpressionPurpose,
    path: &str,
    validator: Option<&dyn ContractExpressionValidator>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(expression) = expression.filter(|value| !value.trim().is_empty()) else {
        error(
            issues,
            "invalid-expression",
            path,
            "Expression must not be null or empty.",
        );
        return;
    };
    if expression.len() > 4096 {
        error(
            issues,
            "expression-too-large",
            path,
            "Expression exceeds the 4096-byte limit.",
        );
        return;
    }
    if let Some(validator) = validator {
        let result = validator.validate(expression, attributes, result_schema, purpose);
        if !result.is_valid {
            error(
                issues,
                "invalid-expression",
                path,
                result.error.as_deref().unwrap_or("Expression is invalid."),
            );
        }
    }
}

fn boolean_schema() -> ValueSchema {
    ValueSchema {
        kind: "boolean".into(),
        minimum: None,
        maximum: None,
        exclusive_minimum: None,
        exclusive_maximum: None,
        multiple_of: None,
        min_length: None,
        max_length: None,
        items: None,
        min_items: None,
        max_items: None,
        unique_items: None,
        properties: None,
        required: None,
        additional_properties: None,
        min_properties: None,
        max_properties: None,
        description: None,
    }
}

pub(crate) fn error(
    issues: &mut Vec<ValidationIssue>,
    code: &str,
    path: &str,
    message: impl Into<String>,
) {
    issues.push(ValidationIssue {
        code: code.into(),
        severity: "error".into(),
        path: path.into(),
        message: message.into(),
    });
}

pub(crate) fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
