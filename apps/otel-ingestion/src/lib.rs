use std::{sync::Arc, time::SystemTime};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use flaggo_raw_otlp_inbox::{RawOtlpInbox, RawOtlpInboxLimits};
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram},
};
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
pub const INSTRUMENTATION_SCOPE: &str = "flaggo.otel-ingestion";

#[derive(Clone)]
pub(crate) struct AppState {
    inbox: Arc<dyn RawOtlpInbox>,
    inbox_limits: RawOtlpInboxLimits,
    observability: OtlpIngestionObservability,
    receiver: OtlpReceiverConfig,
}

pub fn router(
    inbox: Arc<dyn RawOtlpInbox>,
    receiver: OtlpReceiverConfig,
    inbox_limits: RawOtlpInboxLimits,
) -> Router {
    instrumented_router(
        inbox,
        receiver,
        inbox_limits,
        OtlpIngestionObservability::new(),
    )
}

pub fn instrumented_router(
    inbox: Arc<dyn RawOtlpInbox>,
    receiver: OtlpReceiverConfig,
    inbox_limits: RawOtlpInboxLimits,
    observability: OtlpIngestionObservability,
) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .merge(receiver::routes())
        .with_state(AppState {
            inbox,
            inbox_limits,
            observability,
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
        Ok(health) => {
            state.observability.record_health(&health);
            (
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
                .into_response()
        }
        Err(error) => {
            tracing::event!(
                target: INSTRUMENTATION_SCOPE,
                tracing::Level::ERROR,
                {
                    "error.type" = std::any::type_name_of_val(&error),
                    "flaggo.failure.category" = "dependency",
                },
                "Raw OTLP inbox readiness check failed"
            );
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

#[derive(Clone)]
pub struct OtlpIngestionObservability {
    requests: Counter<u64>,
    request_size: Histogram<u64>,
    retained_batches: Gauge<u64>,
    retained_bytes: Gauge<u64>,
    oldest_age: Gauge<f64>,
    expired_batches: Counter<u64>,
    expired_bytes: Counter<u64>,
}

impl OtlpIngestionObservability {
    #[must_use]
    pub fn new() -> Self {
        let meter = flaggo_service_observability::ServiceObservability::meter(
            INSTRUMENTATION_SCOPE,
            env!("CARGO_PKG_VERSION"),
        );
        Self {
            requests: meter
                .u64_counter("flaggo.otlp.requests")
                .with_unit("{request}")
                .build(),
            request_size: meter
                .u64_histogram("flaggo.otlp.request.size")
                .with_unit("By")
                .build(),
            retained_batches: meter
                .u64_gauge("flaggo.inbox.retained.batches")
                .with_unit("{batch}")
                .build(),
            retained_bytes: meter
                .u64_gauge("flaggo.inbox.retained.bytes")
                .with_unit("By")
                .build(),
            oldest_age: meter
                .f64_gauge("flaggo.inbox.oldest.age")
                .with_unit("s")
                .build(),
            expired_batches: meter
                .u64_counter("flaggo.inbox.expired.batches")
                .with_unit("{batch}")
                .build(),
            expired_bytes: meter
                .u64_counter("flaggo.inbox.expired.bytes")
                .with_unit("By")
                .build(),
        }
    }

    pub(crate) fn record_request(
        &self,
        signal: &str,
        encoding: &str,
        compression: &str,
        outcome: &str,
        failure_category: Option<&str>,
        payload_size: Option<u64>,
    ) {
        let mut attributes = vec![
            KeyValue::new("flaggo.otlp.signal", signal.to_owned()),
            KeyValue::new("flaggo.otlp.encoding", encoding.to_owned()),
            KeyValue::new("flaggo.otlp.compression", compression.to_owned()),
            KeyValue::new("flaggo.operation.outcome", outcome.to_owned()),
        ];
        if let Some(category) = failure_category {
            attributes.push(KeyValue::new(
                "flaggo.failure.category",
                category.to_owned(),
            ));
        }
        self.requests.add(1, &attributes);
        if let Some(size) = payload_size {
            self.request_size.record(size, &attributes);
        }
    }

    pub fn record_health(&self, health: &flaggo_raw_otlp_inbox::RawOtlpInboxHealth) {
        self.retained_batches
            .record(health.retained_batch_count, &[]);
        self.retained_bytes
            .record(health.retained_payload_bytes, &[]);
        let age = health.oldest_retained_at.map_or(0.0, |oldest| {
            SystemTime::now()
                .duration_since(oldest.into())
                .unwrap_or_default()
                .as_secs_f64()
        });
        self.oldest_age.record(age, &[]);
    }

    pub fn record_retention(&self, result: flaggo_raw_otlp_inbox::RawOtlpInboxRetentionResult) {
        self.expired_batches.add(result.expired_batch_count, &[]);
        self.expired_bytes.add(result.expired_payload_bytes, &[]);
    }
}

impl Default for OtlpIngestionObservability {
    fn default() -> Self {
        Self::new()
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
