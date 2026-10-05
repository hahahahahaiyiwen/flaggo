//! Shared Async Analysis value types with no provider or storage dependencies.

mod error;
mod model;

pub use error::AnalysisError;
pub use model::{
    AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisCycle,
    AnalysisEvidenceSource, AnalysisProfile, AttemptContext, CandidateProposal, CandidateRecord,
    CatalogUpdate, CycleOutcome, EvidenceCutoff, ScheduleError,
};
