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
const EVIDENCE_PAGE_SIZE: u32 = 100;
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
        let page_size = evidence_page_size(&self.tools.describe().await?)?;
        let mut rows = Vec::new();
        let mut cursor = 0;
        loop {
            if let Some(outcome) = self.yield_outcome().await? {
                return Ok(outcome);
            }
            let page = self
                .tools
                .run_sql(
                    &format!(
                        "{EVIDENCE_QUERY} AND observation_sequence > {cursor} \
                     ORDER BY observation_sequence LIMIT {page_size}"
                    ),
                    Some(page_size),
                )
                .await?;
            let result_path = page
                .get("resultPath")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AnalysisError::Evidence("query has no saved result path".to_owned())
                })?;
            let saved = self
                .filesystem
                .read_text(&format!("/workspace/analysis/{result_path}"))
                .await?;
            let page = serde_json::from_str(&saved).map_err(|error| {
                AnalysisError::Evidence(format!("invalid saved query result: {error}"))
            })?;
            match append_evidence_page(&mut rows, &page, cursor, page_size)? {
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

fn evidence_page_size(description: &Value) -> Result<u32, AnalysisError> {
    let capabilities = &description["queryCapabilities"];
    if capabilities["dialect"].as_str() != Some("sqlite")
        || !capabilities["relations"]
            .as_array()
            .is_some_and(|relations| {
                relations
                    .iter()
                    .any(|relation| relation["name"].as_str() == Some("observations"))
            })
    {
        return Err(AnalysisError::Evidence(
            "Tetris playground analysis requires the observations relation and sqlite dialect"
                .to_owned(),
        ));
    }
    let max_rows = description["rules"]["resultRowsLimited"]
        .as_u64()
        .and_then(|limit| u32::try_from(limit).ok())
        .filter(|limit| *limit > 0)
        .ok_or_else(|| {
            AnalysisError::Evidence(
                "Tetris playground analysis requires a positive u32 resultRowsLimited ceiling"
                    .to_owned(),
            )
        })?;
    Ok(EVIDENCE_PAGE_SIZE.min(max_rows))
}

fn append_evidence_page(
    rows: &mut Vec<Value>,
    result: &Value,
    mut cursor: u64,
    page_size: u32,
) -> Result<Option<u64>, AnalysisError> {
    let page = result
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| AnalysisError::Evidence("query page has no rows".to_owned()))?;
    if page.len() > page_size as usize {
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
    Ok((page.len() == page_size as usize).then_some(cursor))
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

    use super::{EVIDENCE_PAGE_SIZE, append_evidence_page, evidence_page_size};

    #[test]
    fn discovers_observations_from_store_owned_sql_capabilities() {
        assert!(
            evidence_page_size(&json!({
                "queryCapabilities": {
                    "dialect": "sqlite",
                    "relations": [
                        {"name": "another_relation", "columns": []},
                        {"name": "observations", "columns": []}
                    ]
                },
                "rules": {"resultRowsLimited": 100}
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
            json!({"queryCapabilities": {"dialect": "sqlite", "relations": []}}),
            json!({"queryCapabilities": {"dialect": "sqlite", "relations": [{"name": "private_relation"}]}}),
            json!({"queryCapabilities": {"dialect": "postgres", "relations": [{"name": "observations"}]}}),
        ] {
            match evidence_page_size(&description) {
                Err(flaggo_analysis_domain::AnalysisError::Evidence(message)) => assert_eq!(
                    message,
                    "Tetris playground analysis requires the observations relation and sqlite dialect"
                ),
                result => panic!("expected a bounded evidence error, got {result:?}"),
            }
        }
    }

    #[test]
    fn chooses_page_size_within_the_advertised_ceiling() {
        for (ceiling, expected) in [(1, 1), (37, 37), (100, 100), (500, 100)] {
            let description = json!({
                "queryCapabilities": {
                    "dialect": "sqlite",
                    "relations": [{"name": "observations"}]
                },
                "rules": {"resultRowsLimited": ceiling}
            });
            assert_eq!(evidence_page_size(&description).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_missing_or_invalid_row_ceilings() {
        for ceiling in [
            json!(null),
            json!(0),
            json!(-1),
            json!("37"),
            json!(1.5),
            json!(u64::from(u32::MAX) + 1),
        ] {
            let description = json!({
                "queryCapabilities": {
                    "dialect": "sqlite",
                    "relations": [{"name": "observations"}]
                },
                "rules": {"resultRowsLimited": ceiling}
            });
            assert!(matches!(
                evidence_page_size(&description),
                Err(flaggo_analysis_domain::AnalysisError::Evidence(_))
            ));
        }
    }

    #[test]
    fn small_pages_continue_until_all_rows_are_read() {
        for page_size in [1, 2, 37] {
            let expected: Vec<_> = (1..=page_size * 2 + 1)
                .map(|sequence| json!({"observation_sequence": sequence * 3}))
                .collect();
            let mut rows = Vec::new();
            let mut cursor = 0;
            for chunk in expected.chunks(page_size as usize) {
                let next =
                    append_evidence_page(&mut rows, &json!({"rows": chunk}), cursor, page_size)
                        .unwrap();
                if chunk.len() == page_size as usize {
                    cursor = next.expect("a full page must continue");
                } else {
                    assert_eq!(next, None);
                }
            }
            assert_eq!(rows, expected);
            assert_eq!(
                append_evidence_page(&mut rows, &json!({"rows": []}), cursor, page_size).unwrap(),
                None
            );
            let mut rejected_rows = Vec::new();
            assert!(
                append_evidence_page(
                    &mut rejected_rows,
                    &json!({"rows": &expected[..page_size as usize + 1]}),
                    0,
                    page_size,
                )
                .is_err()
            );
            assert!(rejected_rows.is_empty());
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
        let next = append_evidence_page(&mut rows, &full, 0, EVIDENCE_PAGE_SIZE)
            .expect("page")
            .expect("more rows");
        let last = json!({"rows": [
            {"observation_sequence": next + 2},
            {"observation_sequence": next + 7}
        ]});
        assert_eq!(
            append_evidence_page(&mut rows, &last, next, EVIDENCE_PAGE_SIZE).expect("last page"),
            None
        );
        assert_eq!(rows.len(), EVIDENCE_PAGE_SIZE as usize + 2);
        assert_eq!(
            append_evidence_page(
                &mut rows,
                &json!({"rows": []}),
                next + 7,
                EVIDENCE_PAGE_SIZE
            )
            .expect("empty page"),
            None
        );
    }

    #[test]
    fn invalid_query_pages_fail_without_appending_partial_data() {
        for result in [
            json!({}),
            json!({"rows": [{"observation_sequence": "1"}]}),
            json!({"rows": [{"observation_sequence": 1}, {"observation_sequence": 1}]}),
            json!({"rows": vec![json!({"observation_sequence": 1}); EVIDENCE_PAGE_SIZE as usize + 1]}),
        ] {
            let mut rows = Vec::new();
            assert!(append_evidence_page(&mut rows, &result, 0, EVIDENCE_PAGE_SIZE).is_err());
            assert!(rows.is_empty());
        }
    }
}
