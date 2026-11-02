mod error;
mod lifecycle;
pub mod models;
mod observability;
pub mod worker;

pub use error::ServiceError;
pub use lifecycle::ContractLifecycle;
pub use observability::ContractObservability;

use axum::{
    Router,
    body::Body,
    extract::{Extension, Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse as _, Response},
    routing::{get, post},
};
use error::Result;
use flaggo_contract::{
    AnalysisCandidateSubmission, DecisionContract, DecisionExecutableActivationRequest,
};
use flaggo_service_hosting::{RequestContext, json_response, read_json, with_http_contract};
use serde::Serialize;
use serde_json::{Value, json};

pub const SERVICE_NAME: &str = "flaggo-contract-service";
pub const INSTRUMENTATION_SCOPE: &str = "flaggo.contract-service";
const BASE: &str = "/v3/decision-contracts/{contractName}";
const VALIDATE: &str = "/v3/decision-contracts/{contractName}/validate";
const VERSIONS: &str = "/v3/decision-contracts/{contractName}/versions";
const VERSION: &str = "/v3/decision-contracts/{contractName}/versions/{contractDigest}";
const CANDIDATES: &str =
    "/v3/decision-contracts/{contractName}/versions/{contractDigest}/candidates";
const EXECUTABLES: &str =
    "/v3/decision-contracts/{contractName}/versions/{contractDigest}/executables";
const HISTORY: &str =
    "/v3/decision-contracts/{contractName}/versions/{contractDigest}/executables/history";
const ACTIVATE: &str = "/v3/decision-contracts/{contractName}/versions/{contractDigest}/executables/{executableDigest}/activate";
const CATALOG: &str = "/v3/decision-contract-catalog/current";
const ROUTES: &[&str] = &[
    BASE,
    VALIDATE,
    VERSIONS,
    VERSION,
    CANDIDATES,
    EXECUTABLES,
    HISTORY,
    ACTIVATE,
    CATALOG,
    "/health/live",
    "/health/ready",
];

pub fn router(lifecycle: ContractLifecycle) -> Router {
    with_http_contract(
        Router::new()
            .route(BASE, get(current).put(deploy))
            .route(VALIDATE, post(validate))
            .route(VERSIONS, get(versions))
            .route(VERSION, get(version))
            .route(CANDIDATES, post(submit))
            .route(EXECUTABLES, get(executables))
            .route(HISTORY, get(history))
            .route(ACTIVATE, post(activate))
            .route(CATALOG, get(catalog))
            .route("/health/live", get(live))
            .route("/health/ready", get(ready))
            .with_state(lifecycle),
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
        ROUTES,
    )
}

async fn validate(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let contract: DecisionContract = read_json(&headers, body).await?;
        let validation = state
            .correlated(&context.correlation_id)
            .validate(&name, &contract)
            .await?;
        response(StatusCode::OK, &validation)
    }
    .await;
    finish(result, &context)
}

async fn deploy(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let contract: DecisionContract = read_json(&headers, body).await?;
        let (version, created) = state
            .correlated(&context.correlation_id)
            .deploy(&name, &contract)
            .await?;
        let mut response = response(
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            &version,
        )?;
        if created {
            let location = format!(
                "/v3/decision-contracts/{name}/versions/{}",
                version.contract_digest
            );
            response.headers_mut().insert(
                header::LOCATION,
                HeaderValue::from_str(&location).map_err(|_| ServiceError::InvalidArgument)?,
            );
        }
        Ok(response)
    }
    .await;
    finish(result, &context)
}

async fn current(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path(name): Path<String>,
) -> Response {
    finish(
        optional(
            state
                .correlated(&context.correlation_id)
                .get(&name, None)
                .await,
        ),
        &context,
    )
}

async fn version(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((name, digest)): Path<(String, String)>,
) -> Response {
    finish(
        optional(
            state
                .correlated(&context.correlation_id)
                .get(&name, Some(&digest))
                .await,
        ),
        &context,
    )
}

async fn versions(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let result = async {
        let (limit, cursor) = pagination(query.as_deref())?;
        optional(
            state
                .correlated(&context.correlation_id)
                .versions(&name, limit, cursor.as_deref())
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn catalog(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        let (catalog, etag) = state.correlated(&context.correlation_id).catalog().await?;
        let matched = headers
            .get_all(header::IF_NONE_MATCH)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .any(|value| value == "*" || value == etag);
        let mut response = if matched {
            StatusCode::NOT_MODIFIED.into_response()
        } else {
            response(StatusCode::OK, &catalog)?
        };
        response.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&etag).expect("ASCII digest etag"),
        );
        Ok(response)
    }
    .await;
    finish(result, &context)
}

async fn submit(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((name, digest)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let submission: AnalysisCandidateSubmission = read_json(&headers, body).await?;
        let (value, created) = state
            .correlated(&context.correlation_id)
            .submit(&name, &digest, submission)
            .await?;
        response(
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            &value,
        )
    }
    .await;
    finish(result, &context)
}

async fn executables(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((name, digest)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    list_executables(state, context, name, digest, query, false).await
}

async fn history(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((name, digest)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    list_executables(state, context, name, digest, query, true).await
}

async fn list_executables(
    state: ContractLifecycle,
    context: RequestContext,
    name: String,
    digest: String,
    query: Option<String>,
    history: bool,
) -> Response {
    let result = async {
        let (limit, cursor) = pagination(query.as_deref())?;
        optional(
            state
                .correlated(&context.correlation_id)
                .executables(&name, &digest, limit, cursor.as_deref(), history)
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn activate(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((name, digest, candidate)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let request: DecisionExecutableActivationRequest = read_json(&headers, body).await?;
        let value = state
            .correlated(&context.correlation_id)
            .activate(&name, &digest, &candidate, request)
            .await?;
        response(StatusCode::OK, &value)
    }
    .await;
    finish(result, &context)
}

async fn live() -> axum::Json<Value> {
    axum::Json(
        json!({"status": "live", "service": SERVICE_NAME, "version": env!("CARGO_PKG_VERSION"),
        "observedAt": lifecycle::utc_timestamp(chrono::Utc::now().fixed_offset())}),
    )
}

async fn ready(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
) -> Response {
    let result = async {
        let contracts = state.contracts.is_available(None).await?;
        let executables = state.executables.is_available(None).await?;
        response(if contracts && executables { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
            &json!({"status": if contracts && executables { "ready" } else { "not-ready" },
                "observedAt": lifecycle::utc_timestamp(chrono::Utc::now().fixed_offset()),
                "checks": [{"name": "contract-store", "status": if contracts { "up" } else { "down" }, "required": true},
                    {"name": "executable-store", "status": if executables { "up" } else { "down" }, "required": true}]}))
    }.await;
    finish(result, &context)
}

fn pagination(query: Option<&str>) -> Result<(i32, Option<String>)> {
    let mut limits = Vec::new();
    let mut cursors = Vec::new();
    for (name, value) in url::form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
        if name.eq_ignore_ascii_case("limit") {
            limits.push(value.into_owned());
        } else if name.eq_ignore_ascii_case("cursor") {
            cursors.push(value.into_owned());
        }
    }
    let limit = match limits.as_slice() {
        [] => 50,
        [value] => value
            .trim()
            .parse()
            .map_err(|_| ServiceError::InvalidQuery)?,
        _ => return Err(ServiceError::InvalidQuery),
    };
    Ok((
        limit,
        if cursors.is_empty() {
            None
        } else {
            Some(cursors.join(","))
        },
    ))
}

fn response(status: StatusCode, value: &impl Serialize) -> Result<Response> {
    Ok(json_response(status, value)?)
}

fn optional(result: Result<Option<impl Serialize>>) -> Result<Response> {
    response(StatusCode::OK, &result?.ok_or(ServiceError::NotFound)?)
}

fn finish(result: Result<Response>, context: &RequestContext) -> Response {
    match result {
        Ok(response) => response,
        Err(error) => {
            if matches!(error.classification().0, "failure" | "unavailable") {
                tracing::error!(target: INSTRUMENTATION_SCOPE, %error, "Contract request failed");
            }
            error.problem().response(context)
        }
    }
}
