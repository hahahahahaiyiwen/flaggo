//! Storage-independent projection rules. Adapters supply transaction-consistent
//! lookup facts and persist the resulting effects with provenance and checkpoint.
use sha2::{Digest, Sha256};

use crate::{
    AuthorityScope, EvidenceDiagnostic, EvidenceMaterializationCommit, EvidenceObservation,
    EvidenceObservationWrite, EvidenceSignal, EvidenceStoreError, MaterializerVersions, SourceKey,
    StoredContractCatalog,
};

/// The first accepted row found by `logical_source_id` alone, across all versions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExistingLogicalObservation<'a> {
    pub observation_id: &'a str,
    pub content_digest: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationDisposition {
    /// Insert the observation and its first provenance.
    Insert,
    /// Keep the accepted row and its first provenance unchanged.
    Duplicate,
    /// Insert the conflict once by ID, and its diagnostic only on first insertion.
    Conflict {
        conflict_id: String,
        diagnostic: EvidenceDiagnostic,
    },
}

/// Plan one candidate using facts read within the adapter's commit transaction.
///
/// `existing_logical` must describe the first row matching the proposed logical
/// source ID, without a version filter. When absent, supply the row matching the
/// proposed observation ID in `existing_by_id`, if any. Alternatively adapters
/// may attempt the insert and call `verify_observation_identity` on a uniqueness
/// collision. Logical-source dedup/conflict takes precedence over ID comparison.
pub fn plan_observation(
    write: &EvidenceObservationWrite,
    inbox_batch_id: u64,
    existing_logical: Option<ExistingLogicalObservation<'_>>,
    existing_by_id: Option<&EvidenceObservation>,
) -> Result<ObservationDisposition, EvidenceStoreError> {
    let observation = &write.observation;
    validate_observation(observation)?;
    if inbox_batch_id == 0 {
        return Err(EvidenceStoreError::InvalidWrite(
            "inbox batch ID must be positive".to_owned(),
        ));
    }
    if let Some(accepted) = existing_logical {
        if accepted.content_digest == observation.content_digest {
            return Ok(ObservationDisposition::Duplicate);
        }
        let conflict_id = digest_parts(&[
            "evidence-conflict/v2",
            &observation.logical_source_id,
            accepted.content_digest,
            &observation.content_digest,
            &observation.versions.decoder,
            &observation.versions.identity,
            &observation.versions.projection,
        ]);
        let diagnostic = EvidenceDiagnostic {
            diagnostic_id: conflict_id.clone(),
            inbox_batch_id,
            candidate_ordinal: Some(write.candidate_ordinal),
            code: "logical-source-conflict".to_owned(),
            message: "Candidate content conflicts with the accepted logical source.".to_owned(),
            detail_json: Some(
                serde_json::to_vec(&serde_json::json!({
                    "acceptedContentDigest": accepted.content_digest,
                    "acceptedObservationId": accepted.observation_id,
                    "logicalSourceId": observation.logical_source_id,
                    "rejectedContentDigest": observation.content_digest
                }))
                .expect("conflict detail JSON must serialize"),
            ),
        };
        return Ok(ObservationDisposition::Conflict {
            conflict_id,
            diagnostic,
        });
    }
    if let Some(existing) = existing_by_id {
        verify_observation_identity(existing, observation)?;
        return Ok(ObservationDisposition::Duplicate);
    }
    Ok(ObservationDisposition::Insert)
}

/// Compare an observation-ID collision, allowing routing-only changes and a
/// different receipt timestamp only when both time sources are `inbox.received_at`.
pub fn verify_observation_identity(
    existing: &EvidenceObservation,
    proposed: &EvidenceObservation,
) -> Result<(), EvidenceStoreError> {
    let mut existing = existing.clone();
    existing.versions.routing = proposed.versions.routing.clone();
    if existing.observed_time_source == "inbox.received_at"
        && proposed.observed_time_source == "inbox.received_at"
    {
        existing.observed_at_unix_nano = proposed.observed_at_unix_nano;
    }
    if existing != *proposed {
        return Err(EvidenceStoreError::CorruptData(format!(
            "observation {} has inconsistent content",
            proposed.observation_id
        )));
    }
    Ok(())
}

pub fn validate_catalog(catalog: &StoredContractCatalog) -> Result<(), EvidenceStoreError> {
    if catalog.etag.is_empty() || catalog.etag.chars().any(char::is_control) {
        return Err(EvidenceStoreError::InvalidWrite(
            "catalog ETag must be non-empty and contain no control characters".to_owned(),
        ));
    }
    validate_json("contract catalog", &catalog.payload)
}

pub fn validate_commit(commit: &EvidenceMaterializationCommit) -> Result<(), EvidenceStoreError> {
    validate_versions(&commit.key.versions)?;
    if commit.inbox_batch_id == 0 {
        return Err(EvidenceStoreError::InvalidWrite(
            "inbox batch ID must be positive".to_owned(),
        ));
    }
    for write in &commit.observations {
        validate_observation(&write.observation)?;
    }
    for diagnostic in &commit.diagnostics {
        validate_diagnostic(diagnostic)?;
        if diagnostic.inbox_batch_id != commit.inbox_batch_id {
            return Err(EvidenceStoreError::InvalidWrite(
                "diagnostic batch must equal the committed batch".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_versions(versions: &MaterializerVersions) -> Result<(), EvidenceStoreError> {
    for (name, value) in [
        ("materializer version", &versions.materializer),
        ("decoder version", &versions.decoder),
        ("identity version", &versions.identity),
        ("projection version", &versions.projection),
        ("routing version", &versions.routing),
    ] {
        if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
            return Err(EvidenceStoreError::InvalidWrite(format!(
                "{name} must be 1-64 non-control characters"
            )));
        }
    }
    Ok(())
}

pub fn validate_observation(observation: &EvidenceObservation) -> Result<(), EvidenceStoreError> {
    for (name, digest) in [
        ("observation ID", &observation.observation_id),
        ("logical source ID", &observation.logical_source_id),
        ("content digest", &observation.content_digest),
    ] {
        validate_digest(name, digest)?;
    }
    if let Some(contract_digest) = &observation.contract_digest {
        validate_digest("observation contract digest", contract_digest)?;
    }
    AuthorityScope::new(
        observation.authority.tenant.clone(),
        observation.authority.application.clone(),
        observation.authority.environment.clone(),
    )?;
    validate_source(&observation.source)?;
    if observation.observed_time_source.is_empty() {
        return Err(EvidenceStoreError::InvalidWrite(
            "observation time source must not be empty".to_owned(),
        ));
    }
    validate_json("observation payload", &observation.payload_json)?;
    validate_versions(&observation.versions)
}

fn validate_source(source: &SourceKey) -> Result<(), EvidenceStoreError> {
    if source.instrumentation_scope.is_empty() || source.signal_name.is_empty() {
        return Err(EvidenceStoreError::InvalidWrite(
            "source instrumentation scope and signal name must not be empty".to_owned(),
        ));
    }
    let shape_is_valid = match source.signal {
        EvidenceSignal::Metric => {
            source
                .metric_kind
                .as_ref()
                .is_some_and(|value| !value.is_empty())
                && source.metric_unit.is_some()
                && source.parent_span_name.is_none()
        }
        EvidenceSignal::SpanEvent => {
            source.metric_kind.is_none()
                && source.metric_unit.is_none()
                && source
                    .parent_span_name
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
        }
        EvidenceSignal::Log | EvidenceSignal::Span => {
            source.metric_kind.is_none()
                && source.metric_unit.is_none()
                && source.parent_span_name.is_none()
        }
    };
    if !shape_is_valid {
        return Err(EvidenceStoreError::InvalidWrite(
            "source fields do not match the signal kind".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_diagnostic(
    diagnostic: &EvidenceDiagnostic,
) -> Result<(), EvidenceStoreError> {
    validate_digest("diagnostic ID", &diagnostic.diagnostic_id)?;
    if diagnostic.inbox_batch_id == 0 || diagnostic.code.is_empty() || diagnostic.message.is_empty()
    {
        return Err(EvidenceStoreError::InvalidWrite(
            "diagnostic provenance, code, and message are required".to_owned(),
        ));
    }
    if let Some(detail) = &diagnostic.detail_json {
        validate_json("diagnostic detail", detail)?;
    }
    Ok(())
}

fn validate_digest(name: &str, value: &str) -> Result<(), EvidenceStoreError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(EvidenceStoreError::InvalidWrite(format!(
            "{name} must be a lowercase sha256 digest"
        )));
    }
    Ok(())
}

fn validate_json(name: &str, value: &[u8]) -> Result<(), EvidenceStoreError> {
    serde_json::from_slice::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|error| {
            EvidenceStoreError::InvalidWrite(format!("{name} is invalid JSON: {error}"))
        })
}

fn digest_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("digest component length must fit in u64")
                .to_be_bytes(),
        );
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}
