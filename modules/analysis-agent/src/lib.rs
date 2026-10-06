//! Provider-neutral analysis-agent contracts, context, and bounded sessions.
//!
//! This crate owns the static analysis-agent context package, agent capacity,
//! and session lifecycle contracts. It does not schedule analysis, persist
//! cycle state, or select a concrete model SDK.

pub mod context;
#[cfg(feature = "copilot")]
pub mod copilot;

mod capability;
mod provider;

pub use capability::{AnalysisCapabilityFactory, AnalysisRunStatus, AnalysisTools, YieldSignal};
pub use provider::{
    AgentLease, AgentPool, AgentProvider, AgentRunOutcome, AgentSession, AgentSessionSpec,
    AnalysisTask,
};
