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
use tracing::Instrument as _;

use crate::action::ToolCorrelation;
use crate::{AnalysisTools, PythonInput};

const BUILTIN_TOOL_NAMES: &[&str] = &["view", "create", "edit", "rg", "glob", "skill"];
pub const CHECK_CYCLE_STATUS_TOOL: &str = "check_cycle_status";
pub const DESCRIBE_TOOL: &str = "describe";
pub const RUN_SQL_TOOL: &str = "run_sql";
pub const CODE_INTERPRETER_TOOL: &str = "code_interpreter";
pub const PROPOSE_EXECUTABLE_TOOL: &str = "propose_executable";
pub const REGISTERED_ANALYSIS_TOOL_NAMES: &[&str] = &[
    CHECK_CYCLE_STATUS_TOOL,
    DESCRIBE_TOOL,
    RUN_SQL_TOOL,
    CODE_INTERPRETER_TOOL,
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
    Describe,
    RunSql,
    CodeInterpreter,
    ProposeExecutable,
}

struct AnalysisToolHandler {
    kind: AnalysisToolKind,
    tools: Arc<dyn AnalysisTools>,
    parent: tracing::Span,
}

#[async_trait]
impl ToolHandler for AnalysisToolHandler {
    async fn call(&self, invocation: ToolInvocation) -> Result<ToolResult, CopilotError> {
        let correlation =
            ToolCorrelation::new(&invocation.tool_call_id, invocation.session_id.as_ref());
        correlation
            .scope(self.call_inner(invocation).instrument(self.parent.clone()))
            .await
    }
}

impl AnalysisToolHandler {
    async fn call_inner(&self, invocation: ToolInvocation) -> Result<ToolResult, CopilotError> {
        let result = match self.kind {
            AnalysisToolKind::CheckCycleStatus => {
                serde_json::to_value(self.tools.check_cycle_status().await.map_err(tool_error)?)?
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
                let row_limit = invocation
                    .arguments
                    .get("row_limit")
                    .map(|value| serde_json::from_value::<u32>(value.clone()))
                    .transpose()?;
                self.tools
                    .run_sql(sql, row_limit)
                    .await
                    .map_err(tool_error)?
            }
            AnalysisToolKind::CodeInterpreter => {
                let input = match (
                    invocation.arguments.get("script_path"),
                    invocation.arguments.get("code"),
                ) {
                    (Some(_), None) => PythonInput::ScriptPath(required_string(
                        &invocation.arguments,
                        "script_path",
                    )?),
                    (None, Some(_)) => {
                        PythonInput::Code(required_string(&invocation.arguments, "code")?)
                    }
                    _ => {
                        return Err(CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            "code_interpreter requires exactly one of script_path or code",
                        ));
                    }
                };
                self.tools
                    .code_interpreter(input)
                    .await
                    .map_err(tool_error)?
            }
            AnalysisToolKind::ProposeExecutable => {
                let rules = proposal_rules(&invocation.arguments)?;
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
            DESCRIBE_TOOL,
            "Describe the available read-only database relation, SQL capabilities, and trusted scope.",
            json!({"type": "object", "additionalProperties": false}),
            AnalysisToolKind::Describe,
            Arc::clone(&tools),
        ),
        tool(
            RUN_SQL_TOOL,
            concat!(
                "Run one scoped read-only SQLite query against live evidence. ",
                "Return metadata, not inline rows. resultPath and historyPath reference the saved result ",
                "and query log, relative to the cycle analysis directory. columns describes output names and ",
                "types (null when unknown), including empty results. Read the result JSON's rows array. ",
                "totalRows is null when truncated, otherwise equals savedRows; it counts query-result rows. ",
                "Use describe for SQL capabilities and limits."
            ),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["sql"],
                "properties": {
                    "sql": {"type": "string"},
                    "row_limit": {"type": "integer", "minimum": 1, "description": "Maximum saved rows. Defaults to min(100, the service ceiling reported by describe); must not exceed that ceiling."}
                }
            }),
            AnalysisToolKind::RunSql,
            Arc::clone(&tools),
        ),
        tool(
            CODE_INTERPRETER_TOOL,
            concat!(
                "Execute Python from script_path or inline code (exactly one) in a fresh, unsandboxed local process. ",
                "Return runtime metadata, execution-record references and execution status, exitCode, stdout and stderr. ",
                "Only execution.status=succeeded denotes success; check capture truncation and outputComplete."
            ),
            json!({
                "type": "object", "additionalProperties": false,
                "properties": {
                    "script_path": {"type": "string", "minLength": 1, "description": "Existing Python file: absolute virtual path or relative to the analysis directory."},
                    "code": {"type": "string", "minLength": 1, "description": "Inline Python source to execute."}
                }
            }),
            AnalysisToolKind::CodeInterpreter,
            Arc::clone(&tools),
        ),
        tool(
            PROPOSE_EXECUTABLE_TOOL,
            "Validate and persist exactly one inactive Candidate for a supported executable change. Do not call for noChange or noCandidate.",
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

fn proposal_rules(arguments: &Value) -> Result<Value, CopilotError> {
    let rules = arguments
        .get("rules")
        .and_then(Value::as_array)
        .filter(|rules| !rules.is_empty())
        .ok_or_else(|| {
            CopilotError::with_message(
                ErrorKind::InvalidConfig,
                "propose_executable requires at least one rule",
            )
        })?;
    Ok(Value::Array(rules.clone()))
}

fn required_string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, CopilotError> {
    arguments.get(name).and_then(Value::as_str).ok_or_else(|| {
        CopilotError::with_message(
            ErrorKind::InvalidConfig,
            format!("{name} requires a string"),
        )
    })
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
        .with_handler(Arc::new(AnalysisToolHandler {
            kind,
            tools,
            parent: tracing::Span::current(),
        }))
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
        available_tools, is_analysis_permission, proposal_rules,
    };

    #[test]
    fn exposes_the_complete_analysis_tool_catalog() {
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
        assert!(!available.iter().any(|name| name == "custom:commit_cutoff"));
    }

    #[test]
    fn tool_descriptions_own_usage_and_results_not_operator_configuration() {
        let catalog = super::copilot_tools(crate::provider::tests::test_spec().tools);
        for (name, mechanics) in [
            (
                super::RUN_SQL_TOOL,
                &[
                    "resultPath",
                    "historyPath",
                    "columns",
                    "including empty results",
                    "rows array",
                    "savedRows",
                    "totalRows is null",
                    "truncated",
                    "relative to the cycle analysis directory",
                ][..],
            ),
            (
                super::CODE_INTERPRETER_TOOL,
                &[
                    "script_path",
                    "inline code (exactly one)",
                    "fresh, unsandboxed local process",
                    "execution.status",
                    "truncation",
                    "outputComplete",
                    "unsandboxed",
                ][..],
            ),
        ] {
            let tool = catalog
                .iter()
                .find(|tool| tool.name == name)
                .expect("registered tool");
            let description = &tool.description;
            for mechanic in mechanics {
                assert!(
                    description.contains(mechanic),
                    "{name} must explain {mechanic}"
                );
            }
            assert!(
                !description.contains("FLAGGO_"),
                "operator configuration belongs in the README"
            );
        }
    }

    #[test]
    fn approves_only_registered_analysis_permissions() {
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

    #[test]
    fn rejects_empty_executable_proposals() {
        let error = proposal_rules(&json!({"rules": []}))
            .expect_err("empty rules must not represent a proposal");

        assert_eq!(
            error.to_string(),
            "propose_executable requires at least one rule"
        );
    }

    #[tokio::test]
    async fn translates_sql_and_python_arguments_and_rejects_malformed_calls() {
        use github_copilot_sdk::{
            tool::ToolHandler as _,
            types::{ToolInvocation, ToolResult},
        };
        for (kind, name, arguments, expected) in [
            (
                super::AnalysisToolKind::RunSql,
                "run_sql",
                json!({"sql": "SELECT count(*) FROM observations", "row_limit": 123}),
                json!({"requestedSql": "SELECT count(*) FROM observations", "rowLimit": 123, "resultPath": "queries/test.json"}),
            ),
            (
                super::AnalysisToolKind::CodeInterpreter,
                "code_interpreter",
                json!({"script_path": "a.py"}),
                json!({"scriptPath": "a.py"}),
            ),
            (
                super::AnalysisToolKind::CodeInterpreter,
                "code_interpreter",
                json!({"code": "print(1)"}),
                json!({"code": "print(1)"}),
            ),
        ] {
            let handler = super::AnalysisToolHandler {
                kind,
                tools: crate::provider::tests::test_spec().tools,
                parent: tracing::Span::none(),
            };
            let call: ToolInvocation = serde_json::from_value(json!({"sessionId": "test", "toolCallId": "call", "toolName": name, "arguments": arguments})).unwrap();
            let ToolResult::Text(result) = handler.call(call).await.unwrap() else {
                panic!("expected text JSON result");
            };
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&result).unwrap(),
                expected
            );
        }
        for arguments in [
            json!({"sql": "SELECT 1", "row_limit": -1}),
            json!({"sql": "SELECT 1", "row_limit": "100"}),
            json!({}),
        ] {
            let handler = super::AnalysisToolHandler {
                kind: super::AnalysisToolKind::RunSql,
                tools: crate::provider::tests::test_spec().tools,
                parent: tracing::Span::none(),
            };
            let call: ToolInvocation = serde_json::from_value(json!({"sessionId": "test", "toolCallId": "call", "toolName": "run_sql", "arguments": arguments})).unwrap();
            assert!(handler.call(call).await.is_err());
        }
        for arguments in [
            json!({}),
            json!({"script_path": "a.py", "code": "print(1)"}),
            json!({"script_path": null}),
            json!({"code": 1}),
        ] {
            let handler = super::AnalysisToolHandler {
                kind: super::AnalysisToolKind::CodeInterpreter,
                tools: crate::provider::tests::test_spec().tools,
                parent: tracing::Span::none(),
            };
            let call: ToolInvocation = serde_json::from_value(json!({"sessionId": "test", "toolCallId": "call", "toolName": "code_interpreter", "arguments": arguments})).unwrap();
            assert!(handler.call(call).await.is_err());
        }
    }

    #[tokio::test]
    async fn scopes_real_sdk_handler_identities_to_the_actual_capability_without_duplicates() {
        use github_copilot_sdk::{tool::ToolHandler as _, types::ToolInvocation};
        let capture = crate::telemetry_support::Capture::default();
        capture
            .run(async {
                let specification = crate::provider::tests::test_spec();
                let handler = super::AnalysisToolHandler {
                    kind: super::AnalysisToolKind::Describe,
                    tools: specification.tools,
                    parent: tracing::info_span!("cycle"),
                };
                let invocation: ToolInvocation = serde_json::from_value(json!({
                    "sessionId": "private session token",
                    "toolCallId": "private \n☃ tool id",
                    "toolName": "describe",
                    "arguments": {"untrusted": "private provider JSON"}
                }))
                .unwrap();
                handler.call(invocation).await.unwrap();
            })
            .await;
        let events = capture.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        for event in events.iter() {
            assert_eq!(
                event.fields["flaggo.analysis.action.kind"],
                "evidence.describe"
            );
            assert_eq!(
                event.fields["flaggo.analysis.session.id"],
                flaggo_analysis_workspace::sha256_digest(b"private session token")
            );
            assert_eq!(
                event.fields["flaggo.analysis.tool_call.id"],
                flaggo_analysis_workspace::sha256_digest("private \n☃ tool id".as_bytes())
            );
            assert!(
                !serde_json::to_string(&event.fields)
                    .unwrap()
                    .contains("private")
            );
        }
        let spans = capture.spans.lock().unwrap();
        let action = spans
            .values()
            .find(|span| span.name == "flaggo.analysis.action")
            .unwrap();
        assert_eq!(spans[&action.parent.unwrap()].name, "cycle");
    }
}
