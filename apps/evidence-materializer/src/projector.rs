use flaggo_evidence_store::{
    DecisionScope, EvidenceAssociation, EvidenceDiagnostic, EvidenceMaterializationCommit,
    EvidenceObservation, EvidenceObservationWrite, MaterializationKey, MaterializerVersions,
};
use flaggo_raw_otlp_inbox::RawOtlpInboxBatch;
use opentelemetry_proto::tonic::common::v1::{KeyValue, any_value};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    builtin::{BuiltInEvaluation, evaluate as evaluate_builtin},
    candidate::{AttributeLookupError, Candidate, lookup_attribute},
    decode_batch,
    selector::CompiledSelectorSnapshot,
};

pub(crate) fn project_batch(
    batch: &RawOtlpInboxBatch,
    snapshot: &CompiledSelectorSnapshot,
    selector_scope: &DecisionScope,
    versions: &MaterializerVersions,
) -> EvidenceMaterializationCommit {
    let decoded = decode_batch(batch);
    let mut observations = Vec::new();
    let mut diagnostics = decoded
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            create_diagnostic(
                &snapshot.snapshot_digest,
                batch.inbox_batch_id.get(),
                diagnostic.candidate_ordinal,
                &diagnostic.code,
                diagnostic.message,
                Some(diagnostic.detail),
            )
        })
        .collect::<Vec<_>>();

    for candidate in decoded.candidates {
        let scope = match derive_scope(&candidate) {
            Ok(scope) => scope,
            Err(error) => {
                diagnostics.push(create_diagnostic(
                    &snapshot.snapshot_digest,
                    batch.inbox_batch_id.get(),
                    Some(candidate.ordinal),
                    "observation.scope_unresolved",
                    "Application/environment scope could not be derived from OTLP resource attributes."
                        .to_owned(),
                    Some(json!({ "reason": error })),
                ));
                continue;
            }
        };

        let (protocol_kind, decision_id, contract_digest, associations) =
            match evaluate_builtin(&candidate) {
                BuiltInEvaluation::Valid(decision) => (
                    Some("decision.received".to_owned()),
                    Some(decision.decision_id),
                    Some(decision.contract_digest),
                    Vec::new(),
                ),
                BuiltInEvaluation::Invalid(diagnostic) => {
                    diagnostics.push(create_diagnostic(
                        &snapshot.snapshot_digest,
                        batch.inbox_batch_id.get(),
                        Some(candidate.ordinal),
                        &diagnostic.code,
                        diagnostic.message,
                        Some(diagnostic.detail),
                    ));
                    continue;
                }
                BuiltInEvaluation::NotBuiltIn => {
                    if &scope != selector_scope {
                        continue;
                    }
                    let evaluation = snapshot.evaluate(&candidate);
                    diagnostics.extend(evaluation.diagnostics.into_iter().map(|diagnostic| {
                        create_diagnostic(
                            &snapshot.snapshot_digest,
                            batch.inbox_batch_id.get(),
                            Some(candidate.ordinal),
                            &diagnostic.code,
                            diagnostic.message,
                            Some(diagnostic.detail),
                        )
                    }));
                    if evaluation.associations.is_empty() {
                        continue;
                    }
                    let associations = evaluation
                        .associations
                        .into_iter()
                        .map(|association| EvidenceAssociation {
                            snapshot_digest: snapshot.snapshot_digest.clone(),
                            contract_digest: association.contract_digest,
                            evidence_name: association.evidence_name,
                            contract_attribute: association.contract_attribute,
                            correlation_json: association.correlation_json,
                            source_json: association.source_json,
                        })
                        .collect();
                    (None, None, None, associations)
                }
            };

        observations.push(create_observation(
            candidate,
            scope,
            protocol_kind,
            decision_id,
            contract_digest,
            associations,
            versions,
        ));
    }

    EvidenceMaterializationCommit {
        key: MaterializationKey {
            snapshot_digest: snapshot.snapshot_digest.clone(),
            versions: versions.clone(),
        },
        inbox_batch_id: batch.inbox_batch_id.get(),
        observations,
        diagnostics,
    }
}

fn create_observation(
    candidate: Candidate,
    scope: DecisionScope,
    protocol_kind: Option<String>,
    decision_id: Option<String>,
    contract_digest: Option<String>,
    associations: Vec<EvidenceAssociation>,
    versions: &MaterializerVersions,
) -> EvidenceObservationWrite {
    let payload_json = serde_json::to_vec(&candidate.canonical_payload())
        .expect("canonical candidate JSON must serialize");
    let content_digest = hash_bytes(&payload_json);
    let logical_source_id = if candidate.identity_is_content_derived {
        hash_parts(&[
            b"flaggo-logical-source-content-v1",
            scope.application.as_bytes(),
            scope.environment.as_bytes(),
            content_digest.as_bytes(),
        ])
    } else {
        let identity_json = serde_json::to_vec(&candidate.canonical_identity())
            .expect("canonical identity JSON must serialize");
        hash_parts(&[
            b"flaggo-logical-source-v1",
            scope.application.as_bytes(),
            scope.environment.as_bytes(),
            &identity_json,
        ])
    };
    let observation_id = hash_parts(&[
        b"flaggo-observation-v1",
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
            scope,
            signal: candidate.signal,
            instrumentation_scope: candidate.instrumentation_scope,
            signal_name: candidate.signal_name,
            metric_kind: candidate.metric_kind,
            metric_unit: candidate.metric_unit,
            observed_at_unix_nano: candidate.observed_at_unix_nano,
            observed_time_source: candidate.observed_time_source.to_owned(),
            protocol_kind,
            decision_id,
            contract_digest,
            payload_json,
            versions: versions.clone(),
        },
        associations,
    }
}

fn derive_scope(candidate: &Candidate) -> Result<DecisionScope, String> {
    let application = preferred_resource_string(
        candidate.resource_attributes(),
        "flaggo.application",
        "service.name",
    )
    .map_err(|error| format!("application: {error}"))?;
    let environment = preferred_resource_string(
        candidate.resource_attributes(),
        "flaggo.environment",
        "deployment.environment.name",
    )
    .map_err(|error| format!("environment: {error}"))?;
    DecisionScope::new(application, environment).map_err(|error| error.to_string())
}

fn preferred_resource_string(
    attributes: &[KeyValue],
    preferred: &str,
    fallback: &str,
) -> Result<String, String> {
    match resource_string(attributes, preferred) {
        Err(AttributeLookupError::Missing(_)) => resource_string(attributes, fallback),
        result => result,
    }
    .map_err(|error| error.to_string())
}

fn resource_string(attributes: &[KeyValue], key: &str) -> Result<String, AttributeLookupError> {
    let value = lookup_attribute(attributes, key)?;
    match &value.value {
        Some(any_value::Value::StringValue(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(AttributeLookupError::Empty(key.to_owned())),
    }
}

fn create_diagnostic(
    snapshot_digest: &str,
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
        b"flaggo-materializer-diagnostic-v1",
        snapshot_digest.as_bytes(),
        inbox_batch_id.to_string().as_bytes(),
        ordinal.as_bytes(),
        code.as_bytes(),
        detail_json.as_deref().unwrap_or_default(),
    ]);
    EvidenceDiagnostic {
        diagnostic_id,
        snapshot_digest: snapshot_digest.to_owned(),
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
