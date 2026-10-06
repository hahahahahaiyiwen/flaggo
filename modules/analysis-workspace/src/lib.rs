//! Durable analysis-workspace contracts and the local filesystem provider.
//!
//! This crate owns claims, cycle persistence, recovery, sealing, and virtual
//! filesystem policy. It does not discover contracts, query evidence, run
//! agents, or submit Candidates.

mod identity;
mod local;
mod provider;

pub use identity::{sha256_digest, workspace_id};
pub use local::LocalWorkspaceProvider;
pub use provider::{
    CyclePreparation, PrepareCycleResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFileInfo,
    WorkspaceFileSystem, WorkspaceLease, WorkspaceProvider,
};
