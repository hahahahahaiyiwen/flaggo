use std::collections::BTreeMap;

use flaggo_analysis_agent::{
    AgentRunOutcome, PythonInput,
    workflow::{WorkflowMetadata, WorkflowState},
};
use flaggo_analysis_domain::AnalysisError;
use flaggo_analysis_workspace::sha256_digest;
use serde_json::{Value, json};

use super::{TetrisPlaygroundAgentSession, evidence::analyze_evidence};

impl TetrisPlaygroundAgentSession {
    pub(super) async fn prepare_exit_draft(
        &self,
        query_paths: &[String],
    ) -> Result<AgentRunOutcome, AnalysisError> {
        let evidence_path = format!(
            "{}/evidence.json",
            self.directory.trim_start_matches("/workspace/analysis/")
        );
        let code = format!(
            r#"import json
from collections import Counter
from pathlib import Path
# The saved script retains Windows extended-length path support.
root = Path(__file__).parents[2]
paths = json.loads({paths:?})
rows = [row for path in paths for row in json.loads((root / path).read_text(encoding="utf-8"))["rows"]]
unique = {{}}
for row in rows:
    key = row["observation_sequence"]
    if key in unique and unique[key] != row:
        raise ValueError("conflicting observation sequence")
    unique[key] = row
ordered = [unique[key] for key in sorted(unique)]
coverage = dict(Counter(row["signal_name"] for row in ordered))
if not coverage.get("flaggo.decision.received"):
    raise ValueError("no decision receipts")
(root / {evidence_path:?}).write_text(json.dumps({{"rows": ordered, "signalCounts": coverage}}), encoding="utf-8")
print(json.dumps({{"inputRows": len(rows), "uniqueRows": len(ordered), "signalCounts": coverage}}))
"#,
            paths = serde_json::to_string(query_paths)
                .map_err(|error| AnalysisError::Agent(error.to_string()))?,
        );
        let report = self
            .tools
            .code_interpreter(PythonInput::Code(&code))
            .await?;
        if report["execution"]["status"] != "succeeded" {
            return Err(AnalysisError::Agent(
                "exit-scenario evidence preparation failed".to_owned(),
            ));
        }
        let mut paths = vec![evidence_path];
        for field in ["submittedCodePath", "executionRecordPath"] {
            paths.push(
                report[field]
                    .as_str()
                    .ok_or_else(|| AnalysisError::Agent(format!("Python report has no {field}")))?
                    .to_owned(),
            );
        }
        self.write_exit_draft(
            paths,
            json!([{
                "name": "unsupported-constant-policy",
                "when": {"expression": "true"},
                "return": {"value": 800}
            }]),
            0,
        )
        .await
    }

    pub(super) async fn revise_exit_draft(
        &self,
        draft: &WorkflowMetadata,
    ) -> Result<AgentRunOutcome, AnalysisError> {
        let review_path = draft.review_path.as_ref().ok_or_else(|| {
            AnalysisError::InvalidState("revision requires saved findings".to_owned())
        })?;
        self.filesystem
            .read_text(&format!("/workspace/analysis/{review_path}"))
            .await?;
        let evidence = self
            .filesystem
            .read_text(&format!("/workspace/analysis/{}", draft.evidence_paths[0]))
            .await?;
        let evidence: Value = serde_json::from_str(&evidence)
            .map_err(|error| AnalysisError::Evidence(error.to_string()))?;
        let rules = analyze_evidence(&evidence)?
            .proposal
            .map_err(AnalysisError::Evidence)?;
        let mut paths = draft.evidence_paths.clone();
        paths.push(review_path.clone());
        self.write_exit_draft(paths, rules, draft.revision + 1)
            .await
    }

    async fn write_exit_draft(
        &self,
        evidence_paths: Vec<String>,
        rules: Value,
        revision: u32,
    ) -> Result<AgentRunOutcome, AnalysisError> {
        let proposal_path = format!(
            "{}/proposal.json",
            self.directory.trim_start_matches("/workspace/analysis/")
        );
        let proposal = json!({
            "rules": rules,
            "method": if revision == 0 {
                "Exit-test fault injection: assume constant 800 without contextual support"
            } else {
                "Correct the unsupported draft using the original Python-prepared saved evidence"
            },
        })
        .to_string();
        self.filesystem
            .write_text(&format!("/workspace/analysis/{proposal_path}"), &proposal)
            .await?;
        let mut evidence_digests = BTreeMap::new();
        for path in &evidence_paths {
            let content = self
                .filesystem
                .read_text(&format!("/workspace/analysis/{path}"))
                .await?;
            evidence_digests.insert(path.clone(), sha256_digest(content.as_bytes()));
        }
        let metadata = WorkflowMetadata {
            outcome: WorkflowState::DraftReady,
            proposal_path,
            proposal_digest: sha256_digest(proposal.as_bytes()),
            evidence_paths,
            evidence_digests,
            revision,
            review_path: None,
        };
        Ok(AgentRunOutcome::Handoff {
            summary: format!("Exit-scenario draft revision {revision} ready for review"),
            metadata: Some(
                serde_json::to_value(metadata)
                    .map_err(|error| AnalysisError::Agent(error.to_string()))?,
            ),
        })
    }
}
