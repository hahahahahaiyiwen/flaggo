//! Copilot SDK adapter for the analysis-agent boundary.
//!
//! This module translates workspace and analysis-tool contracts into an
//! empty-mode Copilot session. It does not own analysis scheduling or durable
//! workspace semantics.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::AgentRunOutcome;
use crate::{AgentProvider, AgentSession, AgentSessionSpec, AnalysisTask, AnalysisTools};
use async_trait::async_trait;
use flaggo_analysis_domain::AnalysisError;
use flaggo_analysis_workspace::{WorkspaceEntryKind, WorkspaceFileSystem};
use github_copilot_sdk::{
    Client, ClientOptions, Error as CopilotError, ErrorKind,
    mode::{ClientMode, ToolSet},
    session::Session,
    session_fs::{
        DirEntry, DirEntryKind, FileInfo, FsError, FsErrorKind, SessionFsConfig,
        SessionFsConventions, SessionFsProvider,
    },
    tool::{JsonSchema, ToolHandler},
    types::{
        MessageOptions, PermissionRequestData, PermissionRequestKind, SessionConfig,
        SystemMessageConfig, Tool, ToolInvocation, ToolResult,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

const CHECK_CURRENT_TOOL: &str = "check_current_contract";
const COMMIT_CUTOFF_TOOL: &str = "commit_evidence_cutoff";
const DESCRIBE_EVIDENCE_TOOL: &str = "describe_evidence";
const QUERY_EVIDENCE_TOOL: &str = "query_evidence";
const PROPOSE_EXECUTABLE_TOOL: &str = "propose_executable";
const SESSION_EVENT_MAX_ARRAY_ITEMS: usize = 32;
const SESSION_EVENT_MAX_BYTES: usize = 16 * 1024;
const SESSION_EVENT_MAX_STRING_BYTES: usize = 4 * 1024;
const SESSION_EVENT_PREVIEW_BYTES: usize = 8 * 1024;
pub const ANALYSIS_SKILL_NAME: &str = "evidence-analysis";
pub const ANALYSIS_SKILL_VERSION: &str = "3";

pub struct CopilotAgentProvider {
    runtime: Arc<CopilotRuntime>,
    model: Option<String>,
    skill_directory: PathBuf,
    run_timeout: Duration,
}

#[derive(Clone)]
pub struct CopilotRuntimeConfig {
    pub base_directory: PathBuf,
    pub github_token: Option<String>,
    pub log_session_events: bool,
}

impl CopilotAgentProvider {
    pub async fn start(
        runtime_config: CopilotRuntimeConfig,
        model: Option<String>,
        skill_directory: PathBuf,
        run_timeout: Duration,
    ) -> Result<Self, AnalysisError> {
        let runtime = Arc::new(CopilotRuntime::new(runtime_config));
        runtime.client().await?;
        Ok(Self {
            runtime,
            model,
            skill_directory,
            run_timeout,
        })
    }

    pub async fn stop(&self) -> Result<(), AnalysisError> {
        self.runtime.stop().await
    }
}

#[async_trait]
impl AgentProvider for CopilotAgentProvider {
    async fn start_session(
        &self,
        specification: AgentSessionSpec,
    ) -> Result<Arc<dyn AgentSession>, AnalysisError> {
        let context = specification.context.clone();
        let tools = specification.tools;
        let filesystem: Arc<dyn SessionFsProvider> =
            Arc::new(CopilotWorkspaceFileSystem::new(specification.filesystem));
        let available_tools = ToolSet::new()
            .add_builtin("view")
            .and_then(|set| set.add_builtin("edit"))
            .and_then(|set| set.add_builtin("apply_patch"))
            .and_then(|set| set.add_builtin("rg"))
            .and_then(|set| set.add_builtin("glob"))
            .and_then(|set| set.add_builtin("skill"))
            .and_then(|set| set.add_custom(CHECK_CURRENT_TOOL))
            .and_then(|set| set.add_custom(COMMIT_CUTOFF_TOOL))
            .and_then(|set| set.add_custom(DESCRIBE_EVIDENCE_TOOL))
            .and_then(|set| set.add_custom(QUERY_EVIDENCE_TOOL))
            .and_then(|set| set.add_custom(PROPOSE_EXECUTABLE_TOOL))
            .map_err(agent_error)?
            .into_vec();

        let client = self.runtime.client().await?;
        let (session, client) = match client
            .create_session(session_config(
                self.model.as_deref(),
                &self.skill_directory,
                available_tools.clone(),
                Arc::clone(&filesystem),
                Arc::clone(&tools),
            ))
            .await
        {
            Ok(session) => (session, client),
            Err(error) if error.is_transport_failure() => {
                self.runtime.invalidate(&client).await;
                let replacement = self.runtime.client().await?;
                let session = replacement
                    .create_session(session_config(
                        self.model.as_deref(),
                        &self.skill_directory,
                        available_tools,
                        filesystem,
                        tools,
                    ))
                    .await
                    .map_err(agent_error)?;
                (session, replacement)
            }
            Err(error) => return Err(agent_error(error)),
        };
        if self.runtime.config.log_session_events {
            log_session_events(&session, context, self.runtime.config.github_token.clone());
        }
        Ok(Arc::new(CopilotAgentSession {
            session,
            client,
            runtime: Arc::clone(&self.runtime),
            run_timeout: self.run_timeout,
            started: AtomicBool::new(false),
        }))
    }
}

struct CopilotAgentSession {
    session: Session,
    client: Arc<Client>,
    runtime: Arc<CopilotRuntime>,
    run_timeout: Duration,
    started: AtomicBool,
}

struct CopilotRuntime {
    config: CopilotRuntimeConfig,
    client: Mutex<Option<Arc<Client>>>,
}

impl CopilotRuntime {
    fn new(config: CopilotRuntimeConfig) -> Self {
        Self {
            config,
            client: Mutex::new(None),
        }
    }

    async fn client(&self) -> Result<Arc<Client>, AnalysisError> {
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_ref() {
            return Ok(Arc::clone(client));
        }
        let options = ClientOptions::default()
            .with_mode(ClientMode::Empty)
            .with_base_directory(&self.config.base_directory)
            .with_session_fs(SessionFsConfig::new(
                "/workspace",
                "/workspace/analysis/.copilot",
                SessionFsConventions::Posix,
            ));
        let options = match &self.config.github_token {
            Some(token) => options
                .with_github_token(token.clone())
                .with_use_logged_in_user(false),
            None => options.with_use_logged_in_user(true),
        };
        let started = Arc::new(Client::start(options).await.map_err(agent_error)?);
        *client = Some(Arc::clone(&started));
        Ok(started)
    }

    async fn invalidate(&self, failed: &Arc<Client>) {
        let removed = {
            let mut client = self.client.lock().await;
            if client
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, failed))
            {
                client.take()
            } else {
                None
            }
        };
        if let Some(client) = removed {
            let _ = client.stop().await;
        }
    }

    async fn stop(&self) -> Result<(), AnalysisError> {
        let client = self.client.lock().await.take();
        if let Some(client) = client {
            client.stop().await.map_err(agent_error)?;
        }
        Ok(())
    }
}

fn session_config(
    model: Option<&str>,
    skill_directory: &std::path::Path,
    available_tools: Vec<String>,
    filesystem: Arc<dyn SessionFsProvider>,
    tools: Arc<dyn AnalysisTools>,
) -> SessionConfig {
    let mut config = SessionConfig::default()
        .approve_permissions_if(is_analysis_permission)
        .with_session_fs_provider(filesystem)
        .with_tools(copilot_tools(tools));
    apply_model_selection(&mut config, model);
    config.client_name = Some("flaggo-async-analysis".to_owned());
    config.available_tools = Some(available_tools);
    config.enable_skills = Some(true);
    config.skill_directories = Some(vec![skill_directory.to_path_buf()]);
    config.enable_config_discovery = Some(false);
    config.enable_on_demand_instruction_discovery = Some(false);
    config.enable_file_hooks = Some(false);
    config.enable_host_git_operations = Some(false);
    config.enable_session_store = Some(false);
    config.streaming = Some(false);
    config.system_message = Some(SystemMessageConfig::new().with_mode("append").with_content(
        "You are a bounded Flaggo evidence-analysis worker. Use only the declared \
                 workspace, skill, and typed tools. Never infer or alter trusted identity. \
                 Persist analysis and checkpoints before returning a terminal outcome.",
    ));
    config
}

fn is_analysis_permission(request: &PermissionRequestData) -> bool {
    match request.kind {
        Some(PermissionRequestKind::Read | PermissionRequestKind::Write) => true,
        Some(PermissionRequestKind::CustomTool) => request
            .extra
            .pointer("/permissionRequest/toolName")
            .or_else(|| request.extra.get("toolName"))
            .and_then(Value::as_str)
            .is_some_and(|name| {
                matches!(
                    name,
                    CHECK_CURRENT_TOOL
                        | COMMIT_CUTOFF_TOOL
                        | DESCRIBE_EVIDENCE_TOOL
                        | QUERY_EVIDENCE_TOOL
                        | PROPOSE_EXECUTABLE_TOOL
                )
            }),
        _ => false,
    }
}

fn apply_model_selection(config: &mut SessionConfig, model: Option<&str>) {
    if let Some(model) = model {
        config.model = Some(model.to_owned());
        config.allowed_models = Some(vec![model.to_owned()]);
    }
}

fn log_session_events(
    session: &Session,
    context: flaggo_analysis_domain::AttemptContext,
    github_token: Option<String>,
) {
    let session_id = session.id().to_string();
    let mut events = session.subscribe();
    drop(tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let event = serde_json::to_value(event).unwrap_or_else(|error| {
                json!({
                    "serializationError": error.to_string()
                })
            });
            let event = sanitize_session_event(event, github_token.as_deref());
            println!(
                "{}",
                json!({
                    "attemptId": context.attempt_id,
                    "contractDigest": context.contract.digest,
                    "contractName": context.contract.name,
                    "cycleId": context.cycle_id,
                    "event": "async_analysis.agent_session_event",
                    "sessionEvent": event,
                    "sessionId": session_id,
                    "workspaceId": context.workspace_id
                })
            );
        }
    }));
}

fn sanitize_session_event(mut event: Value, secret: Option<&str>) -> Value {
    redact_sensitive_values(&mut event, secret);
    let original_bytes = event.to_string().len();
    let mut truncated_values = 0;
    truncate_session_event_value(&mut event, &mut truncated_values);
    if truncated_values == 0 && original_bytes <= SESSION_EVENT_MAX_BYTES {
        return event;
    }

    let truncation = json!({
        "limitBytes": SESSION_EVENT_MAX_BYTES,
        "originalBytes": original_bytes,
        "truncatedValues": truncated_values
    });
    if let Value::Object(fields) = &mut event {
        fields.insert("flaggoTruncation".to_owned(), truncation.clone());
    }
    if event.to_string().len() <= SESSION_EVENT_MAX_BYTES {
        return event;
    }

    let data_preview = event.get("data").map(Value::to_string).unwrap_or_default();
    json!({
        "type": bounded_event_field(&event, "type"),
        "id": bounded_event_field(&event, "id"),
        "parentId": bounded_event_field(&event, "parentId"),
        "timestamp": bounded_event_field(&event, "timestamp"),
        "dataPreview": truncate_text(&data_preview, SESSION_EVENT_PREVIEW_BYTES),
        "flaggoTruncation": truncation
    })
}

fn truncate_session_event_value(value: &mut Value, truncated_values: &mut usize) {
    match value {
        Value::Array(values) => {
            if values.len() > SESSION_EVENT_MAX_ARRAY_ITEMS {
                *truncated_values += values.len() - SESSION_EVENT_MAX_ARRAY_ITEMS;
                values.truncate(SESSION_EVENT_MAX_ARRAY_ITEMS);
            }
            for value in values {
                truncate_session_event_value(value, truncated_values);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                truncate_session_event_value(value, truncated_values);
            }
        }
        Value::String(text) if text.len() > SESSION_EVENT_MAX_STRING_BYTES => {
            *text = truncate_text(text, SESSION_EVENT_MAX_STRING_BYTES);
            *truncated_values += 1;
        }
        _ => {}
    }
}

fn bounded_event_field(event: &Value, name: &str) -> Option<String> {
    event
        .get(name)
        .and_then(Value::as_str)
        .map(|text| truncate_text(text, 256))
}

fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let suffix = format!("... [truncated from {} bytes]", text.len());
    let mut end = max_bytes.saturating_sub(suffix.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], suffix)
}

fn redact_sensitive_values(value: &mut Value, secret: Option<&str>) {
    match value {
        Value::Array(values) => {
            for value in values {
                redact_sensitive_values(value, secret);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let key = key.to_ascii_lowercase();
                if key.contains("authorization")
                    || key.contains("credential")
                    || key.contains("secret")
                    || key.contains("token")
                {
                    *value = Value::String("[redacted]".to_owned());
                } else {
                    redact_sensitive_values(value, secret);
                }
            }
        }
        Value::String(text)
            if secret.is_some_and(|secret| !secret.is_empty() && text.contains(secret)) =>
        {
            *text = "[redacted]".to_owned();
        }
        _ => {}
    }
}

#[async_trait]
impl AgentSession for CopilotAgentSession {
    async fn run(&self, task: AnalysisTask) -> Result<AgentRunOutcome, AnalysisError> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(AnalysisError::Agent(
                "Copilot session may run only one analysis task".to_owned(),
            ));
        }
        let outcome = self
            .session
            .send_and_wait_typed::<FinalOutcome>(
                MessageOptions::new(task.prompt).with_wait_timeout(self.run_timeout),
            )
            .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                if error.is_transport_failure() {
                    self.runtime.invalidate(&self.client).await;
                }
                return Err(agent_error(error));
            }
        };
        outcome.into_domain()
    }

    async fn request_yield(&self, _reason: &str) -> Result<(), AnalysisError> {
        // The shared YieldSignal is visible to every trusted tool. Sending another
        // SDK message while send_and_wait is active is explicitly unsafe.
        Ok(())
    }

    async fn close(&self) -> Result<(), AnalysisError> {
        let abort_error = self.session.abort().await.err();
        let disconnect_result = self.session.disconnect().await;
        match (disconnect_result, abort_error) {
            (Ok(()), _) => Ok(()),
            (Err(error), _) => Err(agent_error(error)),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinalOutcome {
    outcome: FinalOutcomeKind,
    explanation: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum FinalOutcomeKind {
    Candidate,
    NoChange,
    NoCandidate,
    Failure,
    Handoff,
    Superseded,
}

impl FinalOutcome {
    fn into_domain(self) -> Result<AgentRunOutcome, AnalysisError> {
        if self.explanation.trim().is_empty() {
            return Err(AnalysisError::Agent(
                "agent outcome explanation cannot be empty".to_owned(),
            ));
        }
        match self.outcome {
            FinalOutcomeKind::Candidate => Ok(AgentRunOutcome::Candidate),
            FinalOutcomeKind::NoChange => Ok(AgentRunOutcome::NoChange {
                reason: self.explanation,
            }),
            FinalOutcomeKind::NoCandidate => Ok(AgentRunOutcome::NoCandidate {
                reason: self.explanation,
            }),
            FinalOutcomeKind::Failure => Ok(AgentRunOutcome::Failure {
                reason: self.explanation,
            }),
            FinalOutcomeKind::Handoff => Ok(AgentRunOutcome::Handoff {
                summary: self.explanation,
            }),
            FinalOutcomeKind::Superseded => Ok(AgentRunOutcome::Superseded),
        }
    }
}

struct CopilotWorkspaceFileSystem {
    inner: Arc<dyn WorkspaceFileSystem>,
}

impl CopilotWorkspaceFileSystem {
    fn new(inner: Arc<dyn WorkspaceFileSystem>) -> Self {
        Self { inner }
    }

    async fn require_exists(&self, path: &str) -> Result<(), FsError> {
        if self.inner.exists(path).await.map_err(fs_error)? {
            Ok(())
        } else {
            Err(fs_not_found(path))
        }
    }
}

#[async_trait]
impl SessionFsProvider for CopilotWorkspaceFileSystem {
    async fn read_file(&self, path: &str) -> Result<String, FsError> {
        self.require_exists(path).await?;
        self.inner.read_text(path).await.map_err(fs_error)
    }

    async fn write_file(
        &self,
        path: &str,
        content: &str,
        _mode: Option<i64>,
    ) -> Result<(), FsError> {
        self.inner.write_text(path, content).await.map_err(fs_error)
    }

    async fn append_file(
        &self,
        path: &str,
        content: &str,
        _mode: Option<i64>,
    ) -> Result<(), FsError> {
        self.inner
            .append_text(path, content)
            .await
            .map_err(fs_error)
    }

    async fn exists(&self, path: &str) -> Result<bool, FsError> {
        self.inner.exists(path).await.map_err(fs_error)
    }

    async fn stat(&self, path: &str) -> Result<FileInfo, FsError> {
        self.require_exists(path).await?;
        let metadata = self.inner.metadata(path).await.map_err(fs_error)?;
        let size = i64::try_from(metadata.size)
            .map_err(|_| FsError::with_message(FsErrorKind::Other, "file size exceeds i64"))?;
        let timestamp = metadata.modified_at.to_rfc3339();
        Ok(FileInfo::new(
            metadata.is_file,
            metadata.is_directory,
            size,
            timestamp.clone(),
            timestamp,
        ))
    }

    async fn mkdir(&self, path: &str, recursive: bool, _mode: Option<i64>) -> Result<(), FsError> {
        self.inner
            .create_directory(path, recursive)
            .await
            .map_err(fs_error)
    }

    async fn readdir(&self, path: &str) -> Result<Vec<String>, FsError> {
        self.require_exists(path).await?;
        self.inner
            .read_directory(path)
            .await
            .map(|entries| entries.into_iter().map(|entry| entry.name).collect())
            .map_err(fs_error)
    }

    async fn readdir_with_types(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        self.require_exists(path).await?;
        self.inner
            .read_directory(path)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| {
                        let kind = match entry.kind {
                            WorkspaceEntryKind::File => DirEntryKind::File,
                            WorkspaceEntryKind::Directory => DirEntryKind::Directory,
                        };
                        DirEntry::new(entry.name, kind)
                    })
                    .collect()
            })
            .map_err(fs_error)
    }

    async fn rm(&self, path: &str, recursive: bool, force: bool) -> Result<(), FsError> {
        self.inner
            .remove(path, recursive, force)
            .await
            .map_err(fs_error)
    }

    async fn rename(&self, source: &str, destination: &str) -> Result<(), FsError> {
        self.inner
            .rename(source, destination)
            .await
            .map_err(fs_error)
    }
}

#[derive(Clone, Copy)]
enum AnalysisToolKind {
    CheckCurrent,
    CommitCutoff,
    DescribeEvidence,
    QueryEvidence,
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
            AnalysisToolKind::CheckCurrent => serde_json::to_value(
                self.tools
                    .check_current_contract()
                    .await
                    .map_err(tool_error)?,
            )?,
            AnalysisToolKind::CommitCutoff => {
                let cutoff = invocation
                    .arguments
                    .get("cutoff")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            "commit_evidence_cutoff requires an RFC 3339 cutoff",
                        )
                    })?
                    .parse()
                    .map_err(|error| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            format!("invalid RFC 3339 cutoff: {error}"),
                        )
                    })?;
                serde_json::to_value(
                    self.tools
                        .commit_evidence_cutoff(cutoff)
                        .await
                        .map_err(tool_error)?,
                )?
            }
            AnalysisToolKind::DescribeEvidence => {
                self.tools.describe_evidence().await.map_err(tool_error)?
            }
            AnalysisToolKind::QueryEvidence => {
                let sql = invocation
                    .arguments
                    .get("sql")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CopilotError::with_message(
                            ErrorKind::InvalidConfig,
                            "query_evidence requires SQL text",
                        )
                    })?;
                self.tools.query_evidence(sql).await.map_err(tool_error)?
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

fn copilot_tools(tools: Arc<dyn AnalysisTools>) -> Vec<Tool> {
    vec![
        tool(
            CHECK_CURRENT_TOOL,
            "Check whether the cycle's exact contract digest is still current.",
            json!({"type": "object", "additionalProperties": false}),
            AnalysisToolKind::CheckCurrent,
            Arc::clone(&tools),
        ),
        tool(
            COMMIT_CUTOFF_TOOL,
            "Commit the cycle's immutable evidence cutoff and storage watermark.",
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
            DESCRIBE_EVIDENCE_TOOL,
            "Describe the read-only observations relation and trusted evidence scope.",
            json!({"type": "object", "additionalProperties": false}),
            AnalysisToolKind::DescribeEvidence,
            Arc::clone(&tools),
        ),
        tool(
            QUERY_EVIDENCE_TOOL,
            "Run one bounded read-only SQL query over the scoped observations relation.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["sql"],
                "properties": {
                    "sql": {"type": "string"}
                }
            }),
            AnalysisToolKind::QueryEvidence,
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

fn fs_error(error: AnalysisError) -> FsError {
    FsError::with_message(FsErrorKind::Other, error.to_string())
}

fn fs_not_found(path: &str) -> FsError {
    FsError::with_message(
        FsErrorKind::NotFound(path.to_owned()),
        "workspace path does not exist",
    )
}

fn tool_error(error: AnalysisError) -> CopilotError {
    CopilotError::with_message(ErrorKind::InvalidConfig, error.to_string())
}

fn agent_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Agent(error.to_string())
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::{PermissionRequestData, PermissionRequestKind, SessionConfig};
    use serde_json::json;

    use super::{
        CHECK_CURRENT_TOOL, FinalOutcome, FinalOutcomeKind, SESSION_EVENT_MAX_BYTES,
        apply_model_selection, is_analysis_permission, redact_sensitive_values,
        sanitize_session_event,
    };
    use crate::AgentRunOutcome;

    #[test]
    fn maps_strict_terminal_outcomes() {
        assert_eq!(
            FinalOutcome {
                outcome: FinalOutcomeKind::NoChange,
                explanation: "current executable remains appropriate".to_owned(),
            }
            .into_domain()
            .expect("valid outcome"),
            AgentRunOutcome::NoChange {
                reason: "current executable remains appropriate".to_owned()
            }
        );
        assert_eq!(
            FinalOutcome {
                outcome: FinalOutcomeKind::NoCandidate,
                explanation: "not enough data".to_owned(),
            }
            .into_domain()
            .expect("valid outcome"),
            AgentRunOutcome::NoCandidate {
                reason: "not enough data".to_owned()
            }
        );
        assert!(
            FinalOutcome {
                outcome: FinalOutcomeKind::Handoff,
                explanation: " ".to_owned(),
            }
            .into_domain()
            .is_err()
        );
    }

    #[test]
    fn terminal_outcome_schema_avoids_unsupported_unions() {
        let schema = serde_json::to_value(schemars::schema_for!(FinalOutcome))
            .expect("serialize terminal outcome schema");
        let schema = schema.to_string();
        assert!(!schema.contains("\"oneOf\""));
        assert!(!schema.contains("\"anyOf\""));
        assert!(!schema.contains("\"allOf\""));
        assert!(schema.contains("\"additionalProperties\":false"));
        assert!(schema.contains("\"required\":[\"outcome\",\"explanation\"]"));
    }

    #[test]
    fn leaves_model_selection_to_sdk_when_unconfigured() {
        let mut default = SessionConfig::default();
        apply_model_selection(&mut default, None);
        assert_eq!(default.model, None);
        assert_eq!(default.allowed_models, None);

        let mut pinned = SessionConfig::default();
        apply_model_selection(&mut pinned, Some("gpt-test"));
        assert_eq!(pinned.model.as_deref(), Some("gpt-test"));
        assert_eq!(pinned.allowed_models, Some(vec!["gpt-test".to_owned()]));
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
                    "toolName": CHECK_CURRENT_TOOL
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
    fn redacts_sensitive_session_event_fields() {
        let mut event = json!({
            "data": {
                "githubToken": "secret",
                "nested": {"authorization": "Bearer secret"},
                "content": "analysis result"
            }
        });
        redact_sensitive_values(&mut event, Some("gho-secret"));
        assert_eq!(event["data"]["githubToken"], "[redacted]");
        assert_eq!(event["data"]["nested"]["authorization"], "[redacted]");
        assert_eq!(event["data"]["content"], "analysis result");

        let mut literal = json!({"message": "request failed for gho-secret"});
        redact_sensitive_values(&mut literal, Some("gho-secret"));
        assert_eq!(literal["message"], "[redacted]");
    }

    #[test]
    fn bounds_large_session_events_with_diagnostic_context() {
        let event = json!({
            "type": "system.message",
            "id": "event-1",
            "timestamp": "2026-10-05T21:00:00Z",
            "data": {
                "content": "x".repeat(50_000),
                "contentBlocks": (0..100)
                    .map(|index| json!({"index": index, "content": "y".repeat(1_000)}))
                    .collect::<Vec<_>>()
            }
        });

        let bounded = sanitize_session_event(event, None);
        assert!(bounded.to_string().len() <= SESSION_EVENT_MAX_BYTES);
        assert_eq!(bounded["type"], "system.message");
        assert_eq!(bounded["id"], "event-1");
        assert!(
            bounded["flaggoTruncation"]["originalBytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > SESSION_EVENT_MAX_BYTES as u64)
        );
    }
}
