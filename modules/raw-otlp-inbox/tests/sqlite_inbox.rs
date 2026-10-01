use std::{
    fs,
    num::NonZeroU16,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use flaggo_raw_otlp_inbox::{
    DEFAULT_OTLP_PROFILE_VERSION, InboxClock, NewRawOtlpBatch, OtlpProfileVersion, OtlpSignal,
    OtlpTransportCompression, OtlpWireEncoding, RawOtlpInbox, RawOtlpInboxError,
    RawOtlpInboxLimits, RawOtlpInboxLimitsError, SqliteRawOtlpInbox,
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tempfile::TempDir;

struct TestClock {
    current: Mutex<DateTime<Utc>>,
}

impl TestClock {
    fn new(current: DateTime<Utc>) -> Self {
        Self {
            current: Mutex::new(current),
        }
    }

    fn advance(&self, duration: TimeDelta) {
        let mut current = self.current.lock().expect("test clock lock");
        *current += duration;
    }

    fn set(&self, value: DateTime<Utc>) {
        *self.current.lock().expect("test clock lock") = value;
    }
}

impl InboxClock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.current.lock().expect("test clock lock")
    }
}

struct TestInbox {
    inbox: SqliteRawOtlpInbox,
    clock: Arc<TestClock>,
    database_url: String,
    _directory: TempDir,
}

impl TestInbox {
    async fn create(limits: RawOtlpInboxLimits) -> Self {
        let directory = tempfile::tempdir().expect("temporary inbox directory");
        let database_url = sqlite_url(&directory.path().join("inbox.db"));
        let clock = Arc::new(TestClock::new(fixed_time()));
        let inbox = SqliteRawOtlpInbox::connect_with_clock(&database_url, limits, clock.clone())
            .await
            .expect("SQLite inbox");
        Self {
            inbox,
            clock,
            database_url,
            _directory: directory,
        }
    }

    async fn close(&self) {
        self.inbox.close().await;
    }
}

#[tokio::test]
async fn round_trips_complete_batches_through_storage_neutral_contract() {
    let fixture = TestInbox::create(RawOtlpInboxLimits::default()).await;
    let contract: &dyn RawOtlpInbox = &fixture.inbox;
    let payload = br#"{"resourceLogs":[]}"#.to_vec();
    let expected_digest: [u8; 32] = Sha256::digest(&payload).into();

    let receipt = contract
        .append(batch(
            OtlpSignal::Logs,
            OtlpWireEncoding::ProtobufJson,
            OtlpTransportCompression::Gzip,
            payload.clone(),
        ))
        .await
        .expect("durable append");
    fixture.inbox.close().await;
    let reopened = SqliteRawOtlpInbox::connect_with_clock(
        &fixture.database_url,
        RawOtlpInboxLimits::default(),
        fixture.clock.clone(),
    )
    .await
    .expect("reopened SQLite inbox");
    let contract: &dyn RawOtlpInbox = &reopened;
    let stored = contract
        .read_after(None, NonZeroU16::new(10).expect("nonzero limit"))
        .await
        .expect("inbox replay");

    assert_eq!(stored.len(), 1);
    let stored = &stored[0];
    assert_eq!(stored.inbox_batch_id, receipt.inbox_batch_id);
    assert_eq!(stored.signal, OtlpSignal::Logs);
    assert_eq!(stored.wire_encoding, OtlpWireEncoding::ProtobufJson);
    assert_eq!(stored.media_type, "application/json");
    assert_eq!(stored.transport_compression, OtlpTransportCompression::Gzip);
    assert_eq!(
        stored.otlp_profile_version.as_str(),
        DEFAULT_OTLP_PROFILE_VERSION
    );
    assert_eq!(stored.received_at, fixed_time());
    assert_eq!(stored.payload_length, payload.len() as u64);
    assert_eq!(stored.payload_sha256.as_bytes(), &expected_digest);
    assert_eq!(stored.payload, payload);
    assert_eq!(receipt.payload_sha256, stored.payload_sha256);

    let health = contract.inspect().await.expect("inbox health");
    assert_eq!(health.retained_batch_count, 1);
    assert_eq!(health.retained_payload_bytes, stored.payload_length);
    assert_eq!(health.oldest_retained_at, Some(fixed_time()));
    assert_eq!(health.newest_retained_at, Some(fixed_time()));
    assert_eq!(health.earliest_replay_at, Some(fixed_time()));
    assert_eq!(health.expired_batch_count, 0);
    assert_eq!(health.expired_payload_bytes, 0);
    reopened.close().await;
}

#[tokio::test]
async fn reads_forward_from_monotonic_batch_cursor() {
    let fixture = TestInbox::create(RawOtlpInboxLimits::default()).await;
    for signal in [OtlpSignal::Logs, OtlpSignal::Metrics, OtlpSignal::Traces] {
        fixture
            .inbox
            .append(batch(
                signal,
                OtlpWireEncoding::Protobuf,
                OtlpTransportCompression::Identity,
                vec![signal as u8],
            ))
            .await
            .expect("durable append");
    }

    let first_page = fixture
        .inbox
        .read_after(None, NonZeroU16::new(2).expect("nonzero limit"))
        .await
        .expect("first page");
    let second_page = fixture
        .inbox
        .read_after(
            Some(first_page[1].inbox_batch_id),
            NonZeroU16::new(2).expect("nonzero limit"),
        )
        .await
        .expect("second page");

    assert_eq!(first_page.len(), 2);
    assert_eq!(first_page[0].signal, OtlpSignal::Logs);
    assert_eq!(first_page[1].signal, OtlpSignal::Metrics);
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].signal, OtlpSignal::Traces);
    assert!(first_page[1].inbox_batch_id < second_page[0].inbox_batch_id);
    fixture.close().await;
}

#[tokio::test]
async fn receipt_order_does_not_move_backward_when_the_clock_does() {
    let fixture = TestInbox::create(RawOtlpInboxLimits::default()).await;
    let first = fixture
        .inbox
        .append(batch(
            OtlpSignal::Logs,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![1],
        ))
        .await
        .expect("first append");
    fixture.clock.set(fixed_time() - TimeDelta::hours(1));
    let second = fixture
        .inbox
        .append(batch(
            OtlpSignal::Logs,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![2],
        ))
        .await
        .expect("second append");

    assert!(first.inbox_batch_id < second.inbox_batch_id);
    assert_eq!(first.received_at, fixed_time());
    assert_eq!(second.received_at, fixed_time());
    fixture.close().await;
}

#[tokio::test]
async fn capacity_pressure_rejects_without_evicting_nonexpired_batches() {
    let limits = RawOtlpInboxLimits::new(5, Duration::from_secs(60)).expect("valid inbox limits");
    let fixture = TestInbox::create(limits).await;
    fixture
        .inbox
        .append(batch(
            OtlpSignal::Metrics,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![1; 4],
        ))
        .await
        .expect("first append");

    let error = fixture
        .inbox
        .append(batch(
            OtlpSignal::Metrics,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![2; 2],
        ))
        .await
        .expect_err("capacity must reject the complete batch");

    assert!(matches!(
        error,
        RawOtlpInboxError::CapacityExceeded {
            retained_payload_bytes: 4,
            incoming_payload_bytes: 2,
            max_retained_payload_bytes: 5
        }
    ));
    let health = fixture.inbox.inspect().await.expect("inbox health");
    assert_eq!(health.retained_batch_count, 1);
    assert_eq!(health.retained_payload_bytes, 4);
    assert_eq!(health.expired_batch_count, 0);
    fixture.close().await;
}

#[tokio::test]
async fn hard_expiry_commits_even_when_incoming_batch_is_too_large() {
    let limits = RawOtlpInboxLimits::new(3, Duration::from_secs(10)).expect("valid inbox limits");
    let fixture = TestInbox::create(limits).await;
    fixture
        .inbox
        .append(batch(
            OtlpSignal::Traces,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![1; 3],
        ))
        .await
        .expect("initial append");
    fixture.clock.advance(TimeDelta::seconds(11));

    let error = fixture
        .inbox
        .append(batch(
            OtlpSignal::Traces,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![2; 4],
        ))
        .await
        .expect_err("oversized batch must be rejected");
    assert!(matches!(
        error,
        RawOtlpInboxError::CapacityExceeded {
            retained_payload_bytes: 0,
            incoming_payload_bytes: 4,
            max_retained_payload_bytes: 3
        }
    ));

    let health = fixture.inbox.inspect().await.expect("inbox health");
    assert_eq!(health.retained_batch_count, 0);
    assert_eq!(health.retained_payload_bytes, 0);
    assert_eq!(health.expired_batch_count, 1);
    assert_eq!(health.expired_payload_bytes, 3);
    assert_eq!(
        health.earliest_replay_at,
        Some(fixed_time() + TimeDelta::seconds(1))
    );

    fixture.clock.set(fixed_time());
    let receipt = fixture
        .inbox
        .append(batch(
            OtlpSignal::Logs,
            OtlpWireEncoding::ProtobufJson,
            OtlpTransportCompression::Identity,
            vec![3; 2],
        ))
        .await
        .expect("capacity released by hard expiry");
    assert_eq!(receipt.received_at, fixed_time() + TimeDelta::seconds(1));
    let health = fixture.inbox.inspect().await.expect("updated health");
    assert_eq!(health.retained_batch_count, 1);
    assert_eq!(health.retained_payload_bytes, 2);
    assert_eq!(health.expired_batch_count, 1);
    assert_eq!(
        health.earliest_replay_at,
        Some(fixed_time() + TimeDelta::seconds(1))
    );
    fixture.close().await;
}

#[tokio::test]
async fn concurrent_appends_serialize_capacity_decisions() {
    let limits = RawOtlpInboxLimits::new(5, Duration::from_secs(60)).expect("valid inbox limits");
    let fixture = TestInbox::create(limits).await;
    let first = fixture.inbox.append(batch(
        OtlpSignal::Logs,
        OtlpWireEncoding::Protobuf,
        OtlpTransportCompression::Identity,
        vec![1; 4],
    ));
    let second = fixture.inbox.append(batch(
        OtlpSignal::Metrics,
        OtlpWireEncoding::Protobuf,
        OtlpTransportCompression::Identity,
        vec![2; 4],
    ));

    let (first, second) = tokio::join!(first, second);
    let results = [first, second];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(RawOtlpInboxError::CapacityExceeded { .. })))
            .count(),
        1
    );
    let health = fixture.inbox.inspect().await.expect("inbox health");
    assert_eq!(health.retained_batch_count, 1);
    assert_eq!(health.retained_payload_bytes, 4);
    fixture.close().await;
}

#[tokio::test]
async fn replay_detects_payload_corruption() {
    let fixture = TestInbox::create(RawOtlpInboxLimits::default()).await;
    fixture
        .inbox
        .append(batch(
            OtlpSignal::Logs,
            OtlpWireEncoding::Protobuf,
            OtlpTransportCompression::Identity,
            vec![1, 2, 3],
        ))
        .await
        .expect("durable append");
    let pool = SqlitePool::connect(&fixture.database_url)
        .await
        .expect("independent SQLite connection");
    sqlx::query("UPDATE raw_otlp_inbox_batches SET payload = ?")
        .bind(vec![3_u8, 2, 1])
        .execute(&pool)
        .await
        .expect("corrupt stored payload");
    pool.close().await;

    let error = fixture
        .inbox
        .read_after(None, NonZeroU16::new(1).expect("nonzero limit"))
        .await
        .expect_err("corrupt payload must not replay");
    assert!(matches!(error, RawOtlpInboxError::CorruptData(_)));
    fixture.close().await;
}

#[tokio::test]
async fn rejects_an_unsupported_owned_schema_version() {
    let directory = tempfile::tempdir().expect("temporary inbox directory");
    let database_url = sqlite_url(&directory.path().join("schema.db"));
    let inbox = SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
        .await
        .expect("initial SQLite inbox");
    inbox.close().await;
    let pool = SqlitePool::connect(&database_url)
        .await
        .expect("schema mutation connection");
    sqlx::query(
        "UPDATE flaggo_schema_versions
         SET version = 99
         WHERE component = 'raw-otlp-inbox'",
    )
    .execute(&pool)
    .await
    .expect("set unsupported schema version");
    pool.close().await;

    let error = SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
        .await
        .err()
        .expect("unsupported schema version must fail");
    assert!(matches!(
        error,
        RawOtlpInboxError::UnsupportedSchemaVersion {
            component: "raw-otlp-inbox",
            found: 99,
            expected: 1
        }
    ));
}

#[tokio::test]
async fn preserves_schema_versions_owned_by_other_components() {
    let directory = tempfile::tempdir().expect("temporary inbox directory");
    let database_path = directory.path().join("shared.db");
    fs::File::create(&database_path).expect("empty shared database file");
    let database_url = sqlite_url(&database_path);
    let pool = SqlitePool::connect(&database_url)
        .await
        .expect("shared database connection");
    sqlx::query(
        "CREATE TABLE flaggo_schema_versions (
            component TEXT PRIMARY KEY,
            version INTEGER NOT NULL
         )",
    )
    .execute(&pool)
    .await
    .expect("shared schema table");
    sqlx::query(
        "INSERT INTO flaggo_schema_versions(component, version)
         VALUES ('contract-store', 7)",
    )
    .execute(&pool)
    .await
    .expect("other component schema version");
    pool.close().await;

    let inbox = SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
        .await
        .expect("SQLite inbox in shared database");
    inbox.close().await;
    let pool = SqlitePool::connect(&database_url)
        .await
        .expect("schema verification connection");
    let contract_version: i64 = sqlx::query_scalar(
        "SELECT version FROM flaggo_schema_versions WHERE component = 'contract-store'",
    )
    .fetch_one(&pool)
    .await
    .expect("other component schema version remains");
    let inbox_version: i64 = sqlx::query_scalar(
        "SELECT version FROM flaggo_schema_versions WHERE component = 'raw-otlp-inbox'",
    )
    .fetch_one(&pool)
    .await
    .expect("inbox schema version");
    pool.close().await;

    assert_eq!(contract_version, 7);
    assert_eq!(inbox_version, 1);
}

#[test]
fn rejects_invalid_limits_and_profile_versions() {
    assert_eq!(
        RawOtlpInboxLimits::new(0, Duration::from_secs(1)),
        Err(RawOtlpInboxLimitsError::InvalidMaximumPayloadBytes)
    );
    assert_eq!(
        RawOtlpInboxLimits::new(1, Duration::ZERO),
        Err(RawOtlpInboxLimitsError::InvalidHardRetention)
    );
    assert!("".parse::<OtlpProfileVersion>().is_err());
    assert!(" 1.0.0".parse::<OtlpProfileVersion>().is_err());
    assert!("1.0.0\n".parse::<OtlpProfileVersion>().is_err());
}

fn batch(
    signal: OtlpSignal,
    wire_encoding: OtlpWireEncoding,
    transport_compression: OtlpTransportCompression,
    payload: Vec<u8>,
) -> NewRawOtlpBatch {
    NewRawOtlpBatch::new(
        signal,
        wire_encoding,
        transport_compression,
        DEFAULT_OTLP_PROFILE_VERSION
            .parse()
            .expect("profile version"),
        payload,
    )
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T15:00:00Z")
        .expect("fixed test time")
        .to_utc()
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
}
