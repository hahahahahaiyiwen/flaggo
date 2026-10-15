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
    AgentPool, AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisRunStatus,
    AnalysisTask, AnalysisTools,
    context::{ANALYSIS_FINDINGS_PATH, HANDOFF_CHECKPOINT_PATH, SUPERSESSION_NOTE_PATH},
    workflow::{WorkflowAgentProvider, WorkflowMetadata, WorkflowState, work_state},
};
use flaggo_analysis_domain::{
    AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisError,
    AnalysisEvidenceSource, AnalysisProfile, AttemptContext, CandidateRecord, CatalogUpdate,
    CycleLimits, CycleOutcome,
};
use flaggo_analysis_workspace::{
    LocalWorkspaceProvider, WorkspaceFileSystem, sha256_digest, workspace_id,
};
use flaggo_async_analysis::{
    AnalysisCoordinator, Clock, ContractService, CoordinatorConfig, CoordinatorRunOutcome,
    local_capabilities::LocalCapabilityFactory,
};
use flaggo_evidence_store::{
    EvidenceAnalysisStore, EvidenceColumn, EvidenceQueryCapabilities, EvidenceQueryLimits,
    EvidenceQueryRequest, EvidenceQueryResult, EvidenceQueryScope, EvidenceStoreError,
    SQLITE_EVIDENCE_QUERY_CAPABILITIES,
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
    fn query_capabilities(&self) -> EvidenceQueryCapabilities {
        SQLITE_EVIDENCE_QUERY_CAPABILITIES
    }

    async fn query(
        &self,
        _scope: &EvidenceQueryScope,
        _request: EvidenceQueryRequest,
    ) -> Result<EvidenceQueryResult, EvidenceStoreError> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        Ok(EvidenceQueryResult {
            normalized_sql: "SELECT count(*) AS count FROM observations".to_owned(),
            columns: vec![EvidenceColumn {
                name: "count".to_owned(),
                data_type: None,
            }],
            rows: vec![BTreeMap::from([("count".to_owned(), json!(12))])],
            observation_ids: Vec::new(),
            result_bytes: 12,
            truncated: false,
        })
    }
}

#[derive(Clone, Copy)]
enum Script {
    Candidate,
    NoChange,
    FailThenNoCandidate,
    Supersede,
    SupersedeBeforeCompletion,
    IgnoreSupersession,
    ClaimSupersession,
    ShutdownCheckpoint,
    Collaborate,
    RequestRevision,
    RejectDraft,
    SubmitThenInterrupt,
    AlwaysFail,
    NeverFinish,
    StartupFailure,
}

struct ScriptedAgentProvider {
    script: Script,
    starts: AtomicUsize,
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
        if matches!(self.script, Script::StartupFailure) {
            return Err(AnalysisError::Agent("provider startup failed".to_owned()));
        }
        Ok(Arc::new(ScriptedSession {
            workflow: work_state(specification.handoff.as_ref())?,
            attempt_id: specification.context.attempt_id,
            script: self.script,
            ordinal,
            tools: specification.tools,
            filesystem: specification.filesystem,
            contract_service: Arc::clone(&self.contract_service),
            replacement: self.replacement.clone(),
        }))
    }
}

struct ScriptedSession {
    workflow: Option<WorkflowMetadata>,
    attempt_id: String,
    script: Script,
    ordinal: usize,
    tools: Arc<dyn AnalysisTools>,
    filesystem: Arc<dyn WorkspaceFileSystem>,
    contract_service: Arc<FakeContractService>,
    replacement: Option<AnalysisContract>,
}

#[async_trait]
impl AgentSession for ScriptedSession {
    async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        match self.script {
            Script::AlwaysFail => Err(AnalysisError::Agent(
                "recoverable provider error".to_owned(),
            )),
            Script::NeverFinish => std::future::pending().await,
            Script::StartupFailure => unreachable!(),
            Script::Collaborate
            | Script::RequestRevision
            | Script::RejectDraft
            | Script::SubmitThenInterrupt => self.collaborate().await,
            Script::Candidate => {
                for path in [
                    ANALYSIS_FINDINGS_PATH,
                    SUPERSESSION_NOTE_PATH,
                    HANDOFF_CHECKPOINT_PATH,
                ] {
                    assert!(
                        self.filesystem.read_text(path).await?.contains("<!--"),
                        "agent artifact template was not prepared: {path}"
                    );
                }
                self.tools
                    .run_sql("SELECT count(*) AS count FROM observations", None)
                    .await?;
                self.filesystem
                    .write_text(
                        ANALYSIS_FINDINGS_PATH,
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
                self.tools
                    .run_sql("SELECT count(*) AS count FROM observations", None)
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
                loop {
                    match self.tools.check_cycle_status().await? {
                        AnalysisRunStatus::Active => {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                        AnalysisRunStatus::ContractSuperseded { replacement_digest } => {
                            assert_eq!(
                                replacement_digest,
                                self.replacement
                                    .as_ref()
                                    .map(|contract| contract.contract_digest.clone())
                            );
                            break;
                        }
                        AnalysisRunStatus::ServiceShutdown => {
                            panic!("expected contract supersession")
                        }
                    }
                }
                self.filesystem
                    .write_text(
                        "/workspace/analysis/supersession.md",
                        "Checkpointed after the trusted supersession signal.",
                    )
                    .await?;
                Ok(AgentRunOutcome::Superseded)
            }
            Script::SupersedeBeforeCompletion => {
                self.contract_service
                    .set_current(self.replacement.clone().expect("supersession replacement"));
                Ok(AgentRunOutcome::NoChange {
                    reason: "the old executable remains appropriate".to_owned(),
                })
            }
            Script::IgnoreSupersession => {
                self.contract_service
                    .set_current(self.replacement.clone().expect("supersession replacement"));
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(AgentRunOutcome::Superseded)
            }
            Script::ClaimSupersession => Ok(AgentRunOutcome::Superseded),
            Script::ShutdownCheckpoint => {
                loop {
                    match self.tools.check_cycle_status().await? {
                        AnalysisRunStatus::Active => {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                        AnalysisRunStatus::ServiceShutdown => break,
                        AnalysisRunStatus::ContractSuperseded { .. } => {
                            panic!("expected service shutdown")
                        }
                    }
                }
                self.filesystem
                    .write_text(
                        "/workspace/analysis/shutdown-checkpoint.md",
                        "Checkpointed during the shutdown grace period.",
                    )
                    .await?;
                Ok(AgentRunOutcome::Handoff {
                    summary: "service shutdown checkpoint".to_owned(),
                    metadata: None,
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

impl ScriptedSession {
    async fn collaborate(&self) -> Result<AgentRunOutcome, AnalysisError> {
        let directory = format!("attempts/{}", self.attempt_id);
        match self.workflow.as_ref().map(|state| state.outcome) {
            None | Some(WorkflowState::RequestChange) => {
                assert!(!self.tools.can_propose());
                assert!(self.tools.propose_executable(json!([{}])).await.is_err());
                let query = self
                    .tools
                    .run_sql("SELECT count(*) AS count FROM observations", None)
                    .await?;
                let proposal_path = format!("{directory}/proposal.json");
                let content = json!({"rules": [{"name": "learned", "when": {"expression": "true"}, "return": {"value": 5}}], "method": "bounded evidence assessment"}).to_string();
                self.filesystem
                    .write_text(&format!("/workspace/analysis/{proposal_path}"), &content)
                    .await?;
                let metadata = WorkflowMetadata {
                    outcome: WorkflowState::DraftReady,
                    proposal_path,
                    proposal_digest: sha256_digest(content.as_bytes()),
                    evidence_paths: vec![query["resultPath"].as_str().unwrap().to_owned()],
                    evidence_digests: BTreeMap::from([(
                        query["resultPath"].as_str().unwrap().to_owned(),
                        query["contentDigest"].as_str().unwrap().to_owned(),
                    )]),
                    revision: self.workflow.as_ref().map_or(0, |state| state.revision + 1),
                    review_path: None,
                };
                Ok(AgentRunOutcome::Handoff {
                    summary: "draft ready".to_owned(),
                    metadata: Some(serde_json::to_value(metadata).unwrap()),
                })
            }
            Some(WorkflowState::DraftReady) => {
                assert!(self.tools.can_propose());
                let current = self.workflow.as_ref().unwrap();
                let draft: Value = serde_json::from_str(
                    &self
                        .filesystem
                        .read_text(&format!("/workspace/analysis/{}", current.proposal_path))
                        .await?,
                )
                .unwrap();
                assert!(
                    self.filesystem
                        .write_text(
                            &format!("/workspace/analysis/{}", current.proposal_path),
                            "overwrite prior draft"
                        )
                        .await
                        .is_err()
                );
                let review_path = format!("{directory}/review.json");
                self.filesystem.write_text(&format!("/workspace/analysis/{review_path}"), &json!({"proposalPath": current.proposal_path, "proposalDigest": current.proposal_digest, "decision": "reviewed"}).to_string()).await?;
                if matches!(self.script, Script::RejectDraft) {
                    return Ok(AgentRunOutcome::Reject {
                        reason: "insufficient support".to_owned(),
                        review_path,
                    });
                }
                if matches!(self.script, Script::RequestRevision) && current.revision == 0 {
                    let mut metadata = current.clone();
                    metadata.outcome = WorkflowState::RequestChange;
                    metadata.review_path = Some(review_path);
                    return Ok(AgentRunOutcome::Handoff {
                        summary: "revise".to_owned(),
                        metadata: Some(serde_json::to_value(metadata).unwrap()),
                    });
                }
                self.tools
                    .propose_executable(draft["rules"].clone())
                    .await?;
                if matches!(self.script, Script::SubmitThenInterrupt) {
                    return Err(AnalysisError::Agent(
                        "response interrupted after successful submission".to_owned(),
                    ));
                }
                Ok(AgentRunOutcome::Candidate)
            }
            Some(WorkflowState::AnalysisReady) => unreachable!(),
        }
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
    coordinator_with_poll_interval(
        workspace_root,
        service,
        evidence,
        provider,
        now,
        Duration::from_millis(5),
    )
}

fn coordinator_with_poll_interval(
    workspace_root: &std::path::Path,
    service: Arc<FakeContractService>,
    evidence: Arc<FakeEvidenceStore>,
    provider: Arc<ScriptedAgentProvider>,
    now: DateTime<Utc>,
    supersession_poll_interval: Duration,
) -> AnalysisCoordinator {
    let provider: Arc<dyn AgentProvider> = if matches!(
        provider.script,
        Script::Collaborate
            | Script::RequestRevision
            | Script::RejectDraft
            | Script::SubmitThenInterrupt
    ) {
        Arc::new(WorkflowAgentProvider::new(provider, false))
    } else {
        provider
    };
    coordinator_with_limits(
        workspace_root,
        service,
        evidence,
        provider,
        now,
        supersession_poll_interval,
        Duration::from_secs(1_200),
        CycleLimits::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn coordinator_with_limits(
    workspace_root: &std::path::Path,
    service: Arc<dyn ContractService>,
    evidence: Arc<FakeEvidenceStore>,
    provider: Arc<dyn AgentProvider>,
    now: DateTime<Utc>,
    supersession_poll_interval: Duration,
    attempt_timeout: Duration,
    cycle_limits: CycleLimits,
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
            flaggo_async_analysis::python::PythonConfig::default(),
        )),
        AgentPool::new(provider, NonZeroUsize::new(1).expect("nonzero")),
        CoordinatorConfig {
            cycle_limits,
            attempt_timeout,
            supersession_poll_interval,
            yield_grace_period: Duration::from_millis(100),
            analysis_profile: AnalysisProfile {
                skill_names: vec![
                    "analysis-cycle-protocol".to_owned(),
                    "understand-decision-contract".to_owned(),
                    "qualitative-analysis".to_owned(),
                    "author-executable".to_owned(),
                ],
            },
        },
        clock,
    )
}

fn workflow_fixture(
    script: Script,
) -> (
    DateTime<Utc>,
    Arc<FakeContractService>,
    Arc<FakeEvidenceStore>,
    Arc<ScriptedAgentProvider>,
) {
    let accepted_at = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).single().unwrap();
    let now = accepted_at + chrono::Duration::hours(2);
    let service = Arc::new(FakeContractService {
        current: RwLock::new(Some(contract('a', accepted_at))),
        submissions: Mutex::new(vec![]),
        now,
    });
    let evidence = Arc::new(FakeEvidenceStore {
        queries: AtomicUsize::new(0),
    });
    let provider = Arc::new(ScriptedAgentProvider {
        script,
        starts: AtomicUsize::new(0),
        contract_service: service.clone(),
        replacement: None,
    });
    (now, service, evidence, provider)
}

#[tokio::test]
async fn collaborates_across_fresh_coordinators_for_approval_revision_and_rejection() {
    for (script, attempts, expected_reviews) in [
        (Script::Collaborate, 2, 1),
        (Script::RequestRevision, 4, 2),
        (Script::RejectDraft, 2, 1),
        (Script::SubmitThenInterrupt, 3, 1),
    ] {
        let (now, service, evidence, provider) = workflow_fixture(script);
        let directory = tempfile::tempdir().unwrap();
        let mut cycle_id = None;
        for attempt in 0..attempts {
            let coordinator = coordinator(
                directory.path(),
                service.clone(),
                evidence.clone(),
                provider.clone(),
                now,
            );
            let outcome = coordinator.run_once(now).await.unwrap();
            let id = match &outcome {
                CoordinatorRunOutcome::Recoverable { cycle_id, .. }
                | CoordinatorRunOutcome::Completed { cycle_id, .. } => cycle_id,
                _ => panic!("unexpected outcome {outcome:?}"),
            };
            if let Some(previous) = &cycle_id {
                assert_eq!(previous, id);
            } else {
                cycle_id = Some(id.clone());
            }
            if attempt + 1 < attempts {
                assert!(matches!(outcome, CoordinatorRunOutcome::Recoverable { .. }));
            } else if matches!(script, Script::RejectDraft) {
                assert!(matches!(
                    outcome,
                    CoordinatorRunOutcome::Completed {
                        outcome: CycleOutcome::NoCandidate { .. },
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    outcome,
                    CoordinatorRunOutcome::Completed {
                        outcome: CycleOutcome::Candidate { .. },
                        ..
                    }
                ));
            }
        }
        let root = directory
            .path()
            .join(workspace_id("checkout.delay").trim_start_matches("sha256:"))
            .join("cycles")
            .join(cycle_id.unwrap());
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root.join("analysis-manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["files"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|file| file["path"].as_str().unwrap().ends_with("/review.json"))
                .count(),
            expected_reviews
        );
        let submissions = service.submissions.lock().unwrap();
        assert_eq!(
            submissions.len(),
            usize::from(!matches!(script, Script::RejectDraft))
        );
        assert_eq!(
            provider.starts.load(Ordering::SeqCst),
            if matches!(script, Script::SubmitThenInterrupt) {
                2
            } else {
                attempts
            }
        );
    }
}

#[tokio::test]
async fn startup_execution_and_timeout_failures_consume_durable_attempt_limits() {
    for script in [
        Script::StartupFailure,
        Script::AlwaysFail,
        Script::NeverFinish,
    ] {
        let (now, service, evidence, provider) = workflow_fixture(script);
        let directory = tempfile::tempdir().unwrap();
        for attempt in 0..9 {
            let coordinator = coordinator_with_limits(
                directory.path(),
                service.clone(),
                evidence.clone(),
                provider.clone(),
                now,
                Duration::from_secs(1),
                Duration::from_millis(20),
                CycleLimits::default(),
            );
            let outcome = coordinator.run_once(now).await.unwrap();
            if attempt < 8 {
                assert!(matches!(outcome, CoordinatorRunOutcome::Recoverable { .. }));
            } else {
                assert!(matches!(
                    outcome,
                    CoordinatorRunOutcome::Completed {
                        outcome: CycleOutcome::Failure { .. },
                        ..
                    }
                ));
            }
        }
        assert_eq!(provider.starts.load(Ordering::SeqCst), 8);
    }
}

struct LostReplyService {
    inner: Arc<FakeContractService>,
    admitted: tokio::sync::Mutex<Option<(AttemptContext, Value, CandidateRecord)>>,
}

#[async_trait]
impl ContractService for LostReplyService {
    async fn list_current_learning_contracts(
        &self,
        revision: Option<&str>,
    ) -> Result<CatalogUpdate, AnalysisError> {
        self.inner.list_current_learning_contracts(revision).await
    }
    async fn get_current_contract(
        &self,
        name: &str,
    ) -> Result<Option<AnalysisContract>, AnalysisError> {
        self.inner.get_current_contract(name).await
    }
    async fn submit_candidate(
        &self,
        context: &AttemptContext,
        rules: Value,
        manifest: &str,
    ) -> Result<CandidateRecord, AnalysisError> {
        let mut admitted = self.admitted.lock().await;
        if let Some((original, original_rules, candidate)) = admitted.as_ref() {
            assert_eq!(context, original);
            assert_eq!(&rules, original_rules);
            assert_eq!(manifest, candidate.analysis_manifest_digest);
            let mut result = candidate.clone();
            result.created = false;
            return Ok(result);
        }
        let candidate = self
            .inner
            .submit_candidate(context, rules.clone(), manifest)
            .await?;
        *admitted = Some((context.clone(), rules, candidate));
        Err(AnalysisError::Contract(
            "admission response lost".to_owned(),
        ))
    }
}

#[tokio::test]
async fn lost_admission_reply_retries_original_provenance_without_starting_another_agent() {
    let (now, service, evidence, provider) = workflow_fixture(Script::Collaborate);
    let transport = Arc::new(LostReplyService {
        inner: service.clone(),
        admitted: tokio::sync::Mutex::new(None),
    });
    let directory = tempfile::tempdir().unwrap();
    let mut last = None;
    for _ in 0..3 {
        let coordinator = coordinator_with_limits(
            directory.path(),
            transport.clone(),
            evidence.clone(),
            Arc::new(WorkflowAgentProvider::new(provider.clone(), false)),
            now,
            Duration::from_secs(1),
            Duration::from_secs(30),
            CycleLimits {
                max_attempts: 2,
                ..CycleLimits::default()
            },
        );
        last = Some(coordinator.run_once(now).await.unwrap());
    }
    assert!(matches!(
        last,
        Some(CoordinatorRunOutcome::Completed {
            outcome: CycleOutcome::Candidate { .. },
            ..
        })
    ));
    assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    assert_eq!(service.submissions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cached_submission_retries_succeed_changed_rules_conflict_and_superseded_cache_fails() {
    use flaggo_analysis_workspace::{PrepareCycleResult, WorkspaceProvider};
    let (now, service, evidence, _) = workflow_fixture(Script::Collaborate);
    let directory = tempfile::tempdir().unwrap();
    let workspaces = LocalWorkspaceProvider::new(directory.path()).unwrap();
    let bound = service
        .get_current_contract("checkout.delay")
        .await
        .unwrap()
        .unwrap();
    let lease = workspaces.claim(&bound.name, "author").await.unwrap();
    let PrepareCycleResult::Ready(preparation) = lease
        .prepare_cycle(
            &bound,
            &AnalysisProfile {
                skill_names: vec![],
            },
            "author",
            now,
        )
        .await
        .unwrap()
    else {
        panic!("expected cycle")
    };
    let factory = LocalCapabilityFactory::new(
        service.clone(),
        evidence,
        EvidenceQueryLimits {
            max_rows: NonZeroU32::new(10).unwrap(),
            max_bytes: 4096,
            timeout: Duration::from_secs(1),
        },
        Arc::new(FixedClock(now)),
        flaggo_async_analysis::python::PythonConfig::default(),
    );
    use flaggo_analysis_agent::AnalysisCapabilityFactory;
    let tools = factory
        .create(
            AttemptContext::new(&preparation.cycle, "author".to_owned()),
            bound.clone(),
            lease,
        )
        .await
        .unwrap();
    let rules =
        json!([{"name": "learned", "when": {"expression": "true"}, "return": {"value": 5}}]);
    let candidate = tools.propose_executable(rules.clone()).await.unwrap();
    assert_eq!(
        tools.propose_executable(rules.clone()).await.unwrap(),
        candidate
    );
    assert!(matches!(
        tools.propose_executable(json!([{"changed": true}])).await,
        Err(AnalysisError::ProposalConflict(_))
    ));
    service.set_current(contract('b', bound.accepted_at));
    assert!(matches!(
        tools.propose_executable(rules).await,
        Err(AnalysisError::Superseded(_))
    ));
    assert_eq!(service.submissions.lock().unwrap().len(), 1);
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
        .join("analysis")
        .join("supersession.md");
    assert_eq!(
        std::fs::read_to_string(checkpoint).expect("supersession checkpoint"),
        "Checkpointed after the trusted supersession signal."
    );
}

#[tokio::test]
async fn final_currentness_check_supersedes_an_agent_result_that_wins_the_poll_race() {
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
        script: Script::SupersedeBeforeCompletion,
        starts: AtomicUsize::new(0),
        contract_service: Arc::clone(&service),
        replacement: Some(new_contract.clone()),
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator_with_poll_interval(
        directory.path(),
        service,
        evidence,
        provider,
        now,
        Duration::from_secs(60),
    );

    let outcome = coordinator
        .run_once(now)
        .await
        .expect("final currentness check");
    let CoordinatorRunOutcome::Completed {
        outcome: CycleOutcome::Superseded { replacement_digest },
        ..
    } = outcome
    else {
        panic!("expected coordinator supersession, got {outcome:?}");
    };
    assert_eq!(
        replacement_digest.as_deref(),
        Some(new_contract.contract_digest.as_str())
    );
}

#[tokio::test]
async fn coordinator_finalizes_supersession_when_the_agent_ignores_the_yield_signal() {
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
        script: Script::IgnoreSupersession,
        starts: AtomicUsize::new(0),
        contract_service: Arc::clone(&service),
        replacement: Some(new_contract.clone()),
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(directory.path(), service, evidence, provider, now);

    let outcome = tokio::time::timeout(Duration::from_secs(2), coordinator.run_once(now))
        .await
        .expect("coordinator should enforce the yield grace period")
        .expect("superseded attempt");
    let CoordinatorRunOutcome::Completed {
        outcome: CycleOutcome::Superseded { replacement_digest },
        ..
    } = outcome
    else {
        panic!("expected coordinator supersession, got {outcome:?}");
    };
    assert_eq!(
        replacement_digest.as_deref(),
        Some(new_contract.contract_digest.as_str())
    );
}

#[tokio::test]
async fn rejects_agent_claimed_supersession_without_a_coordinator_signal() {
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
        script: Script::ClaimSupersession,
        starts: AtomicUsize::new(0),
        contract_service: Arc::clone(&service),
        replacement: None,
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let coordinator = coordinator(directory.path(), service, evidence, provider, now);

    let outcome = coordinator
        .run_once(now)
        .await
        .expect("agent-claimed supersession");
    let CoordinatorRunOutcome::Recoverable {
        failure_category,
        reason,
        ..
    } = outcome
    else {
        panic!("expected recoverable integrity failure, got {outcome:?}");
    };
    assert_eq!(failure_category, "integrity");
    assert!(reason.contains("without a coordinator supersession signal"));
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
