use flaggo_contract::{DecisionContract, DecisionExecutable, digests, strict_json};
use flaggo_contract_store::{AcceptedContractVersion, SqliteContractVersionStore, timestamp};
use flaggo_decision::{ActiveExecutableReader, DecisionRuntime, StoreReadError};
use flaggo_executable_store::*;
use flaggo_expression::ExecutableCompiler;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tempfile::TempDir;

struct Fixture {
    directory: TempDir,
    store: SqliteExecutableStore,
    contracts: SqliteContractVersionStore,
    pool: SqlitePool,
    contract: AcceptedContractVersion,
}

#[tokio::test]
async fn competing_manual_activations_have_one_winner_and_supersede_the_loser() {
    let fixture = Fixture::new().await;
    let initial = fixture.candidate(1);
    fixture.store.put_candidate(&initial, None).await.unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &initial.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    let mut commands = Vec::new();
    for n in 2..=3 {
        let record = fixture.candidate(n);
        fixture
            .store
            .put_analysis_candidate(&record, &fixture.admission(&record, &n.to_string()), None)
            .await
            .unwrap();
        commands.push(CandidateActivationCommand {
            contract_name: "store.parity".into(),
            contract_digest: fixture.contract.contract_digest.clone(),
            executable_digest: record.executable_digest,
            expected_candidate_state_version: 0,
            expected_active_executable: ExecutableLifecycleToken {
                executable_digest: initial.executable_digest.clone(),
                state_version: 1,
            },
        });
    }
    let (a, b) = tokio::join!(
        fixture.store.activate_candidate(&commands[0], None),
        fixture.store.activate_candidate(&commands[1], None),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let winner = a.as_ref().ok().or(b.as_ref().ok()).unwrap();
    assert_eq!(winner.superseded_candidate_count, 1);
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(ExecutableStoreError::CandidateConflict { .. })
    ));
    assert!(
        fixture
            .store
            .list_pending(100, None, None)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    assert_eq!(
        fixture
            .store
            .get_active(&fixture.contract.contract_digest, None)
            .await
            .unwrap()
            .unwrap()
            .state_version,
        2
    );
    fixture.close().await;
}

#[tokio::test]
async fn concurrent_v4_initialization_migrates_once_without_resetting_records() {
    let fixture = Fixture::new().await;
    let record = fixture.candidate(1);
    fixture
        .store
        .put_analysis_candidate(&record, &fixture.admission(&record, "cycle"), None)
        .await
        .unwrap();
    let before: Vec<u8> = sqlx::query_scalar("SELECT executable_json FROM decision_executables")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_cutoff TEXT;
        ALTER TABLE analysis_candidate_admissions ADD COLUMN evidence_watermark INTEGER;
        UPDATE flaggo_schema_versions SET version=4 WHERE component='executable-store';",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        fixture.store.initialize(None),
        fixture.store.initialize(None)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>("SELECT executable_json FROM decision_executables")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        fixture
            .store
            .list_pending(100, None, None)
            .await
            .unwrap()
            .candidates
            .len(),
        1
    );
    let columns = sqlx::query("PRAGMA table_info(analysis_candidate_admissions)")
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(columns.len(), 8);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT version FROM flaggo_schema_versions WHERE component='executable-store'"
        )
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        5
    );
    fixture.close().await;
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("store.db");
        let now = timestamp::parse_exact("2026-03-03T10:00:00.1234567+00:00").unwrap();
        let store = SqliteExecutableStore::open(&path, None)
            .await
            .unwrap()
            .with_clock(Arc::new(move || now));
        let contracts = SqliteContractVersionStore::open(&path, None).await.unwrap();
        contracts.initialize(None).await.unwrap();
        store.initialize(None).await.unwrap();
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let model: DecisionContract = strict_json::deserialize(
            br#"{
            "authority":{"tenant":"t","application":"a","environment":"e"},
            "name":"store.parity","expression_syntax":"flaggo.cel/v1",
            "attributes":[],"result":{"schema":{"type":"integer"},"default":99}
        }"#,
        )
        .unwrap();
        let contract = AcceptedContractVersion {
            contract_digest: digests::contract_digest(&model, None).unwrap(),
            accepted_at: now,
            contract: model,
        };
        contracts.put(&contract, None).await.unwrap();
        contracts
            .set_current(&contract.contract.name, &contract.contract_digest, None)
            .await
            .unwrap();
        Self {
            directory,
            store,
            contracts,
            pool,
            contract,
        }
    }

    fn candidate(&self, variant: i64) -> StoredExecutable {
        let source: DecisionExecutable = strict_json::deserialize(format!(
            r#"{{"contractDigest":"{}","rules":[{{"name":"rule{}","when":{{"expression":"true"}},"return":{{"value":{}}}}}]}}"#,
            self.contract.contract_digest, variant, variant
        ).as_bytes()).unwrap();
        let compiled = ExecutableCompiler::default()
            .compile(&self.contract.contract, &source)
            .unwrap();
        StoredExecutable {
            executable_digest: compiled.executable_digest,
            executable: compiled.executable,
            checked_executable: compiled.checked_executable,
            state: ExecutableLifecycleState::Candidate,
            state_version: 0,
            created_at: self.contract.accepted_at,
            activated_at: None,
            provenance: Some(
                strict_json::deserialize(br#"{"n":1.00,"source":"analysis"}"#).unwrap(),
            ),
        }
    }

    fn admission(&self, record: &StoredExecutable, cycle: &str) -> AnalysisCandidateAdmission {
        AnalysisCandidateAdmission {
            workspace_id: "workspace".into(),
            cycle_id: cycle.into(),
            attempt_id: "attempt".into(),
            contract_name: self.contract.contract.name.clone(),
            contract_digest: self.contract.contract_digest.clone(),
            executable_digest: record.executable_digest.clone(),
            analysis_manifest_digest: format!("sha256:{}", "a".repeat(64)),
            created_at: record.created_at,
        }
    }

    async fn close(self) {
        self.store.close().await;
        self.contracts.close().await;
        self.pool.close().await;
        self.directory.close().unwrap();
    }
}

#[tokio::test]
async fn immutable_versions_restart_and_real_runtime_read_the_active_checked_program() {
    let fixture = Fixture::new().await;
    let first = fixture.candidate(7);
    fixture.store.put_candidate(&first, None).await.unwrap();
    let mut duplicate = first.clone();
    duplicate.provenance = None;
    duplicate.created_at = timestamp::parse_exact("2026-04-03T10:00:00.0000000+00:00").unwrap();
    assert_eq!(
        fixture.store.put_candidate(&duplicate, None).await.unwrap(),
        ExecutableStoreWriteResult::Existing
    );
    fixture
        .store
        .activate_if_none(
            &fixture.contract.contract_digest,
            &first.executable_digest,
            None,
        )
        .await
        .unwrap();
    let reopened = SqliteExecutableStore::open(fixture.directory.path().join("store.db"), None)
        .await
        .unwrap();
    reopened.initialize(None).await.unwrap();
    let stored = reopened
        .get(&first.executable_digest, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.created_at, first.created_at);
    assert_eq!(
        strict_json::serialize(&stored.provenance).unwrap(),
        strict_json::serialize(&first.provenance).unwrap()
    );
    assert_eq!(stored.state_version, 1);
    let runtime = DecisionRuntime::new(fixture.contracts.clone(), reopened.clone());
    let input = strict_json::deserialize(br#"{"attributes":{"_random":0.5}}"#).unwrap();
    let decision = runtime
        .decide(
            "store.parity",
            &fixture.contract.contract_digest,
            &input,
            None,
        )
        .await
        .unwrap();
    assert_eq!(strict_json::serialize(&decision.result).unwrap(), b"7");
    reopened.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn competing_expected_activations_have_exactly_one_winner() {
    let fixture = Fixture::new().await;
    let initial = fixture.candidate(1);
    let left = fixture.candidate(2);
    let right = fixture.candidate(3);
    for candidate in [&initial, &left, &right] {
        fixture.store.put_candidate(candidate, None).await.unwrap();
    }
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &initial.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        fixture.store.activate(
            &fixture.contract.contract_digest,
            &left.executable_digest,
            Some(&initial.executable_digest),
            None
        ),
        fixture.store.activate(
            &fixture.contract.contract_digest,
            &right.executable_digest,
            Some(&initial.executable_digest),
            None
        ),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(ExecutableStoreError::ActivationConflict { .. })
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM decision_executables WHERE lifecycle_state='active'",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    fixture.close().await;
}

#[tokio::test]
async fn concurrent_cycle_admissions_are_idempotent_and_conflicts_do_not_leak_candidates() {
    let fixture = Fixture::new().await;
    let first = fixture.candidate(1);
    let admission = fixture.admission(&first, "cycle");
    let (a, b) = tokio::join!(
        fixture
            .store
            .put_analysis_candidate(&first, &admission, None),
        fixture
            .store
            .put_analysis_candidate(&first, &admission, None),
    );
    assert_ne!(a.unwrap().created, b.unwrap().created);
    let second = fixture.candidate(2);
    let other = fixture.admission(&second, "cycle");
    assert!(matches!(
        fixture
            .store
            .put_analysis_candidate(&second, &other, None)
            .await,
        Err(ExecutableStoreError::AdmissionConflict { .. })
    ));
    assert!(
        fixture
            .store
            .get(&second.executable_digest, None)
            .await
            .unwrap()
            .is_none()
    );
    fixture.close().await;
}

#[tokio::test]
async fn automatic_resolution_detects_aba_and_concurrent_resolution_is_idempotent() {
    let fixture = Fixture::new().await;
    let first = fixture.candidate(1);
    let other = fixture.candidate(2);
    let candidate = fixture.candidate(3);
    for record in [&first, &other] {
        fixture.store.put_candidate(record, None).await.unwrap();
    }
    fixture
        .store
        .put_analysis_candidate(&candidate, &fixture.admission(&candidate, "cycle"), None)
        .await
        .unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &first.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    let snapshot = fixture
        .store
        .list_pending(1, None, None)
        .await
        .unwrap()
        .candidates
        .remove(0);
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &other.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &first.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    let mut command = AnalysisCandidateResolutionCommand {
        admission: snapshot.admission,
        expected_candidate_state_version: snapshot.candidate_state_version,
        expected_active_executable: snapshot.active_executable,
        action: AnalysisCandidateResolutionAction::Activate,
    };
    assert_eq!(
        fixture.store.resolve(&command, None).await.unwrap().outcome,
        AnalysisCandidateResolutionOutcome::Yielded
    );
    command.expected_active_executable = Some(ExecutableLifecycleToken {
        executable_digest: first.executable_digest,
        state_version: 3,
    });
    let (a, b) = tokio::join!(
        fixture.store.resolve(&command, None),
        fixture.store.resolve(&command, None)
    );
    let outcomes = [a.unwrap().outcome, b.unwrap().outcome];
    assert!(outcomes.contains(&AnalysisCandidateResolutionOutcome::Activated));
    assert!(outcomes.contains(&AnalysisCandidateResolutionOutcome::AlreadyResolved));
    fixture.close().await;
}

#[tokio::test]
async fn failed_activation_rolls_back_deactivation_and_preserves_pending_candidate() {
    let fixture = Fixture::new().await;
    let initial = fixture.candidate(1);
    let selected = fixture.candidate(2);
    fixture.store.put_candidate(&initial, None).await.unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &initial.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    fixture
        .store
        .put_analysis_candidate(&selected, &fixture.admission(&selected, "cycle"), None)
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_activation BEFORE UPDATE OF lifecycle_state ON decision_executables
        WHEN NEW.lifecycle_state='active' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let snapshot = fixture
        .store
        .list_pending(1, None, None)
        .await
        .unwrap()
        .candidates
        .remove(0);
    let command = AnalysisCandidateResolutionCommand {
        admission: snapshot.admission,
        expected_candidate_state_version: 0,
        expected_active_executable: snapshot.active_executable,
        action: AnalysisCandidateResolutionAction::Activate,
    };
    assert!(matches!(
        fixture.store.resolve(&command, None).await,
        Err(ExecutableStoreError::Database(_))
    ));
    let active = fixture
        .store
        .get_active(&fixture.contract.contract_digest, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.executable_digest, initial.executable_digest);
    assert_eq!(active.state_version, 1);
    let pending = fixture.store.list_pending(1, None, None).await.unwrap();
    assert_eq!(pending.candidates[0].candidate_state_version, 0);
    sqlx::query("DROP TRIGGER reject_activation")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture.store.resolve(&command, None).await.unwrap().outcome,
        AnalysisCandidateResolutionOutcome::Activated
    );
    fixture.close().await;
}

#[tokio::test]
async fn cancellation_while_blocked_rolls_back_admission_and_candidate() {
    let fixture = Fixture::new().await;
    let blocker = fixture.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = {
        let store = fixture.store.clone();
        let candidate = fixture.candidate(1);
        let admission = fixture.admission(&candidate, "cycle");
        let cancel = Arc::clone(&cancel);
        tokio::spawn(async move {
            store
                .put_analysis_candidate(&candidate, &admission, Some(cancel))
                .await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.store(true, Ordering::Release);
    blocker.rollback().await.unwrap();
    assert!(matches!(
        worker.await.unwrap(),
        Err(ExecutableStoreError::Cancelled)
    ));
    assert!(
        fixture
            .store
            .list_executables(&fixture.contract.contract_digest, 200, None, None)
            .await
            .unwrap()
            .executables
            .is_empty()
    );
    assert!(
        fixture
            .store
            .list_pending(200, None, None)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn manual_activation_fences_candidate_active_token_and_contract_currentness() {
    let fixture = Fixture::new().await;
    let initial = fixture.candidate(1);
    let candidate = fixture.candidate(2);
    let backlog = fixture.candidate(3);
    fixture.store.put_candidate(&initial, None).await.unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &initial.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    for record in [&candidate, &backlog] {
        fixture
            .store
            .put_analysis_candidate(
                record,
                &fixture.admission(record, &record.executable_digest),
                None,
            )
            .await
            .unwrap();
    }
    let mut command = CandidateActivationCommand {
        contract_name: "store.parity".into(),
        contract_digest: fixture.contract.contract_digest.clone(),
        executable_digest: candidate.executable_digest.clone(),
        expected_candidate_state_version: 0,
        expected_active_executable: ExecutableLifecycleToken {
            executable_digest: initial.executable_digest,
            state_version: 0,
        },
    };
    assert!(matches!(
        fixture.store.activate_candidate(&command, None).await,
        Err(ExecutableStoreError::ActivationConflict { .. })
    ));
    command.expected_active_executable.state_version = 1;
    sqlx::query("DELETE FROM decision_contract_current")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(matches!(
        fixture.store.activate_candidate(&command, None).await,
        Err(ExecutableStoreError::CurrentContractConflict { .. })
    ));
    fixture
        .contracts
        .set_current("store.parity", &fixture.contract.contract_digest, None)
        .await
        .unwrap();
    let result = fixture
        .store
        .activate_candidate(&command, None)
        .await
        .unwrap();
    assert_eq!(result.state_version, 2);
    assert_eq!(result.superseded_candidate_count, 1);
    assert!(matches!(
        fixture.store.activate_candidate(&command, None).await,
        Err(ExecutableStoreError::CandidateConflict { .. })
    ));
    assert_eq!(
        fixture
            .store
            .get(&backlog.executable_digest, None)
            .await
            .unwrap()
            .unwrap()
            .state,
        ExecutableLifecycleState::Inactive
    );
    fixture.close().await;
}

#[tokio::test]
async fn history_uses_stable_tie_breaks_and_validates_the_lookahead() {
    let fixture = Fixture::new().await;
    let mut identities = Vec::new();
    for n in 1..=3 {
        let record = fixture.candidate(n);
        fixture.store.put_candidate(&record, None).await.unwrap();
        identities.push(record.executable_digest);
    }
    identities.sort_by(|a, b| b.cmp(a));
    let first = fixture
        .store
        .list_executables(&fixture.contract.contract_digest, 2, None, None)
        .await
        .unwrap();
    let second = fixture
        .store
        .list_executables(
            &fixture.contract.contract_digest,
            2,
            first.next_cursor.as_deref(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        first
            .executables
            .iter()
            .chain(&second.executables)
            .map(|record| record.executable_digest.clone())
            .collect::<Vec<_>>(),
        identities
    );
    assert!(second.next_cursor.is_none());
    sqlx::query(
        "UPDATE decision_executables SET checked_executable_json=X'7B' WHERE executable_digest=?",
    )
    .bind(&identities[2])
    .execute(&fixture.pool)
    .await
    .unwrap();
    assert!(matches!(
        fixture
            .store
            .list_executables(&fixture.contract.contract_digest, 2, None, None)
            .await,
        Err(ExecutableStoreError::Contract(_))
    ));
    fixture.close().await;
}

#[tokio::test]
async fn active_reader_preserves_native_json_versus_integrity_faults() {
    let fixture = Fixture::new().await;
    let record = fixture.candidate(1);
    fixture.store.put_candidate(&record, None).await.unwrap();
    fixture
        .store
        .activate(
            &fixture.contract.contract_digest,
            &record.executable_digest,
            None,
            None,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE decision_executables SET checked_executable_json=CAST('null' AS BLOB)")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(matches!(
        ActiveExecutableReader::get_active(&fixture.store, &fixture.contract.contract_digest, None)
            .await,
        Err(StoreReadError::Integrity(_))
    ));
    sqlx::query("UPDATE decision_executables SET checked_executable_json=X'7B'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(matches!(
        ActiveExecutableReader::get_active(&fixture.store, &fixture.contract.contract_digest, None)
            .await,
        Err(StoreReadError::Native(_))
    ));
    let row = sqlx::query("SELECT lifecycle_state, state_version FROM decision_executables")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(row.get::<&str, _>("lifecycle_state"), "active");
    assert_eq!(row.get::<i64, _>("state_version"), 1);
    fixture.close().await;
}
