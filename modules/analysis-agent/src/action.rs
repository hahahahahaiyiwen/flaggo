//! Content-free observations at trusted analysis execution boundaries.

use std::time::Instant;

use flaggo_analysis_domain::{AnalysisError, AttemptContext};
use flaggo_analysis_workspace::sha256_digest;
use tracing::Span;
use uuid::Uuid;

pub const TARGET: &str = "flaggo.async-analysis";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKind {
    SessionStart,
    SessionRun,
    ProviderRequest,
    EvidenceDescribe,
    EvidenceSql,
    PythonRun,
    ArtifactRead,
    ArtifactWrite,
    ArtifactAppend,
    ProposalSubmit,
}

impl ActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session.start",
            Self::SessionRun => "session.run",
            Self::ProviderRequest => "provider.request",
            Self::EvidenceDescribe => "evidence.describe",
            Self::EvidenceSql => "evidence.sql",
            Self::PythonRun => "python.run",
            Self::ArtifactRead => "artifact.read",
            Self::ArtifactWrite => "artifact.write",
            Self::ArtifactAppend => "artifact.append",
            Self::ProposalSubmit => "proposal.submit",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Success,
    Failed,
    Timeout,
    Cancelled,
    Interrupted,
    Superseded,
    Handoff,
    NoChange,
    NoCandidate,
}

impl ActionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Superseded => "superseded",
            Self::Handoff => "handoff",
            Self::NoChange => "no_change",
            Self::NoCandidate => "no_candidate",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureCategory {
    Cancelled,
    Capacity,
    Configuration,
    Conflict,
    Dependency,
    Exporter,
    Integrity,
    Internal,
    NotFound,
    Timeout,
    Validation,
}

impl FailureCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Capacity => "capacity",
            Self::Configuration => "configuration",
            Self::Conflict => "conflict",
            Self::Dependency => "dependency",
            Self::Exporter => "exporter",
            Self::Integrity => "integrity",
            Self::Internal => "internal",
            Self::NotFound => "not_found",
            Self::Timeout => "timeout",
            Self::Validation => "validation",
        }
    }
}

pub fn failure_category(error: &AnalysisError) -> FailureCategory {
    match error {
        AnalysisError::WorkspaceBusy(_) | AnalysisError::ProposalConflict(_) => {
            FailureCategory::Conflict
        }
        AnalysisError::LimitExceeded(_) | AnalysisError::Superseded(_) => {
            FailureCategory::Validation
        }
        AnalysisError::InvalidState(_) => FailureCategory::Integrity,
        _ => FailureCategory::Dependency,
    }
}

/// Correlation is scoped to an actual handler invocation, never global state.
pub struct ToolCorrelation {
    tool_call_id: String,
    session_id: String,
}

impl ToolCorrelation {
    pub fn new(tool_call_id: &str, session_id: &str) -> Self {
        // Hash even short identities: provider-controlled strings may contain content.
        Self {
            tool_call_id: sha256_digest(tool_call_id.as_bytes()),
            session_id: sha256_digest(session_id.as_bytes()),
        }
    }

    pub async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        TOOL_CORRELATION.scope(self, future).await
    }
}

tokio::task_local! {
    static TOOL_CORRELATION: ToolCorrelation;
}

/// All optional attributes are constructed from typed trusted results or hashes.
pub struct ActionObservation {
    context: AttemptContext,
    kind: ActionKind,
    id: String,
    span: Span,
    started: Instant,
    finished: bool,
    query_digest: Option<String>,
    rows: Option<u64>,
    bytes: Option<u64>,
    artifact_digest: Option<String>,
    tool_call_id: Option<String>,
    session_id: Option<String>,
    failure: Option<FailureCategory>,
    enabled: bool,
}

impl ActionObservation {
    pub fn start(context: &AttemptContext, kind: ActionKind, parent: &Span) -> Self {
        let id = Uuid::new_v4().to_string();
        let correlation = TOOL_CORRELATION
            .try_with(|correlation| {
                (
                    correlation.tool_call_id.clone(),
                    correlation.session_id.clone(),
                )
            })
            .ok();
        let tool_call_id = correlation.as_ref().map(|(tool, _)| tool.clone());
        let session_id = correlation.map(|(_, session)| session);
        let enabled = valid_context(context);
        if !enabled {
            tracing::warn!(
                target: "flaggo_analysis_agent::action",
                "Analysis action telemetry omitted: invalid bounded correlation"
            );
        }
        let span = if enabled {
            tracing::info_span!(
                target: TARGET,
                parent: parent,
                "flaggo.analysis.action",
                flaggo.contract.name = %context.contract.name,
                flaggo.contract.digest = %context.contract.digest,
                flaggo.analysis.cycle.id = %context.cycle_id,
                flaggo.analysis.attempt.id = %context.attempt_id,
                flaggo.analysis.profile = tracing::field::Empty,
                flaggo.analysis.action.id = %id,
                flaggo.analysis.action.kind = kind.as_str(),
                flaggo.analysis.action.outcome = "running",
                flaggo.analysis.action.duration_ms = tracing::field::Empty,
                flaggo.analysis.query.digest = tracing::field::Empty,
                flaggo.analysis.result.rows = tracing::field::Empty,
                flaggo.analysis.result.bytes = tracing::field::Empty,
                flaggo.analysis.artifact.digest = tracing::field::Empty,
                flaggo.analysis.tool_call.id = tool_call_id.as_deref(),
                flaggo.analysis.session.id = session_id.as_deref(),
                flaggo.failure.category = tracing::field::Empty,
            )
        } else {
            Span::none()
        };
        let observation = Self {
            context: context.clone(),
            kind,
            id,
            span,
            started: Instant::now(),
            finished: false,
            query_digest: None,
            rows: None,
            bytes: None,
            artifact_digest: None,
            tool_call_id,
            session_id,
            failure: None,
            enabled,
        };
        observation.log("flaggo.analysis.action.started", "running", None);
        observation
    }

    pub fn span(&self) -> Span {
        self.span.clone()
    }

    pub fn query(&mut self, sql: &str) {
        let digest = sha256_digest(sql.as_bytes());
        self.span.record("flaggo.analysis.query.digest", &digest);
        self.query_digest = Some(digest);
    }

    pub fn result_size(&mut self, rows: usize, bytes: usize) {
        let rows = u64::try_from(rows).unwrap_or(u64::MAX);
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        self.rows = Some(rows);
        self.bytes = Some(bytes);
    }

    pub fn artifact(&mut self, content: &str) {
        let digest = sha256_digest(content.as_bytes());
        self.span.record("flaggo.analysis.artifact.digest", &digest);
        self.artifact_digest = Some(digest);
        self.bytes = Some(content.len() as u64);
    }

    pub fn finish_result<T>(&mut self, result: &Result<T, AnalysisError>) {
        match result {
            Ok(_) => self.finish(ActionOutcome::Success, None),
            Err(error) => self.finish(ActionOutcome::Failed, Some(failure_category(error))),
        }
    }

    pub fn finish(&mut self, outcome: ActionOutcome, failure: Option<FailureCategory>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.failure = failure;
        if outcome == ActionOutcome::Success {
            if let Some(rows) = self.rows {
                self.span.record("flaggo.analysis.result.rows", rows);
            }
            if let Some(bytes) = self.bytes {
                self.span.record("flaggo.analysis.result.bytes", bytes);
            }
        } else {
            self.rows = None;
            self.bytes = None;
        }
        let duration = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.span
            .record("flaggo.analysis.action.outcome", outcome.as_str());
        self.span
            .record("flaggo.analysis.action.duration_ms", duration);
        if let Some(failure) = failure {
            self.span
                .record("flaggo.failure.category", failure.as_str());
        }
        self.log(
            "flaggo.analysis.action.completed",
            outcome.as_str(),
            Some(duration),
        );
    }

    fn log(&self, event: &'static str, outcome: &'static str, duration: Option<u64>) {
        if !self.enabled {
            return;
        }
        let _entered = self.span.enter();
        tracing::event!(
            target: TARGET,
            tracing::Level::INFO,
            {
                "event.name" = event,
                "flaggo.contract.name" = %self.context.contract.name,
                "flaggo.contract.digest" = %self.context.contract.digest,
                "flaggo.analysis.cycle.id" = %self.context.cycle_id,
                "flaggo.analysis.attempt.id" = %self.context.attempt_id,
                "flaggo.analysis.action.id" = %self.id,
                "flaggo.analysis.action.kind" = self.kind.as_str(),
                "flaggo.analysis.action.outcome" = outcome,
                "flaggo.analysis.action.duration_ms" = duration,
                "flaggo.analysis.query.digest" = self.query_digest.as_deref(),
                "flaggo.analysis.result.rows" = self.rows,
                "flaggo.analysis.result.bytes" = self.bytes,
                "flaggo.analysis.artifact.digest" = self.artifact_digest.as_deref(),
                "flaggo.analysis.tool_call.id" = self.tool_call_id.as_deref(),
                "flaggo.analysis.session.id" = self.session_id.as_deref(),
                "flaggo.failure.category" = self.failure.map(FailureCategory::as_str),
            },
            "Analysis action"
        );
    }
}

fn valid_context(context: &AttemptContext) -> bool {
    let name = &context.contract.name;
    let digest = &context.contract.digest;
    !name.trim().is_empty()
        && name.len() <= 256
        && !name.chars().any(char::is_control)
        && digest.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        && [&context.cycle_id, &context.attempt_id]
            .into_iter()
            .all(|id| {
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
}

impl Drop for ActionObservation {
    fn drop(&mut self) {
        if !self.finished {
            // This is an observed in-process drop, not a claim about process loss.
            self.finish(ActionOutcome::Interrupted, Some(FailureCategory::Cancelled));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry_support::Capture;
    use flaggo_analysis_domain::AnalysisContractIdentity;
    use tracing::Instrument as _;

    fn context() -> AttemptContext {
        AttemptContext {
            workspace_id: "not exported".to_owned(),
            cycle_id: "cycle-1".to_owned(),
            attempt_id: "attempt-1".to_owned(),
            contract: AnalysisContractIdentity {
                name: "checkout.delay".to_owned(),
                digest: format!("sha256:{}", "a".repeat(64)),
            },
        }
    }

    #[tokio::test]
    async fn emits_correlated_info_events_and_final_span_without_sensitive_content() {
        let capture = Capture::default();
        let sql = format!(
            "SELECT 'secret prompt token dataset {}'",
            "x".repeat(100_000)
        );
        let artifact = "sensitive artifact contents";
        capture
            .run(async {
                let parent = tracing::info_span!("cycle");
                let mut action =
                    ActionObservation::start(&context(), ActionKind::EvidenceSql, &parent);
                action.query(&sql);
                async { tokio::task::yield_now().await }
                    .instrument(action.span())
                    .await;
                action.result_size(3, 11);
                action.artifact(artifact);
                action.finish(ActionOutcome::Success, None);
                action.finish(ActionOutcome::Failed, Some(FailureCategory::Internal));
            })
            .await;
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        let start = &events[0];
        let terminal = &events[1];
        assert_eq!(start.fields["event.name"], "flaggo.analysis.action.started");
        assert_eq!(start.fields["flaggo.analysis.action.outcome"], "running");
        assert!(
            !start
                .fields
                .contains_key("flaggo.analysis.action.duration_ms")
        );
        assert_eq!(
            terminal.fields["event.name"],
            "flaggo.analysis.action.completed"
        );
        assert_eq!(terminal.fields["flaggo.analysis.action.outcome"], "success");
        assert_eq!(
            terminal.fields["flaggo.analysis.query.digest"],
            sha256_digest(sql.as_bytes())
        );
        assert!(
            !terminal
                .fields
                .contains_key("flaggo.analysis.snapshot.watermark")
        );
        assert_eq!(terminal.fields["flaggo.analysis.result.rows"], 3);
        assert_eq!(
            terminal.fields["flaggo.analysis.artifact.digest"],
            sha256_digest(artifact.as_bytes())
        );
        assert!(
            terminal.fields["flaggo.analysis.action.duration_ms"]
                .as_u64()
                .is_some()
        );
        assert_eq!(
            start.fields["flaggo.analysis.action.id"],
            terminal.fields["flaggo.analysis.action.id"]
        );
        Uuid::parse_str(
            terminal.fields["flaggo.analysis.action.id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        for event in events.iter() {
            assert_eq!(event.target, TARGET);
            assert_eq!(event.level, "INFO");
            assert!(event.parent.is_some());
            let encoded = serde_json::to_string(&event.fields).unwrap();
            assert!(encoded.len() < 2048);
            for forbidden in [
                "secret prompt",
                "sensitive artifact",
                "not exported",
                "dataset",
                "requestedSql",
                "operation.name",
                "operation.outcome",
            ] {
                assert!(!encoded.contains(forbidden), "{forbidden}");
            }
        }
        let spans = capture.spans.lock().unwrap();
        let action = spans
            .values()
            .find(|span| span.name == "flaggo.analysis.action")
            .unwrap();
        assert!(action.closed);
        assert_eq!(action.fields["flaggo.analysis.action.outcome"], "success");
        assert_eq!(spans[&action.parent.unwrap()].name, "cycle");
        assert_eq!(
            action.fields["flaggo.analysis.query.digest"],
            terminal.fields["flaggo.analysis.query.digest"]
        );
    }

    #[tokio::test]
    async fn hashes_provider_identities_and_never_exports_payload_or_raw_error() {
        let capture = Capture::default();
        capture
            .run(
                ToolCorrelation::new(&"secret \n☃".repeat(10_000), "token/session").scope(async {
                    let mut action = ActionObservation::start(
                        &context(),
                        ActionKind::EvidenceDescribe,
                        &Span::none(),
                    );
                    action.finish_result::<()>(&Err(AnalysisError::Agent(
                        "raw token prompt".to_owned(),
                    )));
                }),
            )
            .await;
        let events = capture.events.lock().unwrap();
        let terminal = &events[1].fields;
        assert_eq!(terminal["flaggo.analysis.action.outcome"], "failed");
        assert_eq!(terminal["flaggo.failure.category"], "dependency");
        assert_eq!(
            terminal["flaggo.analysis.session.id"],
            sha256_digest(b"token/session")
        );
        assert_eq!(
            terminal["flaggo.analysis.tool_call.id"]
                .as_str()
                .unwrap()
                .len(),
            71
        );
        let encoded = serde_json::to_string(terminal).unwrap();
        assert!(!encoded.contains("raw token"));
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("token/session"));
    }

    #[tokio::test]
    async fn observed_cancellation_emits_interrupted_but_unpolled_future_emits_nothing() {
        let capture = Capture::default();
        capture
            .run(async {
                let unpolled = async {
                    let _action =
                        ActionObservation::start(&context(), ActionKind::SessionRun, &Span::none());
                };
                drop(unpolled);
                assert!(capture.events.lock().unwrap().is_empty());
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(1), async {
                        let action = ActionObservation::start(
                            &context(),
                            ActionKind::SessionRun,
                            &Span::none(),
                        );
                        std::future::pending::<()>().instrument(action.span()).await;
                    })
                    .await
                    .is_err()
                );
            })
            .await;
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1].fields["flaggo.analysis.action.outcome"],
            "interrupted"
        );
        assert_eq!(events[1].fields["flaggo.failure.category"], "cancelled");
    }

    #[tokio::test]
    async fn invalid_correlations_are_omitted_without_mutating_domain_identity() {
        let capture = Capture::default();
        let mut context = context();
        context.contract.name = "private name".repeat(1000);
        let original = context.contract.name.clone();
        capture
            .run(async {
                let mut action =
                    ActionObservation::start(&context, ActionKind::SessionStart, &Span::none());
                action.finish(ActionOutcome::Success, None);
            })
            .await;
        assert_eq!(context.contract.name, original);
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, "WARN");
        assert!(
            !serde_json::to_string(&events[0].fields)
                .unwrap()
                .contains("private")
        );
        assert!(capture.spans.lock().unwrap().is_empty());
    }

    #[test]
    fn validates_exact_bounded_identity_shape() {
        let mut context = context();
        assert!(valid_context(&context));
        context.contract.name = "a".repeat(256);
        context.attempt_id = "b".repeat(128);
        assert!(valid_context(&context));
        context.attempt_id.push('b');
        assert!(!valid_context(&context));
        context.attempt_id = "normal".to_owned();
        context.contract.name.push('a');
        assert!(!valid_context(&context));
        context.contract.name = "normal".to_owned();
        context.contract.digest = format!("sha256:{}", "g".repeat(64));
        assert!(!valid_context(&context));
        context.contract.digest = format!("sha256:{}", "a".repeat(64));
        context.cycle_id = "raw path/segment".to_owned();
        assert!(!valid_context(&context));
    }
}
