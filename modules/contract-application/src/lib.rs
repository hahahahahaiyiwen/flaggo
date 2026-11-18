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
    AnalysisCandidateSubmission, ContractResourceId, DecisionContract,
    DecisionExecutableActivationRequest, ResourceScope,
};
use flaggo_service_hosting::{
    RequestContext, RequestSecurity, json_response, read_json, with_http_contract,
    with_resource_security,
};
use serde::Serialize;
use serde_json::{Value, json};

pub const SERVICE_NAME: &str = "flaggo-contract-service";
pub const INSTRUMENTATION_SCOPE: &str = "flaggo.contract-service";
const BASE: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}";
const VALIDATE: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/validate";
const VERSIONS: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions";
const VERSION: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}";
const CANDIDATES: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}/candidates";
const EXECUTABLES: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}/executables";
const HISTORY: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}/executables/history";
const ACTIVATE: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/versions/{contractDigest}/executables/{executableDigest}/activate";
const CATALOG: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contract-catalog/current";
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

pub fn router(
    lifecycle: ContractLifecycle,
    security: std::sync::Arc<dyn RequestSecurity>,
) -> Router {
    with_http_contract(
        with_resource_security(
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
            security,
        ),
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
        ROUTES,
    )
}

async fn validate(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        let contract: DecisionContract = read_json(&headers, body).await?;
        let validation = state
            .correlated(&context.correlation_id)
            .validate(&resource, &contract)
            .await?;
        response(StatusCode::OK, &validation)
    }
    .await;
    finish(result, &context)
}

async fn deploy(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        let contract: DecisionContract = read_json(&headers, body).await?;
        let (version, created) = state
            .correlated(&context.correlation_id)
            .deploy(&resource, &contract)
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
                "/v4/tenants/{}/applications/{}/environments/{}/decision-contracts/{}/versions/{}",
                resource.scope.tenant,
                resource.scope.application,
                resource.scope.environment,
                resource.name,
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
    Path((tenant, application, environment, name)): Path<(String, String, String, String)>,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        optional(
            state
                .correlated(&context.correlation_id)
                .get(&resource, None)
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn version(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name, digest)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        optional(
            state
                .correlated(&context.correlation_id)
                .get(&resource, Some(&digest))
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn versions(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name)): Path<(String, String, String, String)>,
    RawQuery(query): RawQuery,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        let (limit, cursor) = pagination(query.as_deref())?;
        optional(
            state
                .correlated(&context.correlation_id)
                .versions(&resource, limit, cursor.as_deref())
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn catalog(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path(scope): Path<ResourceScope>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        let (catalog, etag) = state
            .correlated(&context.correlation_id)
            .catalog(&scope)
            .await?;
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
    Path((tenant, application, environment, name, digest)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        let submission: AnalysisCandidateSubmission = read_json(&headers, body).await?;
        let (value, created) = state
            .correlated(&context.correlation_id)
            .submit(&resource, &digest, submission)
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
    Path((tenant, application, environment, name, digest)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    RawQuery(query): RawQuery,
) -> Response {
    list_executables(
        state,
        context,
        resource(tenant, application, environment, name),
        digest,
        query,
        false,
    )
    .await
}

async fn history(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name, digest)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    RawQuery(query): RawQuery,
) -> Response {
    list_executables(
        state,
        context,
        resource(tenant, application, environment, name),
        digest,
        query,
        true,
    )
    .await
}

async fn list_executables(
    state: ContractLifecycle,
    context: RequestContext,
    resource: Result<ContractResourceId>,
    digest: String,
    query: Option<String>,
    history: bool,
) -> Response {
    let result = async {
        let resource = resource?;
        let (limit, cursor) = pagination(query.as_deref())?;
        optional(
            state
                .correlated(&context.correlation_id)
                .executables(&resource, &digest, limit, cursor.as_deref(), history)
                .await,
        )
    }
    .await;
    finish(result, &context)
}

async fn activate(
    State(state): State<ContractLifecycle>,
    Extension(context): Extension<RequestContext>,
    Path((tenant, application, environment, name, digest, candidate)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let result = async {
        let resource = resource(tenant, application, environment, name)?;
        let request: DecisionExecutableActivationRequest = read_json(&headers, body).await?;
        let value = state
            .correlated(&context.correlation_id)
            .activate(&resource, &digest, &candidate, request)
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
        let (contracts, executables) = state.authority.readiness().await?;
        response(if contracts && executables { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
            &json!({"status": if contracts && executables { "ready" } else { "not-ready" },
                "observedAt": lifecycle::utc_timestamp(chrono::Utc::now().fixed_offset()),
                "checks": [{"name": "contract-store", "status": if contracts { "up" } else { "down" }, "required": true},
                    {"name": "executable-store", "status": if executables { "up" } else { "down" }, "required": true}]}))
    }.await;
    finish(result, &context)
}

fn resource(
    tenant: String,
    application: String,
    environment: String,
    name: String,
) -> Result<ContractResourceId> {
    let scope = ResourceScope::new(tenant, application, environment)
        .map_err(|_| ServiceError::InvalidArgument)?;
    ContractResourceId::new(scope, name).map_err(|_| ServiceError::InvalidArgument)
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
