use std::{num::NonZeroUsize, sync::Arc};

use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::Instrument as _;

use flaggo_analysis_domain::{
    AnalysisContract, AnalysisError, AttemptContext, CycleLimits, WorkflowHandoff,
};
use flaggo_analysis_workspace::WorkspaceFileSystem;

use crate::AnalysisTools;
use crate::action::{ActionKind, ActionObservation, ActionOutcome, FailureCategory};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRunOutcome {
    Candidate,
    NoChange {
        reason: String,
    },
    NoCandidate {
        reason: String,
    },
    Failure {
        reason: String,
    },
    Handoff {
        summary: String,
        metadata: Option<serde_json::Value>,
    },
    Reject {
        reason: String,
        review_path: String,
    },
    Superseded,
}

pub struct AgentSessionSpec {
    pub context: AttemptContext,
    pub contract: AnalysisContract,
    pub filesystem: Arc<dyn WorkspaceFileSystem>,
    pub tools: Arc<dyn AnalysisTools>,
    pub task: AnalysisTask,
    pub handoff: Option<WorkflowHandoff>,
    pub cycle_limits: CycleLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisTask {
    pub prompt: String,
}

#[async_trait]
pub trait AgentSession: Send + Sync {
    async fn run(&self, task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError>;

    async fn request_yield(&self, reason: &str) -> Result<(), AnalysisError>;

    async fn close(&self) -> Result<(), AnalysisError>;
}

#[async_trait]
pub trait AgentProvider: Send + Sync {
    async fn start_session(
        &self,
        specification: AgentSessionSpec,
    ) -> Result<Arc<dyn AgentSession>, AnalysisError>;
}

pub struct AgentPool {
    provider: Arc<dyn AgentProvider>,
    permits: Arc<Semaphore>,
}

impl AgentPool {
    #[must_use]
    pub fn new(provider: Arc<dyn AgentProvider>, capacity: NonZeroUsize) -> Self {
        Self {
            provider,
            permits: Arc::new(Semaphore::new(capacity.get())),
        }
    }

    pub async fn acquire(
        &self,
        specification: AgentSessionSpec,
    ) -> Result<AgentLease, AnalysisError> {
        let context = specification.context.clone();
        let parent = tracing::Span::current();
        let mut action = ActionObservation::start(&context, ActionKind::SessionStart, &parent);
        let result = async {
            let permit = self
                .permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| AnalysisError::Agent("agent pool is closed".to_owned()))?;
            match self.provider.start_session(specification).await {
                Ok(session) => Ok(AgentLease {
                    session,
                    context,
                    parent,
                    _permit: permit,
                    closed: tokio::sync::Mutex::new(false),
                }),
                Err(error) => {
                    drop(permit);
                    Err(error)
                }
            }
        }
        .instrument(action.span())
        .await;
        action.finish_result(&result);
        result
    }
}

pub struct AgentLease {
    session: Arc<dyn AgentSession>,
    context: AttemptContext,
    parent: tracing::Span,
    _permit: OwnedSemaphorePermit,
    closed: tokio::sync::Mutex<bool>,
}

impl AgentLease {
    pub async fn run(&self, task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        let mut action =
            ActionObservation::start(&self.context, ActionKind::SessionRun, &self.parent);
        let result = self.session.run(task).instrument(action.span()).await;
        match &result {
            Ok(outcome) => {
                let (outcome, category) = match outcome {
                    AgentRunOutcome::Candidate => (ActionOutcome::Success, None),
                    AgentRunOutcome::NoChange { .. } => (ActionOutcome::NoChange, None),
                    AgentRunOutcome::NoCandidate { .. } => (ActionOutcome::NoCandidate, None),
                    AgentRunOutcome::Reject { .. } => (ActionOutcome::NoCandidate, None),
                    AgentRunOutcome::Failure { .. } => {
                        (ActionOutcome::Failed, Some(FailureCategory::Internal))
                    }
                    AgentRunOutcome::Handoff { .. } => (ActionOutcome::Handoff, None),
                    AgentRunOutcome::Superseded => (ActionOutcome::Superseded, None),
                };
                action.finish(outcome, category);
            }
            Err(_) => action.finish_result(&result),
        }
        result
    }

    pub async fn request_yield(&self, reason: &str) -> Result<(), AnalysisError> {
        self.session.request_yield(reason).await
    }

    pub async fn close(&self) -> Result<(), AnalysisError> {
        let mut closed = self.closed.lock().await;
        if !*closed {
            self.session.close().await?;
            *closed = true;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{num::NonZeroUsize, sync::Arc, time::Duration};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use tokio::time::timeout;

    use flaggo_analysis_domain::{
        AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisError,
        AttemptContext, CandidateRecord,
    };
    use flaggo_analysis_workspace::{WorkspaceEntry, WorkspaceFileInfo, WorkspaceFileSystem};

    use crate::{
        AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisRunStatus,
        AnalysisTask, AnalysisTools, YieldSignal,
    };

    struct TestProvider;

    #[async_trait]
    impl AgentProvider for TestProvider {
        async fn start_session(
            &self,
            _specification: AgentSessionSpec,
        ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
            Ok(Arc::new(TestSession))
        }
    }

    struct TestSession;

    #[async_trait]
    impl AgentSession for TestSession {
        async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
            Ok(AgentRunOutcome::NoCandidate {
                reason: "test".to_owned(),
            })
        }

        async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
            Ok(())
        }

        async fn close(&self) -> Result<(), AnalysisError> {
            Ok(())
        }
    }

    struct TestFileSystem;

    #[async_trait]
    impl WorkspaceFileSystem for TestFileSystem {
        async fn read_text(&self, path: &str) -> Result<String, AnalysisError> {
            if path == "missing" {
                return Err(AnalysisError::Workspace(
                    "sensitive missing path".to_owned(),
                ));
            }
            Ok(String::new())
        }

        async fn write_text(&self, path: &str, _content: &str) -> Result<(), AnalysisError> {
            if path == "denied" {
                return Err(AnalysisError::Workspace("sensitive denied path".to_owned()));
            }
            Ok(())
        }

        async fn append_text(&self, _path: &str, _content: &str) -> Result<(), AnalysisError> {
            Ok(())
        }

        async fn exists(&self, _path: &str) -> Result<bool, AnalysisError> {
            Ok(false)
        }

        async fn metadata(&self, _path: &str) -> Result<WorkspaceFileInfo, AnalysisError> {
            Err(AnalysisError::Workspace("not found".to_owned()))
        }

        async fn create_directory(
            &self,
            _path: &str,
            _recursive: bool,
        ) -> Result<(), AnalysisError> {
            Ok(())
        }

        async fn read_directory(&self, _path: &str) -> Result<Vec<WorkspaceEntry>, AnalysisError> {
            Ok(Vec::new())
        }

        async fn remove(
            &self,
            _path: &str,
            _recursive: bool,
            _force: bool,
        ) -> Result<(), AnalysisError> {
            Ok(())
        }

        async fn rename(&self, _source: &str, _destination: &str) -> Result<(), AnalysisError> {
            Ok(())
        }
    }

    struct TestTools {
        can_propose: bool,
        yield_signal: Arc<YieldSignal>,
        context: AttemptContext,
    }

    #[async_trait]
    impl AnalysisTools for TestTools {
        fn can_propose(&self) -> bool {
            self.can_propose
        }
        async fn check_cycle_status(&self) -> Result<AnalysisRunStatus, AnalysisError> {
            self.yield_signal.status()
        }

        async fn describe(&self) -> Result<serde_json::Value, AnalysisError> {
            let mut action = crate::action::ActionObservation::start(
                &self.context,
                crate::action::ActionKind::EvidenceDescribe,
                &tracing::Span::current(),
            );
            tokio::task::yield_now().await;
            action.finish(crate::action::ActionOutcome::Success, None);
            Ok(json!({"private": "not public telemetry"}))
        }

        async fn run_sql(
            &self,
            sql: &str,
            row_limit: Option<u32>,
        ) -> Result<serde_json::Value, AnalysisError> {
            Ok(
                json!({"requestedSql": sql, "rowLimit": row_limit, "resultPath": "queries/test.json"}),
            )
        }

        async fn code_interpreter(
            &self,
            input: crate::PythonInput<'_>,
        ) -> Result<serde_json::Value, AnalysisError> {
            Ok(match input {
                crate::PythonInput::ScriptPath(path) => json!({"scriptPath": path}),
                crate::PythonInput::Code(code) => json!({"code": code}),
            })
        }

        async fn propose_executable(
            &self,
            _rules: serde_json::Value,
        ) -> Result<CandidateRecord, AnalysisError> {
            unreachable!()
        }

        async fn proposed_candidate(&self) -> Result<Option<CandidateRecord>, AnalysisError> {
            Ok(None)
        }

        fn yield_signal(&self) -> Arc<YieldSignal> {
            self.yield_signal.clone()
        }
    }

    #[tokio::test]
    async fn pool_holds_capacity_until_lease_is_released() {
        let pool = super::AgentPool::new(Arc::new(TestProvider), NonZeroUsize::new(1).unwrap());
        let first = pool.acquire(test_spec()).await.unwrap();
        assert!(
            timeout(Duration::from_millis(20), pool.acquire(test_spec()))
                .await
                .is_err()
        );
        drop(first);
        timeout(Duration::from_secs(1), pool.acquire(test_spec()))
            .await
            .unwrap()
            .unwrap();
    }

    struct OutcomeProvider {
        outcome: Option<AgentRunOutcome>,
        fail_start: bool,
        fail_run: bool,
    }

    #[async_trait]
    impl AgentProvider for OutcomeProvider {
        async fn start_session(
            &self,
            _specification: AgentSessionSpec,
        ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
            if self.fail_start {
                return Err(AnalysisError::Agent("private provider token".to_owned()));
            }
            Ok(Arc::new(OutcomeSession {
                outcome: self.outcome.clone(),
                fail_run: self.fail_run,
            }))
        }
    }

    struct OutcomeSession {
        outcome: Option<AgentRunOutcome>,
        fail_run: bool,
    }

    #[async_trait]
    impl AgentSession for OutcomeSession {
        async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
            tokio::task::yield_now().await;
            if self.fail_run {
                return Err(AnalysisError::Agent("private provider token".to_owned()));
            }
            if let Some(outcome) = &self.outcome {
                Ok(outcome.clone())
            } else {
                std::future::pending().await
            }
        }
        async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
            Ok(())
        }
        async fn close(&self) -> Result<(), AnalysisError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn emits_provider_neutral_sessions_with_accurate_domain_outcomes() {
        use tracing::Instrument as _;
        for (outcome, expected) in [
            (AgentRunOutcome::Candidate, "success"),
            (
                AgentRunOutcome::NoChange {
                    reason: "private".to_owned(),
                },
                "no_change",
            ),
            (
                AgentRunOutcome::NoCandidate {
                    reason: "private".to_owned(),
                },
                "no_candidate",
            ),
            (
                AgentRunOutcome::Failure {
                    reason: "private".to_owned(),
                },
                "failed",
            ),
            (
                AgentRunOutcome::Handoff {
                    summary: "private".to_owned(),
                    metadata: None,
                },
                "handoff",
            ),
            (AgentRunOutcome::Superseded, "superseded"),
        ] {
            let capture = crate::telemetry_support::Capture::default();
            capture
                .run(async {
                    let parent = tracing::info_span!("cycle");
                    async {
                        let pool = super::AgentPool::new(
                            Arc::new(OutcomeProvider {
                                outcome: Some(outcome),
                                fail_start: false,
                                fail_run: false,
                            }),
                            NonZeroUsize::new(1).unwrap(),
                        );
                        let lease = pool.acquire(test_spec()).await.unwrap();
                        lease
                            .run(AnalysisTask {
                                prompt: "private prompt".to_owned(),
                            })
                            .await
                            .unwrap();
                    }
                    .instrument(parent)
                    .await;
                })
                .await;
            let events = capture.events.lock().unwrap();
            assert_eq!(events.len(), 4);
            assert_eq!(
                events[1].fields["flaggo.analysis.action.kind"],
                "session.start"
            );
            assert_eq!(
                events[1].fields["flaggo.analysis.action.outcome"],
                "success"
            );
            assert_eq!(
                events[3].fields["flaggo.analysis.action.kind"],
                "session.run"
            );
            assert_eq!(events[3].fields["flaggo.analysis.action.outcome"], expected);
            assert!(
                !serde_json::to_string(&events[3].fields)
                    .unwrap()
                    .contains("private")
            );
            let spans = capture.spans.lock().unwrap();
            for span in spans
                .values()
                .filter(|span| span.name == "flaggo.analysis.action")
            {
                assert!(span.closed);
                assert_eq!(spans[&span.parent.unwrap()].name, "cycle");
            }
        }
    }

    #[tokio::test]
    async fn observes_session_start_errors_run_errors_and_dropped_runs() {
        for (fail_start, fail_run, expected, count) in [
            (true, false, "failed", 2),
            (false, true, "failed", 4),
            (false, false, "interrupted", 4),
        ] {
            let capture = crate::telemetry_support::Capture::default();
            capture
                .run(async {
                    let pool = super::AgentPool::new(
                        Arc::new(OutcomeProvider {
                            outcome: None,
                            fail_start,
                            fail_run,
                        }),
                        NonZeroUsize::new(1).unwrap(),
                    );
                    if let Ok(lease) = pool.acquire(test_spec()).await {
                        assert!(
                            timeout(
                                Duration::from_millis(1),
                                lease.run(AnalysisTask {
                                    prompt: "private".to_owned()
                                })
                            )
                            .await
                            .map_or(true, |result| result.is_err())
                        );
                    } else {
                        assert!(fail_start);
                    }
                })
                .await;
            let events = capture.events.lock().unwrap();
            assert_eq!(events.len(), count);
            let terminal = &events.last().unwrap().fields;
            assert_eq!(terminal["flaggo.analysis.action.outcome"], expected);
            assert!(
                terminal["flaggo.analysis.action.duration_ms"]
                    .as_u64()
                    .is_some()
            );
            assert!(!serde_json::to_string(terminal).unwrap().contains("private"));
        }
    }

    #[tokio::test]
    async fn observes_content_filesystem_actions_but_not_metadata_or_internal_noise() {
        use tracing::Instrument as _;
        let capture = crate::telemetry_support::Capture::default();
        capture
            .run(async {
                let parent = tracing::info_span!("cycle");
                async {
                    let filesystem = crate::ObservedWorkspaceFileSystem::new(
                        Arc::new(TestFileSystem),
                        test_spec().context,
                    );
                    filesystem.exists("private path").await.unwrap();
                    filesystem
                        .create_directory("private path", true)
                        .await
                        .unwrap();
                    filesystem.read_directory("private path").await.unwrap();
                    filesystem
                        .write_text(
                            "/workspace/analysis/.copilot/internal.json",
                            "private token",
                        )
                        .await
                        .unwrap();
                    filesystem
                        .read_text("/workspace/.git/internal")
                        .await
                        .unwrap();
                    filesystem
                        .read_text("/workspace/analysis/findings.md")
                        .await
                        .unwrap();
                    filesystem
                        .write_text("/workspace/analysis/findings.md", "private prompt")
                        .await
                        .unwrap();
                    filesystem
                        .append_text("/workspace/analysis/findings.md", "private appendix")
                        .await
                        .unwrap();
                    assert!(filesystem.read_text("missing").await.is_err());
                    assert!(
                        filesystem
                            .write_text("denied", "private content")
                            .await
                            .is_err()
                    );
                }
                .instrument(parent)
                .await;
            })
            .await;
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 10);
        for (index, (kind, size)) in [
            ("artifact.read", 0_u64),
            ("artifact.write", 14),
            ("artifact.append", 16),
        ]
        .into_iter()
        .enumerate()
        {
            let terminal = &events[index * 2 + 1].fields;
            assert_eq!(terminal["flaggo.analysis.action.kind"], kind);
            assert_eq!(terminal["flaggo.analysis.action.outcome"], "success");
            assert_eq!(terminal["flaggo.analysis.result.bytes"], size);
            assert_eq!(
                terminal["flaggo.analysis.artifact.digest"]
                    .as_str()
                    .unwrap()
                    .len(),
                71
            );
        }
        for terminal in [&events[7], &events[9]] {
            assert_eq!(terminal.fields["flaggo.analysis.action.outcome"], "failed");
            assert!(
                !terminal
                    .fields
                    .contains_key("flaggo.analysis.artifact.digest")
            );
            assert!(!terminal.fields.contains_key("flaggo.analysis.result.bytes"));
        }
        for event in events.iter() {
            let encoded = serde_json::to_string(&event.fields).unwrap();
            for forbidden in [
                "private",
                "findings.md",
                "missing path",
                "denied path",
                "/workspace",
            ] {
                assert!(!encoded.contains(forbidden));
            }
        }
    }

    pub(crate) fn test_spec() -> AgentSessionSpec {
        test_spec_with_proposal_permission(true)
    }

    pub(crate) fn test_spec_with_proposal_permission(can_propose: bool) -> AgentSessionSpec {
        let accepted_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
        let contract = AnalysisContract {
            name: "test.contract".to_owned(),
            contract_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            accepted_at,
            authority: AnalysisAuthority {
                tenant: "local".to_owned(),
                application: "test".to_owned(),
                environment: "test".to_owned(),
            },
            evaluation_interval: "PT1M".to_owned(),
            evidence_sources: Vec::new(),
            contract: json!({}),
        };
        let context = AttemptContext {
            workspace_id: "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_owned(),
            cycle_id: "cycle".to_owned(),
            attempt_id: "attempt".to_owned(),
            contract: AnalysisContractIdentity {
                name: contract.name.clone(),
                digest: contract.contract_digest.clone(),
            },
        };
        AgentSessionSpec {
            handoff: None,
            cycle_limits: Default::default(),
            context: context.clone(),
            contract,
            filesystem: Arc::new(TestFileSystem),
            tools: Arc::new(TestTools {
                can_propose,
                yield_signal: Arc::new(YieldSignal::default()),
                context,
            }),
            task: AnalysisTask {
                prompt: "test".to_owned(),
            },
        }
    }
}
