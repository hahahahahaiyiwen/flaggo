use std::{num::NonZeroU16, path::Path};

use chrono::{DateTime, Utc};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceMaterializationCommit, EvidenceMaterializationMode,
    EvidenceObservation, EvidenceObservationWrite, EvidenceSignal, EvidenceStore,
    EvidenceStoreError, ForwardMaterializationKey, MaterializerVersions, SourceKey,
    SqliteEvidenceStore, StoredContractCatalog,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
async fn commits_global_forward_progress_and_idempotent_replay() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("SQLite evidence store");
    store
        .save_catalog(StoredContractCatalog {
            etag: "\"catalog:opaque\"".to_owned(),
            payload: br#"{"contracts":[]}"#.to_vec(),
            fetched_at: fixed_time(),
        })
        .await
        .expect("save catalog");
    let key = key();
    let first = observation("first", "logical", br#"{"value":1}"#);

    let created = store
        .commit(commit(
            1,
            &key,
            EvidenceMaterializationMode::Forward,
            first.clone(),
        ))
        .await
        .expect("first commit");
    assert_eq!(created.observations_created, 1);
    assert_eq!(created.provenance_created, 1);
    assert_eq!(
        store.forward_checkpoint(&key).await.expect("checkpoint"),
        Some(1)
    );

    let already_checkpointed = store
        .commit(commit(
            1,
            &key,
            EvidenceMaterializationMode::Forward,
            first.clone(),
        ))
        .await
        .expect("idempotent checkpoint");
    assert!(already_checkpointed.already_checkpointed);

    let replay = store
        .commit(commit(
            1,
            &key,
            EvidenceMaterializationMode::Replay,
            first.clone(),
        ))
        .await
        .expect("idempotent replay");
    assert!(!replay.already_checkpointed);
    assert_eq!(replay.duplicate_observations, 1);
    assert_eq!(replay.provenance_created, 0);
    assert_eq!(
        store.forward_checkpoint(&key).await.expect("checkpoint"),
        Some(1)
    );

    let duplicate = store
        .commit(commit(2, &key, EvidenceMaterializationMode::Forward, first))
        .await
        .expect("duplicate observation");
    assert_eq!(duplicate.observations_created, 0);
    assert_eq!(duplicate.duplicate_observations, 1);
    assert_eq!(duplicate.provenance_created, 1);

    let conflict = store
        .commit(commit(
            3,
            &key,
            EvidenceMaterializationMode::Forward,
            observation("second", "logical", br#"{"value":2}"#),
        ))
        .await
        .expect("conflicting observation");
    assert_eq!(conflict.observations_created, 1);
    assert_eq!(conflict.conflicts_created, 1);
    assert_eq!(conflict.diagnostics_created, 1);

    let mut routing_key = key.clone();
    routing_key.versions.routing = "2".to_owned();
    let mut routing_changed = observation("first", "logical", br#"{"value":1}"#);
    routing_changed.versions.routing = "2".to_owned();
    let rerouted = store
        .commit(commit(
            4,
            &routing_key,
            EvidenceMaterializationMode::Forward,
            routing_changed,
        ))
        .await
        .expect("routing-only version change");
    assert_eq!(rerouted.duplicate_observations, 1);
    assert_eq!(rerouted.provenance_created, 1);

    let health = store.inspect().await.expect("evidence health");
    assert!(health.has_cached_catalog);
    assert_eq!(health.observation_count, 2);
    assert_eq!(health.provenance_count, 4);
    assert_eq!(health.conflict_count, 1);
    assert_eq!(health.diagnostic_count, 1);
    assert_eq!(
        store.forward_checkpoint(&key).await.expect("checkpoint"),
        Some(3)
    );
    assert_eq!(
        store
            .forward_checkpoint(&routing_key)
            .await
            .expect("routing checkpoint"),
        Some(4)
    );

    let observations = store
        .list_observations(&authority(), NonZeroU16::new(10).expect("nonzero limit"))
        .await
        .expect("observations");
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].observation.authority.tenant, "local");
    assert_eq!(
        observations[0].observation.source.instrumentation_scope,
        "worker"
    );

    store.close().await;
    let reopened = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("reopened evidence store");
    assert_eq!(
        reopened
            .load_catalog()
            .await
            .expect("cached catalog")
            .expect("stored catalog")
            .etag,
        "\"catalog:opaque\""
    );
    assert_eq!(
        reopened.forward_checkpoint(&key).await.expect("checkpoint"),
        Some(3)
    );
    reopened.close().await;
}

#[tokio::test]
async fn rejects_version_one_schema_without_migration() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("SQLite evidence store");
    store.close().await;

    let mut connection = SqliteConnection::connect(&database_url)
        .await
        .expect("SQLite connection");
    sqlx::query(
        "UPDATE flaggo_schema_versions
         SET version = 1
         WHERE component = 'evidence-store'",
    )
    .execute(&mut connection)
    .await
    .expect("downgrade schema marker");
    connection.close().await.expect("close SQLite connection");

    assert!(matches!(
        SqliteEvidenceStore::connect(&database_url).await,
        Err(EvidenceStoreError::UnsupportedSchemaVersion {
            component: "evidence-store",
            found: 1,
            expected: 2
        })
    ));
}

fn commit(
    inbox_batch_id: u64,
    key: &ForwardMaterializationKey,
    mode: EvidenceMaterializationMode,
    observation: EvidenceObservation,
) -> EvidenceMaterializationCommit {
    EvidenceMaterializationCommit {
        key: key.clone(),
        mode,
        inbox_batch_id,
        observations: vec![EvidenceObservationWrite {
            candidate_ordinal: 0,
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
        authority: authority(),
        source: SourceKey {
            signal: EvidenceSignal::Log,
            instrumentation_scope: "worker".to_owned(),
            signal_name: "worker.latency".to_owned(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        },
        observed_at_unix_nano: 1_799_999_999_000_000_000,
        observed_time_source: "timeUnixNano".to_owned(),
        protocol_kind: None,
        decision_id: None,
        contract_digest: None,
        payload_json: payload.to_vec(),
        versions: versions(),
    }
}

fn key() -> ForwardMaterializationKey {
    ForwardMaterializationKey {
        versions: versions(),
    }
}

fn versions() -> MaterializerVersions {
    MaterializerVersions {
        materializer: "2".to_owned(),
        decoder: "1".to_owned(),
        identity: "2".to_owned(),
        projection: "2".to_owned(),
        routing: "1".to_owned(),
    }
}

fn authority() -> AuthorityScope {
    AuthorityScope::new("local".to_owned(), "worker".to_owned(), "test".to_owned())
        .expect("valid authority")
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
