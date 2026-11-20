//! Adapter-independent projection policy corpus; no database or query dialect.
use flaggo_evidence_store::*;

use crate::evidence::{commit, observation};

pub const FIXTURE_REVISION: &str = "flaggo.evidence-policy-conformance/v1";

pub fn c09_projection_policy() {
    let original = observation("first", br#"{"value":1}"#);
    let accepted = ExistingLogicalObservation {
        observation_id: &original.observation_id,
        content_digest: &original.content_digest,
    };
    let mut write = EvidenceObservationWrite {
        observation: original.clone(),
        candidate_ordinal: 7,
    };
    assert_eq!(
        plan_observation(&write, 1, None, None).unwrap(),
        ObservationDisposition::Insert
    );
    assert_eq!(
        plan_observation(&write, 2, Some(accepted), None).unwrap(),
        ObservationDisposition::Duplicate
    );
    assert_eq!(
        plan_observation(&write, 2, None, Some(&original)).unwrap(),
        ObservationDisposition::Duplicate
    );

    // Logical identity wins even when producer versions or the proposed ID change.
    write.observation.observation_id = observation("different", b"{}").observation_id;
    write.observation.versions.decoder = "next-decoder".into();
    write.observation.versions.identity = "next-identity".into();
    write.observation.versions.projection = "next-projection".into();
    assert_eq!(
        plan_observation(&write, 2, Some(accepted), None).unwrap(),
        ObservationDisposition::Duplicate
    );

    write.observation = observation("conflicting", br#"{"value":9}"#);
    write.observation.logical_source_id = original.logical_source_id.clone();
    let ObservationDisposition::Conflict {
        conflict_id,
        diagnostic,
    } = plan_observation(&write, 3, Some(accepted), None).unwrap()
    else {
        panic!("changed content must conflict")
    };
    assert_eq!(
        conflict_id,
        "sha256:07cebe52112864bf6d68e352372d7a1375d7e7cc5576ac6ff884312d84851998"
    );
    assert_eq!(diagnostic.diagnostic_id, conflict_id);
    assert_eq!(diagnostic.inbox_batch_id, 3);
    assert_eq!(diagnostic.candidate_ordinal, Some(7));
    assert_eq!(diagnostic.code, "logical-source-conflict");
    assert_eq!(
        diagnostic.message,
        "Candidate content conflicts with the accepted logical source."
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(diagnostic.detail_json.as_ref().unwrap())
            .unwrap(),
        serde_json::json!({
            "acceptedContentDigest": original.content_digest,
            "acceptedObservationId": original.observation_id,
            "logicalSourceId": original.logical_source_id,
            "rejectedContentDigest": write.observation.content_digest
        })
    );
    // Retry provenance, routing, materializer, and observation ID are not hash inputs.
    write.candidate_ordinal = 9;
    write.observation.versions.routing = "next".into();
    write.observation.versions.materializer = "next".into();
    write.observation.observation_id = observation("another", b"{}").observation_id;
    let ObservationDisposition::Conflict {
        conflict_id: retry_id,
        diagnostic: retry,
    } = plan_observation(&write, 4, Some(accepted), None).unwrap()
    else {
        panic!("retry must conflict")
    };
    assert_eq!(conflict_id, retry_id);
    assert_eq!(retry.inbox_batch_id, 4);
    assert_eq!(retry.candidate_ordinal, Some(9));
    for version in ["decoder", "identity", "projection"] {
        let mut changed = write.clone();
        match version {
            "decoder" => changed.observation.versions.decoder = "changed".into(),
            "identity" => changed.observation.versions.identity = "changed".into(),
            _ => changed.observation.versions.projection = "changed".into(),
        }
        let ObservationDisposition::Conflict {
            conflict_id: changed_id,
            ..
        } = plan_observation(&changed, 4, Some(accepted), None).unwrap()
        else {
            panic!("version change must still conflict")
        };
        assert_ne!(conflict_id, changed_id);
    }

    let mut receipt_timed = original.clone();
    receipt_timed.observed_time_source = "inbox.received_at".into();
    let mut retry = receipt_timed.clone();
    retry.observed_at_unix_nano += 1;
    retry.versions.routing = "next".into();
    verify_observation_identity(&receipt_timed, &retry).unwrap();
    let mut routing_only = original.clone();
    routing_only.versions.routing = "next".into();
    verify_observation_identity(&original, &routing_only).unwrap();
    for field in [
        "timestamp",
        "time-source",
        "payload",
        "materializer",
        "projection",
        "decoder",
        "identity",
        "logical",
        "authority",
    ] {
        let mut changed = original.clone();
        match field {
            "timestamp" => changed.observed_at_unix_nano += 1,
            "time-source" => changed.observed_time_source = "inbox.received_at".into(),
            "payload" => changed.payload_json = b"{}".to_vec(),
            "materializer" => changed.versions.materializer = "next".into(),
            "projection" => changed.versions.projection = "next".into(),
            "decoder" => changed.versions.decoder = "next".into(),
            "identity" => changed.versions.identity = "next".into(),
            "logical" => {
                changed.logical_source_id = observation("another", b"{}").logical_source_id
            }
            _ => changed.authority.tenant = "another".into(),
        }
        assert!(
            matches!(
                verify_observation_identity(&original, &changed),
                Err(EvidenceStoreError::CorruptData(_))
            ),
            "{field}"
        );
        let changed_write = EvidenceObservationWrite {
            observation: changed,
            candidate_ordinal: 0,
        };
        assert!(
            matches!(
                plan_observation(&changed_write, 5, None, Some(&original)),
                Err(EvidenceStoreError::CorruptData(_))
            ),
            "{field}"
        );
    }
    validate_observation(&original).unwrap();
    for field in [
        "id",
        "logical",
        "content",
        "contract",
        "authority",
        "source",
        "metric",
        "span-event",
        "time",
        "json",
        "version",
        "version-control",
        "version-length",
    ] {
        let mut changed = original.clone();
        match field {
            "id" => changed.observation_id = "bad".into(),
            "logical" => changed.logical_source_id = "bad".into(),
            "content" => changed.content_digest = format!("sha256:{}", "A".repeat(64)),
            "contract" => changed.contract_digest = Some("bad".into()),
            "authority" => changed.authority.tenant = "bad tenant".into(),
            "source" => changed.source.signal_name.clear(),
            "metric" => changed.source.signal = EvidenceSignal::Metric,
            "span-event" => changed.source.signal = EvidenceSignal::SpanEvent,
            "time" => changed.observed_time_source.clear(),
            "json" => changed.payload_json = b"{".to_vec(),
            "version" => changed.versions.decoder.clear(),
            "version-control" => changed.versions.identity = "\n".into(),
            _ => changed.versions.projection = "x".repeat(65),
        }
        assert!(
            matches!(
                validate_observation(&changed),
                Err(EvidenceStoreError::InvalidWrite(_))
            ),
            "{field}"
        );
        assert!(
            matches!(
                validate_commit(&commit(1, vec![changed])),
                Err(EvidenceStoreError::InvalidWrite(_))
            ),
            "{field}"
        );
    }
    for signal in [
        EvidenceSignal::Metric,
        EvidenceSignal::SpanEvent,
        EvidenceSignal::Span,
    ] {
        let mut valid = original.clone();
        valid.source.signal = signal;
        if signal == EvidenceSignal::Metric {
            valid.source.metric_kind = Some("gauge".into());
            valid.source.metric_unit = Some(String::new());
        } else if signal == EvidenceSignal::SpanEvent {
            valid.source.parent_span_name = Some("parent".into());
        }
        validate_observation(&valid).unwrap();
    }
    let catalog = StoredContractCatalog {
        etag: "\"opaque\"".into(),
        payload: br#"{"contracts":[]}"#.to_vec(),
        fetched_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
    };
    validate_catalog(&catalog).unwrap();
    for etag in ["", "invalid\n"] {
        assert!(matches!(
            validate_catalog(&StoredContractCatalog {
                etag: etag.into(),
                ..catalog.clone()
            }),
            Err(EvidenceStoreError::InvalidWrite(_))
        ));
    }
    assert!(matches!(
        validate_catalog(&StoredContractCatalog {
            payload: b"{".to_vec(),
            ..catalog
        }),
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
    let mut batch = commit(3, vec![original]);
    batch.diagnostics.push(diagnostic);
    validate_commit(&batch).unwrap();
    batch.diagnostics[0].inbox_batch_id = 4;
    assert!(matches!(
        validate_commit(&batch),
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
    batch.diagnostics[0].inbox_batch_id = 3;
    batch.diagnostics[0].detail_json = Some(b"{".to_vec());
    assert!(matches!(
        validate_commit(&batch),
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
    batch.diagnostics.clear();
    batch.inbox_batch_id = 0;
    assert!(matches!(
        validate_commit(&batch),
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
    assert!(matches!(
        plan_observation(&write, 0, None, None),
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
}

#[cfg(test)]
mod tests {
    #[test]
    fn projection_policy_corpus() {
        super::c09_projection_policy();
    }
}
