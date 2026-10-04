use std::{
    collections::{BTreeSet, HashSet},
    env,
    num::NonZeroU16,
};

use flaggo_evidence_materializer::{
    DECODER_VERSION, IDENTITY_VERSION, MATERIALIZER_VERSION, PROJECTION_VERSION, ROUTING_VERSION,
};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceObservation, EvidenceSignal, EvidenceStore, SqliteEvidenceStore,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_EVIDENCE_DATABASE_URL";
const CONTRACT_DIGEST_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_CONTRACT_DIGEST";
const EXECUTABLE_DIGEST_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_EXECUTABLE_DIGEST";

#[tokio::test]
#[ignore = "requires the database produced by the Tetris real-host integration"]
async fn persisted_tetris_evidence_is_semantically_correct() {
    let database_url = required_environment(DATABASE_URL_ENVIRONMENT_VARIABLE);
    let expected_contract_digest = required_environment(CONTRACT_DIGEST_ENVIRONMENT_VARIABLE);
    let expected_executable_digest = required_environment(EXECUTABLE_DIGEST_ENVIRONMENT_VARIABLE);
    let authority = AuthorityScope::new(
        "local".to_owned(),
        "tetris".to_owned(),
        "integration".to_owned(),
    )
    .expect("Tetris authority");
    let store = SqliteEvidenceStore::connect(&database_url)
        .await
        .expect("Tetris Evidence Store");
    let health = store.inspect().await.expect("Evidence Store health");
    let observations = store
        .list_observations(
            &authority,
            NonZeroU16::new(100).expect("nonzero observation limit"),
        )
        .await
        .expect("Tetris observations");

    assert!(health.has_cached_catalog);
    assert_eq!(
        health.observation_count,
        u64::try_from(observations.len()).expect("observation count fits u64")
    );
    assert_eq!(health.provenance_count, health.observation_count);
    assert_eq!(health.conflict_count, 0);
    assert_eq!(health.diagnostic_count, 0);

    let mut observation_ids = HashSet::new();
    let mut protocol_decision_ids = HashSet::new();
    let mut decision_results = BTreeSet::new();
    let mut decision_rules = BTreeSet::new();
    let mut placement_sessions = BTreeSet::new();
    let mut recovery_sessions = BTreeSet::new();

    for stored in observations {
        let observation = stored.observation;
        assert_eq!(observation.authority, authority);
        assert!(observation_ids.insert(observation.observation_id.clone()));
        assert_eq!(
            observation.content_digest,
            format!("sha256:{:x}", Sha256::digest(&observation.payload_json))
        );
        assert_eq!(observation.versions.materializer, MATERIALIZER_VERSION);
        assert_eq!(observation.versions.decoder, DECODER_VERSION);
        assert_eq!(observation.versions.identity, IDENTITY_VERSION);
        assert_eq!(observation.versions.projection, PROJECTION_VERSION);
        assert_eq!(observation.versions.routing, ROUTING_VERSION);
        assert_ne!(observation.observed_time_source, "inbox.received_at");

        let payload: Value =
            serde_json::from_slice(&observation.payload_json).expect("observation payload JSON");
        assert_common_payload(&payload, &observation);

        match observation.protocol_kind.as_deref() {
            Some("decision.received") => {
                assert_protocol_observation(
                    &observation,
                    &payload,
                    &expected_contract_digest,
                    &expected_executable_digest,
                    &mut protocol_decision_ids,
                    &mut decision_results,
                    &mut decision_rules,
                );
            }
            None => {
                assert_metric_observation(
                    &observation,
                    &payload,
                    &mut placement_sessions,
                    &mut recovery_sessions,
                );
            }
            Some(other) => panic!("unexpected protocol observation '{other}'"),
        }
    }

    assert_eq!(protocol_decision_ids.len(), 2);
    assert_eq!(
        decision_results,
        BTreeSet::from(["750".to_owned(), "850".to_owned()])
    );
    assert_eq!(
        decision_rules,
        BTreeSet::from(["high-pressure".to_owned(), "low-pressure".to_owned()])
    );
    assert_eq!(
        placement_sessions,
        BTreeSet::from(["tetris-e2e-high".to_owned(), "tetris-e2e-low".to_owned()])
    );
    assert_eq!(
        recovery_sessions,
        BTreeSet::from(["tetris-e2e-high".to_owned()])
    );

    store.close().await;
}

fn assert_common_payload(payload: &Value, observation: &EvidenceObservation) {
    assert_eq!(
        string_attribute(payload, "/resource/attributes", "flaggo.tenant"),
        observation.authority.tenant
    );
    assert_eq!(
        string_attribute(payload, "/resource/attributes", "flaggo.application"),
        observation.authority.application
    );
    assert_eq!(
        string_attribute(payload, "/resource/attributes", "flaggo.environment"),
        observation.authority.environment
    );
    assert_eq!(
        payload.pointer("/scope/name").and_then(Value::as_str),
        Some(observation.source.instrumentation_scope.as_str())
    );
    assert_eq!(
        payload.pointer("/signal/kind").and_then(Value::as_str),
        Some(observation.source.signal.as_str())
    );
    assert_eq!(
        payload.pointer("/signal/name").and_then(Value::as_str),
        Some(observation.source.signal_name.as_str())
    );
}

fn assert_protocol_observation(
    observation: &EvidenceObservation,
    payload: &Value,
    expected_contract_digest: &str,
    expected_executable_digest: &str,
    protocol_decision_ids: &mut HashSet<String>,
    decision_results: &mut BTreeSet<String>,
    decision_rules: &mut BTreeSet<String>,
) {
    assert_eq!(observation.source.signal, EvidenceSignal::Log);
    assert_eq!(observation.source.instrumentation_scope, "@flaggo/sdk");
    assert_eq!(observation.source.signal_name, "flaggo.decision.received");
    assert_eq!(
        observation.contract_digest.as_deref(),
        Some(expected_contract_digest)
    );
    let decision_id = observation
        .decision_id
        .as_deref()
        .expect("protocol observation decision ID");
    assert!(protocol_decision_ids.insert(decision_id.to_owned()));
    assert_eq!(
        string_attribute(payload, "/signal/payload/attributes", "flaggo.decision.id"),
        decision_id
    );
    assert_eq!(
        string_attribute(
            payload,
            "/signal/payload/attributes",
            "flaggo.contract.name"
        ),
        "tetris.dropInterval"
    );
    assert_eq!(
        string_attribute(
            payload,
            "/signal/payload/attributes",
            "flaggo.contract.digest"
        ),
        expected_contract_digest
    );
    assert_eq!(
        string_attribute(
            payload,
            "/signal/payload/attributes",
            "flaggo.executable.digest"
        ),
        expected_executable_digest
    );
    decision_results.insert(string_attribute(
        payload,
        "/signal/payload/attributes",
        "flaggo.result.json",
    ));
    decision_rules.insert(string_attribute(
        payload,
        "/signal/payload/attributes",
        "flaggo.evaluation.rule",
    ));
}

fn assert_metric_observation(
    observation: &EvidenceObservation,
    payload: &Value,
    placement_sessions: &mut BTreeSet<String>,
    recovery_sessions: &mut BTreeSet<String>,
) {
    assert_eq!(observation.source.signal, EvidenceSignal::Metric);
    assert_eq!(observation.source.instrumentation_scope, "tetris.engine");
    assert_eq!(observation.protocol_kind, None);
    assert_eq!(observation.decision_id, None);
    assert_eq!(observation.contract_digest, None);
    let session_id = string_attribute(
        payload,
        "/signal/payload/dataPoint/attributes",
        "tetris.session.id",
    );
    match observation.source.signal_name.as_str() {
        "tetris.placement_time" => {
            assert_eq!(observation.source.metric_kind.as_deref(), Some("histogram"));
            assert_eq!(observation.source.metric_unit.as_deref(), Some("ms"));
            placement_sessions.insert(session_id);
        }
        "tetris.recovery_failure" => {
            assert_eq!(observation.source.metric_kind.as_deref(), Some("sum"));
            assert_eq!(observation.source.metric_unit.as_deref(), Some("{failure}"));
            recovery_sessions.insert(session_id);
        }
        other => panic!("unselected metric '{other}' was persisted as evidence"),
    }
}

fn string_attribute(payload: &Value, pointer: &str, key: &str) -> String {
    let attributes = payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing attribute collection at '{pointer}'"));
    let matches = attributes
        .iter()
        .filter(|attribute| attribute.get("key").and_then(Value::as_str) == Some(key))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "attribute '{key}' occurrence count");
    assert_eq!(
        matches[0].pointer("/value/type").and_then(Value::as_str),
        Some("string")
    );
    matches[0]
        .pointer("/value/value")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("attribute '{key}' is not a string"))
        .to_owned()
}

fn required_environment(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set by the Tetris E2E harness"))
}
