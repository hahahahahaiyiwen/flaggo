use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use flaggo_analysis_agent::{
    AgentPool, AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisTask,
    AnalysisTools,
};
use flaggo_analysis_domain::{
    AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisError,
    AnalysisEvidenceSource, AnalysisProfile, AttemptContext, CandidateRecord, CatalogUpdate,
    CycleOutcome, EvidenceCutoff,
};
use flaggo_analysis_workspace::{LocalWorkspaceProvider, WorkspaceFileSystem, workspace_id};
use flaggo_async_analysis::{
    AnalysisCoordinator, Clock, ContractService, CoordinatorConfig, CoordinatorRunOutcome,
    local_capabilities::LocalCapabilityFactory,
};
use flaggo_evidence_store::{
    EvidenceAnalysisStore, EvidenceQueryLimits, EvidenceQueryRequest, EvidenceQueryResult,
    EvidenceQueryScope, EvidenceStoreError, ObservationWatermark,
};
use serde_json::{Value, json};

struct FixedClock(DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

struct FakeContractService {
    current: RwLock<Option<AnalysisContract>>,
    submissions: Mutex<Vec<AttemptContext>>,
    now: DateTime<Utc>,
}

impl FakeContractService {
    fn set_current(&self, contract: AnalysisContract) {
        *self.current.write().expect("current contract lock") = Some(contract);
    }
}

#[async_trait]
impl ContractService for FakeContractService {
    async fn list_current_learning_contracts(
        &self,
        _revision: Option<&str>,
    ) -> Result<CatalogUpdate, AnalysisError> {
        let contracts = self
            .current
            .read()
            .expect("current contract lock")
            .iter()
            .map(|contract| AnalysisContractIdentity {
                name: contract.name.clone(),
                digest: contract.contract_digest.clone(),
            })
            .collect();
        Ok(CatalogUpdate::Updated {
            revision: format!("revision-{}", self.now.timestamp()),
            contracts,
        })
    }

    async fn get_current_contract(
        &self,
        contract_name: &str,
    ) -> Result<Option<AnalysisContract>, AnalysisError> {
        Ok(self
            .current
            .read()
            .expect("current contract lock")
            .as_ref()
            .filter(|contract| contract.name == contract_name)
            .cloned())
    }

    async fn submit_candidate(
        &self,
        context: &AttemptContext,
        _rules: Value,
        _cutoff: &EvidenceCutoff,
        analysis_manifest_digest: &str,
    ) -> Result<CandidateRecord, AnalysisError> {
        self.submissions
            .lock()
            .expect("submission lock")
            .push(context.clone());
        Ok(CandidateRecord {
            contract_name: context.contract.name.clone(),
            contract_digest: context.contract.digest.clone(),
            executable_digest:
                "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_owned(),
            created_at: self.now,
            created: true,
            analysis_manifest_digest: analysis_manifest_digest.to_owned(),
        })
    }
}

struct FakeEvidenceStore {
    queries: AtomicUsize,
}

#[async_trait]
impl EvidenceAnalysisStore for FakeEvidenceStore {
    async fn capture_watermark(&self) -> Result<ObservationWatermark, EvidenceStoreError> {
        Ok(ObservationWatermark::new(7))
    }

    async fn query(
        &self,
        _scope: &EvidenceQueryScope,
        _request: EvidenceQueryRequest,
    ) -> Result<EvidenceQueryResult, EvidenceStoreError> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        Ok(EvidenceQueryResult {
            normalized_sql: "SELECT count(*) AS count FROM observations".to_owned(),
            columns: vec!["count".to_owned()],
            rows: vec![BTreeMap::from([("count".to_owned(), json!(12))])],
            observation_ids: Vec::new(),
            result_bytes: 12,
        })
    }
}

#[derive(Clone, Copy)]
enum Script {
    Candidate,
    NoChange,
    FailThenNoCandidate,
    Supersede,
    ShutdownCheckpoint,
}

struct ScriptedAgentProvider {
    script: Script,
    starts: AtomicUsize,
    cutoff: DateTime<Utc>,
    contract_service: Arc<FakeContractService>,
    replacement: Option<AnalysisContract>,
}

#[async_trait]
impl AgentProvider for ScriptedAgentProvider {
    async fn start_session(
        &self,
        specification: AgentSessionSpec,
    ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
        let ordinal = self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(ScriptedSession {
            script: self.script,
            ordinal,
            cutoff: self.cutoff,
            tools: specification.tools,
            filesystem: specification.filesystem,
            contract_service: Arc::clone(&self.contract_service),
            replacement: self.replacement.clone(),
        }))
    }
}

struct ScriptedSession {
    script: Script,
    ordinal: usize,
    cutoff: DateTime<Utc>,
    tools: Arc<dyn AnalysisTools>,
    filesystem: Arc<dyn WorkspaceFileSystem>,
    contract_service: Arc<FakeContractService>,
    replacement: Option<AnalysisContract>,
}

#[async_trait]
impl AgentSession for ScriptedSession {
    async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        match self.script {
            Script::Candidate => {
                self.tools.commit_evidence_cutoff(self.cutoff).await?;
                self.tools
                    .query_evidence("SELECT count(*) AS count FROM observations")
                    .await?;
                self.filesystem
                    .write_text(
                        "/workspace/analysis/findings.md",
                        "The bounded evidence supports a candidate.",
                    )
                    .await?;
                self.tools
                    .propose_executable(json!([{
                        "name": "learned",
                        "when": {"expression": "true"},
                        "return": {"value": 5}
                    }]))
                    .await?;
                Ok(AgentRunOutcome::Candidate)
            }
            Script::NoChange => {
                self.tools.commit_evidence_cutoff(self.cutoff).await?;
                self.tools
                    .query_evidence("SELECT count(*) AS count FROM observations")
                    .await?;
                self.filesystem
                    .write_text(
                        "/workspace/analysis/no-change.md",
                        "The current executable remains appropriate.",
                    )
                    .await?;
                Ok(AgentRunOutcome::NoChange {
                    reason: "the current executable remains appropriate".to_owned(),
                })
            }
            Script::FailThenNoCandidate if self.ordinal == 0 => {
                Err(AnalysisError::Agent("provider interrupted".to_owned()))
            }
            Script::FailThenNoCandidate => {
                self.tools.commit_evidence_cutoff(self.cutoff).await?;
                self.filesystem
                    .write_text(
                        "/workspace/analysis/recovery.md",
                        "Recovered from the prior attempt.",
                    )
                    .await?;
                Ok(AgentRunOutcome::NoCandidate {
                    reason: "evidence remains insufficient".to_owned(),
                })
            }
            Script::Supersede => {
                self.contract_service
                    .set_current(self.replacement.clone().expect("supersession replacement"));
                while !self.tools.yield_signal().is_requested() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                self.filesystem
                    .write_text(
                        "/workspace/handoffs/superseded.md",
                        "Checkpointed after the trusted supersession signal.",
                    )
                    .await?;
                Ok(AgentRunOutcome::Superseded)
            }
            Script::ShutdownCheckpoint => {
                while !self.tools.yield_signal().is_requested() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                self.filesystem
                    .write_text(
                        "/workspace/analysis/shutdown-checkpoint.md",
                        "Checkpointed during the shutdown grace period.",
                    )
                    .await?;
                Ok(AgentRunOutcome::Handoff {
                    summary: "service shutdown checkpoint".to_owned(),
                })
            }
        }
    }

    async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
        Ok(())
    }

    async fn close(&self) -> Result<(), AnalysisError> {
        Ok(())
    }
}

fn contract(digest: char, accepted_at: DateTime<Utc>) -> AnalysisContract {
    AnalysisContract {
        name: "checkout.delay".to_owned(),
        contract_digest: format!("sha256:{}", digest.to_string().repeat(64)),
        accepted_at,
        authority: AnalysisAuthority {
            tenant: "local".to_owned(),
            application: "checkout".to_owned(),
            environment: "test".to_owned(),
        },
        evaluation_interval: "PT1H".to_owned(),
        evidence_sources: vec![AnalysisEvidenceSource {
            kind: "metric".to_owned(),
            scope: "checkout".to_owned(),
            name: "request.duration".to_owned(),
            metric_kind: Some("histogram".to_owned()),
            unit: Some("ms".to_owned()),
            span_name: None,
        }],
        contract: json!({"name": "checkout.delay"}),
    }
}

fn coordinator(
    workspace_root: &std::path::Path,
    service: Arc<FakeContractService>,
    evidence: Arc<FakeEvidenceStore>,
    provider: Arc<ScriptedAgentProvider>,
    now: DateTime<Utc>,
) -> AnalysisCoordinator {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
    AnalysisCoordinator::new(
        service.clone(),
        Arc::new(LocalWorkspaceProvider::new(workspace_root).expect("workspace provider")),
        Arc::new(LocalCapabilityFactory::new(
            service,
            evidence,
            EvidenceQueryLimits {
                max_rows: NonZeroU32::new(100).expect("nonzero"),
                max_bytes: 16_384,
                timeout: Duration::from_secs(1),
            },
            Arc::clone(&clock),
        )),
        AgentPool::new(provider, NonZeroUsize::new(1).expect("nonzero")),
        CoordinatorConfig {
            supersession_poll_interval: Duration::from_millis(5),
            yield_grace_period: Duration::from_millis(100),
            analysis_profile: AnalysisProfile {
                skill_name: "evidence-analysis".to_owned(),
                skill_version: "1".to_owned(),
            },
        },
        clock,
    )
}

#[tokio::test]
async fn persists_candidate_through_real_workspace_and_capability_boundaries() {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("valid timestamp");
    let now = accepted_at + chrono::Duration::hours(2);
    let contract = contract('a', accepted_at);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(contract)),
        submissions: Mutex::new(Vec::new()),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script: Script::Candidate,
        starts: AtomicUsize::new(0),
        cutoff: now,
        contract_service: Arc::clone(&service),
        replacement: None,
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(
        directory.path(),
        Arc::clone(&service),
        Arc::clone(&evidence),
        provider,
        now,
    );

    let outcome = coordinator.run_once(now).await.expect("run cycle");
    let CoordinatorRunOutcome::Completed {
        outcome: CycleOutcome::Candidate { candidate },
        ..
    } = outcome
    else {
        panic!("expected completed Candidate, got {outcome:?}");
    };
    assert_eq!(
        candidate.executable_digest,
        "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    );
    assert_eq!(evidence.queries.load(Ordering::Relaxed), 1);
    assert_eq!(
        service.submissions.lock().expect("submission lock").len(),
        1
    );
}

#[tokio::test]
async fn completes_no_change_without_persisting_a_candidate() {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("valid timestamp");
    let now = accepted_at + chrono::Duration::hours(2);
    let contract = contract('a', accepted_at);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(contract)),
        submissions: Mutex::new(Vec::new()),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script: Script::NoChange,
        starts: AtomicUsize::new(0),
        cutoff: now,
        contract_service: Arc::clone(&service),
        replacement: None,
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(
        directory.path(),
        Arc::clone(&service),
        Arc::clone(&evidence),
        provider,
        now,
    );

    let outcome = coordinator.run_once(now).await.expect("run cycle");
    let CoordinatorRunOutcome::Completed {
        outcome: CycleOutcome::NoChange { reason },
        ..
    } = outcome
    else {
        panic!("expected completed no-change outcome, got {outcome:?}");
    };
    assert_eq!(reason, "the current executable remains appropriate");
    assert_eq!(evidence.queries.load(Ordering::Relaxed), 1);
    assert!(
        service
            .submissions
            .lock()
            .expect("submission lock")
            .is_empty()
    );
}

#[tokio::test]
async fn resumes_same_cycle_after_provider_failure() {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("valid timestamp");
    let now = accepted_at + chrono::Duration::hours(2);
    let contract = contract('a', accepted_at);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(contract)),
        submissions: Mutex::new(Vec::new()),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script: Script::FailThenNoCandidate,
        starts: AtomicUsize::new(0),
        cutoff: now,
        contract_service: Arc::clone(&service),
        replacement: None,
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(directory.path(), service, evidence, provider, now);

    let first = coordinator.run_once(now).await.expect("failed attempt");
    let CoordinatorRunOutcome::Recoverable {
        cycle_id: first_cycle,
        ..
    } = first
    else {
        panic!("expected recoverable attempt, got {first:?}");
    };
    let second = coordinator.run_once(now).await.expect("recovered attempt");
    let CoordinatorRunOutcome::Completed {
        cycle_id: second_cycle,
        outcome: CycleOutcome::NoCandidate { .. },
        ..
    } = second
    else {
        panic!("expected completed no-Candidate, got {second:?}");
    };
    assert_eq!(first_cycle, second_cycle);
}

#[tokio::test]
async fn checkpoints_workspace_during_graceful_supersession() {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("valid timestamp");
    let now = accepted_at + chrono::Duration::hours(2);
    let old_contract = contract('a', accepted_at);
    let new_contract = contract('c', accepted_at);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(old_contract)),
        submissions: Mutex::new(Vec::new()),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script: Script::Supersede,
        starts: AtomicUsize::new(0),
        cutoff: now,
        contract_service: Arc::clone(&service),
        replacement: Some(new_contract.clone()),
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(directory.path(), service, evidence, provider, now);

    let outcome = coordinator.run_once(now).await.expect("superseded attempt");
    let CoordinatorRunOutcome::Completed {
        cycle_id,
        outcome: CycleOutcome::Superseded { replacement_digest },
        ..
    } = outcome
    else {
        panic!("expected superseded cycle, got {outcome:?}");
    };
    assert_eq!(
        replacement_digest.as_deref(),
        Some(new_contract.contract_digest.as_str())
    );

    let directory_name = workspace_id("checkout.delay")
        .strip_prefix("sha256:")
        .expect("workspace digest")
        .to_owned();
    let checkpoint = directory
        .path()
        .join(directory_name)
        .join("cycles")
        .join(cycle_id)
        .join("handoffs")
        .join("superseded.md");
    assert_eq!(
        std::fs::read_to_string(checkpoint).expect("supersession checkpoint"),
        "Checkpointed after the trusted supersession signal."
    );
}

#[tokio::test]
async fn checkpoints_and_retains_cycle_during_graceful_shutdown() {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("valid timestamp");
    let now = accepted_at + chrono::Duration::hours(2);
    let contract = contract('a', accepted_at);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(contract)),
        submissions: Mutex::new(Vec::new()),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script: Script::ShutdownCheckpoint,
        starts: AtomicUsize::new(0),
        cutoff: now,
        contract_service: Arc::clone(&service),
        replacement: None,
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(
        directory.path(),
        service,
        evidence,
        Arc::clone(&provider),
        now,
    );
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    let request_shutdown = async {
        while provider.starts.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        shutdown_sender.send(true).expect("request shutdown");
    };
    let (outcome, ()) = tokio::join!(
        coordinator.run_once_until(now, shutdown_receiver),
        request_shutdown
    );
    let CoordinatorRunOutcome::Recoverable {
        cycle_id, reason, ..
    } = outcome.expect("shutdown attempt")
    else {
        panic!("expected recoverable shutdown checkpoint");
    };
    assert_eq!(reason, "agent committed a handoff checkpoint");

    let workspace_root = directory.path().join(
        workspace_id("checkout.delay")
            .strip_prefix("sha256:")
            .expect("workspace digest"),
    );
    let cycle_root = workspace_root.join("cycles").join(&cycle_id);
    assert_eq!(
        std::fs::read_to_string(cycle_root.join("analysis/shutdown-checkpoint.md"))
            .expect("shutdown checkpoint"),
        "Checkpointed during the shutdown grace period."
    );
    let pointer: Value = serde_json::from_slice(
        &std::fs::read(workspace_root.join("current-cycle.json")).expect("current pointer"),
    )
    .expect("parse current pointer");
    assert_eq!(pointer["cycleId"], cycle_id);
    assert!(!cycle_root.join("outcome.json").exists());
}
