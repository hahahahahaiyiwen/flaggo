//! Durable analysis-workspace contracts and the local filesystem provider.
//!
//! This crate owns claims, cycle persistence, recovery, sealing, and virtual
//! filesystem policy. It does not discover contracts, query evidence, run
//! agents, or submit Candidates.

mod identity;
#[cfg(feature = "local")]
mod local;
pub mod policy;
mod provider;

pub use identity::{sha256_digest, workspace_id};
#[cfg(feature = "local")]
pub use local::LocalWorkspaceProvider;
pub use provider::{
    CyclePreparation, EXECUTION_CAPABILITY, ExecutionReceipt, ExecutionRequest,
    ExecutionReservation, ExecutionState, HandoffPublication, PrepareCycleResult,
    WORKSPACE_CAPABILITY, WorkspaceArtifact, WorkspaceClaim, WorkspaceEntry, WorkspaceEntryKind,
    WorkspaceFileInfo, WorkspaceFileSystem, WorkspaceLease, WorkspaceProvider,
};
