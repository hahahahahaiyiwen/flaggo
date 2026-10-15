use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use flaggo_analysis_domain::{AnalysisError, CandidateRecord, CycleLimits, WorkflowHandoff};
use flaggo_analysis_workspace::{WorkspaceFileSystem, sha256_digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisRunStatus,
    AnalysisTask, AnalysisTools, PythonInput, YieldSignal,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowState {
    AnalysisReady,
    DraftReady,
    RequestChange,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowMetadata {
    pub outcome: WorkflowState,
    pub proposal_path: String,
    pub proposal_digest: String,
    pub evidence_paths: Vec<String>,
    pub evidence_digests: BTreeMap<String, String>,
    pub revision: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfile {
    Proposer,
    Reviewer,
    General,
}

pub struct WorkflowAgentProvider {
    inner: Arc<dyn AgentProvider>,
    general: bool,
    selections: opentelemetry::metrics::Counter<u64>,
}

impl WorkflowAgentProvider {
    #[must_use]
    pub fn new(inner: Arc<dyn AgentProvider>, general: bool) -> Self {
        let scope = opentelemetry::InstrumentationScope::builder("flaggo.async-analysis")
            .with_version(env!("CARGO_PKG_VERSION"))
            .build();
        Self {
            inner,
            general,
            selections: opentelemetry::global::meter_with_scope(scope)
                .u64_counter("flaggo.analysis.profile.selections")
                .with_unit("{selection}")
                .build(),
        }
    }
}

pub fn work_state(
    handoff: Option<&WorkflowHandoff>,
) -> Result<Option<WorkflowMetadata>, AnalysisError> {
    let metadata = handoff
        .map(|handoff| serde_json::from_value::<WorkflowMetadata>(handoff.metadata.clone()))
        .transpose()
        .map_err(|error| {
            AnalysisError::InvalidState(format!("invalid workflow metadata: {error}"))
        })?;
    if metadata
        .as_ref()
        .is_some_and(|metadata| metadata.outcome == WorkflowState::AnalysisReady)
    {
        return Err(AnalysisError::InvalidState(
            "AnalysisReady is only an initial state".to_owned(),
        ));
    }
    Ok(metadata)
}

#[async_trait]
impl AgentProvider for WorkflowAgentProvider {
    async fn start_session(
        &self,
        mut spec: AgentSessionSpec,
    ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
        let current = work_state(spec.handoff.as_ref())?;
        let draft_rules = if let Some(metadata) = &current {
            if metadata.revision > spec.cycle_limits.max_revisions {
                return Err(AnalysisError::InvalidState(
                    "handoff exceeds the cycle revision limit".to_owned(),
                ));
            }
            Some(validate_artifacts(spec.filesystem.as_ref(), metadata).await?)
        } else {
            None
        };
        let state = current
            .as_ref()
            .map_or(WorkflowState::AnalysisReady, |metadata| metadata.outcome);
        let profile = if self.general {
            AgentProfile::General
        } else if state == WorkflowState::DraftReady {
            AgentProfile::Reviewer
        } else {
            AgentProfile::Proposer
        };
        let directory = format!("/workspace/analysis/attempts/{}", spec.context.attempt_id);
        spec.filesystem.create_directory(&directory, true).await?;
        let filesystem = spec.filesystem.clone();
        let tools = spec.tools.clone();
        let limits = spec.cycle_limits.clone();
        let identity = spec.context.clone();
        spec.tools = Arc::new(ProfileTools {
            inner: tools.clone(),
            profile,
            state,
            draft_rules,
        });
        spec.task.prompt.push_str(&format!(
            "\n\n<workflow-context>{}</workflow-context>",
            json!({
                "profile": profile,
                "state": state,
                "handoff": spec.handoff,
                "writeDirectory": directory,
                "maxRevisions": limits.max_revisions,
                "outcomes": match profile {
                    AgentProfile::Proposer => vec!["draftReady", "noChange", "noCandidate", "handoff", "failure", "superseded"],
                    AgentProfile::Reviewer => vec!["candidate", "reject", "requestChange", "handoff", "failure", "superseded"],
                    AgentProfile::General => vec!["draftReady", "noChange", "noCandidate", "candidate", "reject", "requestChange", "handoff", "failure", "superseded"],
                }
            })
        ));
        spec.filesystem = Arc::new(ProfileFileSystem {
            inner: filesystem.clone(),
            directory,
        });
        let task = spec.task.clone();
        let profile_name = match profile {
            AgentProfile::Proposer => "proposer",
            AgentProfile::Reviewer => "reviewer",
            AgentProfile::General => "general",
        };
        tracing::Span::current().record("flaggo.analysis.profile", profile_name);
        self.selections.add(
            1,
            &[opentelemetry::KeyValue::new(
                "flaggo.analysis.profile",
                profile_name,
            )],
        );
        tracing::event!(
            target: "flaggo.async-analysis",
            tracing::Level::INFO,
            {
            "event.name" = "flaggo.analysis.profile.selected",
            "flaggo.analysis.profile" = profile_name,
            "flaggo.analysis.cycle.id" = %identity.cycle_id,
            "flaggo.analysis.attempt.id" = %identity.attempt_id,
            "flaggo.contract.name" = %identity.contract.name,
            "flaggo.contract.digest" = %identity.contract.digest,
            },
            "Selected analysis profile"
        );
        let session = self.inner.start_session(spec).await?;
        Ok(Arc::new(WorkflowSession {
            session,
            task,
            current,
            profile,
            filesystem,
            tools,
            limits,
            directory: format!("attempts/{}", identity.attempt_id),
        }))
    }
}

struct WorkflowSession {
    session: Arc<dyn AgentSession>,
    task: AnalysisTask,
    current: Option<WorkflowMetadata>,
    profile: AgentProfile,
    filesystem: Arc<dyn WorkspaceFileSystem>,
    tools: Arc<dyn AnalysisTools>,
    limits: CycleLimits,
    directory: String,
}

#[async_trait]
impl AgentSession for WorkflowSession {
    async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        let result = self.session.run(self.task.clone()).await?;
        let explanation = match &result {
            AgentRunOutcome::NoChange { reason }
            | AgentRunOutcome::NoCandidate { reason }
            | AgentRunOutcome::Failure { reason }
            | AgentRunOutcome::Reject { reason, .. } => Some(reason),
            AgentRunOutcome::Handoff { summary, .. } => Some(summary),
            _ => None,
        };
        if explanation.is_some_and(|value| value.trim().is_empty()) {
            return Err(AnalysisError::InvalidState(
                "workflow outcome requires a non-empty explanation".to_owned(),
            ));
        }
        let state = self
            .current
            .as_ref()
            .map_or(WorkflowState::AnalysisReady, |metadata| metadata.outcome);
        let invalid = match &result {
            AgentRunOutcome::Candidate => {
                self.profile == AgentProfile::Proposer || state != WorkflowState::DraftReady
            }
            AgentRunOutcome::Reject { review_path, .. } => {
                read_review(self.filesystem.as_ref(), review_path).await?;
                self.profile == AgentProfile::Proposer
                    || state != WorkflowState::DraftReady
                    || !review_path.starts_with(&format!("{}/", self.directory))
            }
            AgentRunOutcome::NoChange { .. } | AgentRunOutcome::NoCandidate { .. } => {
                self.profile == AgentProfile::Reviewer
            }
            AgentRunOutcome::Handoff {
                metadata: Some(value),
                ..
            } => {
                let next: WorkflowMetadata =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        AnalysisError::InvalidState(format!("invalid handoff metadata: {error}"))
                    })?;
                validate_artifacts(self.filesystem.as_ref(), &next).await?;
                if next.outcome == WorkflowState::DraftReady
                    && !next
                        .proposal_path
                        .starts_with(&format!("{}/", self.directory))
                    || next.outcome == WorkflowState::RequestChange
                        && next
                            .review_path
                            .as_ref()
                            .is_none_or(|path| !path.starts_with(&format!("{}/", self.directory)))
                {
                    return Err(AnalysisError::InvalidState(
                        "new drafts and reviews must belong to the current attempt".to_owned(),
                    ));
                }
                let revision = self
                    .current
                    .as_ref()
                    .map_or(0, |metadata| metadata.revision);
                match (state, next.outcome) {
                    (WorkflowState::AnalysisReady, WorkflowState::DraftReady) => {
                        self.profile == AgentProfile::Reviewer || next.revision != 0
                    }
                    (WorkflowState::RequestChange, WorkflowState::DraftReady) => {
                        self.profile == AgentProfile::Reviewer
                            || next.revision
                                != revision.checked_add(1).ok_or_else(|| {
                                    AnalysisError::InvalidState("revision overflow".to_owned())
                                })?
                            || next.revision > self.limits.max_revisions
                            || self.current.as_ref().is_some_and(|previous| {
                                previous.proposal_path == next.proposal_path
                            })
                    }
                    (WorkflowState::DraftReady, WorkflowState::RequestChange) => {
                        next.revision != revision
                            || revision >= self.limits.max_revisions
                            || self.profile == AgentProfile::Proposer
                            || self.current.as_ref().is_none_or(|previous| {
                                previous.proposal_path != next.proposal_path
                                    || previous.proposal_digest != next.proposal_digest
                                    || previous.evidence_paths != next.evidence_paths
                                    || previous.evidence_digests != next.evidence_digests
                            })
                            || next.review_path.is_none()
                    }
                    _ => true,
                }
            }
            _ => false,
        };
        if invalid {
            return Err(AnalysisError::InvalidState(
                "outcome is not permitted by the workflow state and profile".to_owned(),
            ));
        }
        if matches!(result, AgentRunOutcome::Candidate)
            != self.tools.proposed_candidate().await?.is_some()
        {
            return Err(AnalysisError::InvalidState(
                "Candidate outcome must match a successful submission".to_owned(),
            ));
        }
        match result {
            AgentRunOutcome::Reject {
                reason,
                review_path,
            } => {
                self.filesystem
                    .write_text(
                        &format!("/workspace/analysis/{}/rejection.json", self.directory),
                        &json!({"outcome": "reject", "reason": reason, "reviewPath": review_path})
                            .to_string(),
                    )
                    .await?;
                Ok(AgentRunOutcome::NoCandidate { reason })
            }
            result => Ok(result),
        }
    }

    async fn request_yield(&self, reason: &str) -> Result<(), AnalysisError> {
        self.session.request_yield(reason).await
    }

    async fn close(&self) -> Result<(), AnalysisError> {
        self.session.close().await
    }
}

fn artifact_path(path: &str) -> Result<String, AnalysisError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AnalysisError::InvalidState(
            "workflow artifact path must be relative to analysis".to_owned(),
        ));
    }
    Ok(format!("/workspace/analysis/{path}"))
}

async fn read_review(
    filesystem: &dyn WorkspaceFileSystem,
    path: &str,
) -> Result<Value, AnalysisError> {
    let content = read_artifact(filesystem, path).await?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|error| AnalysisError::InvalidState(format!("review must be JSON: {error}")))?;
    if !value.is_object() {
        return Err(AnalysisError::InvalidState(
            "review must be a JSON object".to_owned(),
        ));
    }
    Ok(value)
}

async fn read_artifact(
    filesystem: &dyn WorkspaceFileSystem,
    path: &str,
) -> Result<String, AnalysisError> {
    let path = artifact_path(path)?;
    if !filesystem.exists(&path).await? {
        return Err(AnalysisError::InvalidState(
            "referenced workflow artifact is missing".to_owned(),
        ));
    }
    filesystem.read_text(&path).await
}

async fn validate_artifacts(
    filesystem: &dyn WorkspaceFileSystem,
    metadata: &WorkflowMetadata,
) -> Result<Value, AnalysisError> {
    let proposal = read_artifact(filesystem, &metadata.proposal_path).await?;
    if sha256_digest(proposal.as_bytes()) != metadata.proposal_digest {
        return Err(AnalysisError::InvalidState(
            "draft does not match handoff digest".to_owned(),
        ));
    }
    let draft: Value = serde_json::from_str(&proposal)
        .map_err(|error| AnalysisError::InvalidState(format!("draft must be JSON: {error}")))?;
    if !draft
        .get("rules")
        .and_then(Value::as_array)
        .is_some_and(|rules| !rules.is_empty())
    {
        return Err(AnalysisError::InvalidState(
            "draft requires a non-empty rules array".to_owned(),
        ));
    }
    if metadata.evidence_paths.len() != metadata.evidence_digests.len()
        || metadata
            .evidence_digests
            .keys()
            .any(|path| !metadata.evidence_paths.contains(path))
    {
        return Err(AnalysisError::InvalidState(
            "each saved evidence path requires exactly one digest".to_owned(),
        ));
    }
    for path in &metadata.evidence_paths {
        let content = read_artifact(filesystem, path).await?;
        if metadata.evidence_digests.get(path) != Some(&sha256_digest(content.as_bytes())) {
            return Err(AnalysisError::InvalidState(
                "saved evidence does not match handoff digest".to_owned(),
            ));
        }
    }
    if let Some(path) = &metadata.review_path {
        read_review(filesystem, path).await?;
    }
    Ok(draft["rules"].clone())
}

struct ProfileTools {
    inner: Arc<dyn AnalysisTools>,
    profile: AgentProfile,
    state: WorkflowState,
    draft_rules: Option<Value>,
}

#[async_trait]
impl AnalysisTools for ProfileTools {
    fn can_propose(&self) -> bool {
        self.profile != AgentProfile::Proposer && self.state == WorkflowState::DraftReady
    }
    async fn check_cycle_status(&self) -> Result<AnalysisRunStatus, AnalysisError> {
        self.inner.check_cycle_status().await
    }
    async fn describe(&self) -> Result<Value, AnalysisError> {
        self.inner.describe().await
    }
    async fn run_sql(&self, sql: &str, limit: Option<u32>) -> Result<Value, AnalysisError> {
        self.inner.run_sql(sql, limit).await
    }
    async fn code_interpreter(&self, input: PythonInput<'_>) -> Result<Value, AnalysisError> {
        self.inner.code_interpreter(input).await
    }
    async fn propose_executable(&self, rules: Value) -> Result<CandidateRecord, AnalysisError> {
        if self.profile == AgentProfile::Proposer || self.state != WorkflowState::DraftReady {
            return Err(AnalysisError::InvalidState(
                "submission is not permitted for this profile and workflow state".to_owned(),
            ));
        }
        if self.draft_rules.as_ref() != Some(&rules) {
            return Err(AnalysisError::InvalidState(
                "submitted rules do not match the committed draft".to_owned(),
            ));
        }
        self.inner.propose_executable(rules).await
    }
    async fn proposed_candidate(&self) -> Result<Option<CandidateRecord>, AnalysisError> {
        self.inner.proposed_candidate().await
    }
    fn yield_signal(&self) -> Arc<YieldSignal> {
        self.inner.yield_signal()
    }
}

struct ProfileFileSystem {
    inner: Arc<dyn WorkspaceFileSystem>,
    directory: String,
}

impl ProfileFileSystem {
    fn writable(&self, path: &str) -> Result<(), AnalysisError> {
        let path = path.trim_start_matches('/');
        if path.contains('\\')
            || path
                .split('/')
                .any(|segment| segment == "." || segment == "..")
            || !(path == self.directory.trim_start_matches('/')
                || path.starts_with(&format!("{}/", self.directory.trim_start_matches('/')))
                || path == "workspace/analysis/.copilot"
                || path.starts_with("workspace/analysis/.copilot/")
                || matches!(
                    path,
                    "workspace/analysis/findings.md"
                        | "workspace/analysis/supersession.md"
                        | "workspace/handoffs/checkpoint.md"
                ))
        {
            return Err(AnalysisError::Workspace(
                "profile writes are restricted to its attempt directory and lifecycle notes"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl WorkspaceFileSystem for ProfileFileSystem {
    async fn read_text(&self, path: &str) -> Result<String, AnalysisError> {
        self.inner.read_text(path).await
    }
    async fn exists(&self, path: &str) -> Result<bool, AnalysisError> {
        self.inner.exists(path).await
    }
    async fn metadata(
        &self,
        path: &str,
    ) -> Result<flaggo_analysis_workspace::WorkspaceFileInfo, AnalysisError> {
        self.inner.metadata(path).await
    }
    async fn read_directory(
        &self,
        path: &str,
    ) -> Result<Vec<flaggo_analysis_workspace::WorkspaceEntry>, AnalysisError> {
        self.inner.read_directory(path).await
    }
    async fn write_text(&self, path: &str, text: &str) -> Result<(), AnalysisError> {
        self.writable(path)?;
        self.inner.write_text(path, text).await
    }
    async fn append_text(&self, path: &str, text: &str) -> Result<(), AnalysisError> {
        self.writable(path)?;
        self.inner.append_text(path, text).await
    }
    async fn create_directory(&self, path: &str, recursive: bool) -> Result<(), AnalysisError> {
        self.writable(path)?;
        self.inner.create_directory(path, recursive).await
    }
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), AnalysisError> {
        self.writable(path)?;
        self.inner.remove(path, recursive, force).await
    }
    async fn rename(&self, source: &str, destination: &str) -> Result<(), AnalysisError> {
        self.writable(source)?;
        self.writable(destination)?;
        self.inner.rename(source, destination).await
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use flaggo_analysis_domain::{AnalysisProfile, AttemptContext};
    use flaggo_analysis_workspace::{
        LocalWorkspaceProvider, PrepareCycleResult, WorkspaceProvider,
    };
    use std::sync::Mutex;

    use super::*;

    struct OutcomeProvider {
        outcome: AgentRunOutcome,
        observed: Mutex<Vec<(bool, String)>>,
    }

    #[async_trait]
    impl AgentProvider for OutcomeProvider {
        async fn start_session(
            &self,
            spec: AgentSessionSpec,
        ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
            self.observed
                .lock()
                .unwrap()
                .push((spec.tools.can_propose(), spec.task.prompt));
            assert!(
                spec.filesystem
                    .write_text("/workspace/analysis/attempts/another/proposal.json", "{}")
                    .await
                    .is_err()
            );
            assert!(
                spec.filesystem
                    .write_text("/workspace/cycle.json", "{}")
                    .await
                    .is_err()
            );
            spec.filesystem
                .write_text("/workspace/analysis/.copilot/provider-state.json", "{}")
                .await?;
            Ok(Arc::new(OutcomeSession(self.outcome.clone())))
        }
    }

    struct OutcomeSession(AgentRunOutcome);

    #[async_trait]
    impl AgentSession for OutcomeSession {
        async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
            Ok(self.0.clone())
        }
        async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
            Ok(())
        }
        async fn close(&self) -> Result<(), AnalysisError> {
            Ok(())
        }
    }

    async fn setup() -> (tempfile::TempDir, AgentSessionSpec) {
        let root = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(root.path()).unwrap();
        let mut spec = crate::provider::tests::test_spec();
        let lease = provider
            .claim(&spec.contract.name, "attempt")
            .await
            .unwrap();
        let PrepareCycleResult::Ready(preparation) = lease
            .prepare_cycle(
                &spec.contract,
                &AnalysisProfile {
                    skill_names: vec![],
                },
                "attempt",
                Utc::now(),
            )
            .await
            .unwrap()
        else {
            panic!("expected eligible cycle")
        };
        spec.context = AttemptContext::new(&preparation.cycle, "attempt".to_owned());
        spec.filesystem = lease
            .filesystem(&preparation.cycle.cycle_id, "attempt")
            .await
            .unwrap();
        (root, spec)
    }

    async fn draft(spec: &AgentSessionSpec, attempt: &str, revision: u32) -> WorkflowMetadata {
        let path = format!("attempts/{attempt}/proposal.json");
        let content =
            json!({"rules": [{"when": true, "then": 1}], "method": "saved evidence"}).to_string();
        spec.filesystem
            .write_text(&format!("/workspace/analysis/{path}"), &content)
            .await
            .unwrap();
        spec.filesystem
            .write_text("/workspace/analysis/queries/input.json", "{\"rows\": []}")
            .await
            .unwrap();
        WorkflowMetadata {
            outcome: WorkflowState::DraftReady,
            proposal_path: path,
            proposal_digest: sha256_digest(content.as_bytes()),
            evidence_paths: vec!["queries/input.json".to_owned()],
            evidence_digests: BTreeMap::from([(
                "queries/input.json".to_owned(),
                sha256_digest(b"{\"rows\": []}"),
            )]),
            revision,
            review_path: None,
        }
    }

    fn handoff(metadata: &WorkflowMetadata) -> WorkflowHandoff {
        WorkflowHandoff {
            attempt_id: "previous".to_owned(),
            summary: "ready".to_owned(),
            metadata: serde_json::to_value(metadata).unwrap(),
        }
    }

    async fn execute(
        spec: AgentSessionSpec,
        outcome: AgentRunOutcome,
        general: bool,
    ) -> (Result<AgentRunOutcome, AnalysisError>, Arc<OutcomeProvider>) {
        let inner = Arc::new(OutcomeProvider {
            outcome,
            observed: Mutex::new(vec![]),
        });
        let task = spec.task.clone();
        let provider = WorkflowAgentProvider::new(inner.clone(), general);
        let session = provider.start_session(spec).await.unwrap();
        let result = session.run(task).await;
        session.close().await.unwrap();
        (result, inner)
    }

    #[tokio::test]
    async fn routes_initial_work_to_proposer_and_enforces_submission_and_catalog_permissions() {
        let (_root, spec) = setup().await;
        let metadata = draft(&spec, "attempt", 0).await;
        let (result, inner) = execute(
            spec,
            AgentRunOutcome::Handoff {
                summary: "draft".to_owned(),
                metadata: Some(serde_json::to_value(metadata).unwrap()),
            },
            false,
        )
        .await;
        assert!(result.is_ok());
        {
            let observed = inner.observed.lock().unwrap();
            assert!(!observed[0].0);
            assert!(observed[0].1.contains("\"profile\":\"proposer\""));
        }
        let tools = ProfileTools {
            inner: crate::provider::tests::test_spec().tools,
            profile: AgentProfile::Proposer,
            state: WorkflowState::AnalysisReady,
            draft_rules: None,
        };
        assert!(matches!(
            tools.propose_executable(json!([{}])).await,
            Err(AnalysisError::InvalidState(_))
        ));
    }

    #[tokio::test]
    async fn reviewer_request_change_and_revised_draft_preserve_version_and_bound_revisions() {
        let (_root, mut spec) = setup().await;
        let initial = draft(&spec, "previous", 0).await;
        spec.handoff = Some(handoff(&initial));
        let mut change = initial.clone();
        change.outcome = WorkflowState::RequestChange;
        change.review_path = Some("attempts/attempt/review.json".to_owned());
        spec.filesystem
            .write_text(
                "/workspace/analysis/attempts/attempt/review.json",
                "{\"changes\":\"clarify evidence\"}",
            )
            .await
            .unwrap();
        let (result, inner) = execute(
            spec,
            AgentRunOutcome::Handoff {
                summary: "revise".to_owned(),
                metadata: Some(serde_json::to_value(&change).unwrap()),
            },
            false,
        )
        .await;
        assert!(result.is_ok());
        assert!(inner.observed.lock().unwrap()[0].0);
        assert!(
            inner.observed.lock().unwrap()[0]
                .1
                .contains("\"profile\":\"reviewer\"")
        );

        let (_root, mut spec) = setup().await;
        let mut change = draft(&spec, "previous", 0).await;
        change.outcome = WorkflowState::RequestChange;
        change.review_path = Some("attempts/previous/review.json".to_owned());
        spec.filesystem
            .write_text("/workspace/analysis/attempts/previous/review.json", "{}")
            .await
            .unwrap();
        spec.handoff = Some(handoff(&change));
        let revision = draft(&spec, "attempt", 1).await;
        assert!(
            execute(
                spec,
                AgentRunOutcome::Handoff {
                    summary: "revision".to_owned(),
                    metadata: Some(serde_json::to_value(revision).unwrap())
                },
                false
            )
            .await
            .0
            .is_ok()
        );

        let (_root, mut spec) = setup().await;
        let mut exhausted = draft(&spec, "previous", 1).await;
        spec.handoff = Some(handoff(&exhausted));
        exhausted.outcome = WorkflowState::RequestChange;
        exhausted.review_path = Some("attempts/attempt/review.json".to_owned());
        spec.filesystem
            .write_text("/workspace/analysis/attempts/attempt/review.json", "{}")
            .await
            .unwrap();
        assert!(matches!(
            execute(
                spec,
                AgentRunOutcome::Handoff {
                    summary: "one more".to_owned(),
                    metadata: Some(serde_json::to_value(exhausted).unwrap())
                },
                false
            )
            .await
            .0,
            Err(AnalysisError::InvalidState(_))
        ));
    }

    #[tokio::test]
    async fn general_profile_is_state_driven_and_reviewer_can_reject_or_checkpoint() {
        for general in [false, true] {
            let (_root, mut spec) = setup().await;
            let metadata = draft(&spec, "previous", 0).await;
            spec.handoff = Some(handoff(&metadata));
            spec.filesystem
                .write_text(
                    "/workspace/analysis/attempts/attempt/review.json",
                    "{\"reject\":\"unsupported\"}",
                )
                .await
                .unwrap();
            let (result, provider) = execute(
                spec,
                AgentRunOutcome::Reject {
                    reason: "unsupported".to_owned(),
                    review_path: "attempts/attempt/review.json".to_owned(),
                },
                general,
            )
            .await;
            assert!(matches!(result, Ok(AgentRunOutcome::NoCandidate { .. })));
            assert!(provider.observed.lock().unwrap()[0].0);
        }
        let (_root, mut spec) = setup().await;
        spec.handoff = Some(handoff(&draft(&spec, "previous", 0).await));
        assert!(
            execute(
                spec,
                AgentRunOutcome::Handoff {
                    summary: "checkpoint".to_owned(),
                    metadata: None
                },
                false
            )
            .await
            .0
            .is_ok()
        );
        let (_root, mut spec) = setup().await;
        spec.handoff = Some(handoff(&draft(&spec, "previous", 0).await));
        assert!(matches!(
            execute(
                spec,
                AgentRunOutcome::NoChange {
                    reason: "not my outcome".to_owned()
                },
                false
            )
            .await
            .0,
            Err(AnalysisError::InvalidState(_))
        ));
    }

    #[tokio::test]
    async fn rejects_corrupt_metadata_or_changed_draft_before_creating_session() {
        for corruption in ["metadata", "draft", "evidence"] {
            let (_root, mut spec) = setup().await;
            let mut metadata = draft(&spec, "previous", 0).await;
            if corruption == "draft" {
                metadata.proposal_digest = "changed".to_owned();
            }
            let mut saved = handoff(&metadata);
            if corruption == "metadata" {
                saved.metadata["unknown"] = json!(true);
            }
            if corruption == "evidence" {
                spec.filesystem
                    .write_text("/workspace/analysis/queries/input.json", "{\"rows\": [1]}")
                    .await
                    .unwrap();
            }
            spec.handoff = Some(saved);
            let inner = Arc::new(OutcomeProvider {
                outcome: AgentRunOutcome::Candidate,
                observed: Mutex::new(vec![]),
            });
            assert!(matches!(
                WorkflowAgentProvider::new(inner.clone(), false)
                    .start_session(spec)
                    .await,
                Err(AnalysisError::InvalidState(_))
            ));
            assert!(inner.observed.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn emits_bounded_profile_selection_with_exact_attempt_correlation() {
        for (general, reviewing, profile) in [
            (false, false, "proposer"),
            (false, true, "reviewer"),
            (true, false, "general"),
        ] {
            let (_root, mut spec) = setup().await;
            let identity = spec.context.clone();
            if reviewing {
                spec.handoff = Some(handoff(&draft(&spec, "previous", 0).await));
            }
            let capture = crate::telemetry_support::Capture::default();
            capture
                .run(async {
                    use tracing::Instrument as _;
                    let mut action = crate::action::ActionObservation::start(
                        &identity,
                        crate::action::ActionKind::SessionStart,
                        &tracing::Span::none(),
                    );
                    let result = execute(
                        spec,
                        AgentRunOutcome::Handoff {
                            summary: "private checkpoint narrative".to_owned(),
                            metadata: None,
                        },
                        general,
                    )
                    .instrument(action.span())
                    .await
                    .0;
                    action.finish_result(&result);
                    assert!(result.is_ok());
                })
                .await;
            let events = capture.events.lock().unwrap();
            let selected = events
                .iter()
                .find(|event| {
                    event.fields.get("event.name").and_then(Value::as_str)
                        == Some("flaggo.analysis.profile.selected")
                })
                .unwrap();
            assert_eq!(selected.fields["flaggo.analysis.profile"], profile);
            assert_eq!(
                selected.fields["flaggo.analysis.attempt.id"],
                identity.attempt_id
            );
            assert_eq!(
                selected.fields["flaggo.analysis.cycle.id"],
                identity.cycle_id
            );
            assert!(
                !serde_json::to_string(&selected.fields)
                    .unwrap()
                    .contains("private checkpoint")
            );
            let spans = capture.spans.lock().unwrap();
            assert!(spans.values().any(|span| {
                span.fields
                    .get("flaggo.analysis.profile")
                    .and_then(Value::as_str)
                    == Some(profile)
            }));
        }
    }

    #[test]
    fn attempt_filesystem_rejects_traversal_and_other_attempt_mutation() {
        let filesystem = ProfileFileSystem {
            inner: crate::provider::tests::test_spec().filesystem,
            directory: "/workspace/analysis/attempts/new".to_owned(),
        };
        for path in [
            "/workspace/analysis/attempts/old/draft.json",
            "/workspace/analysis/attempts/new/../old/a",
            "/workspace/analysis/attempts/new\\old",
            "/workspace/analysis/queries/q.json",
            "/workspace/cycle.json",
        ] {
            assert!(filesystem.writable(path).is_err());
        }
        for path in [
            "/workspace/analysis/attempts/new",
            "/workspace/analysis/attempts/new/model.py",
            "/workspace/analysis/findings.md",
            "/workspace/handoffs/checkpoint.md",
        ] {
            filesystem.writable(path).unwrap();
        }
    }
}
