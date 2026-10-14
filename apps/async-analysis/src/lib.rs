pub mod config;
pub mod contract_client;
pub mod coordinator;
pub mod local_capabilities;
pub mod observability;
pub mod python;
pub mod service;

pub use coordinator::{
    AnalysisCoordinator, Clock, ContractService, CoordinatorConfig, CoordinatorRunOutcome,
    SystemClock,
};
#[cfg(test)]
#[path = "../../../modules/analysis-agent/tests/support/mod.rs"]
mod telemetry_support;
