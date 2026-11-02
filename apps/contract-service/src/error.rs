use axum::http::StatusCode;
use flaggo_contract::ContractError;
use flaggo_contract_store::ContractStoreError;
use flaggo_executable_store::ExecutableStoreError;
use flaggo_expression::{ExecutableCompilationError, ExecutableMaterializationError};
use flaggo_service_hosting::{HttpJsonError, Problem};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("The request could not be processed.")]
    InvalidQuery,
    #[error("The request path or query parameters are invalid.")]
    InvalidArgument,
    #[error("The route contractName must equal the DecisionContract name.")]
    NameMismatch,
    #[error("The decision contract failed semantic validation.")]
    InvalidContract,
    #[error("The requested DecisionContract resource does not exist.")]
    NotFound,
    #[error("Contract '{name}' current digest is '{}', not '{requested}'.", .current.as_deref().unwrap_or("<none>"))]
    Stale {
        name: String,
        requested: String,
        current: Option<String>,
    },
    #[error("{0}")]
    Integrity(String),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    ContractStore(#[from] ContractStoreError),
    #[error(transparent)]
    ExecutableStore(#[from] ExecutableStoreError),
    #[error(transparent)]
    Compilation(#[from] ExecutableCompilationError),
    #[error(transparent)]
    Materialization(#[from] ExecutableMaterializationError),
    #[error(transparent)]
    Body(#[from] HttpJsonError),
}

impl ServiceError {
    pub fn classification(&self) -> (&'static str, &'static str) {
        use ContractStoreError as C;
        use ExecutableStoreError as E;
        match self {
            Self::Body(
                HttpJsonError::Invalid(_) | HttpJsonError::Native(ContractError::Serialization(_)),
            )
            | Self::InvalidQuery
            | Self::InvalidArgument
            | Self::NameMismatch
            | Self::InvalidContract
            | Self::Compilation(
                ExecutableCompilationError::Validation(_)
                | ExecutableCompilationError::Expression(_)
                | ExecutableCompilationError::Contract(ContractError::Serialization(_)),
            )
            | Self::Contract(ContractError::Serialization(_))
            | Self::ContractStore(C::Contract(ContractError::Serialization(_)))
            | Self::ExecutableStore(E::Contract(ContractError::Serialization(_)))
            | Self::ContractStore(C::InvalidArgument { .. } | C::NullArgument(_))
            | Self::ExecutableStore(E::InvalidArgument { .. } | E::Timestamp(_))
            | Self::ContractStore(C::Timestamp(_)) => ("invalid", "validation"),
            Self::NotFound
            | Self::ContractStore(C::NotFound { .. })
            | Self::ExecutableStore(E::NotFound(_) | E::ContractMismatch { .. }) => {
                ("invalid", "not_found")
            }
            Self::Stale { .. } => ("stale", "conflict"),
            Self::ContractStore(C::AuthorityConflict { .. })
            | Self::ExecutableStore(
                E::ActivationConflict { .. }
                | E::CandidateConflict { .. }
                | E::CurrentContractConflict { .. }
                | E::AdmissionConflict { .. }
                | E::LifecycleConflict { .. },
            ) => ("conflict", "conflict"),
            Self::ContractStore(C::Database(sqlx::Error::PoolTimedOut))
            | Self::ExecutableStore(E::Database(sqlx::Error::PoolTimedOut)) => {
                ("unavailable", "timeout")
            }
            Self::ContractStore(C::Database(_))
            | Self::ExecutableStore(E::Database(_))
            | Self::Body(HttpJsonError::Body(_)) => ("unavailable", "dependency"),
            Self::ContractStore(C::Cancelled) | Self::ExecutableStore(E::Cancelled) => {
                ("cancelled", "cancelled")
            }
            Self::Integrity(_)
            | Self::Materialization(_)
            | Self::ContractStore(C::Integrity(_) | C::Contract(ContractError::Json(_)))
            | Self::ExecutableStore(E::Integrity(_) | E::Contract(ContractError::Json(_)))
            | Self::Contract(ContractError::Json(_)) => ("failure", "integrity"),
            Self::Compilation(ExecutableCompilationError::Contract(ContractError::Json(_))) => {
                ("failure", "integrity")
            }
            _ => ("failure", "internal"),
        }
    }

    pub fn problem(&self) -> Problem {
        use ContractStoreError as C;
        use ExecutableStoreError as E;
        let (status, kind, title, detail) = match self {
            Self::InvalidQuery => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
                self.to_string(),
            ),
            Self::Body(HttpJsonError::Invalid(problem)) => return problem.clone(),
            Self::InvalidArgument
            | Self::Compilation(ExecutableCompilationError::Contract(
                ContractError::Serialization(_),
            ))
            | Self::Contract(ContractError::Serialization(_))
            | Self::ContractStore(C::Contract(ContractError::Serialization(_)))
            | Self::ExecutableStore(E::Contract(ContractError::Serialization(_)))
            | Self::Body(HttpJsonError::Native(ContractError::Serialization(_)))
            | Self::ContractStore(C::InvalidArgument { .. } | C::NullArgument(_))
            | Self::ExecutableStore(E::InvalidArgument { .. } | E::Timestamp(_))
            | Self::ContractStore(C::Timestamp(_)) => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
                "The request path or query parameters are invalid.".into(),
            ),
            Self::NameMismatch => (
                StatusCode::CONFLICT,
                "contract-name-mismatch",
                "Contract name mismatch",
                self.to_string(),
            ),
            Self::InvalidContract => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-decision-contract",
                "Invalid decision contract",
                self.to_string(),
            ),
            Self::Compilation(
                error @ (ExecutableCompilationError::Validation(_)
                | ExecutableCompilationError::Expression(_)),
            ) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-decision-executable",
                "Invalid decision executable",
                match error {
                    ExecutableCompilationError::Validation(issues) => issues
                        .iter()
                        .map(|issue| format!("{} at {}: {}", issue.code, issue.path, issue.message))
                        .collect::<Vec<_>>()
                        .join("; "),
                    _ => error.to_string(),
                },
            ),
            Self::NotFound | Self::ContractStore(C::NotFound { .. }) => (
                StatusCode::NOT_FOUND,
                "contract-version-not-found",
                "Decision contract not found",
                "The requested DecisionContract resource does not exist.".into(),
            ),
            Self::Stale { .. } => (
                StatusCode::CONFLICT,
                "stale-contract-digest",
                "Stale contract digest",
                self.to_string(),
            ),
            Self::ContractStore(C::AuthorityConflict {
                contract_name,
                existing_authority,
                ..
            }) => (
                StatusCode::CONFLICT,
                "contract-name-authority-conflict",
                "Contract name authority conflict",
                format!(
                    "Contract name '{contract_name}' is already owned by authority '{}/{}/{}'.",
                    existing_authority.tenant.as_deref().unwrap_or(""),
                    existing_authority.application.as_deref().unwrap_or(""),
                    existing_authority.environment.as_deref().unwrap_or("")
                ),
            ),
            Self::ExecutableStore(E::AdmissionConflict { .. }) => (
                StatusCode::CONFLICT,
                "candidate-admission-conflict",
                "Candidate admission conflict",
                self.to_string(),
            ),
            Self::ExecutableStore(E::LifecycleConflict { .. }) => (
                StatusCode::CONFLICT,
                "candidate-admission-conflict",
                "Candidate lifecycle conflict",
                self.to_string(),
            ),
            Self::ExecutableStore(E::NotFound(_) | E::ContractMismatch { .. }) => (
                StatusCode::NOT_FOUND,
                "executable-not-found",
                "Decision executable not found",
                "The requested executable does not exist for this contract version.".into(),
            ),
            Self::ExecutableStore(E::ActivationConflict { .. }) => (
                StatusCode::CONFLICT,
                "activation-conflict",
                "Activation conflict",
                "Runtime authority changed before the requested activation completed.".into(),
            ),
            Self::ExecutableStore(
                E::CandidateConflict { .. } | E::CurrentContractConflict { .. },
            ) => (
                StatusCode::CONFLICT,
                "activation-conflict",
                "Activation conflict",
                "Executable lifecycle state changed before the requested activation completed."
                    .into(),
            ),
            _ if self.classification().0 == "unavailable" => return dependency_unavailable(),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal error",
                "The Contract Service encountered an unexpected error.".into(),
            ),
        };
        Problem::new(status, kind, title, detail)
    }
}

pub fn dependency_unavailable() -> Problem {
    Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "dependency-unavailable",
        "Dependency unavailable",
        "A required management dependency is unavailable.",
    )
    .retry_after(1)
}

pub type Result<T> = std::result::Result<T, ServiceError>;
