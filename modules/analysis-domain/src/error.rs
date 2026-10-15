use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("contract boundary failed: {0}")]
    Contract(String),
    #[error("workspace '{0}' is already claimed")]
    WorkspaceBusy(String),
    #[error("workspace failed: {0}")]
    Workspace(String),
    #[error("agent provider failed: {0}")]
    Agent(String),
    #[error("evidence capability failed: {0}")]
    Evidence(String),
    #[error("candidate capability failed: {0}")]
    Candidate(String),
    #[error("analysis state is invalid: {0}")]
    InvalidState(String),
    #[error("cycle execution limit exhausted: {0}")]
    LimitExceeded(String),
    #[error("Candidate proposal conflicts with the sealed cycle submission: {0}")]
    ProposalConflict(String),
    #[error("analysis contract is superseded: {0}")]
    Superseded(String),
}

impl AnalysisError {
    pub fn workspace(message: impl Into<String>) -> Self {
        Self::Workspace(message.into())
    }
}
