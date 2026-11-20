use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use axum::{
    Router,
    extract::{FromRequestParts, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use flaggo_contract::ResourceScope;
use serde::{Deserialize, Serialize};

use crate::{Problem, RequestContext};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Principal {
    pub subject: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    ReadContract,
    DeployContract,
    ValidateContract,
    SubmitCandidate,
    ActivateCandidate,
    Evaluate,
    ReadCatalog,
    IngestOtlp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Hidden,
    Forbidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestAdmission {
    Accepted,
    Limited { retry_after_seconds: NonZeroU32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestAdmissionError {
    #[error("The request admission authority is unavailable.")]
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("Authentication is required.")]
    Unauthenticated { challenge: HeaderValue },
    #[error("Identity or authorization authority unavailable: {0}")]
    Unavailable(String),
}

/// Credentials establish identity independently of route and body declarations.
/// Implementations must not grant access merely because a caller names a scope.
#[async_trait::async_trait]
pub trait RequestSecurity: Send + Sync {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, SecurityError>;
    async fn authorize(
        &self,
        principal: &Principal,
        scope: &ResourceScope,
        contract_name: Option<&str>,
        operation: Operation,
    ) -> Result<Access, SecurityError>;

    /// Admit one authorized product-request attempt before handler validation or lookup.
    ///
    /// Metered adapters charge one unit on acceptance. Later errors or cancellation
    /// do not refund it; retries are new attempts. Adapters own atomic accounting
    /// and cancellation safety, including commits that outlive this future.
    /// The default is non-metered, including for `TrustedLocal`.
    async fn admit_authorized(
        &self,
        _principal: &Principal,
        _scope: &ResourceScope,
        _contract_name: Option<&str>,
        _operation: Operation,
    ) -> Result<RequestAdmission, RequestAdmissionError> {
        Ok(RequestAdmission::Accepted)
    }

    /// Admit one complete OTLP batch after validation and authorization of every scope.
    ///
    /// Metered adapters atomically charge one unit per distinct tenant, not per
    /// scope: either all tenants have quota and are charged, or none are charged.
    /// An empty scope slice incurs no tenant charge (do not infer a caller tenant).
    /// Later append failures or cancellation do not refund acceptance; retries
    /// are new attempts. Adapters own cancellation-safe accounting and separate
    /// row/metadata capacity protections, including for empty batches.
    /// The default accepts without metering; authorization must never charge.
    async fn admit_authorized_otlp(
        &self,
        _principal: &Principal,
        _scopes: &[ResourceScope],
    ) -> Result<RequestAdmission, RequestAdmissionError> {
        Ok(RequestAdmission::Accepted)
    }
}

/// Explicit opt-in for trusted local composition, not a hosted identity adapter.
pub struct TrustedLocal;

#[async_trait::async_trait]
impl RequestSecurity for TrustedLocal {
    async fn authenticate(&self, _: &HeaderMap) -> Result<Principal, SecurityError> {
        Ok(Principal {
            subject: "trusted-local".into(),
        })
    }

    async fn authorize(
        &self,
        _: &Principal,
        _: &ResourceScope,
        _: Option<&str>,
        _: Operation,
    ) -> Result<Access, SecurityError> {
        Ok(Access::Allowed)
    }
}

impl SecurityError {
    pub fn problem(&self) -> Problem {
        match self {
            Self::Unauthenticated { challenge } => {
                let mut problem = Problem::new(
                    StatusCode::UNAUTHORIZED,
                    "authentication-required",
                    "Authentication required",
                    "A valid authenticated identity is required.",
                );
                problem.authentication_challenge = Some(challenge.clone());
                problem
            }
            Self::Unavailable(_) => Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization-unavailable",
                "Authorization unavailable",
                "The identity or authorization authority is unavailable.",
            ),
        }
    }
}

pub fn require_access(access: Access) -> Result<(), Problem> {
    match access {
        Access::Allowed => Ok(()),
        Access::Hidden => Err(Problem::new(
            StatusCode::NOT_FOUND,
            "resource-not-found",
            "Resource not found",
            "The requested resource does not exist.",
        )),
        Access::Forbidden => Err(Problem::new(
            StatusCode::FORBIDDEN,
            "operation-forbidden",
            "Operation forbidden",
            "The authenticated identity cannot perform this operation.",
        )),
    }
}

/// Apply inside `with_http_contract` so denials use the same RFC 9457 contract.
pub fn with_resource_security(router: Router, security: Arc<dyn RequestSecurity>) -> Router {
    router.route_layer(middleware::from_fn_with_state(security, authorize_resource))
}

async fn authorize_resource(
    State(security): State<Arc<dyn RequestSecurity>>,
    request: Request,
    next: Next,
) -> Response {
    let context = request
        .extensions()
        .get::<RequestContext>()
        .expect("resource security runs inside the shared HTTP contract")
        .clone();
    if matches!(request.uri().path(), "/health/live" | "/health/ready") {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let result = async {
        let principal = security
            .authenticate(&parts.headers)
            .await
            .map_err(|error| {
                if matches!(error, SecurityError::Unavailable(_)) {
                    tracing::error!(%error, "Request authentication failed");
                }
                error.problem()
            })?;
        let Path(parameters) = Path::<HashMap<String, String>>::from_request_parts(&mut parts, &())
            .await
            .map_err(|_| invalid_scope())?;
        let field = |key| parameters.get(key).cloned().ok_or_else(invalid_scope);
        let scope = ResourceScope::new(
            field("tenant")?,
            field("application")?,
            field("environment")?,
        )
        .map_err(|_| invalid_scope())?;
        let path = parts.uri.path();
        let operation = if parts.method == Method::GET || parts.method == Method::HEAD {
            if parameters.contains_key("contractName") {
                Operation::ReadContract
            } else {
                Operation::ReadCatalog
            }
        } else if parts.method == Method::PUT {
            Operation::DeployContract
        } else if path.ends_with("/validate") {
            Operation::ValidateContract
        } else if path.ends_with("/candidates") {
            Operation::SubmitCandidate
        } else if path.ends_with("/activate") {
            Operation::ActivateCandidate
        } else if path.ends_with("/decisions") {
            Operation::Evaluate
        } else {
            return Err(invalid_scope());
        };
        let access = security
            .authorize(
                &principal,
                &scope,
                parameters.get("contractName").map(String::as_str),
                operation,
            )
            .await
            .map_err(|error| {
                if matches!(error, SecurityError::Unavailable(_)) {
                    tracing::error!(%error, "Request authorization failed");
                }
                error.problem()
            })?;
        require_access(access)?;
        match security
            .admit_authorized(
                &principal,
                &scope,
                parameters.get("contractName").map(String::as_str),
                operation,
            )
            .await
            .map_err(|error| {
                tracing::error!(%error, "Request admission failed");
                Problem::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "request-admission-unavailable",
                    "Request admission unavailable",
                    "The request admission authority is unavailable.",
                )
            })? {
            RequestAdmission::Accepted => {}
            RequestAdmission::Limited {
                retry_after_seconds,
            } => {
                return Err(Problem::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "request-quota-exceeded",
                    "Request quota exceeded",
                    "The authorized request quota is exhausted.",
                )
                .retry_after(retry_after_seconds.get()));
            }
        }
        Ok(principal)
    }
    .await;
    match result {
        Ok(principal) => {
            parts.extensions.insert(principal);
            next.run(Request::from_parts(parts, body)).await
        }
        Err(problem) => problem.response(&context),
    }
}

fn invalid_scope() -> Problem {
    Problem::new(
        StatusCode::BAD_REQUEST,
        "invalid-request",
        "Invalid request",
        "The resource scope is invalid.",
    )
}
