use std::collections::HashSet;

use flaggo_evidence_store::{
    AuthorityScope, EvidenceDiagnostic, EvidenceMaterializationCommit, EvidenceMaterializationMode,
    EvidenceObservation, EvidenceObservationWrite, ForwardMaterializationKey, MaterializerVersions,
};
use flaggo_raw_otlp_inbox::RawOtlpInboxBatch;
use opentelemetry_proto::tonic::common::v1::{KeyValue, any_value};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    CompiledContractCatalog, MaterializationRoute,
    builtin::{BuiltInEvaluation, evaluate as evaluate_builtin},
    candidate::{AttributeLookupError, Candidate, lookup_attribute},
    decode_batch,
};

pub(crate) enum ProjectionRoutes<'a> {
    Forward(&'a CompiledContractCatalog),
    Replay(&'a HashSet<MaterializationRoute>),
}

impl ProjectionRoutes<'_> {
    fn contains(&self, route: &MaterializationRoute) -> bool {
        match self {
            Self::Forward(catalog) => catalog.contains_route(route),
            Self::Replay(routes) => routes.contains(route),
        }
    }

    const fn includes_built_in_protocol(&self) -> bool {
        matches!(self, Self::Forward(_))
    }

    const fn mode(&self) -> EvidenceMaterializationMode {
        match self {
            Self::Forward(_) => EvidenceMaterializationMode::Forward,
            Self::Replay(_) => EvidenceMaterializationMode::Replay,
        }
    }
}

pub(crate) fn project_batch(
    batch: &RawOtlpInboxBatch,
    routes: ProjectionRoutes<'_>,
    versions: &MaterializerVersions,
) -> EvidenceMaterializationCommit {
    let decoded = decode_batch(batch);
    let mut observations = Vec::new();
    let mut diagnostics = decoded
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            create_diagnostic(
                batch.inbox_batch_id.get(),
                diagnostic.candidate_ordinal,
                &diagnostic.code,
                diagnostic.message,
                Some(diagnostic.detail),
            )
        })
        .collect::<Vec<_>>();

    for candidate in decoded.candidates {
        let authority = match derive_authority(&candidate) {
            Ok(authority) => authority,
            Err(error) => {
                diagnostics.push(create_diagnostic(
                    batch.inbox_batch_id.get(),
                    Some(candidate.ordinal),
                    "observation.authority_unresolved",
                    "Authority scope could not be derived from Flaggo OTLP Resource attributes."
                        .to_owned(),
                    Some(json!({ "reason": error })),
                ));
                continue;
            }
        };

        match evaluate_builtin(&candidate) {
            BuiltInEvaluation::Valid(decision) => {
                if routes.includes_built_in_protocol() {
                    observations.push(create_observation(
                        candidate,
                        authority,
                        Some("decision.received".to_owned()),
                        Some(decision.decision_id),
                        Some(decision.contract_digest),
                        versions,
                    ));
                }
                continue;
            }
            BuiltInEvaluation::Invalid(diagnostic) => {
                if routes.includes_built_in_protocol() {
                    diagnostics.push(create_diagnostic(
                        batch.inbox_batch_id.get(),
                        Some(candidate.ordinal),
                        &diagnostic.code,
                        diagnostic.message,
                        Some(diagnostic.detail),
                    ));
                }
                continue;
            }
            BuiltInEvaluation::NotBuiltIn => {}
        }

        let route = MaterializationRoute {
            authority: authority.clone(),
            source: candidate.source_key(),
        };
        if !routes.contains(&route) {
            continue;
        }
        observations.push(create_observation(
            candidate, authority, None, None, None, versions,
        ));
    }

    EvidenceMaterializationCommit {
        key: ForwardMaterializationKey {
            versions: versions.clone(),
        },
        mode: routes.mode(),
        inbox_batch_id: batch.inbox_batch_id.get(),
        observations,
        diagnostics,
    }
}

fn create_observation(
    candidate: Candidate,
    authority: AuthorityScope,
    protocol_kind: Option<String>,
    decision_id: Option<String>,
    contract_digest: Option<String>,
    versions: &MaterializerVersions,
) -> EvidenceObservationWrite {
    let payload_json = serde_json::to_vec(&candidate.canonical_payload())
        .expect("canonical candidate JSON must serialize");
    let content_digest = hash_bytes(&payload_json);
    let logical_source_id = if candidate.identity_is_content_derived {
        hash_parts(&[
            b"flaggo-logical-source-content-v2",
            authority.tenant.as_bytes(),
            authority.application.as_bytes(),
            authority.environment.as_bytes(),
            content_digest.as_bytes(),
        ])
    } else {
        let identity_json = serde_json::to_vec(&candidate.canonical_identity())
            .expect("canonical identity JSON must serialize");
        hash_parts(&[
            b"flaggo-logical-source-v2",
            authority.tenant.as_bytes(),
            authority.application.as_bytes(),
            authority.environment.as_bytes(),
            &identity_json,
        ])
    };
    let observation_id = hash_parts(&[
        b"flaggo-observation-v2",
        logical_source_id.as_bytes(),
        content_digest.as_bytes(),
        versions.materializer.as_bytes(),
        versions.decoder.as_bytes(),
        versions.identity.as_bytes(),
        versions.projection.as_bytes(),
    ]);

    EvidenceObservationWrite {
        candidate_ordinal: candidate.ordinal,
        observation: EvidenceObservation {
            observation_id,
            logical_source_id,
            content_digest,
            authority,
            source: candidate.source_key(),
            observed_at_unix_nano: candidate.observed_at_unix_nano,
            observed_time_source: candidate.observed_time_source.to_owned(),
            protocol_kind,
            decision_id,
            contract_digest,
            payload_json,
            versions: versions.clone(),
        },
    }
}

fn derive_authority(candidate: &Candidate) -> Result<AuthorityScope, String> {
    let tenant = required_resource_string(candidate.resource_attributes(), "flaggo.tenant")
        .map_err(|error| format!("tenant: {error}"))?;
    let application =
        required_resource_string(candidate.resource_attributes(), "flaggo.application")
            .map_err(|error| format!("application: {error}"))?;
    let environment =
        required_resource_string(candidate.resource_attributes(), "flaggo.environment")
            .map_err(|error| format!("environment: {error}"))?;
    AuthorityScope::new(tenant, application, environment).map_err(|error| error.to_string())
}

fn required_resource_string(attributes: &[KeyValue], key: &str) -> Result<String, String> {
    let value = lookup_attribute(attributes, key).map_err(|error| error.to_string())?;
    match &value.value {
        Some(any_value::Value::StringValue(value)) if !value.is_empty() => Ok(value.clone()),
        Some(any_value::Value::StringValue(_)) => {
            Err(format!("attribute '{key}' must not be empty"))
        }
        Some(_) => Err(format!("attribute '{key}' must be a string")),
        None => Err(AttributeLookupError::Empty(key.to_owned()).to_string()),
    }
}

fn create_diagnostic(
    inbox_batch_id: u64,
    candidate_ordinal: Option<u32>,
    code: &str,
    message: String,
    detail: Option<Value>,
) -> EvidenceDiagnostic {
    let detail_json = detail
        .map(|detail| serde_json::to_vec(&detail).expect("diagnostic detail JSON must serialize"));
    let ordinal = candidate_ordinal
        .map(|value| value.to_string())
        .unwrap_or_default();
    let diagnostic_id = hash_parts(&[
        b"flaggo-materializer-diagnostic-v2",
        inbox_batch_id.to_string().as_bytes(),
        ordinal.as_bytes(),
        code.as_bytes(),
        detail_json.as_deref().unwrap_or_default(),
    ]);
    EvidenceDiagnostic {
        diagnostic_id,
        inbox_batch_id,
        candidate_ordinal,
        code: code.to_owned(),
        message,
        detail_json,
    }
}

fn hash_bytes(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn hash_parts(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("identity component length must fit in u64")
                .to_be_bytes(),
        );
        hasher.update(part);
    }
    format!("sha256:{:x}", hasher.finalize())
}
