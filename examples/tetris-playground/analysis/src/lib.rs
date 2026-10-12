mod evidence;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use flaggo_analysis_agent::{
    AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec, AnalysisRunStatus,
    AnalysisTask, AnalysisTools, context::ANALYSIS_FINDINGS_PATH,
};
use flaggo_analysis_domain::AnalysisError;
use serde_json::{Value, json};

use evidence::analyze_evidence;

const CONTRACT_NAME: &str = "tetris.dropInterval";
const EVIDENCE_PAGE_SIZE: usize = 100;
const EVIDENCE_QUERY: &str = "\
SELECT observation_sequence, signal_name, payload_json \
FROM observations \
WHERE signal_name IN (\
    'flaggo.decision.received', \
    'tetris.experience.challenge_balance', \
    'tetris.experience.lines_cleared', \
    'tetris.experience.game_over_count', \
    'tetris.experience.active_play_ms'\
)";

pub struct TetrisPlaygroundAgentProvider;

#[async_trait]
impl AgentProvider for TetrisPlaygroundAgentProvider {
    async fn start_session(
        &self,
        specification: AgentSessionSpec,
    ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
        if specification.contract.name != CONTRACT_NAME {
            return Err(AnalysisError::Agent(format!(
                "the Tetris playground provider only supports '{CONTRACT_NAME}'"
            )));
        }
        Ok(Arc::new(TetrisPlaygroundAgentSession {
            tools: specification.tools,
            filesystem: specification.filesystem,
            yield_requested: AtomicBool::new(false),
        }))
    }
}

struct TetrisPlaygroundAgentSession {
    tools: Arc<dyn AnalysisTools>,
    filesystem: Arc<dyn flaggo_analysis_workspace::WorkspaceFileSystem>,
    yield_requested: AtomicBool,
}

#[async_trait]
impl AgentSession for TetrisPlaygroundAgentSession {
    async fn run(&self, _task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        if let Some(outcome) = self.yield_outcome().await? {
            return Ok(outcome);
        }
        validate_evidence_capabilities(&self.tools.describe().await?)?;
        self.tools.commit_cutoff(chrono::Utc::now()).await?;
        let mut rows = Vec::new();
        let mut cursor = 0;
        loop {
            if let Some(outcome) = self.yield_outcome().await? {
                return Ok(outcome);
            }
            let page = self
                .tools
                .run_sql(&format!(
                    "{EVIDENCE_QUERY} AND observation_sequence > {cursor} \
                     ORDER BY observation_sequence LIMIT {EVIDENCE_PAGE_SIZE}"
                ))
                .await?;
            match append_evidence_page(&mut rows, &page, cursor)? {
                Some(next) => cursor = next,
                None => break,
            }
        }
        let analysis = analyze_evidence(&json!({"rows": rows}))?;
        self.filesystem
            .write_text(ANALYSIS_FINDINGS_PATH, &analysis.findings)
            .await?;
        if let Some(outcome) = self.yield_outcome().await? {
            return Ok(outcome);
        }
        match analysis.proposal {
            Ok(rules) => {
                self.tools.propose_executable(rules).await?;
                Ok(AgentRunOutcome::Candidate)
            }
            Err(reason) => Ok(AgentRunOutcome::NoCandidate { reason }),
        }
    }

    async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
        self.yield_requested.store(true, Ordering::Release);
        Ok(())
    }

    async fn close(&self) -> Result<(), AnalysisError> {
        Ok(())
    }
}

fn validate_evidence_capabilities(description: &Value) -> Result<(), AnalysisError> {
    if description.get("relation").and_then(Value::as_str) != Some("observations")
        || description
            .get("queryCapabilities")
            .and_then(|capabilities| capabilities.get("dialect"))
            .and_then(Value::as_str)
            != Some("sqlite")
    {
        return Err(AnalysisError::Evidence(
            "Tetris playground analysis requires the observations relation and sqlite dialect"
                .to_owned(),
        ));
    }
    Ok(())
}

fn append_evidence_page(
    rows: &mut Vec<Value>,
    result: &Value,
    mut cursor: u64,
) -> Result<Option<u64>, AnalysisError> {
    let page = result
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| AnalysisError::Evidence("query page has no rows".to_owned()))?;
    if page.len() > EVIDENCE_PAGE_SIZE {
        return Err(AnalysisError::Evidence(
            "query page exceeds the requested row bound".to_owned(),
        ));
    }
    for row in page {
        let next = row
            .get("observation_sequence")
            .and_then(Value::as_u64)
            .filter(|sequence| *sequence > cursor)
            .ok_or_else(|| {
                AnalysisError::Evidence(
                    "query page has no strictly increasing observation sequence".to_owned(),
                )
            })?;
        cursor = next;
    }
    rows.extend(page.iter().cloned());
    Ok((page.len() == EVIDENCE_PAGE_SIZE).then_some(cursor))
}

impl TetrisPlaygroundAgentSession {
    async fn yield_outcome(&self) -> Result<Option<AgentRunOutcome>, AnalysisError> {
        if !self.yield_requested.load(Ordering::Acquire) {
            return Ok(None);
        }
        match self.tools.check_cycle_status().await? {
            AnalysisRunStatus::Active | AnalysisRunStatus::ServiceShutdown => {
                Ok(Some(AgentRunOutcome::Handoff {
                    summary: "Tetris playground analysis yielded for service shutdown".to_owned(),
                }))
            }
            AnalysisRunStatus::ContractSuperseded { .. } => Ok(Some(AgentRunOutcome::Superseded)),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EVIDENCE_PAGE_SIZE, append_evidence_page, validate_evidence_capabilities};

    #[test]
    fn accepts_the_fixed_sql_evidence_capabilities() {
        assert!(
            validate_evidence_capabilities(&json!({
                "relation": "observations",
                "queryCapabilities": {"dialect": "sqlite"}
            }))
            .is_ok()
        );
    }

    #[test]
    fn rejects_incompatible_or_missing_evidence_capabilities_with_a_bounded_error() {
        for description in [
            json!({}),
            json!({"relation": "observations"}),
            json!({"relation": "observations", "queryCapabilities": {"dialect": "postgres"}}),
            json!({"relation": "private_relation", "queryCapabilities": {"dialect": "sqlite"}}),
            json!({"relation": "observations", "queryCapabilities": {"dialect": true}}),
        ] {
            match validate_evidence_capabilities(&description) {
                Err(flaggo_analysis_domain::AnalysisError::Evidence(message)) => assert_eq!(
                    message,
                    "Tetris playground analysis requires the observations relation and sqlite dialect"
                ),
                result => panic!("expected a bounded evidence error, got {result:?}"),
            }
        }
    }

    #[test]
    fn reads_complete_bounded_pages_with_sequence_gaps() {
        let mut rows = Vec::new();
        let full = json!({
            "rows": (1..=EVIDENCE_PAGE_SIZE).map(|sequence| {
                json!({"observation_sequence": sequence})
            }).collect::<Vec<_>>()
        });
        let next = append_evidence_page(&mut rows, &full, 0)
            .expect("page")
            .expect("more rows");
        let last = json!({"rows": [
            {"observation_sequence": next + 2},
            {"observation_sequence": next + 7}
        ]});
        assert_eq!(
            append_evidence_page(&mut rows, &last, next).expect("last page"),
            None
        );
        assert_eq!(rows.len(), EVIDENCE_PAGE_SIZE + 2);
        assert_eq!(
            append_evidence_page(&mut rows, &json!({"rows": []}), next + 7).expect("empty page"),
            None
        );
    }

    #[test]
    fn invalid_query_pages_fail_without_appending_partial_data() {
        for result in [
            json!({}),
            json!({"rows": [{"observation_sequence": "1"}]}),
            json!({"rows": [{"observation_sequence": 1}, {"observation_sequence": 1}]}),
            json!({"rows": vec![json!({"observation_sequence": 1}); EVIDENCE_PAGE_SIZE + 1]}),
        ] {
            let mut rows = Vec::new();
            assert!(append_evidence_page(&mut rows, &result, 0).is_err());
            assert!(rows.is_empty());
        }
    }
}
