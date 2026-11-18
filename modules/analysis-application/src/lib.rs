//! Shared analysis orchestration and capability composition, without local storage or execution.
pub mod capabilities;
pub mod contract_client;
pub mod coordinator;
pub mod observability;

pub use capabilities::CapabilityFactory;
pub use coordinator::{
    AnalysisCoordinator, Clock, ContractService, CoordinatorConfig, CoordinatorRunOutcome,
    SystemClock,
};

#[cfg(test)]
#[path = "../../analysis-agent/tests/support/mod.rs"]
mod telemetry_support;
