use crate::JsonValue as Value;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

fn present_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

fn non_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom(
            "expression/condition must not be empty",
        ));
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityScope {
    // .NET defaults missing constructor parameters to null but rejects explicit nulls.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub tenant: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub application: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub environment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionContract {
    pub authority: AuthorityScope,
    pub name: String,
    #[serde(rename = "expression_syntax")]
    pub expression_syntax: String,
    pub attributes: Vec<Option<ContractAttribute>>,
    pub result: ContractResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authored_executable: Option<AuthoredExecutable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning: Option<LearningDefinition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractAttribute {
    pub name: String,
    pub schema: ValueSchema,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractResult {
    pub schema: ValueSchema,
    pub default: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueSchema {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_maximum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple_of: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<ValueSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_items: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_items: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, Option<ValueSchema>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_properties: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_properties: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredExecutable {
    pub rules: Vec<Option<AuthoredRule>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredRule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub when: RuleWhen,
    #[serde(rename = "return")]
    pub result: RuleReturn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuleWhen {
    #[serde(rename = "condition")]
    Condition(#[serde(deserialize_with = "non_empty")] String),
    #[serde(rename = "expression")]
    Expression(#[serde(deserialize_with = "non_empty")] String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuleReturn {
    #[serde(rename = "value")]
    Value(Value),
    #[serde(rename = "expression")]
    Expression(#[serde(deserialize_with = "non_empty")] String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningDefinition {
    pub policy: LearningPolicy,
    pub evidence: Vec<Option<EvidenceDefinition>>,
    pub objective: LearningObjective,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningPolicy {
    pub mode: String,
    pub evaluate: LearningEvaluationPolicy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningEvaluationPolicy {
    pub interval: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub attribute: String,
    pub correlate_by: Vec<Option<String>>,
    pub source: EvidenceSource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EvidenceSource(pub EvidenceSourceKind);

impl<'de> Deserialize<'de> for EvidenceSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SourceVisitor;
        impl<'de> serde::de::Visitor<'de> for SourceVisitor {
            type Value = EvidenceSource;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an evidence source with kind as its first member")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                // Preserve the current .NET polymorphic discriminator ordering requirement.
                if map.next_key::<String>()?.as_deref() != Some("kind") {
                    return Err(serde::de::Error::custom(
                        "evidence source requires kind first",
                    ));
                }
                EvidenceSourceKind::deserialize(serde::de::value::MapAccessDeserializer::new(
                    WithKind { first: true, map },
                ))
                .map(EvidenceSource)
            }
        }
        deserializer.deserialize_map(SourceVisitor)
    }
}

// Replay only the discriminator key, preserving the original order of nested maps.
struct WithKind<A> {
    first: bool,
    map: A,
}

impl<'de, A: serde::de::MapAccess<'de>> serde::de::MapAccess<'de> for WithKind<A> {
    type Error = A::Error;

    fn next_key_seed<K: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        if self.first {
            self.first = false;
            seed.deserialize(serde::de::value::StrDeserializer::new("kind"))
                .map(Some)
        } else {
            self.map.next_key_seed(seed)
        }
    }

    fn next_value_seed<V: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        self.map.next_value_seed(seed)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum EvidenceSourceKind {
    #[serde(rename = "metric", rename_all = "camelCase")]
    Metric {
        scope: String,
        name: String,
        correlation: IndexMap<String, Option<EvidenceCorrelationAttribute>>,
        metric_kind: String,
        unit: String,
    },
    #[serde(rename = "log")]
    Log {
        scope: String,
        name: String,
        correlation: IndexMap<String, Option<EvidenceCorrelationAttribute>>,
    },
    #[serde(rename = "span")]
    Span {
        scope: String,
        name: String,
        correlation: IndexMap<String, Option<EvidenceCorrelationAttribute>>,
    },
    #[serde(rename = "spanEvent", rename_all = "camelCase")]
    SpanEvent {
        scope: String,
        name: String,
        correlation: IndexMap<String, Option<EvidenceCorrelationAttribute>>,
        span_name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCorrelationAttribute {
    pub location: String,
    pub attribute: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningObjective {
    pub primary: PrimaryObjective,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guardrails: Option<Vec<Option<Guardrail>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrimaryObjective {
    pub evidence: String,
    pub direction: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Guardrail {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub expression: String,
}

fn rules_kind() -> String {
    "rules".to_owned()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionExecutable {
    pub contract_digest: String,
    #[serde(default = "rules_kind")]
    pub kind: String,
    pub rules: Vec<Option<ExecutableRule>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutableRule {
    pub name: String,
    pub when: ExpressionWhen,
    #[serde(rename = "return")]
    pub result: RuleReturn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpressionWhen {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub expression: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeInput {
    pub attributes: IndexMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_exposure: Option<CurrentExposure>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrentExposure {
    pub exposure_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationIssue {
    pub code: String,
    pub severity: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionContractValidationResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_digest: Option<String>,
    pub issues: Vec<ValidationIssue>,
}
