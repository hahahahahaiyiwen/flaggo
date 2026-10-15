use std::{
    num::NonZeroU32,
    path::{Component, Path},
    sync::Arc,
};

use async_trait::async_trait;
use flaggo_analysis_agent::{
    AnalysisCapabilityFactory, AnalysisRunStatus, AnalysisTools, PythonInput, YieldSignal,
    action::{ActionKind, ActionObservation, ActionOutcome, FailureCategory},
};
use flaggo_analysis_domain::{AnalysisContract, AnalysisError, AttemptContext, CandidateRecord};
use flaggo_analysis_workspace::{WorkspaceFileSystem, WorkspaceLease, sha256_digest};
use flaggo_evidence_store::{
    AuthorityScope, EvidenceAnalysisStore, EvidenceQueryLimits, EvidenceQueryRequest,
    EvidenceQueryScope, EvidenceSignal, EvidenceSourceSelector, EvidenceStoreError,
};
use serde_json::{Value, json};
use tracing::Instrument as _;

use crate::{
    Clock, ContractService,
    python::{ExecutionStatus, PythonConfig},
};

const DECISION_OBSERVATION_SCOPE: &str = "@flaggo/sdk";
const DECISION_OBSERVATION_NAME: &str = "flaggo.decision.received";
const DEFAULT_QUERY_ROWS: u32 = 100;
const QUERY_HISTORY_PATH: &str = "queries/history.jsonl";

pub struct LocalCapabilityFactory {
    contract_service: Arc<dyn ContractService>,
    evidence_store: Arc<dyn EvidenceAnalysisStore>,
    query_limits: EvidenceQueryLimits,
    clock: Arc<dyn Clock>,
    python: PythonConfig,
}

impl LocalCapabilityFactory {
    #[must_use]
    pub fn new(
        contract_service: Arc<dyn ContractService>,
        evidence_store: Arc<dyn EvidenceAnalysisStore>,
        query_limits: EvidenceQueryLimits,
        clock: Arc<dyn Clock>,
        python: PythonConfig,
    ) -> Self {
        Self {
            contract_service,
            evidence_store,
            query_limits,
            clock,
            python,
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
            parent: tracing::Span::current(),
            python: self.python.clone(),
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
    parent: tracing::Span,
    python: PythonConfig,
}

#[async_trait]
impl AnalysisTools for LocalAnalysisTools {
    async fn check_cycle_status(&self) -> Result<AnalysisRunStatus, AnalysisError> {
        self.yield_signal.status()
    }

    async fn describe(&self) -> Result<Value, AnalysisError> {
        let mut action =
            ActionObservation::start(&self.context, ActionKind::EvidenceDescribe, &self.parent);
        let result = async {
        self.ensure_not_yielding()?;
        Ok(json!({
            "queryCapabilities": self.evidence_store.query_capabilities(),
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
                "consistency": "live",
                "resultRowsLimited": self.query_limits.max_rows.get(),
                "defaultRowLimit": DEFAULT_QUERY_ROWS.min(self.query_limits.max_rows.get()),
                "resultBytesLimited": self.query_limits.max_bytes,
                "queryTimeoutMs": self.query_limits.timeout.as_millis(),
                "results": "run_sql returns metadata, not rows. resultPath and historyPath reference saved JSON results and query history, relative to the cycle analysis directory. totalRows is null when truncated."
            }
        }))
        }.instrument(action.span()).await;
        action.finish_result(&result);
        result
    }

    async fn run_sql(&self, sql: &str, row_limit: Option<u32>) -> Result<Value, AnalysisError> {
        let mut action =
            ActionObservation::start(&self.context, ActionKind::EvidenceSql, &self.parent);
        action.query(sql);
        let span = action.span();
        let result = async {
            self.ensure_not_yielding()?;
            let row_limit =
                row_limit.unwrap_or(DEFAULT_QUERY_ROWS.min(self.query_limits.max_rows.get()));
            let row_limit = NonZeroU32::new(row_limit)
                .filter(|limit| limit.get() <= self.query_limits.max_rows.get())
                .ok_or_else(|| {
                    AnalysisError::Evidence(format!(
                        "row_limit must be between 1 and {}",
                        self.query_limits.max_rows
                    ))
                })?;
            let filesystem = self
                .workspace
                .filesystem(&self.context.cycle_id, &self.context.attempt_id)
                .await?;
            let query_started_at = self.clock.now();
            let result = self
                .evidence_store
                .query(
                    &self.scope,
                    EvidenceQueryRequest {
                        sql: sql.to_owned(),
                        limits: EvidenceQueryLimits {
                            max_rows: row_limit,
                            ..self.query_limits
                        },
                    },
                )
                .await;
            let query_completed_at = self.clock.now();
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    let (outcome, category) = evidence_query_failure(&error);
                    let record = json!({
                        "queryId": uuid::Uuid::new_v4().to_string(),
                        "requestedSql": sql,
                        "recordedAt": self.clock.now(),
                        "contractDigest": self.context.contract.digest,
                        "queryStartedAt": query_started_at,
                        "queryCompletedAt": query_completed_at,
                        "rowLimit": row_limit.get(),
                        "status": "failed",
                        "diagnostic": error.to_string()
                    });
                    self.record_query(&filesystem, record).await?;
                    action.finish(outcome, Some(category));
                    return Err(evidence_error(error));
                }
            };
            action.result_size(result.rows.len(), result.result_bytes);
            self.ensure_not_yielding()?;
            let query_id = uuid::Uuid::new_v4().to_string();
            let result_path = format!("queries/{query_id}.json");
            let content = serde_json::to_string(&json!({
                "queryId": query_id,
                "contractDigest": self.context.contract.digest,
                "queryStartedAt": query_started_at,
                "queryCompletedAt": query_completed_at,
                "normalizedSql": result.normalized_sql,
                "columns": result.columns,
                "rows": result.rows,
                "observationIds": result.observation_ids,
                "truncated": result.truncated
            }))
            .map_err(evidence_error)?;
            filesystem
                .write_text(&format!("/workspace/analysis/{result_path}"), &content)
                .await?;
            let metadata = json!({
                "queryId": query_id,
                "resultPath": result_path,
                "historyPath": QUERY_HISTORY_PATH,
                "columns": result.columns,
                "savedRows": result.rows.len(),
                "totalRows": if result.truncated { None } else { Some(result.rows.len()) },
                "truncated": result.truncated,
                "rowLimit": row_limit.get(),
                "resultBytes": content.len(),
                "contentDigest": sha256_digest(content.as_bytes()),
                "queryStartedAt": query_started_at,
                "queryCompletedAt": query_completed_at
            });
            self.record_query(
                &filesystem,
                json!({
                    "requestedSql": sql,
                    "recordedAt": self.clock.now(),
                    "status": "succeeded",
                    "result": metadata
                }),
            )
            .await?;
            Ok(metadata)
        }
        .instrument(span)
        .await;
        action.finish_result(&result);
        result
    }

    async fn code_interpreter(&self, input: PythonInput<'_>) -> Result<Value, AnalysisError> {
        let mut action =
            ActionObservation::start(&self.context, ActionKind::PythonRun, &self.parent);
        let span = action.span();
        let result = async {
            self.ensure_not_yielding()?;
            let directory = self.workspace.analysis_directory(&self.context.cycle_id)?;
            let filesystem = self.workspace.filesystem(&self.context.cycle_id, &self.context.attempt_id).await?;
            let (script_path, source, source_directory) = match input {
                PythonInput::ScriptPath(script_path) => {
                    let relative_path = script_path.strip_prefix("/workspace/analysis/").unwrap_or(script_path);
                    let path = Path::new(relative_path);
                    if relative_path.is_empty() || relative_path.contains('\\') || path.components().any(|part| !matches!(part, Component::Normal(_))) {
                        return Err(AnalysisError::Agent("script_path must identify a file in the cycle analysis directory using a POSIX path".to_owned()));
                    }
                    let source = filesystem.read_text(&format!("/workspace/analysis/{relative_path}")).await?;
                    (Some(script_path), source, directory.join(path.parent().unwrap_or(Path::new(""))))
                }
                PythonInput::Code(code) => {
                    if code.trim().is_empty() {
                        return Err(AnalysisError::Agent("inline Python code cannot be empty".to_owned()));
                    }
                    (None, code.to_owned(), directory.clone())
                }
            };
            action.artifact(&source);
            let execution_id = uuid::Uuid::new_v4().to_string();
            let execution_path = format!("executions/{execution_id}");
            let code_path = format!("{execution_path}/script.py");
            let runtime_path = format!("{execution_path}/runtime.json");
            let record_path = format!("{execution_path}/result.json");
            filesystem.write_text(&format!("/workspace/analysis/{code_path}"), &source).await?;
            let submitted = json!({
                "executionId": execution_id,
                "scriptPath": script_path,
                "codePath": code_path,
                "codeDigest": sha256_digest(source.as_bytes()),
                "configuredInterpreter": self.python.executable,
                "contractDigest": self.context.contract.digest,
                "cycleId": self.context.cycle_id,
                "attemptId": self.context.attempt_id,
                "submittedAt": self.clock.now()
            });
            self.workspace.append_audit(&self.context.cycle_id, "pythonSubmitted", submitted.clone()).await?;
            let execution = self.python.run(
                &directory,
                &directory.join(&code_path),
                &directory.join(&runtime_path),
                &source_directory,
                &self.yield_signal,
            ).await;
            let execution = match execution {
                Ok(execution) => execution,
                Err(error) => {
                    let failed = json!({"submission": submitted, "status": "runnerFailed", "diagnostic": error.to_string()});
                    filesystem.write_text(&format!("/workspace/analysis/{record_path}"), &serde_json::to_string(&failed).map_err(evidence_error)?).await?;
                    self.workspace.append_audit(&self.context.cycle_id, "pythonCompleted", failed).await?;
                    return Err(error);
                }
            };
            let runtime = if filesystem.exists(&format!("/workspace/analysis/{runtime_path}")).await? {
                let content = filesystem.read_text(&format!("/workspace/analysis/{runtime_path}")).await?;
                Some(serde_json::from_str::<Value>(&content).map_err(evidence_error)?)
            } else { None };
            let report = json!({
                "executionId": execution_id,
                "executionRecordPath": record_path,
                "submittedCodePath": code_path,
                "runtime": runtime,
                "execution": execution
            });
            let record = json!({"submission": submitted, "report": report});
            filesystem.write_text(&format!("/workspace/analysis/{record_path}"), &serde_json::to_string(&record).map_err(evidence_error)?).await?;
            self.workspace.append_audit(&self.context.cycle_id, "pythonCompleted", json!({
                "executionId": execution_id,
                "executionRecordPath": record_path,
                "status": execution.status
            })).await?;
            let (outcome, category) = match execution.status {
                ExecutionStatus::Succeeded => (ActionOutcome::Success, None),
                ExecutionStatus::TimedOut => (ActionOutcome::Timeout, Some(FailureCategory::Timeout)),
                ExecutionStatus::Cancelled => (ActionOutcome::Cancelled, Some(FailureCategory::Cancelled)),
                ExecutionStatus::Failed => (ActionOutcome::Failed, Some(FailureCategory::Validation)),
                ExecutionStatus::LaunchFailed => (ActionOutcome::Failed, Some(FailureCategory::Configuration)),
            };
            action.finish(outcome, category);
            Ok(report)
        }.instrument(span).await;
        action.finish_result(&result);
        result
    }

    async fn propose_executable(&self, rules: Value) -> Result<CandidateRecord, AnalysisError> {
        let mut action =
            ActionObservation::start(&self.context, ActionKind::ProposalSubmit, &self.parent);
        let span = action.span();
        let result = async {
            self.ensure_not_yielding()?;
            let current = self
                .contract_service
                .get_current_contract(&self.contract.name)
                .await?;
            if current
                .as_ref()
                .map(|contract| contract.contract_digest.as_str())
                != Some(self.contract.contract_digest.as_str())
            {
                return Err(AnalysisError::Superseded(self.contract.name.clone()));
            }
            if !rules.as_array().is_some_and(|rules| !rules.is_empty()) {
                return Err(AnalysisError::Candidate(
                    "Candidate rules must be a non-empty JSON array".to_owned(),
                ));
            }
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
                    &proposal.analysis_manifest_digest,
                )
                .await?;
            self.workspace
                .record_candidate(&self.context.cycle_id, &candidate)
                .await?;
            Ok(candidate)
        }
        .instrument(span)
        .await;
        action.finish_result(&result);
        result
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
    async fn record_query(
        &self,
        filesystem: &Arc<dyn WorkspaceFileSystem>,
        record: Value,
    ) -> Result<(), AnalysisError> {
        let line = serde_json::to_string(&record).map_err(evidence_error)?;
        filesystem
            .append_text(
                &format!("/workspace/analysis/{QUERY_HISTORY_PATH}"),
                &format!("{line}\n"),
            )
            .await?;
        self.workspace
            .append_audit(&self.context.cycle_id, "queries", record)
            .await
    }

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

fn evidence_query_failure(error: &EvidenceStoreError) -> (ActionOutcome, FailureCategory) {
    match error {
        EvidenceStoreError::QueryLimit("time") => {
            (ActionOutcome::Timeout, FailureCategory::Timeout)
        }
        EvidenceStoreError::QueryLimit(_) => (ActionOutcome::Failed, FailureCategory::Capacity),
        EvidenceStoreError::InvalidQuery(_) | EvidenceStoreError::InvalidWrite(_) => {
            (ActionOutcome::Failed, FailureCategory::Validation)
        }
        EvidenceStoreError::CorruptData(_) => (ActionOutcome::Failed, FailureCategory::Integrity),
        EvidenceStoreError::UnsupportedSchemaVersion { .. } => {
            (ActionOutcome::Failed, FailureCategory::Configuration)
        }
        EvidenceStoreError::Unavailable { .. } => {
            (ActionOutcome::Failed, FailureCategory::Dependency)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use chrono::{DateTime, TimeZone, Utc};
    use flaggo_analysis_domain::{
        AnalysisAuthority, AnalysisContractIdentity, AnalysisCycle, AnalysisEvidenceSource,
        AnalysisProfile, CatalogUpdate,
    };
    use flaggo_analysis_workspace::{
        CyclePreparation, LocalWorkspaceProvider, PrepareCycleResult, WorkspaceProvider,
    };
    use flaggo_evidence_store::{
        EvidenceColumn, EvidenceQueryCapabilities, EvidenceQueryResult, EvidenceStoreError,
        SQLITE_EVIDENCE_QUERY_CAPABILITIES,
    };
    use serde_json::json;

    use super::*;

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct FakeEvidenceStore {
        requests: Mutex<Vec<(EvidenceQueryScope, EvidenceQueryRequest)>>,
        behavior: QueryBehavior,
    }

    #[derive(Clone, Copy)]
    enum QueryBehavior {
        Success,
        Rows(u32),
        Timeout,
        Invalid,
        Pending,
    }

    #[async_trait]
    impl EvidenceAnalysisStore for FakeEvidenceStore {
        fn query_capabilities(&self) -> EvidenceQueryCapabilities {
            SQLITE_EVIDENCE_QUERY_CAPABILITIES
        }

        async fn query(
            &self,
            scope: &EvidenceQueryScope,
            request: EvidenceQueryRequest,
        ) -> Result<EvidenceQueryResult, EvidenceStoreError> {
            let limit = request.limits.max_rows.get();
            self.requests
                .lock()
                .expect("request lock")
                .push((scope.clone(), request));
            match self.behavior {
                QueryBehavior::Timeout => return Err(EvidenceStoreError::QueryLimit("time")),
                QueryBehavior::Invalid => {
                    return Err(EvidenceStoreError::InvalidQuery(
                        "private SQL token".to_owned(),
                    ));
                }
                QueryBehavior::Pending => std::future::pending::<()>().await,
                QueryBehavior::Success | QueryBehavior::Rows(_) => {}
            }
            if let QueryBehavior::Rows(count) = self.behavior {
                let rows: Vec<_> = (0..count.min(limit))
                    .map(|index| {
                        std::collections::BTreeMap::from([("value".to_owned(), json!(index))])
                    })
                    .collect();
                return Ok(EvidenceQueryResult {
                    normalized_sql: "SELECT value FROM observations".to_owned(),
                    columns: vec![EvidenceColumn {
                        name: "value".to_owned(),
                        data_type: None,
                    }],
                    result_bytes: serde_json::to_vec(&rows).unwrap().len(),
                    rows,
                    observation_ids: Vec::new(),
                    truncated: count > limit,
                });
            }
            Ok(EvidenceQueryResult {
                normalized_sql: "SELECT count(*) AS count FROM observations".to_owned(),
                columns: vec![EvidenceColumn {
                    name: "count".to_owned(),
                    data_type: None,
                }],
                rows: vec![std::collections::BTreeMap::from([(
                    "count".to_owned(),
                    json!(3),
                )])],
                observation_ids: Vec::new(),
                result_bytes: 11,
                truncated: false,
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
            skill_names: vec![
                "analysis-cycle-protocol".to_owned(),
                "understand-decision-contract".to_owned(),
                "qualitative-analysis".to_owned(),
                "author-executable".to_owned(),
            ],
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
            requests: Mutex::new(Vec::new()),
            behavior: QueryBehavior::Success,
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
            PythonConfig::default(),
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

        let description = tools.describe().await.expect("describe data");
        assert_eq!(
            description["builtInSources"][0]["name"],
            DECISION_OBSERVATION_NAME
        );
        assert_eq!(
            description["builtInSources"][0]["exactContractDigest"],
            true
        );
        assert_eq!(description["queryCapabilities"]["dialect"], "sqlite");
        assert!(
            description["queryCapabilities"]["functions"]
                .as_array()
                .expect("functions")
                .contains(&json!("avg"))
        );
        assert_eq!(description["rules"]["consistency"], "live");
        assert!(description.get("columns").is_none());
        assert_eq!(
            description["queryCapabilities"],
            serde_json::to_value(SQLITE_EVIDENCE_QUERY_CAPABILITIES).unwrap()
        );

        let result = tools
            .run_sql("SELECT count(*) AS count FROM observations", None)
            .await
            .expect("query");
        assert!(result.get("rows").is_none());
        assert_eq!(result["savedRows"], 1);
        assert_eq!(result["totalRows"], 1);
        assert_eq!(result["columns"], json!([{"name": "count", "type": null}]));
        assert_eq!(result["queryStartedAt"], json!(now));
        assert_eq!(result["queryCompletedAt"], json!(now));
        assert!(result.get("cutoff").is_none());
        assert!(result.get("watermark").is_none());
        let filesystem = lease
            .filesystem(&cycle.cycle_id, "attempt-1")
            .await
            .unwrap();
        let saved = filesystem
            .read_text(&format!(
                "/workspace/analysis/{}",
                result["resultPath"].as_str().unwrap()
            ))
            .await
            .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved["rows"][0]["count"], 3);
        assert_eq!(
            result["contentDigest"],
            sha256_digest(
                filesystem
                    .read_text(&format!(
                        "/workspace/analysis/{}",
                        result["resultPath"].as_str().unwrap()
                    ))
                    .await
                    .unwrap()
                    .as_bytes()
            )
        );
        let history = filesystem
            .read_text(&format!(
                "/workspace/analysis/{}",
                result["historyPath"].as_str().unwrap()
            ))
            .await
            .unwrap();
        assert_eq!(history.lines().count(), 1);
        let history: Value = serde_json::from_str(history.trim()).unwrap();
        assert_eq!(history["result"], result);
        assert!(history["result"].get("rows").is_none());
        filesystem.write_text("/workspace/analysis/scripts/model.py", &format!(
            "import json\nfrom pathlib import Path\nrows = json.loads(Path({:?}).read_text())['rows']\nPath('summary.json').write_text(json.dumps({{'count': rows[0]['count']}}))\nprint('modeled', len(rows))\n",
            result["resultPath"].as_str().unwrap(),
        )).await.unwrap();
        let modeled = tools
            .code_interpreter(PythonInput::ScriptPath("scripts/model.py"))
            .await
            .unwrap();
        assert_eq!(modeled["execution"]["status"], "succeeded");
        assert_eq!(
            modeled["execution"]["stdout"].as_str().unwrap().trim(),
            "modeled 1"
        );
        assert!(
            modeled["runtime"]["version"]
                .as_str()
                .unwrap()
                .starts_with('3')
        );
        let summary = filesystem
            .read_text("/workspace/analysis/summary.json")
            .await
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&summary).unwrap()["count"], 3);
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
    async fn rejects_post_yield_tool_calls() {
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
            requests: Mutex::new(Vec::new()),
            behavior: QueryBehavior::Success,
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
            PythonConfig::default(),
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

        assert_eq!(
            tools
                .check_cycle_status()
                .await
                .expect("active cycle status"),
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
                .check_cycle_status()
                .await
                .expect("superseded cycle status"),
            AnalysisRunStatus::ContractSuperseded {
                replacement_digest: Some(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_owned()
                )
            }
        );
        assert!(tools.describe().await.is_err());
    }

    async fn observed_tools(
        behavior: QueryBehavior,
    ) -> (tempfile::TempDir, Arc<dyn AnalysisTools>, DateTime<Utc>) {
        let (directory, tools, now, _) =
            observed_tools_with_python(behavior, PythonConfig::default()).await;
        (directory, tools, now)
    }

    async fn observed_tools_with_python(
        behavior: QueryBehavior,
        python: PythonConfig,
    ) -> (
        tempfile::TempDir,
        Arc<dyn AnalysisTools>,
        DateTime<Utc>,
        Arc<dyn WorkspaceFileSystem>,
    ) {
        let accepted_at = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single().unwrap();
        let now = accepted_at + chrono::Duration::hours(2);
        let contract = contract(accepted_at);
        let factory = LocalCapabilityFactory::new(
            Arc::new(FakeContractService {
                current: contract.clone(),
                submissions: Mutex::new(Vec::new()),
                now,
            }),
            Arc::new(FakeEvidenceStore {
                requests: Mutex::new(Vec::new()),
                behavior,
            }),
            EvidenceQueryLimits {
                max_rows: NonZeroU32::new(1000).unwrap(),
                max_bytes: 65536,
                timeout: Duration::from_secs(1),
            },
            Arc::new(FixedClock(now)),
            python,
        );
        let directory = tempfile::tempdir_in(".").unwrap();
        let workspaces = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let lease = workspaces.claim(&contract.name, "attempt-1").await.unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .unwrap(),
        )
        .cycle;
        let filesystem = lease
            .filesystem(&cycle.cycle_id, "attempt-1")
            .await
            .unwrap();
        let tools = factory
            .create(context(&cycle, "attempt-1"), contract, lease)
            .await
            .unwrap();
        (directory, tools, now, filesystem)
    }

    #[tokio::test]
    async fn saves_default_samples_and_requested_rows_without_inline_data_or_counts() {
        let (_directory, tools, _now) = observed_tools(QueryBehavior::Rows(125)).await;
        let sample = tools
            .run_sql("SELECT value FROM observations ORDER BY value", None)
            .await
            .unwrap();
        assert_eq!(sample["savedRows"], 100);
        assert_eq!(sample["truncated"], true);
        assert!(sample["totalRows"].is_null());
        assert!(sample.get("rows").is_none());
        let complete = tools
            .run_sql("SELECT value FROM observations ORDER BY value", Some(200))
            .await
            .unwrap();
        assert_eq!(complete["savedRows"], 125);
        assert_eq!(complete["totalRows"], 125);
        assert_eq!(complete["truncated"], false);
        assert_ne!(complete["resultPath"], sample["resultPath"]);
        for limit in [0, 1001] {
            assert!(
                tools
                    .run_sql("SELECT value FROM observations", Some(limit))
                    .await
                    .is_err()
            );
        }
        let (_empty_directory, empty, _now, filesystem) =
            observed_tools_with_python(QueryBehavior::Rows(0), PythonConfig::default()).await;
        let result = empty
            .run_sql("SELECT value FROM observations", None)
            .await
            .unwrap();
        assert_eq!(result["savedRows"], 0);
        assert_eq!(result["totalRows"], 0);
        assert_eq!(result["truncated"], false);
        assert_eq!(result["columns"], json!([{"name": "value", "type": null}]));
        let saved: Value = serde_json::from_str(
            &filesystem
                .read_text(&format!(
                    "/workspace/analysis/{}",
                    result["resultPath"].as_str().unwrap()
                ))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved["columns"], result["columns"]);
        assert_eq!(saved["rows"], json!([]));
    }

    #[tokio::test]
    async fn records_python_outcomes_and_rejects_invalid_entry_paths_and_late_operations() {
        let (_directory, tools, _, filesystem) =
            observed_tools_with_python(QueryBehavior::Success, PythonConfig::default()).await;
        filesystem
            .write_text(
                "/workspace/analysis/script.py",
                "print('private model output')\n",
            )
            .await
            .unwrap();
        assert!(
            filesystem
                .write_text("/workspace/contract.json", "overwrite")
                .await
                .is_err()
        );
        for path in [
            "",
            "../script.py",
            "/workspace/contract.json",
            "C:\\host.py",
            "missing.py",
        ] {
            assert!(
                tools
                    .code_interpreter(PythonInput::ScriptPath(path))
                    .await
                    .is_err(),
                "{path}"
            );
        }
        let capture = crate::telemetry_support::Capture::default();
        capture
            .run(async {
                let report = tools
                    .code_interpreter(PythonInput::ScriptPath("/workspace/analysis/script.py"))
                    .await
                    .unwrap();
                assert_eq!(report["execution"]["status"], "succeeded");
            })
            .await;
        {
            let events = capture.events.lock().unwrap();
            let terminal = events
                .iter()
                .find(|event| {
                    event.fields.get("event.name")
                        == Some(&json!("flaggo.analysis.action.completed"))
                        && event.fields.get("flaggo.analysis.action.kind")
                            == Some(&json!("python.run"))
                })
                .unwrap();
            assert_eq!(terminal.fields["flaggo.analysis.action.outcome"], "success");
            assert!(
                terminal.fields["flaggo.analysis.artifact.digest"]
                    .as_str()
                    .unwrap()
                    .starts_with("sha256:")
            );
            assert!(
                !serde_json::to_string(&terminal.fields)
                    .unwrap()
                    .contains("private model output")
            );
        }
        tools
            .yield_signal()
            .request_contract_superseded(None)
            .unwrap();
        assert!(
            tools
                .code_interpreter(PythonInput::ScriptPath("script.py"))
                .await
                .is_err()
        );
        assert!(
            tools
                .code_interpreter(PythonInput::Code("print(1)"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn default_python_environment_supports_data_analysis_modeling_and_plotting() {
        let (_directory, tools, _, filesystem) =
            observed_tools_with_python(QueryBehavior::Success, PythonConfig::default()).await;
        let report = tools
            .code_interpreter(PythonInput::Code(
                r#"
import json, sys
import numpy as np
import pandas as pd
from scipy import stats
from sklearn.linear_model import LinearRegression
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import seaborn as sns
data = pd.DataFrame({"x": [1, 2, 3], "y": [2, 4, 6]})
model = LinearRegression().fit(data[["x"]], data["y"])
prediction = float(model.predict(pd.DataFrame({"x": [4]}))[0])
sns.lineplot(data=data, x="x", y="y")
plt.savefig("analysis.png")
plt.close()
print(json.dumps({"venv": sys.prefix != sys.base_prefix,
                  "prediction": prediction, "mean": float(np.mean(data["y"])),
                  "variance": float(stats.describe(data["y"]).variance)}))
"#,
            ))
            .await
            .unwrap();
        assert_eq!(report["execution"]["status"], "succeeded", "{report}");
        let output: Value =
            serde_json::from_str(report["execution"]["stdout"].as_str().unwrap()).unwrap();
        assert_eq!(output["venv"], true);
        assert!((output["prediction"].as_f64().unwrap() - 8.0).abs() < 1e-9);
        assert_eq!(output["mean"], 4.0);
        assert_eq!(output["variance"], 4.0);
        assert!(
            filesystem
                .metadata("/workspace/analysis/analysis.png")
                .await
                .unwrap()
                .size
                > 0
        );
    }

    #[tokio::test]
    async fn inline_python_saves_source_and_outputs_without_sharing_process_memory() {
        let (_directory, tools, _, filesystem) =
            observed_tools_with_python(QueryBehavior::Success, PythonConfig::default()).await;
        for code in ["", " \n\t"] {
            assert!(
                tools
                    .code_interpreter(PythonInput::Code(code))
                    .await
                    .is_err()
            );
        }
        let code = "from pathlib import Path\nprivate_variable = 42\nPath('inline.txt').write_text('saved')\nprint(private_variable)\n";
        let first = tools
            .code_interpreter(PythonInput::Code(code))
            .await
            .unwrap();
        assert_eq!(first["execution"]["status"], "succeeded");
        assert_eq!(
            first["execution"]["stdout"]
                .as_str()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["42"]
        );
        assert_eq!(
            filesystem
                .read_text("/workspace/analysis/inline.txt")
                .await
                .unwrap(),
            "saved"
        );
        assert_eq!(
            filesystem
                .read_text(&format!(
                    "/workspace/analysis/{}",
                    first["submittedCodePath"].as_str().unwrap(),
                ))
                .await
                .unwrap(),
            code,
        );
        let second = tools.code_interpreter(PythonInput::Code(
            "from pathlib import Path\nprint('private_variable' in globals())\nprint(Path('inline.txt').read_text())\n",
        )).await.unwrap();
        assert_eq!(second["execution"]["status"], "succeeded");
        assert_eq!(
            second["execution"]["stdout"]
                .as_str()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["False", "saved"]
        );
        assert_ne!(first["executionId"], second["executionId"]);
    }

    #[tokio::test]
    async fn persists_python_failure_launch_failure_timeout_and_cancelled_outcomes() {
        for (config, script, expected) in [
            (
                PythonConfig::default(),
                "raise ValueError('expected')",
                "failed",
            ),
            (
                PythonConfig {
                    executable: "flaggo-missing-python-executable".into(),
                    ..PythonConfig::default()
                },
                "print(1)",
                "launchFailed",
            ),
            (
                PythonConfig {
                    timeout: Duration::from_millis(150),
                    ..PythonConfig::default()
                },
                "import time\ntime.sleep(20)",
                "timedOut",
            ),
        ] {
            let (_directory, tools, _, filesystem) =
                observed_tools_with_python(QueryBehavior::Success, config).await;
            let report = tools
                .code_interpreter(PythonInput::Code(script))
                .await
                .unwrap();
            assert_eq!(report["execution"]["status"], expected);
            assert!(
                report["executionRecordPath"]
                    .as_str()
                    .unwrap()
                    .ends_with("result.json")
            );
            let stored: Value = serde_json::from_str(
                &filesystem
                    .read_text(&format!(
                        "/workspace/analysis/{}",
                        report["executionRecordPath"].as_str().unwrap()
                    ))
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(stored["report"], report);
            assert_eq!(stored["report"]["execution"]["status"], expected);
            assert!(stored["submission"]["scriptPath"].is_null());
            assert_eq!(
                filesystem
                    .read_text(&format!(
                        "/workspace/analysis/{}",
                        report["submittedCodePath"].as_str().unwrap()
                    ))
                    .await
                    .unwrap(),
                script,
            );
        }
        let (_directory, tools, _, filesystem) =
            observed_tools_with_python(QueryBehavior::Success, PythonConfig::default()).await;
        let signal = tools.yield_signal();
        let (report, ()) = tokio::join!(
            tools.code_interpreter(PythonInput::Code("import time\ntime.sleep(20)")),
            async {
                tokio::time::sleep(Duration::from_millis(150)).await;
                signal.request_shutdown().unwrap();
            }
        );
        let report = report.unwrap();
        assert_eq!(report["execution"]["status"], "cancelled");
        let stored: Value = serde_json::from_str(
            &filesystem
                .read_text(&format!(
                    "/workspace/analysis/{}",
                    report["executionRecordPath"].as_str().unwrap()
                ))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored["report"], report);
    }

    #[tokio::test]
    async fn real_sqlite_to_python_artifacts_survive_workspace_restart_without_replay() {
        use flaggo_evidence_store::{SqliteEvidenceAnalysisStore, SqliteEvidenceStore};

        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite://{}",
            directory
                .path()
                .join("evidence.db")
                .to_string_lossy()
                .replace('\\', "/")
        );
        let writer = SqliteEvidenceStore::connect(&database_url).await.unwrap();
        let reader = Arc::new(
            SqliteEvidenceAnalysisStore::connect(&database_url)
                .await
                .unwrap(),
        );
        let accepted_at = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single().unwrap();
        let now = accepted_at + chrono::Duration::hours(2);
        let contract = contract(accepted_at);
        let factory = LocalCapabilityFactory::new(
            Arc::new(FakeContractService {
                current: contract.clone(),
                submissions: Mutex::new(Vec::new()),
                now,
            }),
            reader.clone(),
            EvidenceQueryLimits {
                max_rows: NonZeroU32::new(1000).unwrap(),
                max_bytes: 65536,
                timeout: Duration::from_secs(1),
            },
            Arc::new(FixedClock(now)),
            PythonConfig::default(),
        );
        let workspace_root = directory.path().join("workspaces");
        let workspaces = LocalWorkspaceProvider::new(&workspace_root).unwrap();
        let lease = workspaces.claim(&contract.name, "attempt-1").await.unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .unwrap(),
        )
        .cycle;
        let tools = factory
            .create(
                context(&cycle, "attempt-1"),
                contract.clone(),
                lease.clone(),
            )
            .await
            .unwrap();
        let result = tools
            .run_sql("SELECT count(*) AS count FROM observations", None)
            .await
            .unwrap();
        assert_eq!(result["savedRows"], 1);
        assert_eq!(result["totalRows"], 1);
        let filesystem = lease
            .filesystem(&cycle.cycle_id, "attempt-1")
            .await
            .unwrap();
        filesystem.write_text("/workspace/analysis/model.py", &format!(
            "import json\nfrom pathlib import Path\nrows = json.loads(Path({:?}).read_text())['rows']\nPath('summary.json').write_text(json.dumps({{'count': rows[0]['count']}}))\nprint('SQLite -> Python')\n",
            result["resultPath"].as_str().unwrap(),
        )).await.unwrap();
        let execution = tools
            .code_interpreter(PythonInput::ScriptPath("model.py"))
            .await
            .unwrap();
        assert_eq!(execution["execution"]["status"], "succeeded");
        drop(tools);
        drop(filesystem);
        drop(lease);
        drop(workspaces);

        let restarted = LocalWorkspaceProvider::new(&workspace_root).unwrap();
        let lease = restarted.claim(&contract.name, "attempt-2").await.unwrap();
        let recovered = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-2", now)
                .await
                .unwrap(),
        )
        .cycle;
        assert_eq!(recovered.cycle_id, cycle.cycle_id);
        assert!(recovered.resumed);
        let filesystem = lease
            .filesystem(&recovered.cycle_id, "attempt-2")
            .await
            .unwrap();
        let summary: Value = serde_json::from_str(
            &filesystem
                .read_text("/workspace/analysis/summary.json")
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(summary["count"], 0);
        let saved = filesystem
            .read_text(&format!(
                "/workspace/analysis/{}",
                result["resultPath"].as_str().unwrap()
            ))
            .await
            .unwrap();
        assert_eq!(
            sha256_digest(saved.as_bytes()),
            result["contentDigest"].as_str().unwrap()
        );
        let record: Value = serde_json::from_str(
            &filesystem
                .read_text(&format!(
                    "/workspace/analysis/{}",
                    execution["executionRecordPath"].as_str().unwrap()
                ))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["report"], execution);
        assert_eq!(
            filesystem
                .read_directory("/workspace/analysis/executions")
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            filesystem
                .read_text(&format!(
                    "/workspace/analysis/{}",
                    result["historyPath"].as_str().unwrap()
                ))
                .await
                .unwrap()
                .lines()
                .count(),
            1
        );
        reader.close().await;
        writer.close().await;
    }

    #[tokio::test]
    async fn captures_live_sql_results_and_proposal_without_contents() {
        let capture = crate::telemetry_support::Capture::default();
        capture
            .run(async {
                let parent = tracing::info_span!("cycle");
                async {
                    let (_directory, tools, _now) = observed_tools(QueryBehavior::Success).await;
                    tools.describe().await.unwrap();
                    tools
                        .run_sql("SELECT 'private SQL prompt' FROM observations", None)
                        .await
                        .unwrap();
                    tools.propose_executable(json!([{
                "name": "secret-rule", "when": {"expression": "true"}, "return": {"value": 5}
            }])).await.unwrap();
                }
                .instrument(parent)
                .await;
            })
            .await;
        let events = capture.events.lock().unwrap();
        let terminal: Vec<_> = events
            .iter()
            .filter(|event| {
                event.fields.get("event.name") == Some(&json!("flaggo.analysis.action.completed"))
            })
            .collect();
        assert_eq!(terminal.len(), 3);
        for event in &terminal {
            assert_eq!(event.level, "INFO");
            assert_eq!(event.target, "flaggo.async-analysis");
            assert_eq!(event.fields["flaggo.analysis.action.outcome"], "success");
            assert!(
                event.fields["flaggo.analysis.action.duration_ms"]
                    .as_u64()
                    .is_some()
            );
            assert!(event.parent.is_some());
        }
        let sql = terminal
            .iter()
            .find(|event| event.fields["flaggo.analysis.action.kind"] == "evidence.sql")
            .unwrap();
        assert_eq!(
            sql.fields["flaggo.analysis.query.digest"],
            flaggo_analysis_workspace::sha256_digest(
                b"SELECT 'private SQL prompt' FROM observations"
            )
        );
        assert_eq!(sql.fields["flaggo.analysis.result.rows"], 1);
        assert_eq!(sql.fields["flaggo.analysis.result.bytes"], 11);
        assert!(
            !sql.fields
                .contains_key("flaggo.analysis.snapshot.watermark")
        );
        assert!(!sql.fields.contains_key("flaggo.analysis.snapshot.cutoff"));
        for event in events.iter() {
            let encoded = serde_json::to_string(&event.fields).unwrap();
            assert!(!encoded.contains("private SQL prompt"));
            assert!(!encoded.contains("secret-rule"));
            assert!(!encoded.contains("normalizedSql"));
            assert!(encoded.len() < 2048);
        }
        let spans = capture.spans.lock().unwrap();
        for span in spans
            .values()
            .filter(|span| span.name == "flaggo.analysis.action")
        {
            assert!(span.closed);
            assert_eq!(spans[&span.parent.unwrap()].name, "cycle");
            assert_eq!(span.fields["flaggo.analysis.action.outcome"], "success");
            assert!(
                span.fields["flaggo.analysis.action.duration_ms"]
                    .as_u64()
                    .is_some()
            );
        }
    }

    #[tokio::test]
    async fn captures_typed_query_failures_and_observed_cancellation_before_string_conversion() {
        for (behavior, outcome, category) in [
            (QueryBehavior::Timeout, "timeout", "timeout"),
            (QueryBehavior::Invalid, "failed", "validation"),
            (QueryBehavior::Pending, "interrupted", "cancelled"),
        ] {
            let capture = crate::telemetry_support::Capture::default();
            capture
                .run(async {
                    let (_directory, tools, _now) = observed_tools(behavior).await;
                    assert!(
                        tokio::time::timeout(
                            Duration::from_millis(10),
                            tools.run_sql("SELECT 'private' FROM observations", None)
                        )
                        .await
                        .map_or(true, |result| result.is_err())
                    );
                })
                .await;
            let events = capture.events.lock().unwrap();
            let terminal = events
                .iter()
                .find(|event| {
                    event.fields.get("event.name")
                        == Some(&json!("flaggo.analysis.action.completed"))
                        && event.fields.get("flaggo.analysis.action.kind")
                            == Some(&json!("evidence.sql"))
                })
                .unwrap();
            assert_eq!(terminal.fields["flaggo.analysis.action.outcome"], outcome);
            assert_eq!(terminal.fields["flaggo.failure.category"], category);
            assert!(
                terminal.fields["flaggo.analysis.action.duration_ms"]
                    .as_u64()
                    .is_some()
            );
            assert!(!terminal.fields.contains_key("flaggo.analysis.result.rows"));
            assert!(!terminal.fields.contains_key("flaggo.analysis.result.bytes"));
            assert!(
                !serde_json::to_string(&terminal.fields)
                    .unwrap()
                    .contains("private")
            );
        }
    }

    #[tokio::test]
    async fn emits_failed_precondition_and_proposal_actions_not_successful_method_labels() {
        let capture = crate::telemetry_support::Capture::default();
        capture
            .run(async {
                let (_directory, tools, _now) = observed_tools(QueryBehavior::Success).await;
                assert!(tools.run_sql("SELECT 'private'", Some(0)).await.is_err());
                assert!(
                    tools
                        .propose_executable(json!({"raw": "private"}))
                        .await
                        .is_err()
                );
                tools.yield_signal().request_shutdown().unwrap();
                assert!(tools.describe().await.is_err());
            })
            .await;
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 6);
        for terminal in events.iter().filter(|event| {
            event.fields.get("event.name") == Some(&json!("flaggo.analysis.action.completed"))
        }) {
            assert_eq!(terminal.fields["flaggo.analysis.action.outcome"], "failed");
            assert!(
                terminal.fields["flaggo.failure.category"]
                    .as_str()
                    .is_some()
            );
            assert!(
                !serde_json::to_string(&terminal.fields)
                    .unwrap()
                    .contains("private")
            );
        }
    }
}
