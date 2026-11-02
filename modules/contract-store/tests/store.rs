use flaggo_contract::{DecisionContract, digests, strict_json};
use flaggo_contract_store::{
    AcceptedContractVersion, ContractStoreError, ContractStoreWriteResult,
    SqliteContractVersionStore, timestamp,
};
use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, DecisionError, DecisionRuntime,
    StoreReadError,
};
use flaggo_expression::ExpressionCompiler;
use serde_json::json;
use sqlx::{Row, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

async fn fixture() -> (
    tempfile::TempDir,
    SqliteContractVersionStore,
    sqlx::SqlitePool,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let store =
        SqliteContractVersionStore::open(&path, Some(Arc::new(ExpressionCompiler::default())))
            .await
            .unwrap();
    let pool = sqlx::SqlitePool::connect_with(SqliteConnectOptions::new().filename(path))
        .await
        .unwrap();
    (directory, store, pool)
}

fn version(default: i64) -> AcceptedContractVersion {
    let contract: DecisionContract = strict_json::deserialize(
        &serde_json::to_vec(&json!({
            "authority":{"tenant":"local","application":"parity","environment":"test"},
            "name":"store.parity","expression_syntax":"flaggo.cel/v1",
            "attributes":[{"name":"x","schema":{"type":"integer","description":"original"}}],
            "result":{"schema":{"type":"integer"},"default":default},
            "authoredExecutable":{"rules":[{"name":"r","when":{"expression":" attributes.x>=1 "},
                "return":{"expression":"attributes.x + 2"}}]}
        }))
        .unwrap(),
    )
    .unwrap();
    AcceptedContractVersion {
        contract_digest: digests::contract_digest(&contract, Some(&ExpressionCompiler::default()))
            .unwrap(),
        accepted_at: timestamp::parse_exact("2026-03-01T12:00:00.1234567+02:00").unwrap(),
        contract,
    }
}

#[tokio::test]
async fn immutable_canonical_versions_persist_with_exact_timestamps_and_schema() {
    let (directory, store, pool) = fixture().await;
    assert!(!store.is_available(None).await.unwrap());
    store.initialize(None).await.unwrap();
    store.initialize(None).await.unwrap();
    let first = version(99);
    assert_eq!(
        store.put(&first, None).await.unwrap(),
        ContractStoreWriteResult::Created
    );
    let stored_bytes: Vec<u8> =
        sqlx::query_scalar("SELECT contract_json FROM decision_contract_versions")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut duplicate = first.clone();
    duplicate.accepted_at = timestamp::parse_exact("2026-03-02T10:00:00.0000000Z").unwrap();
    duplicate.contract.attributes[0]
        .as_mut()
        .unwrap()
        .schema
        .description = Some("changed".into());
    assert_eq!(
        store.put(&duplicate, None).await.unwrap(),
        ContractStoreWriteResult::Existing
    );
    assert_eq!(
        stored_bytes,
        sqlx::query_scalar::<_, Vec<u8>>("SELECT contract_json FROM decision_contract_versions")
            .fetch_one(&pool)
            .await
            .unwrap()
    );
    let persisted_time: String =
        sqlx::query_scalar("SELECT accepted_at FROM decision_contract_versions")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted_time, "2026-03-01T10:00:00.1234567+00:00");
    store.close().await;
    let restarted = SqliteContractVersionStore::open(directory.path().join("state.db"), None)
        .await
        .unwrap();
    let stored = restarted
        .get("store.parity", &first.contract_digest, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.accepted_at, first.accepted_at);
    assert_eq!(
        stored.contract.authored_executable.as_ref().unwrap().rules[0]
            .as_ref()
            .unwrap()
            .when,
        flaggo_contract::RuleWhen::Expression("attributes.x >= 1".into())
    );
    assert_eq!(
        digests::contract_digest(&stored.contract, None).unwrap(),
        first.contract_digest
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "wal"
    );
    restarted.close().await;
    pool.close().await;
}

#[tokio::test]
async fn concurrent_authority_claims_have_one_winner_and_no_partial_version() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let first = version(99);
    let mut second = first.clone();
    second.contract.authority.tenant = Some("other".into());
    second.contract_digest =
        digests::contract_digest(&second.contract, Some(&ExpressionCompiler::default())).unwrap();
    let (left, right) = tokio::join!(store.put(&first, None), store.put(&second, None));
    let outcomes = [left, right];
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Ok(ContractStoreWriteResult::Created)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ContractStoreError::AuthorityConflict { .. })))
            .count(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM decision_contract_versions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM decision_contract_resources")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn history_cursor_matches_tied_timestamp_order_and_missing_selection_is_atomic() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let versions = [version(1), version(2), version(3)];
    for version in &versions {
        store.put(version, None).await.unwrap();
    }
    assert!(
        store
            .get_current("store.parity", None)
            .await
            .unwrap()
            .is_none()
    );
    store
        .set_current("store.parity", &versions[0].contract_digest, None)
        .await
        .unwrap();
    let missing = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        store.set_current("store.parity", &missing, None).await,
        Err(ContractStoreError::NotFound { .. })
    ));
    assert_eq!(
        store
            .get_current("store.parity", None)
            .await
            .unwrap()
            .unwrap()
            .contract_digest,
        versions[0].contract_digest
    );
    let first = store.list("store.parity", 2, None, None).await.unwrap();
    let second = store
        .list("store.parity", 2, first.next_cursor.as_deref(), None)
        .await
        .unwrap();
    assert_eq!(first.versions.len(), 2);
    assert_eq!(second.versions.len(), 1);
    assert!(second.next_cursor.is_none());
    let mut expected: Vec<_> = versions
        .iter()
        .map(|version| version.contract_digest.clone())
        .collect();
    expected.sort_by(|left, right| right.cmp(left));
    assert_eq!(
        first
            .versions
            .iter()
            .chain(&second.versions)
            .map(|version| version.contract_digest.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(store.list_all_current(None).await.unwrap().len(), 1);
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn rejected_and_cancelled_writes_do_not_claim_a_name() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let mut bad = version(99);
    bad.contract_digest = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        store.put(&bad, None).await,
        Err(ContractStoreError::InvalidArgument { .. })
    ));
    let cancel = Arc::new(AtomicBool::new(true));
    assert!(matches!(
        store.put(&version(99), Some(cancel)).await,
        Err(ContractStoreError::Cancelled)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM decision_contract_resources")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn concurrent_current_updates_serialize_without_snapshot_upgrade_failures() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let first = version(1);
    let second = version(2);
    store.put(&first, None).await.unwrap();
    store.put(&second, None).await.unwrap();
    for _ in 0..16 {
        let (left, right) = tokio::join!(
            store.set_current("store.parity", &first.contract_digest, None),
            store.set_current("store.parity", &second.contract_digest, None),
        );
        left.unwrap();
        right.unwrap();
        let current = store
            .get_current("store.parity", None)
            .await
            .unwrap()
            .unwrap();
        assert!(
            [
                first.contract_digest.as_str(),
                second.contract_digest.as_str()
            ]
            .contains(&current.contract_digest.as_str())
        );
    }
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn cancellation_while_waiting_for_a_writer_rolls_back_the_later_claim() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let blocker = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = {
        let store = store.clone();
        let cancel = Arc::clone(&cancel);
        tokio::spawn(async move { store.put(&version(99), Some(cancel)).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.store(true, Ordering::Release);
    blocker.rollback().await.unwrap();
    assert!(matches!(
        worker.await.unwrap(),
        Err(ContractStoreError::Cancelled)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM decision_contract_resources")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    store.close().await;
    pool.close().await;
}

struct NoActive;
impl ActiveExecutableReader for NoActive {
    async fn get_active(
        &self,
        _: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        Ok(None)
    }
}

#[tokio::test]
async fn runtime_reader_distinguishes_stored_integrity_from_native_json_and_scope_errors() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    let version = version(99);
    store.put(&version, None).await.unwrap();
    let runtime = DecisionRuntime::new(store.clone(), NoActive);
    let input = strict_json::deserialize(br#"{"attributes":{"_random":0.5}}"#).unwrap();
    assert!(matches!(
        runtime
            .decide("store.parity", &version.contract_digest, &input, None)
            .await,
        Err(DecisionError::ActiveExecutableNotFound(_))
    ));
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT contract_json FROM decision_contract_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut changed = strict_json::deserialize::<DecisionContract>(&bytes).unwrap();
    changed.name = "wrong.name".into();
    sqlx::query("UPDATE decision_contract_versions SET contract_json=?")
        .bind(strict_json::serialize(&changed).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .decide("store.parity", &version.contract_digest, &input, None)
            .await,
        Err(DecisionError::Integrity { .. })
    ));
    sqlx::query("UPDATE decision_contract_versions SET contract_json=?")
        .bind(b"{".as_slice())
        .execute(&pool)
        .await
        .unwrap();
    let error = runtime
        .decide("store.parity", &version.contract_digest, &input, None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DecisionError::Store(StoreReadError::Native(_))
    ));
    assert_eq!(error.telemetry_classification(), ("failure", "internal"));
    changed.name = version.contract.name;
    changed.authority.tenant = Some(" ".into());
    sqlx::query("UPDATE decision_contract_versions SET contract_json=?")
        .bind(strict_json::serialize(&changed).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let error = runtime
        .decide("store.parity", &version.contract_digest, &input, None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DecisionError::Store(StoreReadError::InvalidArgument(_))
    ));
    assert_eq!(error.telemetry_classification(), ("invalid", "validation"));
    let overflow = String::from_utf8(bytes).unwrap().replace(
        "\"type\":\"integer\"",
        "\"type\":\"integer\",\"minimum\":1e400",
    );
    sqlx::query("UPDATE decision_contract_versions SET contract_json=?")
        .bind(overflow.into_bytes())
        .execute(&pool)
        .await
        .unwrap();
    let error = runtime
        .decide("store.parity", &version.contract_digest, &input, None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DecisionError::Store(StoreReadError::InvalidArgument(_))
    ));
    assert_eq!(error.telemetry_classification(), ("invalid", "validation"));
    store.close().await;
    pool.close().await;
}

#[tokio::test]
async fn schema_validation_preserves_other_components_and_does_not_rewrite_versions() {
    let (_directory, store, pool) = fixture().await;
    store.initialize(None).await.unwrap();
    sqlx::query("INSERT INTO flaggo_schema_versions VALUES('executable-store', 5)")
        .execute(&pool)
        .await
        .unwrap();
    store.put(&version(99), None).await.unwrap();
    let before = sqlx::query("SELECT accepted_at, contract_json FROM decision_contract_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE flaggo_schema_versions SET version=4 WHERE component='contract-store'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store.is_available(None).await.unwrap());
    assert!(matches!(
        store.initialize(None).await,
        Err(ContractStoreError::UnsupportedSchema(4))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT version FROM flaggo_schema_versions WHERE component='executable-store'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        5
    );
    let after = sqlx::query("SELECT accepted_at, contract_json FROM decision_contract_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before.get::<String, _>("accepted_at"),
        after.get::<String, _>("accepted_at")
    );
    assert_eq!(
        before.get::<Vec<u8>, _>("contract_json"),
        after.get::<Vec<u8>, _>("contract_json")
    );
    store.close().await;
    pool.close().await;
}

#[test]
fn timestamp_boundaries_match_100_nanosecond_datetimeoffset() {
    let time = timestamp::parse_exact("2026-03-01T12:00:00.1234567+02:00").unwrap();
    assert_eq!(
        timestamp::format_exact(time).unwrap(),
        "2026-03-01T10:00:00.1234567+00:00"
    );
    assert!(timestamp::parse_exact("2026-03-01T10:00:00.1234567Z").is_ok());
    for invalid in [
        "0000-01-01T00:00:00.0000000Z",
        "2026-03-01T10:00:00.12345678Z",
        "2026-03-01T10:00:60.0000000Z",
        "2026-03-01T10:00:00.0000000+14:01",
        "0001-01-01T00:00:00.0000000+14:00",
    ] {
        assert!(timestamp::parse_exact(invalid).is_err(), "{invalid}");
    }
}
