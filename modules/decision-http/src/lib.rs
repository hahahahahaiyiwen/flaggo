use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse as _, Response},
    routing::{get, post},
};
use flaggo_contract::{ContractError, ContractResourceId, ResourceScope, RuntimeInput};
use flaggo_decision::{
    ActiveExecutableReader, ContractVersionReader, DecisionError, DecisionRuntime, ReadinessProbe,
    StoreReadError,
};
use flaggo_service_hosting::{
    HttpJsonError, Problem, RequestContext, RequestSecurity, json_response, read_json,
    with_http_contract, with_resource_security,
};
use tracing::{Instrument as _, Span};

mod observability;
pub use observability::DecisionObservability;

pub const SERVICE_NAME: &str = "flaggo-decision-service";
pub const INSTRUMENTATION_SCOPE: &str = "flaggo.decision-service";
const DECISION_ROUTE: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}/decisions";
const ROUTES: &[&str] = &[DECISION_ROUTE, "/health/live", "/health/ready"];

#[derive(Clone)]
struct AppState<C, E> {
    contracts: C,
    executables: E,
    runtime: Arc<DecisionRuntime<C, E>>,
    observability: DecisionObservability,
}

pub fn router<C, E>(contracts: C, executables: E, security: Arc<dyn RequestSecurity>) -> Router
where
    C: ContractVersionReader + ReadinessProbe + Clone + 'static,
    E: ActiveExecutableReader + ReadinessProbe + Clone + 'static,
{
    instrumented_router(
        contracts,
        executables,
        DecisionObservability::new(),
        security,
    )
}

pub fn instrumented_router<C, E>(
    contracts: C,
    executables: E,
    observability: DecisionObservability,
    security: Arc<dyn RequestSecurity>,
) -> Router
where
    C: ContractVersionReader + ReadinessProbe + Clone + 'static,
    E: ActiveExecutableReader + ReadinessProbe + Clone + 'static,
{
    let state = AppState {
        runtime: Arc::new(DecisionRuntime::new(contracts.clone(), executables.clone())),
        contracts,
        executables,
        observability,
    };
    with_http_contract(
        with_resource_security(
            Router::new()
                .route(DECISION_ROUTE, post(decide::<C, E>))
                .route("/health/live", get(liveness))
                .route("/health/ready", get(readiness::<C, E>))
                .with_state(state),
            security,
        ),
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
        ROUTES,
    )
}

async fn decide<C, E>(
    State(state): State<AppState<C, E>>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name, digest)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    body: Body,
) -> Response
where
    C: ContractVersionReader + ReadinessProbe + Clone + 'static,
    E: ActiveExecutableReader + ReadinessProbe + Clone + 'static,
{
    let span = tracing::info_span!(
        target: INSTRUMENTATION_SCOPE,
        "flaggo.decision.evaluate",
        flaggo.operation.name = "decision.evaluate",
        flaggo.operation.outcome = tracing::field::Empty,
        flaggo.failure.category = tracing::field::Empty,
        flaggo.contract.name = %name,
        flaggo.contract.digest = %digest,
        flaggo.executable.digest = tracing::field::Empty,
        flaggo.request.correlation_id = %context.correlation_id,
        flaggo.evaluation.source = tracing::field::Empty,
        error.type = tracing::field::Empty,
    );
    async {
        let mut operation = state.observability.start();
        let resource = match ResourceScope::new(tenant, application, environment)
            .and_then(|scope| ContractResourceId::new(scope, name.clone())) {
                Ok(resource) => resource,
                Err(error) => {
                    operation.fail(&error, "invalid", "validation");
                    return invalid_argument().response(&context);
                }
            };
        let input: RuntimeInput = match read_json(&headers, body).await {
            Ok(input) => input,
            Err(error) => {
                let (outcome, category) = match &error {
                    HttpJsonError::Invalid(_)
                    | HttpJsonError::Native(ContractError::Serialization(_)) => ("invalid", "validation"),
                    HttpJsonError::Body(_) => ("unavailable", "dependency"),
                    _ => ("failure", "internal"),
                };
                operation.fail(&error, outcome, category);
                let problem = match error {
                    HttpJsonError::Invalid(problem) => problem,
                    HttpJsonError::Native(ContractError::Serialization(_)) => invalid_argument(),
                    HttpJsonError::Body(error) => {
                        tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Decision body read failed");
                        dependency_unavailable()
                    }
                    HttpJsonError::Native(error) => {
                        tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Unhandled Decision Service failure");
                        internal_error()
                    }
                };
                return problem.response(&context);
            }
        };
        match state.runtime.decide(&resource, &digest, &input, None).await {
            Ok(decision) => match json_response(StatusCode::OK, &decision) {
                Ok(response) => {
                    Span::current().record("flaggo.executable.digest", &decision.executable_digest);
                    operation.complete(decision.evaluation.source());
                    response
                }
                Err(error) => {
                    operation.fail(&error, "invalid", "validation");
                    invalid_argument().response(&context)
                }
            },
            Err(error) => {
                let (outcome, category) = error.telemetry_classification();
                operation.fail(&error, outcome, category);
                if outcome == "failure" || outcome == "unavailable" {
                    tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Decision request failed");
                }
                decision_problem(&error).response(&context)
            }
        }
    }
    .instrument(span)
    .await
}

async fn liveness() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "live",
        "service": SERVICE_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "observedAt": observed_at(),
    }))
}

async fn readiness<C, E>(
    State(state): State<AppState<C, E>>,
    Extension(context): Extension<RequestContext>,
) -> Response
where
    C: ContractVersionReader + ReadinessProbe + Clone + 'static,
    E: ActiveExecutableReader + ReadinessProbe + Clone + 'static,
{
    let checks = async {
        let contracts = state.contracts.is_available(None).await.map_err(|error| {
            tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Contract Store readiness failed");
        })?;
        let executables = state.executables.is_available(None).await.map_err(|error| {
            tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Executable Store readiness failed");
        })?;
        Ok::<_, ()>((contracts, executables))
    }.await;
    match checks {
        Ok((contracts, executables)) => {
            let ready = contracts && executables;
            (
                if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
                axum::Json(serde_json::json!({
                    "status": if ready { "ready" } else { "not-ready" },
                    "observedAt": observed_at(),
                    "checks": [
                        { "name": "contract-store", "status": if contracts { "up" } else { "down" }, "required": true },
                        { "name": "executable-store", "status": if executables { "up" } else { "down" }, "required": true },
                    ],
                })),
            ).into_response()
        }
        Err(()) => dependency_unavailable().response(&context),
    }
}

fn observed_at() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

fn invalid_argument() -> Problem {
    Problem::new(
        StatusCode::BAD_REQUEST,
        "invalid-request",
        "Invalid request",
        "The request path or parameters are invalid.",
    )
}

fn dependency_unavailable() -> Problem {
    Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "dependency-unavailable",
        "Dependency unavailable",
        "A required runtime dependency is unavailable.",
    )
    .retry_after(1)
}

fn internal_error() -> Problem {
    Problem::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal-error",
        "Internal error",
        "The Decision Service encountered an unexpected error.",
    )
}

fn decision_problem(error: &DecisionError) -> Problem {
    let (status, kind, title, detail) = match error {
        DecisionError::InvalidIdentity(_)
        | DecisionError::NonFiniteResult(_)
        | DecisionError::Contract(ContractError::Serialization(_))
        | DecisionError::Store(StoreReadError::InvalidArgument(_)) => return invalid_argument(),
        DecisionError::MaterializationNative(error)
            if matches!(
                error.as_ref(),
                flaggo_expression::ExecutableMaterializationError::Contract(
                    ContractError::Serialization(_)
                )
            ) =>
        {
            return invalid_argument();
        }
        DecisionError::ContractNotFound { .. } => (
            StatusCode::NOT_FOUND,
            "contract-version-not-found",
            "Contract version not found",
            "The requested contract version does not exist.".to_owned(),
        ),
        DecisionError::InputValidation(issues) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid-runtime-input",
            "Invalid runtime input",
            issues
                .iter()
                .map(|issue| format!("{}: {}", issue.path, issue.message))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        DecisionError::ActiveExecutableNotFound(_) => {
            return Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "executable-not-active",
                "Executable not active",
                "No executable is active for the requested contract version.",
            )
            .retry_after(1);
        }
        DecisionError::Integrity { .. } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "executable-integrity-failure",
            "Executable integrity failure",
            "Stored runtime authority failed integrity validation.".to_owned(),
        ),
        DecisionError::Evaluation { .. } | DecisionError::NonBooleanPredicate { .. } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "evaluation-failed",
            "Evaluation failed",
            "The active executable could not evaluate the runtime input.".to_owned(),
        ),
        DecisionError::ResultValidation { .. } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid-runtime-result",
            "Invalid runtime result",
            "The active executable produced a result outside the contract schema.".to_owned(),
        ),
        DecisionError::Store(StoreReadError::Dependency(_) | StoreReadError::Timeout(_)) => {
            return dependency_unavailable();
        }
        _ => return internal_error(),
    };
    Problem::new(status, kind, title, detail)
}
