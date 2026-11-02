use std::{error::Error as _, time::Instant};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use flaggo_contract::{ContractError, strict_json};
use opentelemetry::{KeyValue, global, metrics::Histogram, trace::TraceContextExt as _};
use opentelemetry_http::HeaderExtractor;
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Instrument as _, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

pub const CORRELATION_HEADER: &str = "x-flaggo-correlation-id";

#[derive(Debug, thiserror::Error)]
#[error(
    "Legacy setting {legacy} is unsupported; set {replacement} explicitly before starting the Rust service."
)]
pub struct LegacyConfigurationError {
    legacy: &'static str,
    replacement: &'static str,
}

pub fn reject_legacy_core_configuration(
    database_setting: &'static str,
    listen_setting: &'static str,
) -> Result<(), LegacyConfigurationError> {
    for (legacy, replacement) in [
        ("ConnectionStrings__Flaggo", database_setting),
        ("ASPNETCORE_URLS", listen_setting),
    ] {
        if std::env::var_os(legacy).is_some() && std::env::var_os(replacement).is_none() {
            return Err(LegacyConfigurationError {
                legacy,
                replacement,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub correlation_id: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub status: StatusCode,
    pub kind: &'static str,
    pub title: &'static str,
    pub detail: String,
    pub retry_after_seconds: Option<u32>,
}

impl Problem {
    pub fn new(
        status: StatusCode,
        kind: &'static str,
        title: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status,
            kind,
            title,
            detail: detail.into(),
            retry_after_seconds: None,
        }
    }

    pub fn retry_after(mut self, seconds: u32) -> Self {
        self.retry_after_seconds = Some(seconds);
        self
    }

    pub fn response(&self, context: &RequestContext) -> Response {
        let document = serde_json::json!({
            "type": format!("https://flaggo.dev/problems/{}", self.kind),
            "title": self.title,
            "status": self.status.as_u16(),
            "detail": self.detail,
            "instance": context.path,
        });
        let mut response = (self.status, axum::Json(document)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Some(seconds) = self.retry_after_seconds {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpJsonError {
    #[error("{}", .0.detail)]
    Invalid(Problem),
    #[error(transparent)]
    Native(#[from] ContractError),
    #[error(transparent)]
    Body(axum::Error),
}

pub async fn read_json<T: DeserializeOwned>(
    headers: &HeaderMap,
    body: Body,
) -> Result<T, HttpJsonError> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok());
    if !media_type
        .is_some_and(|value| value.type_() == mime::APPLICATION && value.subtype() == mime::JSON)
    {
        return Err(HttpJsonError::Invalid(Problem::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported-media-type",
            "Unsupported media type",
            "Content-Type must be application/json.",
        )));
    }
    let too_large = || {
        HttpJsonError::Invalid(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid-request",
            "Invalid request",
            "Request body exceeds the maximum size.",
        ))
    };
    if headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|value| value > strict_json::MAXIMUM_DOCUMENT_BYTES as u64)
    {
        return Err(too_large());
    }
    let bytes = to_bytes(body, strict_json::MAXIMUM_DOCUMENT_BYTES)
        .await
        .map_err(|error| {
            if error
                .source()
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
            {
                too_large()
            } else {
                HttpJsonError::Body(error)
            }
        })?;
    match strict_json::deserialize(&bytes) {
        Ok(value) => Ok(value),
        Err(ContractError::Json(error)) => Err(HttpJsonError::Invalid(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid-request",
            "Invalid request",
            format!("Request body is invalid: {error}"),
        ))),
        Err(error) => Err(HttpJsonError::Native(error)),
    }
}

pub fn json_response<T: Serialize + ?Sized>(
    status: StatusCode,
    value: &T,
) -> Result<Response, ContractError> {
    Ok((
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        strict_json::serialize(value)?,
    )
        .into_response())
}

#[derive(Clone)]
struct Hosting {
    scope: &'static str,
    routes: &'static [&'static str],
    duration: Histogram<f64>,
}

pub fn with_http_contract(
    router: Router,
    scope: &'static str,
    version: &'static str,
    routes: &'static [&'static str],
) -> Router {
    let hosting = Hosting {
        scope,
        routes,
        duration: flaggo_service_observability::ServiceObservability::meter(scope, version)
            .f64_histogram("http.server.request.duration")
            .with_unit("s")
            .build(),
    };
    Router::new()
        .fallback_service(router)
        .layer(middleware::from_fn_with_state(hosting, observe_request))
}

async fn observe_request(
    State(hosting): State<Hosting>,
    mut request: Request,
    next: Next,
) -> Response {
    let started_at = Instant::now();
    let path = request.uri().path().to_owned();
    let route = normalize_route(&mut request, hosting.routes);
    let method = request.method().to_string();
    let span = tracing::info_span!(
        "http.server",
        otel.kind = "server",
        otel.name = %format!("{method} {}", route.unwrap_or("")),
        http.request.method = %request.method(),
        http.route = route,
        url.path = %path,
        http.response.status_code = tracing::field::Empty,
        flaggo.request.correlation_id = tracing::field::Empty,
    );
    let parent = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let parent_trace_id = parent.span().span_context().trace_id();
    if let Err(error) = span.set_parent(parent) {
        tracing::debug!(scope = hosting.scope, %error, "HTTP trace parent was not attached");
    }
    let trace_id = span.context().span().span_context().trace_id();
    let supplied = request
        .headers()
        .get(CORRELATION_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        });
    let correlation_id = supplied.map(str::to_owned).unwrap_or_else(|| {
        if trace_id != opentelemetry::trace::TraceId::INVALID {
            trace_id.to_string()
        } else if parent_trace_id != opentelemetry::trace::TraceId::INVALID {
            parent_trace_id.to_string()
        } else {
            uuid::Uuid::new_v4().simple().to_string()
        }
    });
    span.record("flaggo.request.correlation_id", &correlation_id);
    let context = RequestContext {
        correlation_id,
        path,
    };
    request.extensions_mut().insert(context.clone());
    async {
        let mut response = next.run(request).await;
        if response.status().is_client_error() || response.status().is_server_error() {
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok());
            if content_type.is_none_or(|value| value.starts_with("text/plain")) {
                let mut problem = status_problem(response.status()).response(&context);
                if let Some(allow) = response.headers().get(header::ALLOW) {
                    problem.headers_mut().insert(header::ALLOW, allow.clone());
                }
                response = problem;
            }
        }
        response.headers_mut().insert(
            CORRELATION_HEADER,
            HeaderValue::from_str(&context.correlation_id).expect("validated ASCII correlation ID"),
        );
        Span::current().record("http.response.status_code", response.status().as_u16());
        if response.status().is_server_error() {
            Span::current().set_status(opentelemetry::trace::Status::error(
                response.status().as_str().to_owned(),
            ));
        }
        let method = match method.as_str() {
            "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "TRACE"
            | "CONNECT" => method,
            _ => "_OTHER".to_owned(),
        };
        let mut attributes = vec![
            KeyValue::new("http.request.method", method),
            KeyValue::new(
                "http.response.status_code",
                i64::from(response.status().as_u16()),
            ),
        ];
        if let Some(route) = route {
            attributes.push(KeyValue::new("http.route", route));
        }
        hosting
            .duration
            .record(started_at.elapsed().as_secs_f64(), &attributes);
        response
    }
    .instrument(span)
    .await
}

fn normalize_route(request: &mut Request, routes: &'static [&'static str]) -> Option<&'static str> {
    let path = request.uri().path();
    let path = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<_> = path.split('/').collect();
    for &route in routes {
        let pattern: Vec<_> = route.split('/').collect();
        if segments.len() != pattern.len()
            || !segments.iter().zip(&pattern).all(|(value, expected)| {
                if expected.starts_with('{') && expected.ends_with('}') {
                    !value.is_empty()
                } else {
                    percent_encoding::percent_decode_str(value)
                        .decode_utf8()
                        .is_ok_and(|value| value.eq_ignore_ascii_case(expected))
                }
            })
        {
            continue;
        }
        let canonical = segments
            .iter()
            .zip(pattern)
            .map(|(value, expected)| {
                if expected.starts_with('{') {
                    *value
                } else {
                    expected
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        let query = request
            .uri()
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        let mut parts = request.uri().clone().into_parts();
        parts.path_and_query = Some(
            format!("{canonical}{query}")
                .parse()
                .expect("valid normalized URI path"),
        );
        *request.uri_mut() = axum::http::Uri::from_parts(parts).expect("unchanged URI authority");
        return Some(route);
    }
    None
}

fn status_problem(status: StatusCode) -> Problem {
    let (kind, title, detail) = match status {
        StatusCode::BAD_REQUEST => (
            "invalid-request",
            "Invalid request",
            "The request could not be processed.",
        ),
        StatusCode::NOT_FOUND => (
            "resource-not-found",
            "Resource not found",
            "The requested resource does not exist.",
        ),
        StatusCode::METHOD_NOT_ALLOWED => (
            "method-not-allowed",
            "Method not allowed",
            "The requested method is not supported for this resource.",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "unsupported-media-type",
            "Unsupported media type",
            "The request media type is not supported.",
        ),
        StatusCode::TOO_MANY_REQUESTS => (
            "rate-limited",
            "Rate limited",
            "The request was rate limited.",
        ),
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            (
                "dependency-unavailable",
                "Dependency unavailable",
                "A required dependency is unavailable.",
            )
        }
        _ => ("internal-error", "Request failed", "The request failed."),
    };
    Problem::new(status, kind, title, detail)
}
