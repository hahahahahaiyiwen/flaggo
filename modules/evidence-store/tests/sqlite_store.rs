use std::{num::NonZeroU16, path::Path};

use chrono::{DateTime, Utc};
use flaggo_evidence_store::{
    DecisionScope, EvidenceAssociation, EvidenceMaterializationCommit, EvidenceObservation,
    EvidenceObservationWrite, EvidenceSignal, EvidenceStore, MaterializationKey,
    MaterializerVersions, SqliteEvidenceStore, StoredSelectorSnapshot,
};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn commits_observations_provenance_associations_conflicts_and_checkpoint_atomically() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("SQLite evidence store");
    let snapshot_digest = digest(b"snapshot");
    store
        .activate_snapshot(StoredSelectorSnapshot {
            snapshot_digest: snapshot_digest.clone(),
            payload: serde_json::to_vec(&serde_json::json!({
                "contracts": [],
                "snapshotDigest": snapshot_digest
            }))
            .expect("snapshot JSON"),
            fetched_at: fixed_time(),
        })
        .await
        .expect("activate snapshot");
    let key = key(&snapshot_digest);
    let first = observation("first", "logical", br#"{"value":1}"#);

    let created = store
        .commit(commit(1, &key, first.clone()))
        .await
        .expect("first commit");

    assert_eq!(created.observations_created, 1);
    assert_eq!(created.provenance_created, 1);
    assert_eq!(created.associations_created, 1);
    assert_eq!(store.checkpoint(&key).await.expect("checkpoint"), Some(1));

    let already_checkpointed = store
        .commit(commit(1, &key, first.clone()))
        .await
        .expect("idempotent checkpoint");
    assert!(already_checkpointed.already_checkpointed);

    let duplicate = store
        .commit(commit(2, &key, first))
        .await
        .expect("duplicate provenance");
    assert_eq!(duplicate.observations_created, 0);
    assert_eq!(duplicate.duplicate_observations, 1);
    assert_eq!(duplicate.provenance_created, 1);
    assert_eq!(duplicate.associations_created, 0);

    let conflict = store
        .commit(commit(
            3,
            &key,
            observation("second", "logical", br#"{"value":2}"#),
        ))
        .await
        .expect("conflicting observation");
    assert_eq!(conflict.observations_created, 1);
    assert_eq!(conflict.conflicts_created, 1);
    assert_eq!(conflict.diagnostics_created, 1);

    let health = store.inspect().await.expect("evidence health");
    assert_eq!(health.active_snapshot_digest, Some(snapshot_digest.clone()));
    assert_eq!(health.observation_count, 2);
    assert_eq!(health.provenance_count, 3);
    assert_eq!(health.association_count, 2);
    assert_eq!(health.conflict_count, 1);
    assert_eq!(health.diagnostic_count, 1);
    assert_eq!(store.checkpoint(&key).await.expect("checkpoint"), Some(3));

    let scope = scope();
    let observations = store
        .list_observations(&scope, NonZeroU16::new(10).expect("nonzero limit"))
        .await
        .expect("observations");
    assert_eq!(observations.len(), 2);
    let associations = store
        .list_associations(
            &scope,
            &digest(b"contract"),
            NonZeroU16::new(10).expect("nonzero limit"),
        )
        .await
        .expect("associations");
    assert_eq!(associations.len(), 2);

    store.close().await;
    let reopened = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("reopened evidence store");
    assert_eq!(
        reopened
            .load_active_snapshot()
            .await
            .expect("active snapshot")
            .expect("stored snapshot")
            .snapshot_digest,
        snapshot_digest
    );
    assert_eq!(
        reopened.checkpoint(&key).await.expect("checkpoint"),
        Some(3)
    );
    reopened.close().await;
}

fn commit(
    inbox_batch_id: u64,
    key: &MaterializationKey,
    observation: EvidenceObservation,
) -> EvidenceMaterializationCommit {
    EvidenceMaterializationCommit {
        key: key.clone(),
        inbox_batch_id,
        observations: vec![EvidenceObservationWrite {
            candidate_ordinal: 0,
            associations: vec![EvidenceAssociation {
                snapshot_digest: key.snapshot_digest.clone(),
                contract_digest: digest(b"contract"),
                evidence_name: "latency".to_owned(),
                contract_attribute: "latency_ms".to_owned(),
                correlation_json: br#"{"worker_id":{"type":"string","value":"worker-1"}}"#.to_vec(),
                source_json: br#"{"kind":"log","name":"worker.latency","scope":"worker"}"#.to_vec(),
            }],
            observation,
        }],
        diagnostics: Vec::new(),
    }
}

fn observation(content_key: &str, logical_key: &str, payload: &[u8]) -> EvidenceObservation {
    EvidenceObservation {
        observation_id: digest(format!("observation:{content_key}").as_bytes()),
        logical_source_id: digest(format!("logical:{logical_key}").as_bytes()),
        content_digest: digest(payload),
        scope: scope(),
        signal: EvidenceSignal::Log,
        instrumentation_scope: "worker".to_owned(),
        signal_name: "worker.latency".to_owned(),
        metric_kind: None,
        metric_unit: None,
        observed_at_unix_nano: 1_799_999_999_000_000_000,
        observed_time_source: "timeUnixNano".to_owned(),
        protocol_kind: None,
        decision_id: None,
        contract_digest: None,
        payload_json: payload.to_vec(),
        versions: versions(),
    }
}

fn key(snapshot_digest: &str) -> MaterializationKey {
    MaterializationKey {
        snapshot_digest: snapshot_digest.to_owned(),
        versions: versions(),
    }
}

fn versions() -> MaterializerVersions {
    MaterializerVersions {
        materializer: "1".to_owned(),
        decoder: "1".to_owned(),
        identity: "1".to_owned(),
        projection: "1".to_owned(),
        selector_protocol: "1".to_owned(),
    }
}

fn scope() -> DecisionScope {
    DecisionScope::new("worker".to_owned(), "test".to_owned()).expect("valid scope")
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
        .expect("fixed time")
        .to_utc()
}

fn digest(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
}
