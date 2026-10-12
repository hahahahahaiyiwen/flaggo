use std::sync::Arc;

use async_trait::async_trait;
use flaggo_analysis_domain::{AnalysisError, AttemptContext};
use flaggo_analysis_workspace::{WorkspaceEntry, WorkspaceFileInfo, WorkspaceFileSystem};
use tracing::{Instrument as _, Span};

use crate::action::{ActionKind, ActionObservation};

/// Observes agent-facing content access only, not preparation or durable internals.
pub struct ObservedWorkspaceFileSystem {
    inner: Arc<dyn WorkspaceFileSystem>,
    context: AttemptContext,
    parent: Span,
}

impl ObservedWorkspaceFileSystem {
    pub fn new(inner: Arc<dyn WorkspaceFileSystem>, context: AttemptContext) -> Self {
        Self {
            inner,
            context,
            parent: Span::current(),
        }
    }
}

#[async_trait]
impl WorkspaceFileSystem for ObservedWorkspaceFileSystem {
    async fn read_text(&self, path: &str) -> Result<String, AnalysisError> {
        if internal_path(path) {
            return self.inner.read_text(path).await;
        }
        let mut action =
            ActionObservation::start(&self.context, ActionKind::ArtifactRead, &self.parent);
        let result = self.inner.read_text(path).instrument(action.span()).await;
        if let Ok(content) = &result {
            action.artifact(content);
        }
        action.finish_result(&result);
        result
    }

    async fn write_text(&self, path: &str, content: &str) -> Result<(), AnalysisError> {
        if internal_path(path) {
            return self.inner.write_text(path, content).await;
        }
        let mut action =
            ActionObservation::start(&self.context, ActionKind::ArtifactWrite, &self.parent);
        let result = self
            .inner
            .write_text(path, content)
            .instrument(action.span())
            .await;
        if result.is_ok() {
            action.artifact(content);
        }
        action.finish_result(&result);
        result
    }

    async fn append_text(&self, path: &str, content: &str) -> Result<(), AnalysisError> {
        if internal_path(path) {
            return self.inner.append_text(path, content).await;
        }
        let mut action =
            ActionObservation::start(&self.context, ActionKind::ArtifactAppend, &self.parent);
        let result = self
            .inner
            .append_text(path, content)
            .instrument(action.span())
            .await;
        if result.is_ok() {
            // Digest describes the actual appended fragment, not an unobserved final file.
            action.artifact(content);
        }

        action.finish_result(&result);
        result
    }

    async fn exists(&self, path: &str) -> Result<bool, AnalysisError> {
        self.inner.exists(path).await
    }
    async fn metadata(&self, path: &str) -> Result<WorkspaceFileInfo, AnalysisError> {
        self.inner.metadata(path).await
    }
    async fn create_directory(&self, path: &str, recursive: bool) -> Result<(), AnalysisError> {
        self.inner.create_directory(path, recursive).await
    }
    async fn read_directory(&self, path: &str) -> Result<Vec<WorkspaceEntry>, AnalysisError> {
        self.inner.read_directory(path).await
    }
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), AnalysisError> {
        self.inner.remove(path, recursive, force).await
    }
    async fn rename(&self, source: &str, destination: &str) -> Result<(), AnalysisError> {
        self.inner.rename(source, destination).await
    }
}

fn internal_path(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|segment| matches!(segment, ".copilot" | ".git"))
}
