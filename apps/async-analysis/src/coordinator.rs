use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flaggo_analysis_agent::{
    AgentPool, AgentRunOutcome, AgentSessionSpec, AnalysisCapabilityFactory, AnalysisTask,
};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisContractIdentity, AnalysisError, AnalysisProfile, AttemptContext,
    CandidateRecord, CatalogUpdate, CycleOutcome, EvidenceCutoff,
};
use flaggo_analysis_workspace::{PrepareCycleResult, WorkspaceProvider};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{RwLock, watch};
use tracing::Instrument as _;
use uuid::Uuid;

use crate::observability::{AnalysisObservability, CycleObservation, analysis_failure_category};

#[async_trait]
pub trait ContractService: Send + Sync {
    async fn list_current_learning_contracts(
        &self,
        revision: Option<&str>,
    ) -> Result<CatalogUpdate, AnalysisError>;

    async fn get_current_contract(
        &self,
        contract_name: &str,
    ) -> Result<Option<AnalysisContract>, AnalysisError>;

    async fn submit_candidate(
        &self,
        context: &AttemptContext,
        rules: Value,
        cutoff: &EvidenceCutoff,
        analysis_manifest_digest: &str,
    ) -> Result<CandidateRecord, AnalysisError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone, Debug)]
pub struct CoordinatorConfig {
    pub supersession_poll_interval: Duration,
    pub yield_grace_period: Duration,
    pub analysis_profile: AnalysisProfile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CoordinatorRunOutcome {
    Idle,
    NotEligible {
        contract_name: String,
        eligible_at: DateTime<Utc>,
    },
    Completed {
        contract_name: String,
        cycle_id: String,
        outcome: CycleOutcome,
    },
    Recoverable {
        contract_name: String,
        cycle_id: String,
        reason: String,
        failure_category: &'static str,
    },
    Busy {
        contract_name: String,
    },
}

struct CatalogState {
    revision: Option<String>,
    contracts: Vec<AnalysisContractIdentity>,
}

pub struct AnalysisCoordinator {
    contract_service: Arc<dyn ContractService>,
    workspace_provider: Arc<dyn WorkspaceProvider>,
    capabilities: Arc<dyn AnalysisCapabilityFactory>,
    agent_pool: AgentPool,
    config: CoordinatorConfig,
    clock: Arc<dyn Clock>,
    catalog: RwLock<CatalogState>,
    observability: AnalysisObservability,
}

impl AnalysisCoordinator {
    #[must_use]
    pub fn new(
        contract_service: Arc<dyn ContractService>,
        workspace_provider: Arc<dyn WorkspaceProvider>,
        capabilities: Arc<dyn AnalysisCapabilityFactory>,
        agent_pool: AgentPool,
        config: CoordinatorConfig,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            contract_service,
            workspace_provider,
            capabilities,
            agent_pool,
            config,
            clock,
            catalog: RwLock::new(CatalogState {
                revision: None,
                contracts: Vec::new(),
            }),
            observability: AnalysisObservability::new(),
        }
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<CoordinatorRunOutcome, AnalysisError> {
        self.run_once_internal(now, None).await
    }

    pub async fn run_once_until(
        &self,
        now: DateTime<Utc>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<CoordinatorRunOutcome, AnalysisError> {
        self.run_once_internal(now, Some(shutdown)).await
    }

    async fn run_once_internal(
        &self,
        now: DateTime<Utc>,
        shutdown: Option<watch::Receiver<bool>>,
    ) -> Result<CoordinatorRunOutcome, AnalysisError> {
        if shutdown.as_ref().is_some_and(|receiver| *receiver.borrow()) {
            return Ok(CoordinatorRunOutcome::Idle);
        }
        self.refresh_catalog().await?;
        let contracts = self.catalog.read().await.contracts.clone();
        let mut delayed = None;
        for catalog_contract in contracts {
            let Some(contract) = self
                .contract_service
                .get_current_contract(&catalog_contract.name)
                .await?
            else {
                continue;
            };
            if contract.evidence_sources.is_empty() {
                continue;
            }
            match self.run_contract(contract, now, shutdown.clone()).await? {
                outcome @ CoordinatorRunOutcome::NotEligible { .. } => {
                    delayed.get_or_insert(outcome);
                }
                CoordinatorRunOutcome::Busy { .. } => {}
                CoordinatorRunOutcome::Idle => {}
                outcome => return Ok(outcome),
            }
        }
        Ok(delayed.unwrap_or(CoordinatorRunOutcome::Idle))
    }

    async fn refresh_catalog(&self) -> Result<(), AnalysisError> {
        let revision = self.catalog.read().await.revision.clone();
        if let CatalogUpdate::Updated {
            revision,
            contracts,
        } = self
            .contract_service
            .list_current_learning_contracts(revision.as_deref())
            .await?
        {
            *self.catalog.write().await = CatalogState {
                revision: Some(revision),
                contracts,
            };
        }
        Ok(())
    }

    async fn run_contract(
        &self,
        contract: AnalysisContract,
        now: DateTime<Utc>,
        shutdown: Option<watch::Receiver<bool>>,
    ) -> Result<CoordinatorRunOutcome, AnalysisError> {
        let mut observation = self.observability.start_cycle(&contract);
        let span = observation.span();
        let result = self
            .run_contract_observed(contract, now, shutdown, &mut observation)
            .instrument(span)
            .await;
        observation.finish(&result);
        result
    }

    async fn run_contract_observed(
        &self,
        contract: AnalysisContract,
        now: DateTime<Utc>,
        mut shutdown: Option<watch::Receiver<bool>>,
        observation: &mut CycleObservation,
    ) -> Result<CoordinatorRunOutcome, AnalysisError> {
        let attempt_id = Uuid::new_v4().to_string();
        let workspace = match self
            .workspace_provider
            .claim(&contract.name, &attempt_id)
            .await
        {
            Ok(workspace) => workspace,
            Err(AnalysisError::WorkspaceBusy(_)) => {
                return Ok(CoordinatorRunOutcome::Busy {
                    contract_name: contract.name,
                });
            }
            Err(error) => return Err(error),
        };
        let preparation = workspace
            .prepare_cycle(&contract, &self.config.analysis_profile, &attempt_id, now)
            .await?;
        let preparation = match preparation {
            PrepareCycleResult::Ready(preparation) => preparation,
            PrepareCycleResult::NotEligible { eligible_at } => {
                return Ok(CoordinatorRunOutcome::NotEligible {
                    contract_name: contract.name,
                    eligible_at,
                });
            }
        };
        let cycle = preparation.cycle;
        observation.record_active_attempt(&cycle.cycle_id, &attempt_id);
        let _active_worker = self.observability.worker_active();
        let context = AttemptContext::new(&cycle, attempt_id);
        let tools = self
            .capabilities
            .create(context.clone(), contract.clone(), workspace.clone())
            .await?;
        let filesystem = workspace
            .filesystem(&cycle.cycle_id, &context.attempt_id)
            .await?;
        let task = AnalysisTask {
            prompt: analysis_prompt(&context),
        };
        let lease = self
            .agent_pool
            .acquire(AgentSessionSpec {
                context: context.clone(),
                contract: contract.clone(),
                filesystem,
                tools: tools.clone(),
                task: task.clone(),
            })
            .await?;
        let mut run = Box::pin(lease.run(task));
        let agent_outcome = loop {
            tokio::select! {
                result = &mut run => break result,
                () = tokio::time::sleep(self.config.supersession_poll_interval) => {
                    let current = match self
                        .contract_service
                        .get_current_contract(&contract.name)
                        .await
                    {
                        Ok(current) => current,
                        Err(error) => {
                            lease.close().await?;
                            workspace
                                .record_attempt_failure(
                                    &cycle.cycle_id,
                                    &context.attempt_id,
                                    &error.to_string(),
                                )
                                .await?;
                            return Ok(CoordinatorRunOutcome::Recoverable {
                                contract_name: contract.name,
                                cycle_id: cycle.cycle_id,
                                reason: error.to_string(),
                                failure_category: analysis_failure_category(&error),
                            });
                        }
                    };
                    let current_digest = current.map(|current| current.contract_digest);
                    if current_digest.as_deref() != Some(contract.contract_digest.as_str()) {
                        workspace
                            .mark_superseding(&cycle.cycle_id, current_digest.as_deref())
                            .await?;
                        tools
                            .yield_signal()
                            .request_contract_superseded(current_digest.clone())?;
                        lease
                            .request_yield("The contract digest changed. Checkpoint and stop.")
                            .await?;
                        match tokio::time::timeout(self.config.yield_grace_period, &mut run).await {
                            Ok(Ok(AgentRunOutcome::Superseded)) => {}
                            Ok(Ok(_)) => {
                                workspace
                                    .record_attempt_failure(
                                        &cycle.cycle_id,
                                        &context.attempt_id,
                                        "agent completed without acknowledging the coordinator supersession signal",
                                    )
                                    .await?;
                            }
                            Ok(Err(_)) => {
                                workspace
                                    .record_attempt_failure(
                                        &cycle.cycle_id,
                                        &context.attempt_id,
                                        "agent failed while responding to the coordinator supersession signal",
                                    )
                                    .await?;
                            }
                            Err(_) => {
                                workspace
                                    .record_attempt_failure(
                                        &cycle.cycle_id,
                                        &context.attempt_id,
                                        "agent did not acknowledge supersession within the yield grace period",
                                    )
                                    .await?;
                            }
                        }
                        lease.close().await?;
                        let outcome = CycleOutcome::Superseded {
                            replacement_digest: current_digest,
                        };
                        workspace
                            .complete_cycle(
                                &contract,
                                &cycle.cycle_id,
                                &outcome,
                                self.clock.now(),
                            )
                            .await?;
                        return Ok(CoordinatorRunOutcome::Completed {
                            contract_name: contract.name,
                            cycle_id: cycle.cycle_id,
                            outcome,
                        });
                    }
                }
                () = wait_for_shutdown(&mut shutdown) => {
                    tools.yield_signal().request_shutdown()?;
                    lease
                        .request_yield(
                            "The service is shutting down. Commit a checkpoint and hand off.",
                        )
                        .await?;
                    match tokio::time::timeout(self.config.yield_grace_period, &mut run).await {
                        Ok(result) => break result,
                        Err(_) => {
                            let reason =
                                "service shutdown interrupted the attempt after its yield grace period";
                            workspace
                                .record_attempt_failure(
                                    &cycle.cycle_id,
                                    &context.attempt_id,
                                    reason,
                                )
                                .await?;
                            lease.close().await?;
                            return Ok(CoordinatorRunOutcome::Recoverable {
                                contract_name: contract.name,
                                cycle_id: cycle.cycle_id,
                                reason: reason.to_owned(),
                                failure_category: "timeout",
                            });
                        }
                    }
                }
            }
        };

        let agent_outcome = match agent_outcome {
            Ok(AgentRunOutcome::Handoff { summary }) => {
                workspace
                    .record_handoff(&cycle.cycle_id, &context.attempt_id, &summary)
                    .await?;
                lease.close().await?;
                return Ok(CoordinatorRunOutcome::Recoverable {
                    contract_name: contract.name,
                    cycle_id: cycle.cycle_id,
                    reason: "agent committed a handoff checkpoint".to_owned(),
                    failure_category: "internal",
                });
            }
            Err(error) => {
                workspace
                    .record_attempt_failure(
                        &cycle.cycle_id,
                        &context.attempt_id,
                        &error.to_string(),
                    )
                    .await?;
                lease.close().await?;
                return Ok(CoordinatorRunOutcome::Recoverable {
                    contract_name: contract.name,
                    cycle_id: cycle.cycle_id,
                    reason: error.to_string(),
                    failure_category: analysis_failure_category(&error),
                });
            }
            Ok(outcome) => outcome,
        };

        let current = match self
            .contract_service
            .get_current_contract(&contract.name)
            .await
        {
            Ok(current) => current,
            Err(error) => {
                workspace
                    .record_attempt_failure(
                        &cycle.cycle_id,
                        &context.attempt_id,
                        &error.to_string(),
                    )
                    .await?;
                lease.close().await?;
                return Ok(CoordinatorRunOutcome::Recoverable {
                    contract_name: contract.name,
                    cycle_id: cycle.cycle_id,
                    reason: error.to_string(),
                    failure_category: analysis_failure_category(&error),
                });
            }
        };
        let current_digest = current.map(|current| current.contract_digest);
        if current_digest.as_deref() != Some(contract.contract_digest.as_str()) {
            workspace
                .mark_superseding(&cycle.cycle_id, current_digest.as_deref())
                .await?;
            lease.close().await?;
            let outcome = CycleOutcome::Superseded {
                replacement_digest: current_digest,
            };
            workspace
                .complete_cycle(&contract, &cycle.cycle_id, &outcome, self.clock.now())
                .await?;
            return Ok(CoordinatorRunOutcome::Completed {
                contract_name: contract.name,
                cycle_id: cycle.cycle_id,
                outcome,
            });
        }

        let completed_without_candidate = match &agent_outcome {
            AgentRunOutcome::NoChange { .. } => Some("no change"),
            AgentRunOutcome::NoCandidate { .. } => Some("no Candidate"),
            _ => None,
        };
        if let Some(reported_outcome) = completed_without_candidate {
            let invalid = if workspace.cutoff(&cycle.cycle_id).await?.is_none() {
                Some(format!(
                    "agent reported {reported_outcome} before committing an evidence cutoff"
                ))
            } else if tools.proposed_candidate().await?.is_some() {
                Some(format!(
                    "agent reported {reported_outcome} after a successful proposal tool call"
                ))
            } else {
                None
            };
            if let Some(message) = invalid {
                let error = AnalysisError::InvalidState(message);
                workspace
                    .record_attempt_failure(
                        &cycle.cycle_id,
                        &context.attempt_id,
                        &error.to_string(),
                    )
                    .await?;
                lease.close().await?;
                return Ok(CoordinatorRunOutcome::Recoverable {
                    contract_name: contract.name,
                    cycle_id: cycle.cycle_id,
                    reason: error.to_string(),
                    failure_category: "integrity",
                });
            }
        }

        let outcome = match agent_outcome {
            AgentRunOutcome::Candidate => {
                let candidate = tools.proposed_candidate().await?.ok_or_else(|| {
                    AnalysisError::InvalidState(
                        "agent reported a Candidate without a successful proposal tool call"
                            .to_owned(),
                    )
                })?;
                CycleOutcome::Candidate { candidate }
            }
            AgentRunOutcome::NoChange { reason } => CycleOutcome::NoChange { reason },
            AgentRunOutcome::NoCandidate { reason } => CycleOutcome::NoCandidate { reason },
            AgentRunOutcome::Failure { reason } => CycleOutcome::Failure { reason },
            AgentRunOutcome::Superseded => {
                let error = AnalysisError::InvalidState(
                    "agent reported supersession without a coordinator supersession signal"
                        .to_owned(),
                );
                workspace
                    .record_attempt_failure(
                        &cycle.cycle_id,
                        &context.attempt_id,
                        &error.to_string(),
                    )
                    .await?;
                lease.close().await?;
                return Ok(CoordinatorRunOutcome::Recoverable {
                    contract_name: contract.name,
                    cycle_id: cycle.cycle_id,
                    reason: error.to_string(),
                    failure_category: "integrity",
                });
            }
            AgentRunOutcome::Handoff { .. } => unreachable!("handoff returned above"),
        };
        lease.close().await?;
        workspace
            .complete_cycle(&contract, &cycle.cycle_id, &outcome, self.clock.now())
            .await?;
        Ok(CoordinatorRunOutcome::Completed {
            contract_name: contract.name,
            cycle_id: cycle.cycle_id,
            outcome,
        })
    }
}

async fn wait_for_shutdown(shutdown: &mut Option<watch::Receiver<bool>>) {
    let Some(shutdown) = shutdown else {
        std::future::pending::<()>().await;
        return;
    };
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            return;
        }
    }
}

fn analysis_prompt(context: &AttemptContext) -> String {
    format!(
        "Analyze the current cycle for contract '{}' at digest '{}'. \
         The trusted workspace is '{}', cycle is '{}', and attempt is '{}'. \
         Invoke the evidence-analysis skill, obey the coordinator-owned analysis status, \
         commit one evidence cutoff before querying, \
         directly analyze every primary-objective and guardrail evidence source, \
         checkpoint durable work in the workspace, and finish with exactly one JSON object: \
         {{\"outcome\":\"candidate\",\"explanation\":\"...\"}}, \
         {{\"outcome\":\"noChange\",\"explanation\":\"...\"}}, \
         {{\"outcome\":\"noCandidate\",\"explanation\":\"...\"}}, \
         {{\"outcome\":\"failure\",\"explanation\":\"...\"}}, \
         {{\"outcome\":\"handoff\",\"explanation\":\"...\"}}, or \
         {{\"outcome\":\"superseded\",\"explanation\":\"...\"}}.",
        context.contract.name,
        context.contract.digest,
        context.workspace_id,
        context.cycle_id,
        context.attempt_id
    )
}
