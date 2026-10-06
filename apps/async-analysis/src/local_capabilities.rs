use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flaggo_analysis_agent::{
    AnalysisCapabilityFactory, AnalysisRunStatus, AnalysisTools, YieldSignal,
};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisError, AttemptContext, CandidateRecord, EvidenceCutoff,
};
use flaggo_analysis_workspace::WorkspaceLease;
use flaggo_evidence_store::{
    AuthorityScope, EvidenceAnalysisStore, EvidenceQueryLimits, EvidenceQueryRequest,
    EvidenceQueryScope, EvidenceSignal, EvidenceSourceSelector, ObservationWatermark,
};
use serde_json::{Value, json};

use crate::{Clock, ContractService};

const DECISION_OBSERVATION_SCOPE: &str = "@flaggo/sdk";
const DECISION_OBSERVATION_NAME: &str = "flaggo.decision.received";

pub struct LocalCapabilityFactory {
    contract_service: Arc<dyn ContractService>,
    evidence_store: Arc<dyn EvidenceAnalysisStore>,
    query_limits: EvidenceQueryLimits,
    clock: Arc<dyn Clock>,
}

impl LocalCapabilityFactory {
    #[must_use]
    pub fn new(
        contract_service: Arc<dyn ContractService>,
        evidence_store: Arc<dyn EvidenceAnalysisStore>,
        query_limits: EvidenceQueryLimits,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            contract_service,
            evidence_store,
            query_limits,
            clock,
        }
    }
}

#[async_trait]
impl AnalysisCapabilityFactory for LocalCapabilityFactory {
    async fn create(
        &self,
        context: AttemptContext,
        contract: AnalysisContract,
        workspace: Arc<dyn WorkspaceLease>,
    ) -> Result<Arc<dyn AnalysisTools>, AnalysisError> {
        let scope = evidence_scope(&contract)?;
        Ok(Arc::new(LocalAnalysisTools {
            context,
            contract,
            workspace,
            contract_service: Arc::clone(&self.contract_service),
            evidence_store: Arc::clone(&self.evidence_store),
            scope,
            query_limits: self.query_limits,
            clock: Arc::clone(&self.clock),
            yield_signal: Arc::new(YieldSignal::default()),
        }))
    }
}

struct LocalAnalysisTools {
    context: AttemptContext,
    contract: AnalysisContract,
    workspace: Arc<dyn WorkspaceLease>,
    contract_service: Arc<dyn ContractService>,
    evidence_store: Arc<dyn EvidenceAnalysisStore>,
    scope: EvidenceQueryScope,
    query_limits: EvidenceQueryLimits,
    clock: Arc<dyn Clock>,
    yield_signal: Arc<YieldSignal>,
}

#[async_trait]
impl AnalysisTools for LocalAnalysisTools {
    async fn check_analysis_status(&self) -> Result<AnalysisRunStatus, AnalysisError> {
        self.yield_signal.status()
    }

    async fn commit_evidence_cutoff(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<EvidenceCutoff, AnalysisError> {
        self.ensure_not_yielding()?;
        if cutoff > self.clock.now() {
            return Err(AnalysisError::Evidence(
                "evidence cutoff cannot be in the future".to_owned(),
            ));
        }
        if let Some(existing) = self.workspace.cutoff(&self.context.cycle_id).await? {
            if existing.cutoff != cutoff {
                return Err(AnalysisError::Evidence(
                    "evidence cutoff is immutable for the cycle".to_owned(),
                ));
            }
            return Ok(existing);
        }
        let watermark = self
            .evidence_store
            .capture_watermark()
            .await
            .map_err(evidence_error)?;
        let committed = self
            .workspace
            .commit_cutoff(&self.context.cycle_id, cutoff, watermark.get())
            .await?;
        self.workspace
            .append_audit(
                &self.context.cycle_id,
                "cutoff",
                serde_json::to_value(&committed).map_err(evidence_error)?,
            )
            .await?;
        Ok(committed)
    }

    async fn describe_evidence(&self) -> Result<Value, AnalysisError> {
        self.ensure_not_yielding()?;
        Ok(json!({
            "relation": "observations",
            "columns": [
                "observation_sequence",
                "observation_id",
                "logical_source_id",
                "content_digest",
                "signal_type",
                "instrumentation_scope",
                "signal_name",
                "metric_kind",
                "metric_unit",
                "parent_span_name",
                "observed_at_unix_nano",
                "observed_time_source",
                "protocol_kind",
                "decision_id",
                "contract_digest",
                "payload_json"
            ],
            "contractName": self.contract.name,
            "contractDigest": self.contract.contract_digest,
            "authority": self.contract.authority,
            "sources": self.contract.evidence_sources,
            "builtInSources": [{
                "kind": "log",
                "scope": DECISION_OBSERVATION_SCOPE,
                "name": DECISION_OBSERVATION_NAME,
                "exactContractDigest": true,
                "attributes": [
                    "flaggo.decision.id",
                    "flaggo.contract.name",
                    "flaggo.contract.digest",
                    "flaggo.executable.digest",
                    "flaggo.result.json",
                    "flaggo.result.hash",
                    "flaggo.evaluation.source",
                    "flaggo.evaluation.rule",
                    "flaggo.request.correlation_id",
                    "flaggo.correlation.*"
                ],
                "limitations": [
                    "The active executable content is not included; only its digest is recorded.",
                    "Decision inputs are present only when explicitly emitted as correlation attributes."
                ],
                "temporalSemantics": [
                    "flaggo.correlation.* values describe decision-time inputs, not outcomes.",
                    "Use observed_at_unix_nano and metric data-point times to identify evidence recorded after a decision.",
                    "Do not treat a pre-decision observation window as an outcome caused by that decision.",
                    "Metric observations may be cumulative OTLP snapshots. Repeated cumulative data points are not independent events; use the latest point or changes between points for each correlated population."
                ]
            }],
            "payloadLayout": {
                "decisionAttributes": "$.signal.payload.attributes",
                "metricDataPoint": "$.signal.payload.dataPoint",
                "metricAttributes": "$.signal.payload.dataPoint.attributes",
                "finiteDoubleValue": "Finite doubles preserve exact bits and also expose a queryable decimal at the nested value field, for example $.signal.payload.attributes[10].value.value or $.signal.payload.dataPoint.value.value."
            },
            "rules": {
                "readOnly": true,
                "singleStatement": true,
                "cutoffRequired": true,
                "resultRowsLimited": self.query_limits.max_rows.get(),
                "resultBytesLimited": self.query_limits.max_bytes
            }
        }))
    }

    async fn query_evidence(&self, sql: &str) -> Result<Value, AnalysisError> {
        self.ensure_not_yielding()?;
        let cutoff = self
            .workspace
            .cutoff(&self.context.cycle_id)
            .await?
            .ok_or_else(|| {
                AnalysisError::Evidence(
                    "commit an evidence cutoff before querying observations".to_owned(),
                )
            })?;
        let cutoff_unix_nano =
            u64::try_from(cutoff.cutoff.timestamp_nanos_opt().ok_or_else(|| {
                AnalysisError::Evidence(
                    "evidence cutoff exceeds the supported timestamp range".to_owned(),
                )
            })?)
            .map_err(|_| {
                AnalysisError::Evidence("evidence cutoff predates the Unix epoch".to_owned())
            })?;
        let result = self
            .evidence_store
            .query(
                &self.scope,
                EvidenceQueryRequest {
                    sql: sql.to_owned(),
                    cutoff_unix_nano,
                    watermark: ObservationWatermark::new(cutoff.watermark),
                    limits: self.query_limits,
                },
            )
            .await
            .map_err(evidence_error)?;
        let result_value = serde_json::to_value(&result).map_err(evidence_error)?;
        self.workspace
            .append_audit(
                &self.context.cycle_id,
                "queries",
                json!({
                    "requestedSql": sql,
                    "result": result_value
                }),
            )
            .await?;
        Ok(result_value)
    }

    async fn propose_executable(&self, rules: Value) -> Result<CandidateRecord, AnalysisError> {
        self.ensure_not_yielding()?;
        let cutoff = self
            .workspace
            .cutoff(&self.context.cycle_id)
            .await?
            .ok_or_else(|| {
                AnalysisError::Candidate(
                    "commit an evidence cutoff before proposing an executable".to_owned(),
                )
            })?;
        let proposal = self
            .workspace
            .prepare_candidate_proposal(&self.context.cycle_id, &self.context.attempt_id, rules)
            .await?;
        if let Some(candidate) = self
            .workspace
            .candidate_record(&self.context.cycle_id)
            .await?
        {
            if candidate.analysis_manifest_digest != proposal.analysis_manifest_digest {
                return Err(AnalysisError::InvalidState(
                    "durable Candidate does not match the cycle proposal".to_owned(),
                ));
            }
            return Ok(candidate);
        }

        let mut proposal_context = self.context.clone();
        proposal_context.attempt_id = proposal.attempt_id;
        let candidate = self
            .contract_service
            .submit_candidate(
                &proposal_context,
                proposal.rules,
                &cutoff,
                &proposal.analysis_manifest_digest,
            )
            .await?;
        self.workspace
            .record_candidate(&self.context.cycle_id, &candidate)
            .await?;
        Ok(candidate)
    }

    async fn proposed_candidate(&self) -> Result<Option<CandidateRecord>, AnalysisError> {
        self.workspace
            .candidate_record(&self.context.cycle_id)
            .await
    }

    fn yield_signal(&self) -> Arc<YieldSignal> {
        Arc::clone(&self.yield_signal)
    }
}

impl LocalAnalysisTools {
    fn ensure_not_yielding(&self) -> Result<(), AnalysisError> {
        match self.yield_signal.status()? {
            AnalysisRunStatus::Active => Ok(()),
            AnalysisRunStatus::ContractSuperseded {
                replacement_digest,
            } => Err(AnalysisError::InvalidState(format!(
                "analysis attempt must checkpoint and return superseded; replacement digest: {}",
                replacement_digest.as_deref().unwrap_or("<none>")
            ))),
            AnalysisRunStatus::ServiceShutdown => Err(AnalysisError::InvalidState(
                "analysis attempt must checkpoint and return handoff because the service is shutting down"
                    .to_owned(),
            )),
        }
    }
}

fn evidence_scope(contract: &AnalysisContract) -> Result<EvidenceQueryScope, AnalysisError> {
    let authority = AuthorityScope::new(
        contract.authority.tenant.clone(),
        contract.authority.application.clone(),
        contract.authority.environment.clone(),
    )
    .map_err(evidence_error)?;
    let mut sources = Vec::with_capacity(contract.evidence_sources.len() + 1);
    for source in &contract.evidence_sources {
        let signal = match source.kind.as_str() {
            "log" => EvidenceSignal::Log,
            "metric" => EvidenceSignal::Metric,
            "span" => EvidenceSignal::Span,
            "spanEvent" => EvidenceSignal::SpanEvent,
            kind => {
                return Err(AnalysisError::Evidence(format!(
                    "unsupported evidence signal kind '{kind}'"
                )));
            }
        };
        sources.push(EvidenceSourceSelector {
            signal,
            instrumentation_scope: source.scope.clone(),
            signal_name: source.name.clone(),
            metric_kind: source.metric_kind.clone(),
            metric_unit: source.unit.clone(),
            parent_span_name: source.span_name.clone(),
        });
    }
    if sources.is_empty() {
        return Err(AnalysisError::Evidence(
            "analysis contract has no evidence sources".to_owned(),
        ));
    }
    let decision_observation_is_declared = contract.evidence_sources.iter().any(|source| {
        source.kind == "log"
            && source.scope == DECISION_OBSERVATION_SCOPE
            && source.name == DECISION_OBSERVATION_NAME
    });
    if !decision_observation_is_declared {
        sources.push(EvidenceSourceSelector {
            signal: EvidenceSignal::Log,
            instrumentation_scope: DECISION_OBSERVATION_SCOPE.to_owned(),
            signal_name: DECISION_OBSERVATION_NAME.to_owned(),
            metric_kind: None,
            metric_unit: None,
            parent_span_name: None,
        });
    }
    Ok(EvidenceQueryScope {
        authority,
        contract_digest: contract.contract_digest.clone(),
        sources,
    })
}

fn evidence_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Evidence(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use chrono::{TimeZone, Utc};
    use flaggo_analysis_domain::{
        AnalysisAuthority, AnalysisContractIdentity, AnalysisCycle, AnalysisEvidenceSource,
        AnalysisProfile, CatalogUpdate,
    };
    use flaggo_analysis_workspace::{
        CyclePreparation, LocalWorkspaceProvider, PrepareCycleResult, WorkspaceProvider,
    };
    use flaggo_evidence_store::{EvidenceQueryResult, EvidenceStoreError, ObservationWatermark};
    use serde_json::json;

    use super::*;

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct FakeEvidenceStore {
        captures: Mutex<u32>,
        requests: Mutex<Vec<(EvidenceQueryScope, EvidenceQueryRequest)>>,
    }

    #[async_trait]
    impl EvidenceAnalysisStore for FakeEvidenceStore {
        async fn capture_watermark(&self) -> Result<ObservationWatermark, EvidenceStoreError> {
            let mut captures = self.captures.lock().expect("capture lock");
            *captures += 1;
            Ok(ObservationWatermark::new(42))
        }

        async fn query(
            &self,
            scope: &EvidenceQueryScope,
            request: EvidenceQueryRequest,
        ) -> Result<EvidenceQueryResult, EvidenceStoreError> {
            self.requests
                .lock()
                .expect("request lock")
                .push((scope.clone(), request));
            Ok(EvidenceQueryResult {
                normalized_sql: "SELECT count(*) AS count FROM observations".to_owned(),
                columns: vec!["count".to_owned()],
                rows: vec![std::collections::BTreeMap::from([(
                    "count".to_owned(),
                    json!(3),
                )])],
                observation_ids: Vec::new(),
                result_bytes: 11,
            })
        }
    }

    struct FakeContractService {
        current: AnalysisContract,
        submissions: Mutex<Vec<AttemptContext>>,
        now: DateTime<Utc>,
    }

    #[async_trait]
    impl ContractService for FakeContractService {
        async fn list_current_learning_contracts(
            &self,
            _revision: Option<&str>,
        ) -> Result<CatalogUpdate, AnalysisError> {
            Ok(CatalogUpdate::Updated {
                revision: "revision-1".to_owned(),
                contracts: vec![AnalysisContractIdentity {
                    name: self.current.name.clone(),
                    digest: self.current.contract_digest.clone(),
                }],
            })
        }

        async fn get_current_contract(
            &self,
            contract_name: &str,
        ) -> Result<Option<AnalysisContract>, AnalysisError> {
            Ok((contract_name == self.current.name).then(|| self.current.clone()))
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
                    "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                        .to_owned(),
                created_at: self.now,
                created: true,
                analysis_manifest_digest: analysis_manifest_digest.to_owned(),
            })
        }
    }

    fn contract(accepted_at: DateTime<Utc>) -> AnalysisContract {
        AnalysisContract {
            name: "checkout.delay".to_owned(),
            contract_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
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

    fn profile() -> AnalysisProfile {
        AnalysisProfile {
            skill_name: "evidence-analysis".to_owned(),
            skill_version: "1".to_owned(),
        }
    }

    fn ready(result: PrepareCycleResult) -> CyclePreparation {
        match result {
            PrepareCycleResult::Ready(preparation) => preparation,
            PrepareCycleResult::NotEligible { eligible_at } => {
                panic!("expected ready cycle, eligible at {eligible_at}")
            }
        }
    }

    fn context(cycle: &AnalysisCycle, attempt_id: &str) -> AttemptContext {
        AttemptContext::new(cycle, attempt_id.to_owned())
    }

    #[tokio::test]
    async fn scopes_queries_and_recovers_exact_candidate_submission() {
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 3, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + chrono::Duration::hours(2);
        let contract = contract(accepted_at);
        let service = Arc::new(FakeContractService {
            current: contract.clone(),
            submissions: Mutex::new(Vec::new()),
            now,
        });
        let evidence = Arc::new(FakeEvidenceStore {
            captures: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
        });
        let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
        let factory = LocalCapabilityFactory::new(
            service.clone(),
            evidence.clone(),
            EvidenceQueryLimits {
                max_rows: NonZeroU32::new(100).expect("nonzero"),
                max_bytes: 16_384,
                timeout: Duration::from_secs(2),
            },
            clock,
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspaces = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let lease = workspaces
            .claim(&contract.name, "attempt-1")
            .await
            .expect("claim");
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .expect("prepare"),
        )
        .cycle;
        let tools = factory
            .create(
                context(&cycle, "attempt-1"),
                contract.clone(),
                lease.clone(),
            )
            .await
            .expect("create tools");

        assert!(
            tools
                .query_evidence("SELECT count(*) FROM observations")
                .await
                .is_err()
        );
        let description = tools.describe_evidence().await.expect("describe evidence");
        assert_eq!(
            description["builtInSources"][0]["name"],
            DECISION_OBSERVATION_NAME
        );
        assert_eq!(
            description["builtInSources"][0]["exactContractDigest"],
            true
        );
        tools
            .commit_evidence_cutoff(now)
            .await
            .expect("commit cutoff");
        tools
            .commit_evidence_cutoff(now)
            .await
            .expect("idempotent cutoff");
        assert_eq!(*evidence.captures.lock().expect("capture lock"), 1);

        let result = tools
            .query_evidence("SELECT count(*) AS count FROM observations")
            .await
            .expect("query");
        assert_eq!(result["rows"][0]["count"], 3);
        {
            let requests = evidence.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0.authority.tenant, "local");
            assert_eq!(requests[0].0.contract_digest, contract.contract_digest);
            assert_eq!(requests[0].0.sources.len(), 2);
            let decision_source = requests[0]
                .0
                .sources
                .iter()
                .find(|source| {
                    source.instrumentation_scope == DECISION_OBSERVATION_SCOPE
                        && source.signal_name == DECISION_OBSERVATION_NAME
                })
                .expect("built-in decision source");
            assert!(matches!(&decision_source.signal, EvidenceSignal::Log));
            assert_eq!(requests[0].1.watermark.get(), 42);
        }

        let rules = json!([{
            "name": "learned",
            "when": {"expression": "true"},
            "return": {"value": 5}
        }]);
        let candidate = tools
            .propose_executable(rules.clone())
            .await
            .expect("propose Candidate");
        assert_eq!(candidate.contract_digest, contract.contract_digest);
        drop(tools);
        drop(lease);

        let recovery_lease = workspaces
            .claim(&contract.name, "attempt-2")
            .await
            .expect("recovery claim");
        let resumed = ready(
            recovery_lease
                .prepare_cycle(&contract, &profile(), "attempt-2", now)
                .await
                .expect("resume"),
        );
        assert!(resumed.cycle.resumed);
        let recovery_tools = factory
            .create(
                context(&resumed.cycle, "attempt-2"),
                contract,
                recovery_lease,
            )
            .await
            .expect("recovery tools");
        let recovered = recovery_tools
            .propose_executable(rules)
            .await
            .expect("recover Candidate");
        assert_eq!(recovered, candidate);

        let submissions = service.submissions.lock().expect("submission lock");
        assert_eq!(submissions.len(), 1);
        assert_eq!(submissions[0].attempt_id, "attempt-1");
    }

    #[tokio::test]
    async fn rejects_future_cutoff_and_post_yield_tool_calls() {
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 3, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + chrono::Duration::hours(2);
        let contract = contract(accepted_at);
        let service = Arc::new(FakeContractService {
            current: contract.clone(),
            submissions: Mutex::new(Vec::new()),
            now,
        });
        let evidence = Arc::new(FakeEvidenceStore {
            captures: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
        });
        let factory = LocalCapabilityFactory::new(
            service,
            evidence,
            EvidenceQueryLimits {
                max_rows: NonZeroU32::new(10).expect("nonzero"),
                max_bytes: 1024,
                timeout: Duration::from_secs(1),
            },
            Arc::new(FixedClock(now)),
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspaces = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let lease = workspaces
            .claim(&contract.name, "attempt-1")
            .await
            .expect("claim");
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .expect("prepare"),
        )
        .cycle;
        let tools = factory
            .create(context(&cycle, "attempt-1"), contract, lease)
            .await
            .expect("create tools");

        assert!(
            tools
                .commit_evidence_cutoff(now + chrono::Duration::nanoseconds(1))
                .await
                .is_err()
        );
        assert_eq!(
            tools
                .check_analysis_status()
                .await
                .expect("active analysis status"),
            AnalysisRunStatus::Active
        );
        tools
            .yield_signal()
            .request_contract_superseded(Some(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
            ))
            .expect("request supersession");
        assert_eq!(
            tools
                .check_analysis_status()
                .await
                .expect("superseded analysis status"),
            AnalysisRunStatus::ContractSuperseded {
                replacement_digest: Some(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_owned()
                )
            }
        );
        assert!(tools.describe_evidence().await.is_err());
    }
}
