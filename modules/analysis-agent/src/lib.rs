//! Provider-neutral analysis-agent contracts and bounded session pooling.
//!
//! This crate owns agent capacity and session lifecycle contracts. It does not
//! schedule analysis, persist workspaces, or select a concrete model SDK.

#[cfg(feature = "copilot")]
pub mod copilot;

mod capability;
mod provider;

pub use capability::{AnalysisCapabilityFactory, AnalysisRunStatus, AnalysisTools, YieldSignal};
pub use provider::{
    AgentLease, AgentPool, AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec,
    AnalysisTask,
};
