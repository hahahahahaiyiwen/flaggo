use std::{num::NonZeroU16, path::Path, time::Duration};

use chrono::{DateTime, Utc};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceMaterializationCommit, EvidenceObservation, EvidenceObservationWrite,
    EvidenceSignal, EvidenceStore, EvidenceStoreError, ExistingLogicalObservation,
    ForwardMaterializationKey, MaterializerVersions, ObservationDisposition, SourceKey,
    SqliteEvidenceStore, StoredContractCatalog, plan_observation,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
async fn commits_forward_progress_with_idempotency_and_conflict_rejection() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("SQLite evidence store");
    let claim = store.replace_projection_claim(None).await.unwrap();
    store
        .save_catalog(
            claim,
            StoredContractCatalog {
                etag: "\"catalog:opaque\"".to_owned(),
                payload: br#"{"contracts":[]}"#.to_vec(),
                fetched_at: fixed_time(),
            },
        )
        .await
        .expect("save catalog");
    let key = key();
    let first = observation("first", "logical", br#"{"value":1}"#);

    let created = store
        .commit(claim, None, commit(1, &key, first.clone()))
        .await
        .expect("first commit");
    assert_eq!(created.observations_created, 1);
    assert_eq!(created.provenance_created, 1);
    assert_eq!(
        store.forward_checkpoint().await.expect("checkpoint"),
        Some(1)
    );

    let already_checkpointed = store
        .commit(claim, None, commit(1, &key, first.clone()))
        .await
        .expect("idempotent checkpoint");
    assert!(already_checkpointed.already_checkpointed);

    let duplicate = store
        .commit(claim, Some(1), commit(2, &key, first))
        .await
        .expect("duplicate observation");
    assert_eq!(duplicate.observations_created, 0);
    assert_eq!(duplicate.duplicate_observations, 1);
    assert_eq!(duplicate.provenance_created, 0);

    let conflict = store
        .commit(
            claim,
            Some(2),
            commit(3, &key, observation("second", "logical", br#"{"value":2}"#)),
        )
        .await
        .expect("conflicting observation");
    assert_eq!(conflict.observations_created, 0);
    assert_eq!(conflict.provenance_created, 0);
    assert_eq!(conflict.conflicts_created, 1);
    assert_eq!(conflict.diagnostics_created, 1);

    let mut routing_key = key.clone();
    routing_key.versions.routing = "2".to_owned();
    let mut routing_changed = observation("first", "logical", br#"{"value":1}"#);
    routing_changed.versions.routing = "2".to_owned();
    let rerouted = store
        .commit(claim, Some(3), commit(4, &routing_key, routing_changed))
        .await
        .expect("routing-only version change");
    assert_eq!(rerouted.duplicate_observations, 1);
    assert_eq!(rerouted.provenance_created, 0);

    let mut projection_key = key.clone();
    projection_key.versions.projection = "3".to_owned();
    let mut reprojected = observation("third", "logical", br#"{"value":1}"#);
    reprojected.versions.projection = "3".to_owned();
    let new_projection = store
        .commit(claim, Some(4), commit(5, &projection_key, reprojected))
        .await
        .expect("new projection version");
    assert_eq!(new_projection.observations_created, 0);
    assert_eq!(new_projection.duplicate_observations, 1);
    assert_eq!(new_projection.conflicts_created, 0);
    assert_eq!(new_projection.provenance_created, 0);

    let mut changed_projection_key = key.clone();
    changed_projection_key.versions.projection = "4".to_owned();
    let mut changed_projection = observation("fourth", "logical", br#"{"value":2}"#);
    changed_projection.versions.projection = "4".to_owned();
    let projection_conflict = store
        .commit(
            claim,
            Some(5),
            commit(6, &changed_projection_key, changed_projection),
        )
        .await
        .expect("changed projection content");
    assert_eq!(projection_conflict.observations_created, 0);
    assert_eq!(projection_conflict.conflicts_created, 1);
    assert_eq!(projection_conflict.diagnostics_created, 1);
    assert_eq!(projection_conflict.provenance_created, 0);

    let health = store.inspect().await.expect("evidence health");
    assert!(health.has_cached_catalog);
    assert_eq!(health.observation_count, 1);
    assert_eq!(health.provenance_count, 1);
    assert_eq!(health.conflict_count, 2);
    assert_eq!(health.diagnostic_count, 2);
    assert_eq!(
        store.forward_checkpoint().await.expect("checkpoint"),
        Some(6)
    );

    let observations = store
        .list_observations(&authority(), NonZeroU16::new(10).expect("nonzero limit"))
        .await
        .expect("observations");
    assert_eq!(observations.len(), 1);
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
        reopened.forward_checkpoint().await.expect("checkpoint"),
        Some(6)
    );
    reopened.close().await;
}

#[tokio::test]
async fn waits_for_a_competing_writer_before_starting_a_commit() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let database_url = sqlite_url(&directory.path().join("evidence.db"));
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("SQLite evidence store");
    let mut blocker = SqliteConnection::connect(&database_url)
        .await
        .expect("blocking SQLite connection");
    let claim = store.replace_projection_claim(None).await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut blocker)
        .await
        .expect("begin competing write");
    sqlx::query(
        "UPDATE flaggo_schema_versions
         SET version = version
         WHERE component = 'evidence-store'",
    )
    .execute(&mut blocker)
    .await
    .expect("hold competing write");

    let pending_store = store.clone();
    let pending = tokio::spawn(async move {
        pending_store
            .commit(
                claim,
                None,
                commit(
                    1,
                    &key(),
                    observation("first", "logical", br#"{"value":1}"#),
                ),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!pending.is_finished());

    sqlx::query("COMMIT")
        .execute(&mut blocker)
        .await
        .expect("release competing write");
    let result = pending
        .await
        .expect("commit task")
        .expect("commit after competing write");
    assert_eq!(result.observations_created, 1);

    blocker.close().await.expect("close blocking connection");
    store.close().await;
}

#[tokio::test]
async fn shared_policy_matches_stored_conflict_diagnostics_and_first_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let url = sqlite_url(&directory.path().join("policy.db"));
    let store = SqliteEvidenceStore::connect(&url).await.unwrap();
    let claim = store.replace_projection_claim(None).await.unwrap();
    let first = observation("first", "logical", br#"{"value":1}"#);
    store
        .commit(claim, None, commit(1, &key(), first.clone()))
        .await
        .unwrap();
    let mut conflicting = commit(
        2,
        &key(),
        observation("second", "logical", br#"{"value":2}"#),
    );
    conflicting.observations[0].candidate_ordinal = 7;
    let ObservationDisposition::Conflict {
        conflict_id,
        diagnostic,
    } = plan_observation(
        &conflicting.observations[0],
        2,
        Some(ExistingLogicalObservation {
            observation_id: &first.observation_id,
            content_digest: &first.content_digest,
        }),
        None,
    )
    .unwrap()
    else {
        panic!("conflict plan")
    };
    let result = store
        .commit(claim, Some(1), conflicting.clone())
        .await
        .unwrap();
    assert_eq!(result.conflicts_created, 1);
    assert_eq!(result.diagnostics_created, 1);
    conflicting.inbox_batch_id = 3;
    conflicting.observations[0].candidate_ordinal = 9;
    let retry = store.commit(claim, Some(2), conflicting).await.unwrap();
    assert_eq!(retry.conflicts_created, 0);
    assert_eq!(retry.diagnostics_created, 0);
    assert_eq!(retry.provenance_created, 0);
    let mut connection = SqliteConnection::connect(&url).await.unwrap();
    let stored: (String, i64, i64, String, String, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT diagnostic_id, inbox_batch_id, candidate_ordinal, code, message, detail_json
         FROM evidence_materializer_diagnostics",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            diagnostic.diagnostic_id,
            2,
            7,
            diagnostic.code,
            diagnostic.message,
            diagnostic.detail_json
        )
    );
    let conflict: (String, String, String, String, i64, i64) = sqlx::query_as(
        "SELECT conflict_id, accepted_observation_id, accepted_content_digest, rejected_content_digest,
         rejected_inbox_batch_id, rejected_candidate_ordinal FROM evidence_observation_conflicts",
    ).fetch_one(&mut connection).await.unwrap();
    assert_eq!(
        conflict,
        (
            conflict_id,
            first.observation_id.clone(),
            first.content_digest.clone(),
            digest(br#"{"value":2}"#),
            2,
            7
        )
    );
    let provenance: (String, i64, i64) = sqlx::query_as(
        "SELECT observation_id, inbox_batch_id, candidate_ordinal FROM evidence_observation_provenance",
    ).fetch_one(&mut connection).await.unwrap();
    assert_eq!(provenance, (first.observation_id.clone(), 1, 0));

    // A different logical key with the same observation ID must roll back, not dedup.
    let mut collision = observation("first", "different-logical", br#"{"value":1}"#);
    collision.versions.routing = "next".into();
    assert!(matches!(
        store
            .commit(claim, Some(3), commit(4, &key(), collision))
            .await,
        Err(EvidenceStoreError::CorruptData(_))
    ));
    assert_eq!(store.forward_checkpoint().await.unwrap(), Some(3));
    assert_eq!(store.inspect().await.unwrap().observation_count, 1);
    connection.close().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn rejects_version_two_schema_without_migration() {
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
         SET version = 2
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
            found: 2,
            expected: 5
        })
    ));
}

#[tokio::test]
async fn explicit_fence_migration_preserves_evidence_and_never_reuses_exhausted_epochs() {
    let directory = tempfile::tempdir().unwrap();
    let url = sqlite_url(&directory.path().join("migration.db"));
    let store = SqliteEvidenceStore::connect(&url).await.unwrap();
    let claim = store.replace_projection_claim(None).await.unwrap();
    store
        .commit(
            claim,
            None,
            commit(
                1,
                &key(),
                observation("original", "logical", br#"{"value":1}"#),
            ),
        )
        .await
        .unwrap();
    let before = store
        .list_observations(&authority(), NonZeroU16::new(10).unwrap())
        .await
        .unwrap();
    store.close().await;
    let mut connection = SqliteConnection::connect(&url).await.unwrap();
    sqlx::raw_sql("DROP TABLE evidence_projection_claim; UPDATE flaggo_schema_versions SET version=4 WHERE component='evidence-store';")
        .execute(&mut connection).await.unwrap();
    assert!(matches!(
        SqliteEvidenceStore::connect(&url).await,
        Err(EvidenceStoreError::UnsupportedSchemaVersion {
            found: 4,
            expected: 5,
            ..
        })
    ));
    assert!(SqliteEvidenceStore::migrate_v4_to_v5(&url).await.unwrap());
    assert!(!SqliteEvidenceStore::migrate_v4_to_v5(&url).await.unwrap());
    let store = SqliteEvidenceStore::connect(&url).await.unwrap();
    assert_eq!(store.forward_checkpoint().await.unwrap(), Some(1));
    assert_eq!(
        store
            .list_observations(&authority(), NonZeroU16::new(10).unwrap())
            .await
            .unwrap(),
        before
    );
    assert!(store.projection_claim().await.unwrap().is_none());
    sqlx::query("UPDATE evidence_projection_claim SET epoch=9223372036854775807 WHERE singleton=1")
        .execute(&mut connection)
        .await
        .unwrap();
    let exhausted = store.projection_claim().await.unwrap();
    assert!(matches!(
        store.replace_projection_claim(exhausted).await,
        Err(EvidenceStoreError::InvalidWrite(_))
    ));
    assert_eq!(store.projection_claim().await.unwrap(), exhausted);
    connection.close().await.unwrap();
    store.close().await;
}

fn commit(
    inbox_batch_id: u64,
    key: &ForwardMaterializationKey,
    observation: EvidenceObservation,
) -> EvidenceMaterializationCommit {
    EvidenceMaterializationCommit {
        key: key.clone(),
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
