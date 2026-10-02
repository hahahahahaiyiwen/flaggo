use std::collections::{BTreeMap, BTreeSet, HashSet};

use flaggo_evidence_store::EvidenceSignal;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::candidate::{AttributeLocation, Candidate, any_value_json, lookup_attribute};

#[derive(Clone, Debug)]
pub struct CompiledSelectorSnapshot {
    pub snapshot_digest: String,
    pub payload: Vec<u8>,
    selectors: Vec<CompiledSelector>,
}

impl CompiledSelectorSnapshot {
    pub fn compile(payload: Vec<u8>) -> Result<Self, SelectorSnapshotError> {
        let document: SelectorSnapshotDocument =
            serde_json::from_slice(&payload).map_err(SelectorSnapshotError::InvalidJson)?;
        validate_digest(&document.snapshot_digest)?;

        let mut contracts = document.contracts;
        contracts.sort_by(|left, right| {
            (&left.name, &left.contract_digest).cmp(&(&right.name, &right.contract_digest))
        });
        let computed_digest = compute_snapshot_digest(
            contracts
                .iter()
                .map(|contract| (&contract.name, &contract.contract_digest)),
        );
        if document.snapshot_digest != computed_digest {
            return Err(SelectorSnapshotError::DigestMismatch {
                declared: document.snapshot_digest,
                computed: computed_digest,
            });
        }

        let mut contract_names = HashSet::new();
        let mut selectors = Vec::new();
        for contract in contracts {
            validate_digest(&contract.contract_digest)?;
            if !contract_names.insert(contract.name.clone()) {
                return Err(SelectorSnapshotError::DuplicateContract(
                    contract.name.clone(),
                ));
            }
            if contract.name != contract.contract.name {
                return Err(SelectorSnapshotError::ContractNameMismatch {
                    wrapper: contract.name,
                    contract: contract.contract.name,
                });
            }

            let Some(learning) = contract.contract.learning else {
                continue;
            };
            let mut evidence_names = HashSet::new();
            for evidence in learning.evidence {
                if evidence.name.is_empty() || evidence.attribute.is_empty() {
                    return Err(SelectorSnapshotError::InvalidEvidence {
                        contract: contract.name.clone(),
                        evidence: evidence.name,
                        reason: "evidence name and attribute must not be empty".to_owned(),
                    });
                }
                if !evidence_names.insert(evidence.name.clone()) {
                    return Err(SelectorSnapshotError::InvalidEvidence {
                        contract: contract.name.clone(),
                        evidence: evidence.name,
                        reason: "evidence names must be unique within a contract".to_owned(),
                    });
                }
                validate_correlation(&contract.name, &evidence)?;
                let source_json = serde_json::to_vec(&evidence.source)
                    .expect("serializing a parsed evidence source must succeed");
                selectors.push(CompiledSelector {
                    contract_name: contract.name.clone(),
                    contract_digest: contract.contract_digest.clone(),
                    evidence_name: evidence.name,
                    contract_attribute: evidence.attribute,
                    source: evidence.source,
                    source_json,
                });
            }
        }

        Ok(Self {
            snapshot_digest: document.snapshot_digest,
            payload,
            selectors,
        })
    }

    pub fn built_in_only() -> Self {
        let snapshot_digest = compute_snapshot_digest(std::iter::empty::<(&String, &String)>());
        let payload = serde_json::to_vec(&json!({
            "contracts": [],
            "snapshotDigest": snapshot_digest
        }))
        .expect("serializing the built-in selector snapshot must succeed");
        Self {
            snapshot_digest,
            payload,
            selectors: Vec::new(),
        }
    }

    pub(crate) fn evaluate(&self, candidate: &Candidate) -> SelectorEvaluation {
        let mut evaluation = SelectorEvaluation::default();
        for selector in self
            .selectors
            .iter()
            .filter(|selector| selector.source.matches(candidate))
        {
            match selector.correlation(candidate) {
                Ok(correlation_json) => {
                    evaluation.associations.push(SelectorAssociation {
                        contract_digest: selector.contract_digest.clone(),
                        evidence_name: selector.evidence_name.clone(),
                        contract_attribute: selector.contract_attribute.clone(),
                        correlation_json,
                        source_json: selector.source_json.clone(),
                    });
                }
                Err(error) => {
                    evaluation.diagnostics.push(SelectorDiagnostic {
                        code: "selector.correlation_unresolved".to_owned(),
                        message: format!(
                            "Evidence '{}.{}' matched the signal but its correlation could not be resolved.",
                            selector.contract_name, selector.evidence_name
                        ),
                        detail: json!({
                            "attribute": error.attribute,
                            "contractDigest": selector.contract_digest,
                            "contractName": selector.contract_name,
                            "evidenceName": selector.evidence_name,
                            "reason": error.reason
                        }),
                    });
                }
            }
        }
        evaluation
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SelectorEvaluation {
    pub associations: Vec<SelectorAssociation>,
    pub diagnostics: Vec<SelectorDiagnostic>,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectorAssociation {
    pub contract_digest: String,
    pub evidence_name: String,
    pub contract_attribute: String,
    pub correlation_json: Vec<u8>,
    pub source_json: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectorDiagnostic {
    pub code: String,
    pub message: String,
    pub detail: Value,
}

#[derive(Debug, Error)]
pub enum SelectorSnapshotError {
    #[error("selector snapshot is not valid JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("selector snapshot digest '{0}' is invalid")]
    InvalidDigest(String),
    #[error("selector snapshot digest mismatch: declared {declared}, computed {computed}")]
    DigestMismatch { declared: String, computed: String },
    #[error("selector snapshot contains duplicate current contract '{0}'")]
    DuplicateContract(String),
    #[error(
        "selector snapshot wrapper contract name '{wrapper}' does not match contract name '{contract}'"
    )]
    ContractNameMismatch { wrapper: String, contract: String },
    #[error("invalid evidence '{contract}.{evidence}': {reason}")]
    InvalidEvidence {
        contract: String,
        evidence: String,
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectorSnapshotDocument {
    snapshot_digest: String,
    contracts: Vec<SnapshotContractDocument>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotContractDocument {
    name: String,
    contract_digest: String,
    contract: SelectorContractDocument,
}

#[derive(Deserialize)]
struct SelectorContractDocument {
    name: String,
    learning: Option<LearningDocument>,
}

#[derive(Deserialize)]
struct LearningDocument {
    #[serde(default)]
    evidence: Vec<EvidenceDocument>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceDocument {
    name: String,
    attribute: String,
    correlate_by: Vec<String>,
    source: EvidenceSource,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum EvidenceSource {
    Metric {
        scope: String,
        name: String,
        metric_kind: String,
        unit: String,
        correlation: BTreeMap<String, CorrelationAttribute>,
    },
    Log {
        scope: String,
        name: String,
        correlation: BTreeMap<String, CorrelationAttribute>,
    },
    Span {
        scope: String,
        name: String,
        correlation: BTreeMap<String, CorrelationAttribute>,
    },
    SpanEvent {
        scope: String,
        span_name: String,
        name: String,
        correlation: BTreeMap<String, CorrelationAttribute>,
    },
}

impl EvidenceSource {
    fn correlation(&self) -> &BTreeMap<String, CorrelationAttribute> {
        match self {
            Self::Metric { correlation, .. }
            | Self::Log { correlation, .. }
            | Self::Span { correlation, .. }
            | Self::SpanEvent { correlation, .. } => correlation,
        }
    }

    fn matches(&self, candidate: &Candidate) -> bool {
        match self {
            Self::Metric {
                scope,
                name,
                metric_kind,
                unit,
                ..
            } => {
                candidate.signal == EvidenceSignal::Metric
                    && candidate.instrumentation_scope == *scope
                    && candidate.signal_name == *name
                    && candidate.metric_kind.as_deref() == Some(metric_kind.as_str())
                    && candidate.metric_unit.as_deref() == Some(unit.as_str())
            }
            Self::Log { scope, name, .. } => {
                candidate.signal == EvidenceSignal::Log
                    && candidate.instrumentation_scope == *scope
                    && candidate.signal_name == *name
            }
            Self::Span { scope, name, .. } => {
                candidate.signal == EvidenceSignal::Span
                    && candidate.instrumentation_scope == *scope
                    && candidate.signal_name == *name
            }
            Self::SpanEvent {
                scope,
                span_name,
                name,
                ..
            } => {
                candidate.signal == EvidenceSignal::SpanEvent
                    && candidate.instrumentation_scope == *scope
                    && candidate.parent_span_name.as_deref() == Some(span_name.as_str())
                    && candidate.signal_name == *name
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CorrelationAttribute {
    location: CorrelationLocation,
    attribute: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum CorrelationLocation {
    Resource,
    Scope,
    Signal,
    ParentSpan,
}

impl From<CorrelationLocation> for AttributeLocation {
    fn from(value: CorrelationLocation) -> Self {
        match value {
            CorrelationLocation::Resource => Self::Resource,
            CorrelationLocation::Scope => Self::Scope,
            CorrelationLocation::Signal => Self::Signal,
            CorrelationLocation::ParentSpan => Self::ParentSpan,
        }
    }
}

#[derive(Clone, Debug)]
struct CompiledSelector {
    contract_name: String,
    contract_digest: String,
    evidence_name: String,
    contract_attribute: String,
    source: EvidenceSource,
    source_json: Vec<u8>,
}

impl CompiledSelector {
    fn correlation(&self, candidate: &Candidate) -> Result<Vec<u8>, CorrelationFailure> {
        let mut values = Map::new();
        for (contract_attribute, mapping) in self.source.correlation() {
            let location: AttributeLocation = mapping.location.into();
            let attributes = match location {
                AttributeLocation::Resource => candidate.resource_attributes(),
                AttributeLocation::Scope => candidate.scope_attributes(),
                AttributeLocation::Signal => &candidate.signal_attributes,
                AttributeLocation::ParentSpan => &candidate.parent_span_attributes,
            };
            let value = lookup_attribute(attributes, &mapping.attribute).map_err(|error| {
                CorrelationFailure {
                    attribute: contract_attribute.clone(),
                    reason: error.to_string(),
                }
            })?;
            values.insert(contract_attribute.clone(), any_value_json(value));
        }
        serde_json::to_vec(&Value::Object(values)).map_err(|error| CorrelationFailure {
            attribute: String::new(),
            reason: error.to_string(),
        })
    }
}

struct CorrelationFailure {
    attribute: String,
    reason: String,
}

fn validate_correlation(
    contract_name: &str,
    evidence: &EvidenceDocument,
) -> Result<(), SelectorSnapshotError> {
    let correlated: BTreeSet<_> = evidence.correlate_by.iter().cloned().collect();
    if correlated.len() != evidence.correlate_by.len() {
        return Err(SelectorSnapshotError::InvalidEvidence {
            contract: contract_name.to_owned(),
            evidence: evidence.name.clone(),
            reason: "correlateBy must not contain duplicate attributes".to_owned(),
        });
    }
    let mapped: BTreeSet<_> = evidence.source.correlation().keys().cloned().collect();
    if correlated != mapped {
        return Err(SelectorSnapshotError::InvalidEvidence {
            contract: contract_name.to_owned(),
            evidence: evidence.name.clone(),
            reason: "source.correlation keys must exactly equal correlateBy".to_owned(),
        });
    }
    for (attribute, mapping) in evidence.source.correlation() {
        if mapping.attribute.is_empty() {
            return Err(SelectorSnapshotError::InvalidEvidence {
                contract: contract_name.to_owned(),
                evidence: evidence.name.clone(),
                reason: format!("correlation attribute '{attribute}' has an empty OTLP key"),
            });
        }
        if matches!(mapping.location, CorrelationLocation::ParentSpan)
            && !matches!(evidence.source, EvidenceSource::SpanEvent { .. })
        {
            return Err(SelectorSnapshotError::InvalidEvidence {
                contract: contract_name.to_owned(),
                evidence: evidence.name.clone(),
                reason: "parentSpan correlation is valid only for spanEvent sources".to_owned(),
            });
        }
    }
    Ok(())
}

pub(crate) fn compute_snapshot_digest<'a>(
    contracts: impl Iterator<Item = (&'a String, &'a String)>,
) -> String {
    let mut hasher = Sha256::new();
    update_length_prefixed(&mut hasher, b"flaggo-selector-snapshot-v1");
    for (name, digest) in contracts {
        update_length_prefixed(&mut hasher, name.as_bytes());
        update_length_prefixed(&mut hasher, digest.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(
        u64::try_from(value.len())
            .expect("selector snapshot values must fit in u64")
            .to_be_bytes(),
    );
    hasher.update(value);
}

fn validate_digest(value: &str) -> Result<(), SelectorSnapshotError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(SelectorSnapshotError::InvalidDigest(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CompiledSelectorSnapshot, SelectorSnapshotError, compute_snapshot_digest};

    #[test]
    fn rejects_correlation_keys_that_do_not_exactly_match_correlate_by() {
        let name = "demo.contract".to_owned();
        let digest = format!("sha256:{}", "1".repeat(64));
        let snapshot_digest = compute_snapshot_digest(std::iter::once((&name, &digest)));
        let payload = serde_json::to_vec(&json!({
            "snapshotDigest": snapshot_digest,
            "contracts": [{
                "name": name,
                "contractDigest": digest,
                "contract": {
                    "name": "demo.contract",
                    "learning": {
                        "evidence": [{
                            "name": "latency",
                            "attribute": "latencyMs",
                            "correlateBy": ["workerId"],
                            "source": {
                                "kind": "log",
                                "scope": "demo",
                                "name": "demo.latency",
                                "correlation": {}
                            }
                        }]
                    }
                }
            }]
        }))
        .expect("test snapshot must serialize");

        assert!(matches!(
            CompiledSelectorSnapshot::compile(payload),
            Err(SelectorSnapshotError::InvalidEvidence { .. })
        ));
    }

    #[test]
    fn built_in_snapshot_is_self_consistent() {
        let snapshot = CompiledSelectorSnapshot::built_in_only();
        let compiled = CompiledSelectorSnapshot::compile(snapshot.payload.clone())
            .expect("built-in snapshot must compile");
        assert_eq!(compiled.snapshot_digest, snapshot.snapshot_digest);
    }
}
