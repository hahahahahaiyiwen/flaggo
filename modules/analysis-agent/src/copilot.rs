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
    Client, ClientOptions,
    mode::ClientMode,
    session::Session,
    session_fs::{
        DirEntry, DirEntryKind, FileInfo, FsError, FsErrorKind, SessionFsConfig,
        SessionFsConventions, SessionFsProvider,
    },
    tool::JsonSchema,
    types::{MessageOptions, SessionConfig, SystemMessageConfig},
};
use serde::Deserialize;
use tokio::sync::Mutex;

pub mod tools;

use tools::{available_tools, copilot_tools, is_analysis_permission};

/// SDK starts describe concrete calls, but terminals describe retrying dispatches;
/// they cannot be paired into truthful concrete request actions.
pub const PROVIDER_REQUEST_OBSERVABILITY_SUPPORTED: bool = false;

pub struct CopilotAgentProvider {
    runtime: Arc<CopilotRuntime>,
    model: Option<String>,
    skill_directory: PathBuf,
    system_message: String,
    run_timeout: Duration,
}

#[derive(Clone)]
pub struct CopilotRuntimeConfig {
    pub base_directory: PathBuf,
    pub github_token: Option<String>,
}

impl CopilotAgentProvider {
    pub async fn start(
        runtime_config: CopilotRuntimeConfig,
        model: Option<String>,
        skill_directory: PathBuf,
        system_message: String,
        run_timeout: Duration,
    ) -> Result<Self, AnalysisError> {
        let runtime = Arc::new(CopilotRuntime::new(runtime_config));
        runtime.client().await?;
        Ok(Self {
            runtime,
            model,
            skill_directory,
            system_message,
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
        let tools = specification.tools;
        let filesystem: Arc<dyn SessionFsProvider> =
            Arc::new(CopilotWorkspaceFileSystem::new(specification.filesystem));
        let available_tools = available_tools()?;

        let client = self.runtime.client().await?;
        let (session, client) = match client
            .create_session(session_config(
                self.model.as_deref(),
                &self.skill_directory,
                &self.system_message,
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
                        &self.system_message,
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
    system_message: &str,
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
    config.system_message = Some(
        SystemMessageConfig::new()
            .with_mode("append")
            .with_content(system_message),
    );
    config
}

fn apply_model_selection(config: &mut SessionConfig, model: Option<&str>) {
    if let Some(model) = model {
        config.model = Some(model.to_owned());
        config.allowed_models = Some(vec![model.to_owned()]);
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
        // The coordinator-owned status is visible through the status tool and
        // gates every trusted operation. Sending another SDK message while
        // send_and_wait is active is explicitly unsafe.
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

fn fs_error(error: AnalysisError) -> FsError {
    FsError::with_message(FsErrorKind::Other, error.to_string())
}

fn fs_not_found(path: &str) -> FsError {
    FsError::with_message(
        FsErrorKind::NotFound(path.to_owned()),
        "workspace path does not exist",
    )
}

fn agent_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Agent(error.to_string())
}

#[cfg(test)]
mod tests {
    use github_copilot_sdk::types::SessionConfig;

    use super::{FinalOutcome, FinalOutcomeKind, apply_model_selection};
    use crate::AgentRunOutcome;

    #[tokio::test]
    async fn missing_workspace_files_map_to_sdk_not_found() {
        use github_copilot_sdk::session_fs::{FsErrorKind, SessionFsProvider as _};
        let filesystem =
            super::CopilotWorkspaceFileSystem::new(crate::provider::tests::test_spec().filesystem);
        let error = filesystem
            .read_file("/workspace/missing")
            .await
            .unwrap_err();
        assert!(
            matches!(error.kind(), FsErrorKind::NotFound(path) if path == "/workspace/missing")
        );
    }

    #[tokio::test]
    #[ignore = "requires the installed Copilot runtime; makes no model calls"]
    async fn native_create_edit_and_view_use_the_workspace_filesystem() {
        use super::{CopilotRuntime, CopilotRuntimeConfig, CopilotWorkspaceFileSystem};
        use flaggo_analysis_workspace::{
            LocalWorkspaceProvider, PrepareCycleResult, WorkspaceProvider as _,
        };
        use github_copilot_sdk::rpc::{ToolResult, ToolResultType, ToolsExecuteRequest};
        use serde_json::json;
        use std::sync::Arc;

        let directory = tempfile::tempdir_in(".").unwrap();
        let specification = crate::provider::tests::test_spec();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let lease = provider
            .claim(&specification.contract.name, "native-files")
            .await
            .unwrap();
        let PrepareCycleResult::Ready(preparation) = lease
            .prepare_cycle(
                &specification.contract,
                &crate::context::analysis_profile(),
                "native-files",
                chrono::Utc::now(),
            )
            .await
            .unwrap()
        else {
            panic!("expected a ready cycle");
        };
        let filesystem = lease
            .filesystem(&preparation.cycle.cycle_id, "native-files")
            .await
            .unwrap();
        let runtime = CopilotRuntime::new(CopilotRuntimeConfig {
            base_directory: directory.path().canonicalize().unwrap(),
            github_token: None,
        });
        let client = runtime.client().await.unwrap();
        let session = client
            .create_session(super::session_config(
                None,
                &crate::context::analysis_skill_directory(),
                crate::context::ANALYSIS_SYSTEM_PROMPT,
                super::available_tools().unwrap(),
                Arc::new(CopilotWorkspaceFileSystem::new(filesystem.clone())),
                specification.tools,
            ))
            .await
            .unwrap();
        let path = "/workspace/analysis/model.py";
        let created = session
            .rpc()
            .tools()
            .execute(ToolsExecuteRequest {
                name: "create".to_owned(),
                arguments: json!({"path": path, "file_text": "print(1)\n"}),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(filesystem.exists(path).await.unwrap(), "{created:?}");
        assert_eq!(
            filesystem
                .read_text(path)
                .await
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["print(1)"],
            "{created:?}"
        );
        let edited = session
            .rpc()
            .tools()
            .execute(ToolsExecuteRequest {
                name: "edit".to_owned(),
                arguments: json!({"path": path, "old_str": "print(1)", "new_str": "print(2)"}),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            filesystem
                .read_text(path)
                .await
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["print(2)"],
            "{edited:?}"
        );
        let viewed = session
            .rpc()
            .tools()
            .execute(ToolsExecuteRequest {
                name: "view".to_owned(),
                arguments: json!({"path": path}),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(format!("{viewed:?}").contains("print(2)"), "{viewed:?}");
        let contract = filesystem
            .read_text("/workspace/contract.json")
            .await
            .unwrap();
        let denied = session
            .rpc()
            .tools()
            .execute(ToolsExecuteRequest {
                name: "edit".to_owned(),
                arguments: json!({"path": "/workspace/contract.json", "old_str": contract, "new_str": "overwrite"}),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            matches!(&denied, ToolResult::ToolResultExpanded(report) if report.result_type == ToolResultType::Failure),
            "native editing must not overwrite cycle control records: {denied:?}",
        );
        assert_eq!(
            filesystem
                .read_text("/workspace/contract.json")
                .await
                .unwrap(),
            contract
        );
        let missing_parent = session.rpc().tools().execute(ToolsExecuteRequest {
            name: "create".to_owned(),
            arguments: json!({"path": "/workspace/analysis/scripts/nested/model.py", "file_text": "print(1)\n"}),
            ..Default::default()
        }).await.unwrap();
        assert!(
            matches!(&missing_parent, ToolResult::ToolResultExpanded(report) if report.result_type == ToolResultType::Failure),
            "{missing_parent:?}"
        );
        filesystem
            .create_directory("/workspace/analysis/scripts/nested", true)
            .await
            .unwrap();
        let nested = session.rpc().tools().execute(ToolsExecuteRequest {
            name: "create".to_owned(),
            arguments: json!({"path": "/workspace/analysis/scripts/nested/model.py", "file_text": "print(3)\n"}),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(
            filesystem
                .read_text("/workspace/analysis/scripts/nested/model.py")
                .await
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["print(3)"],
            "{nested:?}"
        );
        session.disconnect().await.unwrap();
        runtime.stop().await.unwrap();
    }

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
}
