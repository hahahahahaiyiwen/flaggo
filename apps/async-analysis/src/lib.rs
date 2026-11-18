pub mod config;
pub use flaggo_analysis_application::{contract_client, coordinator, observability};
pub mod local_capabilities;
pub mod python;
pub mod service;

pub use coordinator::{
    AnalysisCoordinator, Clock, ContractService, CoordinatorConfig, CoordinatorRunOutcome,
    SystemClock,
};
