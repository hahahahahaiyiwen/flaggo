//! Path-free execution capability with explicit durable reconciliation.
use std::sync::Arc;

use async_trait::async_trait;
use flaggo_analysis_agent::YieldSignal;
use flaggo_analysis_domain::AnalysisError;
use flaggo_analysis_workspace::{ExecutionReceipt, ExecutionRequest, WorkspaceLease};

#[cfg(feature = "local")]
pub mod local;

#[async_trait]
pub trait ExecutionProvider: Send + Sync {
    fn profile(&self) -> &str;

    fn timeout_ms(&self) -> Result<u64, AnalysisError>;

    fn output_limit_bytes(&self) -> Result<u64, AnalysisError>;

    /// Existing pending requests remain uncertain; they are never automatically rerun.
    async fn execute(
        &self,
        workspace: Arc<dyn WorkspaceLease>,
        request: &ExecutionRequest,
        signal: &YieldSignal,
    ) -> Result<ExecutionReceipt, AnalysisError>;

    async fn reconcile(
        &self,
        workspace: Arc<dyn WorkspaceLease>,
        cycle_id: &str,
        execution_id: &str,
    ) -> Result<Option<ExecutionReceipt>, AnalysisError> {
        workspace.execution_receipt(cycle_id, execution_id).await
    }
}
