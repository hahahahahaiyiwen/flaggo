mod checked;
mod executable;
mod lexer;
mod profile;
mod syntax;

use cel::common::ast::IdedExpr;
use cel::{Context, Env};
pub use checked::CheckedExpression;
pub use executable::{
    CheckedDecisionExecutable, CheckedExecutableRule, CompiledDecisionExecutable,
    CompiledExecutableRule, ExecutableCompilation, ExecutableCompilationError, ExecutableCompiler,
    ExecutableMaterializationError,
};
use flaggo_contract::digests::ExpressionCanonicalizer;
use flaggo_contract::validation::{
    ContractExpressionValidator, ExpressionPurpose, ExpressionValidation,
};
use flaggo_contract::{JsonValue, ValueSchema};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub const MAXIMUM_EXPRESSION_BYTES: usize = 4096;
pub const MAXIMUM_AST_DEPTH: usize = 32;
pub const MAXIMUM_AST_NODES: usize = 256;
pub const MAXIMUM_STATIC_COST: usize = 1000;
pub const MAXIMUM_RUNTIME_ITERATIONS: usize = 10_000;
pub const MAXIMUM_CHECKED_EXPRESSION_BYTES: usize = 65_536;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CompilationError(pub String);

impl From<String> for CompilationError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("expression produced a non-finite JSON result")]
    NonFiniteResult,
    #[error("{0}")]
    Failed(String),
}

pub struct ExpressionCompiler {
    environment: Arc<Env>,
}

impl Default for ExpressionCompiler {
    fn default() -> Self {
        Self {
            environment: Arc::new(Env::default().with_optional_support(false).with_stdlib()),
        }
    }
}

impl ExpressionCompiler {
    pub fn compile(
        &self,
        source: &str,
        attributes: &IndexMap<String, ValueSchema>,
        expected_result: &ValueSchema,
        purpose: ExpressionPurpose,
    ) -> Result<CompiledExpression, CompilationError> {
        let parsed = syntax::parse(source)?;
        let expression = &parsed.expression;
        profile::check(
            expression,
            attributes,
            expected_result,
            purpose,
            None,
            None,
            Some(&parsed),
        )?;
        let canonical = profile::canonical(expression, 0, false)?;
        let checked = checked::compile(expression, source, attributes, canonical)?;
        Ok(self.compiled(source.to_owned(), parsed.expression, purpose, checked))
    }

    pub fn materialize(
        &self,
        checked: &CheckedExpression,
        attributes: &IndexMap<String, ValueSchema>,
        expected_result: &ValueSchema,
        purpose: ExpressionPurpose,
    ) -> Result<CompiledExpression, CompilationError> {
        let (expression, types) = checked::decode(checked)?;
        profile::check(
            &expression,
            attributes,
            expected_result,
            purpose,
            Some(checked),
            Some(&types),
            None,
        )?;
        Ok(self.compiled(
            checked.canonical_expression.clone(),
            expression,
            purpose,
            checked.clone(),
        ))
    }

    fn compiled(
        &self,
        source: String,
        expression: IdedExpr,
        purpose: ExpressionPurpose,
        checked: CheckedExpression,
    ) -> CompiledExpression {
        CompiledExpression {
            source,
            purpose,
            checked,
            runtime: profile::runtime_expression(&expression),
            environment: Arc::clone(&self.environment),
        }
    }
}

impl ExpressionCanonicalizer for ExpressionCompiler {
    fn canonicalize(&self, source: &str) -> Result<String, String> {
        profile::canonical(&syntax::parse(source)?.expression, 0, false)
    }
}

impl ContractExpressionValidator for ExpressionCompiler {
    fn validate(
        &self,
        expression: &str,
        attributes: &IndexMap<String, ValueSchema>,
        expected_result: &ValueSchema,
        purpose: ExpressionPurpose,
    ) -> ExpressionValidation {
        match self.compile(expression, attributes, expected_result, purpose) {
            Ok(compiled) => ExpressionValidation {
                is_valid: true,
                canonical_expression: Some(compiled.checked.canonical_expression),
                referenced_attributes: compiled.checked.referenced_attributes.into_iter().collect(),
                error: None,
            },
            Err(error) => invalid(error.to_string()),
        }
    }

    fn validate_syntax(&self, expression: &str) -> ExpressionValidation {
        match self.canonicalize(expression) {
            Ok(canonical) => ExpressionValidation {
                is_valid: true,
                canonical_expression: Some(canonical),
                referenced_attributes: Default::default(),
                error: None,
            },
            Err(error) => invalid(error),
        }
    }
}

fn invalid(error: String) -> ExpressionValidation {
    ExpressionValidation {
        is_valid: false,
        canonical_expression: None,
        referenced_attributes: Default::default(),
        error: Some(error),
    }
}

pub struct CompiledExpression {
    source: String,
    purpose: ExpressionPurpose,
    checked: CheckedExpression,
    runtime: IdedExpr,
    environment: Arc<Env>,
}

impl CompiledExpression {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn purpose(&self) -> ExpressionPurpose {
        self.purpose
    }

    pub fn checked(&self) -> &CheckedExpression {
        &self.checked
    }

    pub fn evaluate(
        &self,
        attributes: &IndexMap<String, JsonValue>,
        _cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<JsonValue, EvaluationError> {
        // Celly checks cancellation only in comprehension iterations, which this profile forbids.
        let mut context = Context::with_env(Arc::clone(&self.environment));
        let values = attributes
            .iter()
            .map(|(name, value)| Ok((name.clone(), cel_input(value)?)))
            .collect::<Result<HashMap<_, _>, EvaluationError>>()?;
        context.add_variable_from_value("attributes", cel::Value::Map(values.into()));
        context
            .add_function("__flaggo_string", profile::string_value)
            .map_err(|error| EvaluationError::Failed(error.to_string()))?;
        let value = cel::Value::resolve(&self.runtime, &context);
        let value = value.map_err(|error| EvaluationError::Failed(error.to_string()))?;
        if matches!(&value, cel::Value::Float(number) if !number.is_finite()) {
            return Err(EvaluationError::NonFiniteResult);
        }
        value
            .json()
            .map(Into::into)
            .map_err(|error| EvaluationError::Failed(error.to_string()))
    }
}

fn cel_input(value: &JsonValue) -> Result<cel::Value, EvaluationError> {
    Ok(match value {
        JsonValue::Null => cel::Value::Null,
        JsonValue::Bool(value) => cel::Value::Bool(*value),
        JsonValue::String(value) => cel::Value::String(value.clone().into()),
        JsonValue::Number(value) => {
            if let Some(integer) = value.as_i64() {
                cel::Value::Int(integer)
            } else {
                cel::Value::Float(
                    value
                        .as_f64()
                        .filter(|number| number.is_finite())
                        .ok_or_else(|| {
                            EvaluationError::Failed(
                                "input number is outside the Flaggo JSON profile".to_owned(),
                            )
                        })?,
                )
            }
        }
        JsonValue::Array(values) => cel::Value::List(
            values
                .iter()
                .map(cel_input)
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        ),
        JsonValue::Object(values) => cel::Value::Map(
            values
                .iter()
                .map(|(name, value)| Ok((name.clone(), cel_input(value)?)))
                .collect::<Result<HashMap<_, _>, EvaluationError>>()?
                .into(),
        ),
    })
}
