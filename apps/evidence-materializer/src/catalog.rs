use std::collections::{HashMap, HashSet};

use flaggo_evidence_store::{AuthorityScope, EvidenceSignal, SourceKey};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::route::{CurrentContractKey, MaterializationRoute};

#[derive(Clone, Debug)]
pub struct CompiledContractCatalog {
    payload: Vec<u8>,
    current_contracts: HashMap<CurrentContractKey, String>,
    active_source_counts: HashMap<MaterializationRoute, u32>,
}

impl CompiledContractCatalog {
    pub fn compile(payload: Vec<u8>) -> Result<Self, ContractCatalogError> {
        let document: ContractCatalogDocument =
            serde_json::from_slice(&payload).map_err(ContractCatalogError::InvalidJson)?;
        let mut current_contracts = HashMap::new();
        let mut contract_digests = HashSet::new();
        let mut active_source_counts = HashMap::new();
        let mut previous_order_key: Option<(String, String, String, String, String)> = None;

        for entry in document.contracts {
            validate_digest(&entry.contract_digest)?;
            if !contract_digests.insert(entry.contract_digest.clone()) {
                return Err(ContractCatalogError::DuplicateDigest(entry.contract_digest));
            }
            validate_identifier("contract name", &entry.contract.name)?;
            let authority = AuthorityScope::new(
                entry.contract.authority.tenant,
                entry.contract.authority.application,
                entry.contract.authority.environment,
            )
            .map_err(|error| ContractCatalogError::InvalidAuthority(error.to_string()))?;
            let order_key = (
                authority.tenant.clone(),
                authority.application.clone(),
                authority.environment.clone(),
                entry.contract.name.clone(),
                entry.contract_digest.clone(),
            );
            if previous_order_key
                .as_ref()
                .is_some_and(|previous| previous >= &order_key)
            {
                return Err(ContractCatalogError::InvalidOrder);
            }
            previous_order_key = Some(order_key);

            let key = CurrentContractKey {
                authority: authority.clone(),
                contract_name: entry.contract.name.clone(),
            };
            if current_contracts
                .insert(key.clone(), entry.contract_digest)
                .is_some()
            {
                return Err(ContractCatalogError::DuplicateContract {
                    tenant: key.authority.tenant,
                    application: key.authority.application,
                    environment: key.authority.environment,
                    contract_name: key.contract_name,
                });
            }

            let mut contract_routes = HashSet::new();
            if let Some(learning) = entry.contract.learning {
                for evidence in learning.evidence {
                    contract_routes.insert(MaterializationRoute {
                        authority: authority.clone(),
                        source: evidence.source.into_source_key()?,
                    });
                }
            }
            for route in contract_routes {
                let count = active_source_counts.entry(route).or_insert(0_u32);
                *count = count
                    .checked_add(1)
                    .ok_or(ContractCatalogError::ReferenceCountOverflow)?;
            }
        }

        Ok(Self {
            payload,
            current_contracts,
            active_source_counts,
        })
    }

    #[must_use]
    pub fn empty() -> Self {
        let payload = serde_json::to_vec(&json!({ "contracts": [] }))
            .expect("serializing an empty contract catalog must succeed");
        Self {
            payload,
            current_contracts: HashMap::new(),
            active_source_counts: HashMap::new(),
        }
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub fn current_contracts(&self) -> &HashMap<CurrentContractKey, String> {
        &self.current_contracts
    }

    #[must_use]
    pub fn active_source_counts(&self) -> &HashMap<MaterializationRoute, u32> {
        &self.active_source_counts
    }

    #[must_use]
    pub fn contains_route(&self, route: &MaterializationRoute) -> bool {
        self.active_source_counts.contains_key(route)
    }
}

#[derive(Debug, Error)]
pub enum ContractCatalogError {
    #[error("contract catalog is not valid JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("contract catalog digest '{0}' is invalid")]
    InvalidDigest(String),
    #[error("contract catalog contains duplicate digest '{0}'")]
    DuplicateDigest(String),
    #[error(
        "contract catalog contains duplicate current contract '{tenant}/{application}/{environment}/{contract_name}'"
    )]
    DuplicateContract {
        tenant: String,
        application: String,
        environment: String,
        contract_name: String,
    },
    #[error("contract catalog entries are not in canonical authority/name/digest order")]
    InvalidOrder,
    #[error("contract catalog authority is invalid: {0}")]
    InvalidAuthority(String),
    #[error("contract catalog {0} is invalid")]
    InvalidIdentifier(&'static str),
    #[error("contract catalog evidence source is invalid: {0}")]
    InvalidSource(String),
    #[error("contract catalog route reference count exceeds u32::MAX")]
    ReferenceCountOverflow,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractCatalogDocument {
    contracts: Vec<ContractCatalogEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractCatalogEntry {
    contract_digest: String,
    contract: ContractDocument,
}

#[derive(Deserialize)]
struct ContractDocument {
    authority: AuthorityDocument,
    name: String,
    learning: Option<LearningDocument>,
}

#[derive(Deserialize)]
struct AuthorityDocument {
    tenant: String,
    application: String,
    environment: String,
}

#[derive(Deserialize)]
struct LearningDocument {
    #[serde(default)]
    evidence: Vec<EvidenceDocument>,
}

#[derive(Deserialize)]
struct EvidenceDocument {
    source: EvidenceSource,
}

#[derive(Deserialize)]
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
        #[serde(rename = "correlation")]
        _correlation: Value,
    },
    Log {
        scope: String,
        name: String,
        #[serde(rename = "correlation")]
        _correlation: Value,
    },
    Span {
        scope: String,
        name: String,
        #[serde(rename = "correlation")]
        _correlation: Value,
    },
    SpanEvent {
        scope: String,
        span_name: String,
        name: String,
        #[serde(rename = "correlation")]
        _correlation: Value,
    },
}

impl EvidenceSource {
    fn into_source_key(self) -> Result<SourceKey, ContractCatalogError> {
        let source = match self {
            Self::Metric {
                scope,
                name,
                metric_kind,
                unit,
                _correlation,
            } => {
                validate_source_text("scope", &scope, 256, false)?;
                validate_source_text("name", &name, 256, false)?;
                validate_source_text("unit", &unit, 128, true)?;
                if !matches!(
                    metric_kind.as_str(),
                    "gauge" | "sum" | "histogram" | "exponentialHistogram" | "summary"
                ) {
                    return Err(ContractCatalogError::InvalidSource(format!(
                        "unknown metric kind '{metric_kind}'"
                    )));
                }
                validate_correlation(&_correlation)?;
                SourceKey {
                    signal: EvidenceSignal::Metric,
                    instrumentation_scope: scope,
                    signal_name: name,
                    metric_kind: Some(metric_kind),
                    metric_unit: Some(unit),
                    parent_span_name: None,
                }
            }
            Self::Log {
                scope,
                name,
                _correlation,
            } => {
                validate_source_text("scope", &scope, 256, false)?;
                validate_source_text("name", &name, 256, false)?;
                validate_correlation(&_correlation)?;
                SourceKey {
                    signal: EvidenceSignal::Log,
                    instrumentation_scope: scope,
                    signal_name: name,
                    metric_kind: None,
                    metric_unit: None,
                    parent_span_name: None,
                }
            }
            Self::Span {
                scope,
                name,
                _correlation,
            } => {
                validate_source_text("scope", &scope, 256, false)?;
                validate_source_text("name", &name, 256, false)?;
                validate_correlation(&_correlation)?;
                SourceKey {
                    signal: EvidenceSignal::Span,
                    instrumentation_scope: scope,
                    signal_name: name,
                    metric_kind: None,
                    metric_unit: None,
                    parent_span_name: None,
                }
            }
            Self::SpanEvent {
                scope,
                span_name,
                name,
                _correlation,
            } => {
                validate_source_text("scope", &scope, 256, false)?;
                validate_source_text("name", &name, 256, false)?;
                validate_source_text("spanName", &span_name, 256, false)?;
                validate_correlation(&_correlation)?;
                SourceKey {
                    signal: EvidenceSignal::SpanEvent,
                    instrumentation_scope: scope,
                    signal_name: name,
                    metric_kind: None,
                    metric_unit: None,
                    parent_span_name: Some(span_name),
                }
            }
        };
        Ok(source)
    }
}

fn validate_digest(value: &str) -> Result<(), ContractCatalogError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ContractCatalogError::InvalidDigest(value.to_owned()));
    }
    Ok(())
}

fn validate_identifier(name: &'static str, value: &str) -> Result<(), ContractCatalogError> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphabetic()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ContractCatalogError::InvalidIdentifier(name));
    }
    Ok(())
}

fn validate_source_text(
    name: &str,
    value: &str,
    maximum_length: usize,
    allow_empty: bool,
) -> Result<(), ContractCatalogError> {
    let length = value.chars().count();
    if (!allow_empty && length == 0) || length > maximum_length {
        return Err(ContractCatalogError::InvalidSource(format!(
            "{name} must contain {}-{maximum_length} characters",
            usize::from(!allow_empty)
        )));
    }
    Ok(())
}

fn validate_correlation(value: &Value) -> Result<(), ContractCatalogError> {
    if !value.is_object() {
        return Err(ContractCatalogError::InvalidSource(
            "correlation must be an object".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CompiledContractCatalog, ContractCatalogError};

    #[test]
    fn compiles_authority_routes_and_reference_counts() {
        let catalog = CompiledContractCatalog::compile(catalog_payload(&[
            ("first", "1", "shared"),
            ("second", "2", "shared"),
            ("third", "3", "other"),
        ]))
        .expect("valid catalog");

        assert_eq!(catalog.current_contracts().len(), 3);
        let mut counts = catalog
            .active_source_counts()
            .values()
            .copied()
            .collect::<Vec<_>>();
        counts.sort_unstable();
        assert_eq!(counts, vec![1, 2]);
    }

    #[test]
    fn rejects_noncanonical_catalog_order() {
        let payload = catalog_payload(&[("second", "2", "second"), ("first", "1", "first")]);
        assert!(matches!(
            CompiledContractCatalog::compile(payload),
            Err(ContractCatalogError::InvalidOrder)
        ));
    }

    fn catalog_payload(contracts: &[(&str, &str, &str)]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "contracts": contracts.iter().map(|(name, digest, signal)| json!({
                "contractDigest": format!("sha256:{}", digest.repeat(64)),
                "contract": {
                    "authority": {
                        "tenant": "local",
                        "application": "demo",
                        "environment": "test"
                    },
                    "name": name,
                    "learning": {
                        "evidence": [{
                            "source": {
                                "kind": "log",
                                "scope": "demo",
                                "name": signal,
                                "correlation": {}
                            }
                        }]
                    }
                }
            })).collect::<Vec<_>>()
        }))
        .expect("catalog JSON")
    }
}
