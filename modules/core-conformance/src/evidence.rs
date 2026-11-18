//! Semantic inbox, projection and negotiated-query outcomes. Adapter fixtures
//! own fault injection, fresh clients and dialect-specific representative SQL.
use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use flaggo_evidence_store::*;
use flaggo_raw_otlp_inbox::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const FIXTURE_REVISION: &str = "flaggo.evidence-conformance/v1";

#[async_trait]
pub trait InboxFixture {
    fn inbox(&self) -> Arc<dyn RawOtlpInbox>;
    async fn fresh_inbox_client(&self) -> Arc<dyn RawOtlpInbox>;
    async fn fail_append(&self, enabled: bool);
}

pub async fn c08_durable_acceptance(fixture: &impl InboxFixture) {
    let inbox = fixture.inbox();
    let batch = NewRawOtlpBatch {
        signal: OtlpSignal::Logs,
        wire_encoding: OtlpWireEncoding::ProtobufJson,
        transport_compression: OtlpTransportCompression::Identity,
        otlp_profile_version: DEFAULT_OTLP_PROFILE_VERSION.parse().unwrap(),
        payload: br#"{"resourceLogs":[]}"#.to_vec(),
    };
    let first = inbox.append(batch.clone()).await.unwrap();
    fixture.fail_append(true).await;
    assert!(
        inbox.append(batch.clone()).await.is_err(),
        "failed acceptance cannot return a receipt"
    );
    assert_eq!(inbox.inspect().await.unwrap().retained_batch_count, 1);
    fixture.fail_append(false).await;
    let second = inbox.append(batch.clone()).await.unwrap();
    assert!(second.inbox_batch_id > first.inbox_batch_id);
    let fresh = fixture.fresh_inbox_client().await;
    let rows = fresh
        .read_after(None, NonZeroU16::new(10).unwrap())
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        2,
        "transport duplicates remain separate durable batches"
    );
    for (row, receipt) in rows.iter().zip([first, second]) {
        assert_eq!(row.payload, batch.payload);
        assert_eq!(row.inbox_batch_id, receipt.inbox_batch_id);
        assert_eq!(row.payload_sha256, receipt.payload_sha256);
        assert_eq!(row.received_at, receipt.received_at);
        assert_eq!(row.payload_length, receipt.payload_length);
    }
}

#[async_trait]
pub trait EvidenceFixture {
    fn writer(&self) -> Arc<dyn EvidenceStore>;
    async fn fresh_writer(&self) -> Arc<dyn EvidenceStore>;
    fn reader(&self) -> Arc<dyn EvidenceAnalysisStore>;
    async fn fail_projection_commit(&self, enabled: bool);
    fn queries(&self) -> QueryCases;
}

/// Equivalent analysis questions, not a promise of portable SQL text.
pub struct QueryCases {
    pub ordered_ids: &'static str,
    pub empty: &'static str,
    pub aggregate: &'static str,
    pub typed_values: &'static str,
    pub forbidden: &'static [&'static str],
    pub corpus: QueryCorpus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCorpus {
    pub profile: String,
    pub cases: Vec<QueryCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCase {
    pub name: String,
    pub sql: String,
    pub expect: QueryExpectation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryExpectation {
    Rows(Vec<BTreeMap<String, EvidenceValue>>),
    InvalidQuery,
}

pub fn scope() -> EvidenceQueryScope {
    EvidenceQueryScope {
        authority: AuthorityScope::new("local".into(), "conformance".into(), "test".into())
            .unwrap(),
        contract_digest: digest(b"contract"),
        sources: vec![EvidenceSourceSelector {
            signal: EvidenceSignal::Log,
            instrumentation_scope: "conformance".into(),
            signal_name: "sample".into(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        }],
    }
}

pub fn observation(key: &str, payload: &[u8]) -> EvidenceObservation {
    EvidenceObservation {
        observation_id: digest(format!("observation:{key}").as_bytes()),
        logical_source_id: digest(format!("logical:{key}").as_bytes()),
        content_digest: digest(payload),
        authority: scope().authority,
        source: SourceKey {
            signal: EvidenceSignal::Log,
            instrumentation_scope: "conformance".into(),
            signal_name: "sample".into(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        },
        observed_at_unix_nano: 100,
        observed_time_source: "timeUnixNano".into(),
        protocol_kind: None,
        decision_id: None,
        contract_digest: Some(scope().contract_digest),
        payload_json: payload.to_vec(),
        versions: MaterializerVersions {
            materializer: "2".into(),
            decoder: "1".into(),
            identity: "2".into(),
            projection: "2".into(),
            routing: "1".into(),
        },
    }
}

pub fn commit(batch: u64, observations: Vec<EvidenceObservation>) -> EvidenceMaterializationCommit {
    EvidenceMaterializationCommit {
        key: ForwardMaterializationKey {
            versions: observations[0].versions.clone(),
        },
        inbox_batch_id: batch,
        observations: observations
            .into_iter()
            .enumerate()
            .map(|(index, observation)| EvidenceObservationWrite {
                observation,
                candidate_ordinal: u32::try_from(index).unwrap(),
            })
            .collect(),
        diagnostics: vec![],
    }
}

pub async fn c09_atomic_projection(fixture: &impl EvidenceFixture) {
    let writer = fixture.writer();
    let original = observation("first", br#"{"value":1}"#);
    let first = commit(1, vec![original.clone()]);
    assert_eq!(
        writer
            .commit(first.clone())
            .await
            .unwrap()
            .observations_created,
        1
    );
    assert!(writer.commit(first).await.unwrap().already_checkpointed);
    fixture.fail_projection_commit(true).await;
    let second = commit(2, vec![observation("second", br#"{"value":2}"#)]);
    assert!(writer.commit(second.clone()).await.is_err());
    let fresh = fixture.fresh_writer().await;
    assert_eq!(fresh.forward_checkpoint().await.unwrap(), Some(1));
    assert_eq!(
        fresh.inspect().await.unwrap().observation_count,
        1,
        "failed checkpoint cannot leak projections"
    );
    fixture.fail_projection_commit(false).await;
    assert_eq!(writer.commit(second).await.unwrap().observations_created, 1);
    let duplicate = writer
        .commit(commit(3, vec![original.clone()]))
        .await
        .unwrap();
    assert_eq!(duplicate.observations_created, 0);
    assert_eq!(duplicate.duplicate_observations, 1);
    let mut conflicting = observation("conflicting", br#"{"value":9}"#);
    conflicting.logical_source_id = original.logical_source_id;
    let conflict = writer.commit(commit(4, vec![conflicting])).await.unwrap();
    assert_eq!(conflict.observations_created, 0);
    assert_eq!(conflict.conflicts_created, 1);
    assert_eq!(conflict.diagnostics_created, 1);
    assert_eq!(fresh.forward_checkpoint().await.unwrap(), Some(4));
    assert_eq!(fresh.inspect().await.unwrap().observation_count, 2);
}

fn request(reader: &dyn EvidenceAnalysisStore, sql: &str) -> EvidenceQueryRequest {
    EvidenceQueryRequest {
        profile: reader.query_capabilities().profile.to_owned(),
        sql: sql.to_owned(),
        limits: EvidenceQueryLimits {
            max_rows: NonZeroU32::new(10).unwrap(),
            max_bytes: 65_536,
            timeout: Duration::from_secs(2),
        },
    }
}

pub async fn c11_negotiated_queries(fixture: &impl EvidenceFixture) {
    let writer = fixture.writer();
    let mut unbound = observation("unbound", br#"{"value":3,"numeric":"3"}"#);
    unbound.contract_digest = None;
    unbound.observed_at_unix_nano = 200;
    let mut foreign_scope = observation(
        "foreign-scope",
        br#"{"value":1,"numeric":"1","bits":"7ff8000000000042"}"#,
    );
    foreign_scope.authority.tenant = "other".into();
    let mut foreign_unbound = observation("foreign-unbound", br#"{"value":3,"numeric":"3"}"#);
    foreign_unbound.authority.tenant = "other".into();
    foreign_unbound.contract_digest = None;
    foreign_unbound.observed_at_unix_nano = 200;
    let mut foreign_source = observation("foreign-source", b"{}");
    foreign_source.source.signal_name = "other".into();
    let mut foreign_contract = observation("foreign-contract", b"{}");
    foreign_contract.contract_digest = Some(digest(b"other-contract"));
    writer
        .commit(commit(
            1,
            vec![
                observation(
                    "exact",
                    br#"{"value":1,"numeric":"1","bits":"7ff8000000000042"}"#,
                ),
                unbound,
                foreign_scope,
                foreign_unbound,
                foreign_source,
                foreign_contract,
            ],
        ))
        .await
        .unwrap();
    let reader = fixture.reader();
    let cases = fixture.queries();
    let capabilities = reader.query_capabilities();
    assert_eq!(capabilities.result_format, EVIDENCE_RESULT_FORMAT);
    assert_eq!(capabilities.value_format, EVIDENCE_VALUE_FORMAT);
    let query = request(&*reader, cases.ordered_ids);
    let mut unsupported = query.clone();
    unsupported.profile = "unsupported/v1".into();
    assert!(matches!(
        reader.query(&scope(), unsupported).await,
        Err(EvidenceStoreError::InvalidQuery(_))
    ));
    let rows = reader.query(&scope(), query.clone()).await.unwrap();
    assert_eq!(
        rows.rows.len(),
        2,
        "scope, source, exact-or-null contract filter"
    );
    assert_eq!(rows.observation_ids.len(), 2);
    assert_eq!(rows.profile, capabilities.profile);
    let mut bounded = query.clone();
    bounded.limits.max_rows = NonZeroU32::new(1).unwrap();
    let truncated = reader.query(&scope(), bounded).await.unwrap();
    assert_eq!(truncated.rows.len(), 1);
    assert!(truncated.truncated);
    assert_eq!(
        truncated.observation_ids.len(),
        1,
        "do not claim IDs outside the saved projection"
    );
    let empty = reader
        .query(&scope(), request(&*reader, cases.empty))
        .await
        .unwrap();
    assert!(empty.rows.is_empty() && !empty.truncated);
    assert!(
        !empty.columns.is_empty(),
        "empty results retain prepared metadata"
    );
    let aggregate = reader
        .query(&scope(), request(&*reader, cases.aggregate))
        .await
        .unwrap();
    assert_eq!(
        aggregate.rows[0]["samples"],
        EvidenceValue::Int64 { value: 2 }
    );
    assert!(
        aggregate.observation_ids.is_empty(),
        "aggregate input lineage is not projected"
    );
    let values = reader
        .query(&scope(), request(&*reader, cases.typed_values))
        .await
        .unwrap();
    assert_eq!(
        values.rows[0]["largest"],
        EvidenceValue::Int64 { value: i64::MAX }
    );
    assert_eq!(
        values.rows[0]["smallest"],
        EvidenceValue::Int64 { value: i64::MIN }
    );
    assert_eq!(
        values.rows[0]["real_value"],
        EvidenceValue::Float64 {
            bits: 1.0_f64.to_bits()
        }
    );
    assert_eq!(
        values.rows[0]["text_value"],
        EvidenceValue::Text { value: "1".into() }
    );
    assert_eq!(values.rows[0]["missing"], EvidenceValue::Null {});
    assert_eq!(cases.corpus.profile, capabilities.profile);
    let required = BTreeSet::from([
        "source-time-filter",
        "ordered-projection",
        "grouped-count",
        "json-extraction",
        "named-cte",
        "empty-with-types",
        "int64-extremes",
        "numeric-looking-text",
        "finite-non-finite",
        "null-missing-json",
        "division-overflow-collation",
        "duplicate-aliases",
    ]);
    let advertised: BTreeSet<_> = cases
        .corpus
        .cases
        .iter()
        .map(|case| case.name.as_str())
        .collect();
    assert!(
        required.is_subset(&advertised),
        "profile corpus must cover every required scenario"
    );
    assert_eq!(
        advertised.len(),
        cases.corpus.cases.len(),
        "case names must be unique"
    );
    let mut other_scope = scope();
    other_scope.authority.tenant = "other".into();
    for selected_scope in [scope(), other_scope] {
        for case in &cases.corpus.cases {
            let actual = reader
                .query(&selected_scope, request(&*reader, &case.sql))
                .await;
            match &case.expect {
                QueryExpectation::Rows(rows) => {
                    let result = actual.unwrap_or_else(|error| panic!("{}: {error}", case.name));
                    assert_eq!(&result.rows, rows, "profile case {}", case.name);
                    assert!(
                        !result.truncated,
                        "profile case {} must be complete",
                        case.name
                    );
                    assert!(
                        !result.columns.is_empty(),
                        "profile case {} retains metadata",
                        case.name
                    );
                }
                QueryExpectation::InvalidQuery => assert!(
                    matches!(actual, Err(EvidenceStoreError::InvalidQuery(_))),
                    "profile case {} must fail explicitly",
                    case.name
                ),
            }
        }
    }
    let mut bytes = query.clone();
    bytes.limits.max_bytes = 1;
    assert!(matches!(
        reader.query(&scope(), bytes).await,
        Err(EvidenceStoreError::QueryLimit("byte"))
    ));
    for sql in cases.forbidden {
        assert!(matches!(
            reader.query(&scope(), request(&*reader, sql)).await,
            Err(EvidenceStoreError::InvalidQuery(_))
        ));
    }
    let mut late = observation("late", b"{}");
    late.observed_at_unix_nano = 1;
    writer.commit(commit(2, vec![late])).await.unwrap();
    let refreshed = reader.query(&scope(), query).await.unwrap();
    assert_eq!(
        refreshed.rows.len(),
        3,
        "live reads include late committed evidence"
    );
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
