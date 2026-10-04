use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use flaggo_raw_otlp_inbox::{RawOtlpInbox, RawOtlpInboxLimits};
use serde_json::{Value, json};

mod config;
mod receiver;

pub use config::{
    DATABASE_URL_ENVIRONMENT_VARIABLE, DEFAULT_DATABASE_URL, DEFAULT_LISTEN_ADDRESS,
    DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES, INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE,
    INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE, LISTEN_ADDRESS_ENVIRONMENT_VARIABLE,
    MAXIMUM_DECOMPRESSED_REQUEST_BYTES_ENVIRONMENT_VARIABLE, OtlpReceiverConfig,
    configured_database_url, configured_inbox_limits, configured_listen_address,
    configured_receiver, parse_inbox_limits, parse_listen_address, parse_receiver_config,
};

pub const SERVICE_NAME: &str = "flaggo-otel-ingestion";

#[derive(Clone)]
pub(crate) struct AppState {
    inbox: Arc<dyn RawOtlpInbox>,
    inbox_limits: RawOtlpInboxLimits,
    receiver: OtlpReceiverConfig,
}

pub fn router(
    inbox: Arc<dyn RawOtlpInbox>,
    receiver: OtlpReceiverConfig,
    inbox_limits: RawOtlpInboxLimits,
) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .merge(receiver::routes())
        .with_state(AppState {
            inbox,
            inbox_limits,
            receiver,
        })
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
                    "hardRetentionSeconds": state.inbox_limits.hard_retention().as_secs(),
                    "maximumRetainedPayloadBytes": state
                        .inbox_limits
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
        InboxAppendReceipt, InboxBatchId, NewRawOtlpBatch, RawOtlpInbox, RawOtlpInboxBacklog,
        RawOtlpInboxBatch, RawOtlpInboxError, RawOtlpInboxHealth, RawOtlpInboxLimits,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::{OtlpReceiverConfig, SERVICE_NAME, router};

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
            })
        }

        async fn inspect_after(
            &self,
            _after: Option<InboxBatchId>,
        ) -> Result<RawOtlpInboxBacklog, RawOtlpInboxError> {
            Err(not_implemented())
        }
    }

    #[tokio::test]
    async fn reports_liveness() {
        let response = router(
            stub_inbox(true),
            OtlpReceiverConfig::default(),
            RawOtlpInboxLimits::default(),
        )
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
        let response = router(
            stub_inbox(true),
            OtlpReceiverConfig::default(),
            RawOtlpInboxLimits::default(),
        )
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
        let response = router(
            stub_inbox(false),
            OtlpReceiverConfig::default(),
            RawOtlpInboxLimits::default(),
        )
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
