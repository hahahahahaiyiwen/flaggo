use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    num::NonZeroU16,
};

use flaggo_evidence_materializer::{
    DECODER_VERSION, IDENTITY_VERSION, MATERIALIZER_VERSION, PROJECTION_VERSION, ROUTING_VERSION,
};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceObservation, EvidenceSignal, EvidenceStore, SqliteEvidenceStore,
};
use flaggo_raw_otlp_inbox::{
    OtlpTransportCompression, OtlpWireEncoding, RawOtlpInbox, RawOtlpInboxBatch, SqliteRawOtlpInbox,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_EVIDENCE_DATABASE_URL";
const CONTRACT_DIGEST_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_CONTRACT_DIGEST";
const EXECUTABLE_DIGEST_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_EXECUTABLE_DIGEST";
const EXPECTATIONS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_TETRIS_EVIDENCE_EXPECTATIONS";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedEvidence {
    decision_count: usize,
    episodes: Vec<ExpectedEpisode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedEpisode {
    session_id: String,
    dataset: String,
    totals: EpisodeTotals,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpisodeTotals {
    active_play_ms: f64,
    balanced_play_ms: f64,
    lines_cleared: f64,
    game_overs: f64,
}

struct DecisionReceipt {
    session_id: String,
    exposure_id: String,
    interval_ms: f64,
}

struct ExperienceWindow {
    attributes: Value,
    values: BTreeMap<String, f64>,
}

#[tokio::test]
#[ignore = "requires the database produced by the Tetris real-host integration"]
async fn persisted_tetris_evidence_is_semantically_correct() {
    let database_url = required_environment(DATABASE_URL_ENVIRONMENT_VARIABLE);
    let expected_contract_digest = required_environment(CONTRACT_DIGEST_ENVIRONMENT_VARIABLE);
    let expected_executable_digest = required_environment(EXECUTABLE_DIGEST_ENVIRONMENT_VARIABLE);
    let expected: ExpectedEvidence =
        serde_json::from_str(&required_environment(EXPECTATIONS_ENVIRONMENT_VARIABLE))
            .expect("real episode expectations");
    assert_live_transport_conformance(&database_url).await;
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
            NonZeroU16::new(
                u16::try_from(health.observation_count)
                    .expect("fixture fits the bounded store API"),
            )
            .expect("fixture has observations"),
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
    let mut receipts = BTreeMap::new();
    let mut windows = BTreeMap::new();

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
                let receipt = assert_protocol_observation(
                    &observation,
                    &payload,
                    &expected_contract_digest,
                    &expected_executable_digest,
                    &mut protocol_decision_ids,
                    &mut decision_results,
                    &mut decision_rules,
                );
                assert!(
                    receipts
                        .insert(
                            (receipt.session_id, receipt.exposure_id),
                            receipt.interval_ms,
                        )
                        .is_none(),
                    "one SDK receipt per prospective exposure"
                );
            }
            None => {
                assert_metric_observation(&observation, &payload, &mut windows);
            }
            Some(other) => panic!("unexpected protocol observation '{other}'"),
        }
    }

    assert_eq!(protocol_decision_ids.len(), expected.decision_count);
    assert_eq!(
        decision_results,
        BTreeSet::from(["200".to_owned(), "800".to_owned(), "1000".to_owned()])
    );
    assert_eq!(
        decision_rules,
        BTreeSet::from([
            "retain-current-without-observations".to_owned(),
            "explore-fast-gravity".to_owned(),
            "explore-slow-gravity".to_owned(),
        ])
    );
    assert_experience_windows(&windows, &receipts, &expected);

    let other_authority = AuthorityScope::new(
        "local".to_owned(),
        "tetris".to_owned(),
        "playground".to_owned(),
    )
    .expect("other example authority");
    assert!(
        store
            .list_observations(
                &other_authority,
                NonZeroU16::new(1).expect("nonzero isolation limit"),
            )
            .await
            .expect("authority isolation")
            .is_empty()
    );

    store.close().await;
}

async fn assert_live_transport_conformance(database_url: &str) {
    let inbox = SqliteRawOtlpInbox::connect_reader(database_url)
        .await
        .expect("Tetris Raw OTLP Inbox");
    let batches = inbox
        .read_after(
            None,
            NonZeroU16::new(100).expect("nonzero inbox read limit"),
        )
        .await
        .expect("retained Tetris OTLP batches");
    let conformance = batches
        .iter()
        .filter(|batch| batch.wire_encoding == OtlpWireEncoding::Protobuf)
        .collect::<Vec<_>>();

    assert_eq!(conformance.len(), 6);
    assert!(conformance.iter().all(|batch| {
        batch.media_type == "application/x-protobuf"
            && batch.payload == [0x0a, 0x00]
            && batch.payload_length == 2
    }));
    assert_eq!(
        transport_matrix(&conformance),
        BTreeSet::from([
            ("logs", "gzip"),
            ("logs", "identity"),
            ("metrics", "gzip"),
            ("metrics", "identity"),
            ("traces", "gzip"),
            ("traces", "identity"),
        ])
    );
    inbox.close().await;
}

fn transport_matrix<'a>(batches: &'a [&RawOtlpInboxBatch]) -> BTreeSet<(&'a str, &'a str)> {
    batches
        .iter()
        .map(|batch| {
            let compression = match batch.transport_compression {
                OtlpTransportCompression::Identity => "identity",
                OtlpTransportCompression::Gzip => "gzip",
            };
            (batch.signal.as_str(), compression)
        })
        .collect()
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
) -> DecisionReceipt {
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
    DecisionReceipt {
        session_id: string_attribute(
            payload,
            "/signal/payload/attributes",
            "flaggo.correlation.session_id",
        ),
        exposure_id: string_attribute(
            payload,
            "/signal/payload/attributes",
            "flaggo.correlation.exposure_id",
        ),
        interval_ms: string_attribute(payload, "/signal/payload/attributes", "flaggo.result.json")
            .parse()
            .expect("numeric drop interval"),
    }
}

fn assert_metric_observation(
    observation: &EvidenceObservation,
    payload: &Value,
    windows: &mut BTreeMap<String, ExperienceWindow>,
) {
    assert_eq!(observation.source.signal, EvidenceSignal::Metric);
    assert_eq!(
        observation.source.instrumentation_scope,
        "tetris.experience"
    );
    assert_eq!(observation.source.metric_kind.as_deref(), Some("gauge"));
    assert_eq!(observation.protocol_kind, None);
    assert_eq!(observation.decision_id, None);
    assert_eq!(observation.contract_digest, None);
    let window_id = string_attribute(
        payload,
        "/signal/payload/dataPoint/attributes",
        "tetris.experience.window.id",
    );
    let unit = match observation.source.signal_name.as_str() {
        "tetris.experience.challenge_balance" => "1",
        "tetris.experience.lines_cleared" => "{line}",
        "tetris.experience.game_over_count" => "{game}",
        "tetris.experience.active_play_ms" => "ms",
        other => panic!("unselected metric '{other}' was persisted as evidence"),
    };
    assert_eq!(observation.source.metric_unit.as_deref(), Some(unit));
    let attributes = payload
        .pointer("/signal/payload/dataPoint/attributes")
        .expect("experience attributes");
    let window = windows
        .entry(window_id)
        .or_insert_with(|| ExperienceWindow {
            attributes: attributes.clone(),
            values: BTreeMap::new(),
        });
    assert_eq!(&window.attributes, attributes, "immutable window metadata");
    let value = payload
        .pointer("/signal/payload/dataPoint/value")
        .expect("gauge point value");
    let number = value
        .get("value")
        .and_then(Value::as_f64)
        .or_else(|| value.get("int64").and_then(Value::as_f64))
        .expect("finite gauge value");
    assert!(number.is_finite());
    if let Some(previous) = window
        .values
        .insert(observation.source.signal_name.clone(), number)
    {
        assert_eq!(previous, number, "immutable re-exported value");
    }
}

fn assert_experience_windows(
    windows: &BTreeMap<String, ExperienceWindow>,
    receipts: &BTreeMap<(String, String), f64>,
    expected: &ExpectedEvidence,
) {
    let mut totals = BTreeMap::<String, EpisodeTotals>::new();
    let mut completions = BTreeMap::<String, usize>::new();
    let mut intervals = BTreeMap::<String, Vec<(f64, f64)>>::new();
    for window in windows.values() {
        let payload = serde_json::json!({ "attributes": window.attributes });
        let session = string_attribute(&payload, "/attributes", "tetris.session.id");
        let exposure = string_attribute(&payload, "/attributes", "tetris.exposure.id");
        let episode = expected
            .episodes
            .iter()
            .find(|episode| episode.session_id == session)
            .expect("expected autonomous episode");
        assert_eq!(
            string_attribute(&payload, "/attributes", "tetris.experience.dataset"),
            episode.dataset
        );
        assert_eq!(
            string_attribute(&payload, "/attributes", "tetris.experience.clock"),
            "virtual"
        );
        assert_eq!(window.values.len(), 4, "complete four-gauge window");
        let start = number_attribute(&payload, "tetris.experience.window.start_ms");
        let end = number_attribute(&payload, "tetris.experience.window.end_ms");
        let interval = number_attribute(&payload, "tetris.drop_interval.ms");
        assert_eq!(
            receipts.get(&(session.clone(), exposure)),
            Some(&interval),
            "outcomes join the actual SDK result through session AND exposure"
        );
        assert!((200.0..=1500.0).contains(&interval) && interval % 50.0 == 0.0);
        assert!(start >= 0.0 && start <= end && end <= 120_000.0);
        for key in [
            "tetris.input.board_pressure_mean_5s",
            "tetris.input.hard_drop_ratio_5s",
        ] {
            assert!((0.0..=1.0).contains(&number_attribute(&payload, key)));
        }
        let pieces = number_attribute(&payload, "tetris.input.pieces_locked_5s");
        assert!(pieces >= 0.0 && pieces.fract() == 0.0);
        let active = window.values["tetris.experience.active_play_ms"];
        let balance = window.values["tetris.experience.challenge_balance"];
        let lines = window.values["tetris.experience.lines_cleared"];
        let game_overs = window.values["tetris.experience.game_over_count"];
        assert!((0.0..=5000.0).contains(&active));
        assert_eq!(active, end - start, "exact active game-clock duration");
        assert!((0.0..=1.0).contains(&balance));
        assert!(lines >= 0.0 && lines.fract() == 0.0);
        assert!((0.0..=1.0).contains(&game_overs) && game_overs.fract() == 0.0);
        let completion = attribute(
            &payload,
            "/attributes",
            "tetris.experience.episode_complete",
        );
        assert_eq!(completion.get("type").and_then(Value::as_str), Some("bool"));
        let complete = completion
            .get("value")
            .and_then(Value::as_bool)
            .expect("boolean completion fact");
        if complete {
            assert_eq!(active, 0.0);
            assert_eq!(balance, 0.0);
            assert_eq!(lines, 0.0);
            assert_eq!(game_overs, 0.0);
            *completions.entry(session.clone()).or_default() += 1;
        }
        if active > 0.0 {
            intervals
                .entry(session.clone())
                .or_default()
                .push((start, end));
        } else {
            assert_eq!(balance, 0.0, "instantaneous facts add no quality weight");
        }
        let total = totals.entry(session).or_default();
        total.active_play_ms += active;
        total.balanced_play_ms += active * balance;
        total.lines_cleared += lines;
        total.game_overs += game_overs;
    }
    assert_eq!(totals.len(), expected.episodes.len());
    for episode in &expected.episodes {
        let total = &totals[&episode.session_id];
        assert_eq!(total.active_play_ms, episode.totals.active_play_ms);
        assert!(
            (total.balanced_play_ms - episode.totals.balanced_play_ms).abs() < 1e-6,
            "time-weighted balance matches actual gameplay"
        );
        assert_eq!(total.lines_cleared, episode.totals.lines_cleared);
        assert_eq!(total.game_overs, episode.totals.game_overs);
        assert_eq!(completions.get(&episode.session_id), Some(&1));
        let slices = intervals
            .get_mut(&episode.session_id)
            .expect("active outcome slices");
        slices.sort_by(|first, second| first.0.total_cmp(&second.0));
        assert!(
            slices.windows(2).all(|pair| pair[0].1 <= pair[1].0),
            "applied outcome windows must not overlap"
        );
    }
}

fn string_attribute(payload: &Value, pointer: &str, key: &str) -> String {
    let value = attribute(payload, pointer, key);
    assert_eq!(value.get("type").and_then(Value::as_str), Some("string"));
    value
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("attribute '{key}' is not a string"))
        .to_owned()
}

fn number_attribute(payload: &Value, key: &str) -> f64 {
    let attribute = attribute(payload, "/attributes", key);
    let value = attribute.get("value").expect("numeric attribute value");
    let number = match attribute.get("type").and_then(Value::as_str) {
        Some("double") => value.as_f64().expect("double attribute"),
        Some("int64") => value.as_f64().expect("integer attribute"),
        _ => panic!("attribute '{key}' is not numeric"),
    };
    assert!(number.is_finite());
    number
}

fn attribute<'a>(payload: &'a Value, pointer: &str, key: &str) -> &'a Value {
    let attributes = payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing attribute collection at '{pointer}'"));
    let matches = attributes
        .iter()
        .filter(|attribute| attribute.get("key").and_then(Value::as_str) == Some(key))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "attribute '{key}' occurrence count");
    matches[0]
        .get("value")
        .unwrap_or_else(|| panic!("attribute '{key}' has no value"))
}

fn required_environment(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set by the Tetris E2E harness"))
}
