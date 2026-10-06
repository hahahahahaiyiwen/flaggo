use std::sync::Arc;

use async_trait::async_trait;
use flaggo_analysis_domain::AnalysisError;
use github_copilot_sdk::{
    Error as CopilotError, ErrorKind,
    mode::ToolSet,
    tool::ToolHandler,
    types::{PermissionRequestData, PermissionRequestKind, Tool, ToolInvocation, ToolResult},
};
use serde_json::{Value, json};

use crate::AnalysisTools;

const BUILTIN_TOOL_NAMES: &[&str] = &["view", "edit", "rg", "glob", "skill"];
pub const CHECK_CYCLE_STATUS_TOOL: &str = "check_cycle_status";
pub const COMMIT_CUTOFF_TOOL: &str = "commit_cutoff";
pub const DESCRIBE_TOOL: &str = "describe";
pub const RUN_SQL_TOOL: &str = "run_sql";
pub const PROPOSE_EXECUTABLE_TOOL: &str = "propose_executable";
pub const REGISTERED_ANALYSIS_TOOL_NAMES: &[&str] = &[
    CHECK_CYCLE_STATUS_TOOL,
    COMMIT_CUTOFF_TOOL,
    DESCRIBE_TOOL,
    RUN_SQL_TOOL,
    PROPOSE_EXECUTABLE_TOOL,
];

pub(super) fn available_tools() -> Result<Vec<String>, AnalysisError> {
    let tools = BUILTIN_TOOL_NAMES
        .iter()
        .try_fold(ToolSet::new(), |tools, name| tools.add_builtin(name))
        .and_then(|tools| {
            REGISTERED_ANALYSIS_TOOL_NAMES
                .iter()
                .try_fold(tools, |tools, name| tools.add_custom(name))
        })
        .map_err(|error| AnalysisError::Agent(error.to_string()))?;
    Ok(tools.into_vec())
}

pub(super) fn is_analysis_permission(request: &PermissionRequestData) -> bool {
    match request.kind {
        Some(PermissionRequestKind::Read | PermissionRequestKind::Write) => true,
        Some(PermissionRequestKind::CustomTool) => request
            .extra
            .pointer("/permissionRequest/toolName")
            .or_else(|| request.extra.get("toolName"))
            .and_then(Value::as_str)
            .is_some_and(|name| REGISTERED_ANALYSIS_TOOL_NAMES.contains(&name)),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum AnalysisToolKind {
    CheckCycleStatus,
    CommitCutoff,
    Describe,
    RunSql,
    ProposeExecutable,
}

struct AnalysisToolHandler {
    kind: AnalysisToolKind,
    tools: Arc<dyn AnalysisTools>,
}

#[async_trait]
impl ToolHandler for AnalysisToolHandler {
    async fn call(&self, invocation: ToolInvocation) -> Result<ToolResult, CopilotError> {
        let result = match self.kind {
            AnalysisToolKind::CheckCycleStatus => {
                serde_json::to_value(self.tools.check_cycle_status().await.map_err(tool_error)?)?
            }
            AnalysisToolKind::CommitCutoff => {
                let cutoff = invocation
                    .arguments
                    .get("cutoff")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            "commit_cutoff requires an RFC 3339 cutoff",
                        )
                    })?
                    .parse()
                    .map_err(|error| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            format!("invalid RFC 3339 cutoff: {error}"),
                        )
                    })?;
                serde_json::to_value(self.tools.commit_cutoff(cutoff).await.map_err(tool_error)?)?
            }
            AnalysisToolKind::Describe => self.tools.describe().await.map_err(tool_error)?,
            AnalysisToolKind::RunSql => {
                let sql = invocation
                    .arguments
                    .get("sql")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            "run_sql requires SQL text",
                        )
                    })?;
                self.tools.run_sql(sql).await.map_err(tool_error)?
            }
            AnalysisToolKind::ProposeExecutable => {
                let rules = invocation.arguments.get("rules").cloned().ok_or_else(|| {
                    CopilotError::with_message(
                        ErrorKind::InvalidConfig,
                        "propose_executable requires rules",
                    )
                })?;
                serde_json::to_value(
                    self.tools
                        .propose_executable(rules)
                        .await
                        .map_err(tool_error)?,
                )?
            }
        };
        Ok(ToolResult::Text(serde_json::to_string(&result)?))
    }
}

pub(super) fn copilot_tools(tools: Arc<dyn AnalysisTools>) -> Vec<Tool> {
    vec![
        tool(
            CHECK_CYCLE_STATUS_TOOL,
            "Check whether this cycle is active, superseded, or stopping.",
            json!({"type": "object", "additionalProperties": false}),
            AnalysisToolKind::CheckCycleStatus,
            Arc::clone(&tools),
        ),
        tool(
            COMMIT_CUTOFF_TOOL,
            "Commit the cycle's immutable data cutoff and storage watermark.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["cutoff"],
                "properties": {
                    "cutoff": {
                        "type": "string",
                        "description": "RFC 3339 UTC timestamp no later than now."
                    }
                }
            }),
            AnalysisToolKind::CommitCutoff,
            Arc::clone(&tools),
        ),
        tool(
            DESCRIBE_TOOL,
            "Describe the available read-only database relation, SQL capabilities, and trusted scope.",
            json!({"type": "object", "additionalProperties": false}),
            AnalysisToolKind::Describe,
            Arc::clone(&tools),
        ),
        tool(
            RUN_SQL_TOOL,
            "Run one bounded read-only SQLite query over the scoped observations relation.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["sql"],
                "properties": {
                    "sql": {"type": "string"}
                }
            }),
            AnalysisToolKind::RunSql,
            Arc::clone(&tools),
        ),
        tool(
            PROPOSE_EXECUTABLE_TOOL,
            "Validate and persist exactly one inactive Candidate for this cycle.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["rules"],
                "properties": {
                    "rules": {
                        "type": "array",
                        "minItems": 1,
                        "items": {"type": "object"}
                    }
                }
            }),
            AnalysisToolKind::ProposeExecutable,
            tools,
        ),
    ]
}

fn tool(
    name: &'static str,
    description: &'static str,
    parameters: Value,
    kind: AnalysisToolKind,
    tools: Arc<dyn AnalysisTools>,
) -> Tool {
    Tool::new(name)
        .with_description(description)
        .with_parameters(parameters)
        .with_handler(Arc::new(AnalysisToolHandler { kind, tools }))
}

fn tool_error(error: AnalysisError) -> CopilotError {
    CopilotError::with_message(ErrorKind::InvalidConfig, error.to_string())
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::{PermissionRequestData, PermissionRequestKind};
    use serde_json::json;

    use super::{
        BUILTIN_TOOL_NAMES, CHECK_CYCLE_STATUS_TOOL, REGISTERED_ANALYSIS_TOOL_NAMES,
        available_tools, is_analysis_permission,
    };

    #[test]
    fn exposes_the_complete_bounded_tool_catalog() {
        let available = available_tools().expect("build tool catalog");
        let expected = BUILTIN_TOOL_NAMES
            .iter()
            .map(|name| format!("builtin:{name}"))
            .chain(
                REGISTERED_ANALYSIS_TOOL_NAMES
                    .iter()
                    .map(|name| format!("custom:{name}")),
            )
            .collect::<Vec<_>>();

        assert_eq!(available, expected);
    }

    #[test]
    fn approves_only_bounded_analysis_permissions() {
        for kind in [PermissionRequestKind::Read, PermissionRequestKind::Write] {
            assert!(is_analysis_permission(&PermissionRequestData {
                kind: Some(kind),
                ..PermissionRequestData::default()
            }));
        }
        assert!(is_analysis_permission(&PermissionRequestData {
            kind: Some(PermissionRequestKind::CustomTool),
            extra: json!({
                "permissionRequest": {
                    "toolName": CHECK_CYCLE_STATUS_TOOL
                }
            }),
            ..PermissionRequestData::default()
        }));
        for kind in [
            PermissionRequestKind::Shell,
            PermissionRequestKind::Url,
            PermissionRequestKind::Mcp,
            PermissionRequestKind::Memory,
            PermissionRequestKind::Hook,
            PermissionRequestKind::Unknown,
        ] {
            assert!(!is_analysis_permission(&PermissionRequestData {
                kind: Some(kind),
                ..PermissionRequestData::default()
            }));
        }
        assert!(!is_analysis_permission(&PermissionRequestData {
            kind: Some(PermissionRequestKind::CustomTool),
            extra: json!({
                "permissionRequest": {
                    "toolName": "unregistered_tool"
                }
            }),
            ..PermissionRequestData::default()
        }));
    }
}
