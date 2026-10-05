use std::{num::NonZeroUsize, sync::Arc};

use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use flaggo_analysis_domain::{AnalysisContract, AnalysisError, AttemptContext};
use flaggo_analysis_workspace::WorkspaceFileSystem;

use crate::AnalysisTools;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRunOutcome {
    Candidate,
    NoChange { reason: String },
    NoCandidate { reason: String },
    Failure { reason: String },
    Handoff { summary: String },
    Superseded,
}

pub struct AgentSessionSpec {
    pub context: AttemptContext,
    pub contract: AnalysisContract,
    pub filesystem: Arc<dyn WorkspaceFileSystem>,
    pub tools: Arc<dyn AnalysisTools>,
    pub task: AnalysisTask,
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
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AnalysisError::Agent("agent pool is closed".to_owned()))?;
        match self.provider.start_session(specification).await {
            Ok(session) => Ok(AgentLease {
                session,
                _permit: permit,
            }),
            Err(error) => {
                drop(permit);
                Err(error)
            }
        }
    }
}

pub struct AgentLease {
    session: Arc<dyn AgentSession>,
    _permit: OwnedSemaphorePermit,
}

impl AgentLease {
    pub async fn run(&self, task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        self.session.run(task).await
    }

    pub async fn request_yield(&self, reason: &str) -> Result<(), AnalysisError> {
        self.session.request_yield(reason).await
    }

    pub async fn close(&self) -> Result<(), AnalysisError> {
        self.session.close().await
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroUsize, sync::Arc, time::Duration};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use tokio::time::timeout;

    use flaggo_analysis_domain::{
        AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisError,
        AttemptContext, CandidateRecord, EvidenceCutoff,
    };
    use flaggo_analysis_workspace::{WorkspaceEntry, WorkspaceFileInfo, WorkspaceFileSystem};

    use crate::{
        AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisTask,
        AnalysisTools, CurrentContractCheck, YieldSignal,
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
        async fn read_text(&self, _path: &str) -> Result<String, AnalysisError> {
            Ok(String::new())
        }

        async fn write_text(&self, _path: &str, _content: &str) -> Result<(), AnalysisError> {
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
        yield_signal: Arc<YieldSignal>,
    }

    #[async_trait]
    impl AnalysisTools for TestTools {
        async fn check_current_contract(&self) -> Result<CurrentContractCheck, AnalysisError> {
            unreachable!()
        }

        async fn commit_evidence_cutoff(
            &self,
            _cutoff: chrono::DateTime<Utc>,
        ) -> Result<EvidenceCutoff, AnalysisError> {
            unreachable!()
        }

        async fn describe_evidence(&self) -> Result<serde_json::Value, AnalysisError> {
            unreachable!()
        }

        async fn query_evidence(&self, _sql: &str) -> Result<serde_json::Value, AnalysisError> {
            unreachable!()
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

    fn test_spec() -> AgentSessionSpec {
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
        AgentSessionSpec {
            context: AttemptContext {
                workspace_id:
                    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                        .to_owned(),
                cycle_id: "cycle".to_owned(),
                attempt_id: "attempt".to_owned(),
                contract: AnalysisContractIdentity {
                    name: contract.name.clone(),
                    digest: contract.contract_digest.clone(),
                },
            },
            contract,
            filesystem: Arc::new(TestFileSystem),
            tools: Arc::new(TestTools {
                yield_signal: Arc::new(YieldSignal::default()),
            }),
            task: AnalysisTask {
                prompt: "test".to_owned(),
            },
        }
    }
}
