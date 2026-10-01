use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use flaggo_raw_otlp_inbox::{RawOtlpInbox, RawOtlpInboxLimits};
use serde_json::{Value, json};

pub const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_DATABASE_URL";
pub const DEFAULT_DATABASE_URL: &str = "sqlite://flaggo.db";
pub const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:5090";
pub const INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_OTLP_INBOX_HARD_RETENTION_SECONDS";
pub const INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_OTLP_INBOX_MAX_PAYLOAD_BYTES";
pub const LISTEN_ADDRESS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS";
pub const SERVICE_NAME: &str = "flaggo-otel-ingestion";

#[derive(Clone)]
struct AppState {
    inbox: Arc<dyn RawOtlpInbox>,
}

pub fn router(inbox: Arc<dyn RawOtlpInbox>) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .with_state(AppState { inbox })
}

pub fn configured_listen_address() -> Result<SocketAddr, String> {
    let value = env::var(LISTEN_ADDRESS_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_LISTEN_ADDRESS.to_owned());
    parse_listen_address(&value)
}

pub fn configured_database_url() -> Result<String, String> {
    let value = env::var(DATABASE_URL_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
    if value.trim().is_empty() {
        return Err(format!(
            "{DATABASE_URL_ENVIRONMENT_VARIABLE} must not be empty"
        ));
    }
    Ok(value)
}

pub fn configured_inbox_limits() -> Result<RawOtlpInboxLimits, String> {
    let defaults = RawOtlpInboxLimits::default();
    let maximum = env::var(INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| defaults.max_retained_payload_bytes().to_string());
    let retention = env::var(INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| defaults.hard_retention().as_secs().to_string());
    parse_inbox_limits(&maximum, &retention)
}

pub fn parse_listen_address(value: &str) -> Result<SocketAddr, String> {
    value.parse::<SocketAddr>().map_err(|error| {
        format!("{LISTEN_ADDRESS_ENVIRONMENT_VARIABLE} must be an IP socket address: {error}")
    })
}

pub fn parse_inbox_limits(
    maximum_payload_bytes: &str,
    hard_retention_seconds: &str,
) -> Result<RawOtlpInboxLimits, String> {
    let maximum_payload_bytes = maximum_payload_bytes.parse::<u64>().map_err(|error| {
        format!("{INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE} must be an integer: {error}")
    })?;
    let hard_retention_seconds = hard_retention_seconds.parse::<u64>().map_err(|error| {
        format!("{INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE} must be an integer: {error}")
    })?;
    RawOtlpInboxLimits::new(
        maximum_payload_bytes,
        Duration::from_secs(hard_retention_seconds),
    )
    .map_err(|error| format!("invalid raw OTLP inbox limits: {error}"))
}

async fn liveness() -> Json<Value> {
    Json(json!({
        "service": SERVICE_NAME,
        "status": "live",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn readiness(State(state): State<AppState>) -> Response {
    match state.inbox.inspect().await {
        Ok(health) => (
            StatusCode::OK,
            Json(json!({
                "inbox": {
                    "earliestReplayAt": health
                        .earliest_replay_at
                        .map(|value| value.to_rfc3339()),
                    "expiredBatchCount": health.expired_batch_count,
                    "expiredPayloadBytes": health.expired_payload_bytes,
                    "hardRetentionSeconds": health.limits.hard_retention().as_secs(),
                    "maximumRetainedPayloadBytes": health
                        .limits
                        .max_retained_payload_bytes(),
                    "newestRetainedAt": health
                        .newest_retained_at
                        .map(|value| value.to_rfc3339()),
                    "oldestRetainedAt": health
                        .oldest_retained_at
                        .map(|value| value.to_rfc3339()),
                    "retainedBatchCount": health.retained_batch_count,
                    "retainedPayloadBytes": health.retained_payload_bytes
                },
                "service": SERVICE_NAME,
                "status": "ready",
                "version": env!("CARGO_PKG_VERSION")
            })),
        )
            .into_response(),
        Err(error) => {
            eprintln!("Raw OTLP inbox readiness check failed: {error:?}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "service": SERVICE_NAME,
                    "status": "not_ready",
                    "version": env!("CARGO_PKG_VERSION")
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU16, sync::Arc};

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use flaggo_raw_otlp_inbox::{
        InboxAppendReceipt, InboxBatchId, NewRawOtlpBatch, RawOtlpInbox, RawOtlpInboxBatch,
        RawOtlpInboxError, RawOtlpInboxHealth, RawOtlpInboxLimits,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::{SERVICE_NAME, parse_inbox_limits, parse_listen_address, router};

    struct StubInbox {
        available: bool,
    }

    #[async_trait]
    impl RawOtlpInbox for StubInbox {
        async fn append(
            &self,
            _batch: NewRawOtlpBatch,
        ) -> Result<InboxAppendReceipt, RawOtlpInboxError> {
            Err(not_implemented())
        }

        async fn read_after(
            &self,
            _after: Option<InboxBatchId>,
            _limit: NonZeroU16,
        ) -> Result<Vec<RawOtlpInboxBatch>, RawOtlpInboxError> {
            Err(not_implemented())
        }

        async fn inspect(&self) -> Result<RawOtlpInboxHealth, RawOtlpInboxError> {
            if !self.available {
                return Err(RawOtlpInboxError::CorruptData(
                    "simulated unavailable inbox".to_owned(),
                ));
            }
            Ok(RawOtlpInboxHealth {
                retained_batch_count: 2,
                retained_payload_bytes: 42,
                oldest_retained_at: None,
                newest_retained_at: None,
                earliest_replay_at: None,
                expired_batch_count: 1,
                expired_payload_bytes: 10,
                limits: RawOtlpInboxLimits::default(),
            })
        }
    }

    #[test]
    fn parses_ip_socket_listen_addresses() {
        let address = parse_listen_address("127.0.0.1:5090").expect("valid address");

        assert_eq!(address.ip().to_string(), "127.0.0.1");
        assert_eq!(address.port(), 5090);
    }

    #[test]
    fn rejects_non_socket_listen_addresses() {
        let error = parse_listen_address("http://127.0.0.1:5090")
            .expect_err("URL must not be accepted as a socket address");

        assert!(error.contains("must be an IP socket address"));
    }

    #[test]
    fn parses_and_validates_inbox_limits() {
        let limits = parse_inbox_limits("2048", "60").expect("valid limits");
        assert_eq!(limits.max_retained_payload_bytes(), 2048);
        assert_eq!(limits.hard_retention().as_secs(), 60);

        assert!(parse_inbox_limits("0", "60").is_err());
        assert!(parse_inbox_limits("2048", "0").is_err());
        assert!(parse_inbox_limits("many", "60").is_err());
    }

    #[tokio::test]
    async fn reports_liveness() {
        let response = router(stub_inbox(true))
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["service"], SERVICE_NAME);
        assert_eq!(payload["status"], "live");
    }

    #[tokio::test]
    async fn reports_inbox_readiness_and_capacity_health() {
        let response = router(stub_inbox(true))
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["status"], "ready");
        assert_eq!(payload["inbox"]["retainedBatchCount"], 2);
        assert_eq!(payload["inbox"]["retainedPayloadBytes"], 42);
        assert_eq!(payload["inbox"]["expiredBatchCount"], 1);
        assert_eq!(payload["inbox"]["hardRetentionSeconds"], 86_400);
    }

    #[tokio::test]
    async fn reports_unavailable_inbox_as_not_ready() {
        let response = router(stub_inbox(false))
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload = response_json(response).await;
        assert_eq!(payload["status"], "not_ready");
    }

    fn stub_inbox(available: bool) -> Arc<dyn RawOtlpInbox> {
        Arc::new(StubInbox { available })
    }

    fn not_implemented() -> RawOtlpInboxError {
        RawOtlpInboxError::CorruptData("stub operation is not implemented".to_owned())
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), 16_384)
            .await
            .expect("bounded response body");
        serde_json::from_slice(&body).expect("JSON response")
    }
}
