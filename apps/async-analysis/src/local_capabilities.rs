use std::sync::Arc;

use async_trait::async_trait;
use flaggo_analysis_agent::{AnalysisCapabilityFactory, AnalysisTools};
use flaggo_analysis_application::CapabilityFactory;
use flaggo_analysis_domain::{AnalysisContract, AnalysisError, AttemptContext};
use flaggo_analysis_execution::local::{LocalPythonExecution, PythonConfig};
use flaggo_analysis_workspace::{LocalWorkspaceProvider, WorkspaceLease};
use flaggo_evidence_store::{EvidenceAnalysisStore, EvidenceQueryLimits};

use crate::{Clock, ContractService};

pub struct LocalCapabilityFactory(CapabilityFactory);

impl LocalCapabilityFactory {
    pub fn new(
        contracts: Arc<dyn ContractService>,
        evidence: Arc<dyn EvidenceAnalysisStore>,
        limits: EvidenceQueryLimits,
        clock: Arc<dyn Clock>,
        python: PythonConfig,
        workspaces: LocalWorkspaceProvider,
    ) -> Self {
        Self(CapabilityFactory::new(
            contracts,
            evidence,
            limits,
            clock,
            Arc::new(LocalPythonExecution::new(python, workspaces)),
        ))
    }
}

#[async_trait]
impl AnalysisCapabilityFactory for LocalCapabilityFactory {
    async fn create(
        &self,
        context: AttemptContext,
        contract: AnalysisContract,
        workspace: Arc<dyn WorkspaceLease>,
    ) -> Result<Arc<dyn AnalysisTools>, AnalysisError> {
        self.0.create(context, contract, workspace).await
    }
}
