use std::io;

use async_compression::tokio::bufread::GzipDecoder;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CONTENT_ENCODING, CONTENT_TYPE, RETRY_AFTER},
    },
    response::Response,
    routing::post,
};
use flaggo_otlp_codec::decode_export_request;
use flaggo_raw_otlp_inbox::{
    DEFAULT_OTLP_PROFILE_VERSION, NewRawOtlpBatch, OtlpSignal, OtlpTransportCompression,
    OtlpWireEncoding, RawOtlpInboxError,
};
use futures_util::TryStreamExt;
use mime::Mime;
use prost::Message;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;

use crate::AppState;

const INVALID_ARGUMENT: i32 = 3;
const RESOURCE_EXHAUSTED: i32 = 8;
const UNAVAILABLE: i32 = 14;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/logs", post(ingest_logs))
        .route("/v1/metrics", post(ingest_metrics))
        .route("/v1/traces", post(ingest_traces))
}

async fn ingest_logs(State(state): State<AppState>, headers: HeaderMap, body: Body) -> Response {
    ingest(OtlpSignal::Logs, state, headers, body).await
}

async fn ingest_metrics(State(state): State<AppState>, headers: HeaderMap, body: Body) -> Response {
    ingest(OtlpSignal::Metrics, state, headers, body).await
}

async fn ingest_traces(State(state): State<AppState>, headers: HeaderMap, body: Body) -> Response {
    ingest(OtlpSignal::Traces, state, headers, body).await
}

async fn ingest(signal: OtlpSignal, state: AppState, headers: HeaderMap, body: Body) -> Response {
    let wire_encoding = match request_wire_encoding(&headers) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let transport_compression = match request_compression(&headers, wire_encoding) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let payload = match read_decompressed_body(
        body,
        transport_compression,
        state.receiver.maximum_decompressed_request_bytes(),
    )
    .await
    {
        Ok(value) => value,
        Err(BodyReadError::TooLarge) => {
            return OtlpHttpError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                RESOURCE_EXHAUSTED,
                "OTLP request body exceeds the configured decompressed-size limit.",
                wire_encoding,
            )
            .into_response();
        }
        Err(BodyReadError::Invalid) => {
            return OtlpHttpError::new(
                StatusCode::BAD_REQUEST,
                INVALID_ARGUMENT,
                "OTLP request body or compression is invalid.",
                wire_encoding,
            )
            .into_response();
        }
    };
    if !validate_export_request_shape(signal, wire_encoding, &payload) {
        return OtlpHttpError::new(
            StatusCode::BAD_REQUEST,
            INVALID_ARGUMENT,
            "Request body is not a valid export request for this OTLP signal.",
            wire_encoding,
        )
        .into_response();
    }

    let profile_version = DEFAULT_OTLP_PROFILE_VERSION
        .parse()
        .expect("built-in OTLP profile version must be valid");
    let batch = NewRawOtlpBatch::new(
        signal,
        wire_encoding,
        transport_compression,
        profile_version,
        payload,
    );
    match state.inbox.append(batch).await {
        Ok(_) => success_response(wire_encoding),
        Err(error @ RawOtlpInboxError::CapacityExceeded { .. }) => {
            eprintln!("Raw OTLP inbox rejected a request: {error:?}");
            OtlpHttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                RESOURCE_EXHAUSTED,
                "Raw OTLP inbox capacity is exhausted.",
                wire_encoding,
            )
            .retry_after()
            .into_response()
        }
        Err(error) => {
            eprintln!("Raw OTLP inbox append failed: {error:?}");
            OtlpHttpError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                UNAVAILABLE,
                "Raw OTLP inbox is unavailable.",
                wire_encoding,
            )
            .retry_after()
            .into_response()
        }
    }
}

fn request_wire_encoding(headers: &HeaderMap) -> Result<OtlpWireEncoding, OtlpHttpError> {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return Err(unsupported_media_type());
    };
    if values.next().is_some() {
        return Err(unsupported_media_type());
    }
    let value = value.to_str().map_err(|_| unsupported_media_type())?;
    let parsed = value
        .parse::<Mime>()
        .map_err(|_| unsupported_media_type())?;
    let encoding = if parsed.type_() == mime::APPLICATION && parsed.subtype() == mime::JSON {
        OtlpWireEncoding::ProtobufJson
    } else if parsed.type_() == mime::APPLICATION && parsed.subtype().as_str() == "x-protobuf" {
        OtlpWireEncoding::Protobuf
    } else {
        return Err(unsupported_media_type());
    };
    let parameters_are_valid = parsed.params().all(|(name, value)| {
        encoding == OtlpWireEncoding::ProtobufJson && name == mime::CHARSET && value == mime::UTF_8
    });
    if !parameters_are_valid {
        return Err(unsupported_media_type());
    }
    Ok(encoding)
}

fn request_compression(
    headers: &HeaderMap,
    response_encoding: OtlpWireEncoding,
) -> Result<OtlpTransportCompression, OtlpHttpError> {
    let mut values = headers.get_all(CONTENT_ENCODING).iter();
    let Some(value) = values.next() else {
        return Ok(OtlpTransportCompression::Identity);
    };
    if values.next().is_some() {
        return Err(unsupported_content_encoding(response_encoding));
    }
    let value = value
        .to_str()
        .map_err(|_| unsupported_content_encoding(response_encoding))?;
    if value.eq_ignore_ascii_case("identity") {
        Ok(OtlpTransportCompression::Identity)
    } else if value.eq_ignore_ascii_case("gzip") {
        Ok(OtlpTransportCompression::Gzip)
    } else {
        Err(unsupported_content_encoding(response_encoding))
    }
}

async fn read_decompressed_body(
    body: Body,
    compression: OtlpTransportCompression,
    maximum_bytes: usize,
) -> Result<Vec<u8>, BodyReadError> {
    let stream = body
        .into_data_stream()
        .map_err(|error| io::Error::other(error.to_string()));
    let reader = StreamReader::new(stream);
    match compression {
        OtlpTransportCompression::Identity => read_limited(reader, maximum_bytes).await,
        OtlpTransportCompression::Gzip => {
            let mut decoder = GzipDecoder::new(reader);
            decoder.multiple_members(true);
            read_limited(decoder, maximum_bytes).await
        }
    }
}

async fn read_limited(
    reader: impl AsyncRead + Unpin,
    maximum_bytes: usize,
) -> Result<Vec<u8>, BodyReadError> {
    let read_limit =
        u64::try_from(maximum_bytes).expect("configured request limit must fit u64") + 1;
    let mut reader = reader.take(read_limit);
    let mut payload = Vec::with_capacity(maximum_bytes.min(64 * 1024));
    reader
        .read_to_end(&mut payload)
        .await
        .map_err(|_| BodyReadError::Invalid)?;
    if payload.len() > maximum_bytes {
        return Err(BodyReadError::TooLarge);
    }
    Ok(payload)
}

fn validate_export_request_shape(
    signal: OtlpSignal,
    wire_encoding: OtlpWireEncoding,
    payload: &[u8],
) -> bool {
    decode_export_request(signal, wire_encoding, payload).is_ok()
}

fn success_response(encoding: OtlpWireEncoding) -> Response {
    let body = match encoding {
        OtlpWireEncoding::Protobuf => Vec::new(),
        OtlpWireEncoding::ProtobufJson => b"{}".to_vec(),
    };
    response(StatusCode::OK, encoding, body, false)
}

fn unsupported_media_type() -> OtlpHttpError {
    OtlpHttpError::new(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        INVALID_ARGUMENT,
        "Content-Type must be application/json or application/x-protobuf.",
        OtlpWireEncoding::ProtobufJson,
    )
}

fn unsupported_content_encoding(response_encoding: OtlpWireEncoding) -> OtlpHttpError {
    OtlpHttpError::new(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        INVALID_ARGUMENT,
        "Content-Encoding must be identity, gzip, or omitted.",
        response_encoding,
    )
}

fn response(
    status: StatusCode,
    encoding: OtlpWireEncoding,
    body: Vec<u8>,
    retry_after: bool,
) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(encoding.media_type()),
    );
    if retry_after {
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

struct OtlpHttpError {
    status: StatusCode,
    rpc_code: i32,
    message: &'static str,
    response_encoding: OtlpWireEncoding,
    retry_after: bool,
}

impl OtlpHttpError {
    const fn new(
        status: StatusCode,
        rpc_code: i32,
        message: &'static str,
        response_encoding: OtlpWireEncoding,
    ) -> Self {
        Self {
            status,
            rpc_code,
            message,
            response_encoding,
            retry_after: false,
        }
    }

    const fn retry_after(mut self) -> Self {
        self.retry_after = true;
        self
    }

    fn into_response(self) -> Response {
        let body = match self.response_encoding {
            OtlpWireEncoding::Protobuf => RpcStatus {
                code: self.rpc_code,
                message: self.message.to_owned(),
            }
            .encode_to_vec(),
            OtlpWireEncoding::ProtobufJson => serde_json::to_vec(&json!({
                "code": self.rpc_code,
                "message": self.message
            }))
            .expect("static google.rpc.Status JSON must serialize"),
        };
        response(self.status, self.response_encoding, body, self.retry_after)
    }
}

#[derive(Clone, PartialEq, Message)]
struct RpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
}

enum BodyReadError {
    TooLarge,
    Invalid,
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        num::{NonZeroU16, NonZeroU64},
        path::Path,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{
            Request, StatusCode,
            header::{CONTENT_ENCODING, CONTENT_TYPE, RETRY_AFTER},
        },
    };
    use flaggo_raw_otlp_inbox::{
        DEFAULT_OTLP_PROFILE_VERSION, InboxAppendReceipt, InboxBatchId, InboxClock,
        NewRawOtlpBatch, OtlpSignal, OtlpTransportCompression, OtlpWireEncoding, PayloadSha256,
        RawOtlpInbox, RawOtlpInboxBatch, RawOtlpInboxError, RawOtlpInboxHealth, RawOtlpInboxLimits,
        SqliteRawOtlpInbox, SystemInboxClock,
    };
    use flate2::{Compression, write::GzEncoder};
    use opentelemetry_proto::tonic::{
        collector::{
            logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
            trace::v1::ExportTraceServiceRequest,
        },
        logs::v1::ResourceLogs,
        metrics::v1::ResourceMetrics,
        trace::v1::ResourceSpans,
    };
    use prost::Message;
    use serde_json::Value;
    use tokio::sync::Notify;
    use tower::ServiceExt;

    use super::RpcStatus;
    use crate::{OtlpReceiverConfig, router};

    #[derive(Clone, Copy)]
    enum AppendBehavior {
        Success,
        Capacity,
        Unavailable,
    }

    struct TestInbox {
        append_behavior: AppendBehavior,
        append_started: Notify,
        batches: Mutex<Vec<NewRawOtlpBatch>>,
        block_append: bool,
        release_append: Notify,
    }

    impl TestInbox {
        fn new(append_behavior: AppendBehavior) -> Arc<Self> {
            Arc::new(Self {
                append_behavior,
                append_started: Notify::new(),
                batches: Mutex::new(Vec::new()),
                block_append: false,
                release_append: Notify::new(),
            })
        }

        fn blocking() -> Arc<Self> {
            Arc::new(Self {
                append_behavior: AppendBehavior::Success,
                append_started: Notify::new(),
                batches: Mutex::new(Vec::new()),
                block_append: true,
                release_append: Notify::new(),
            })
        }

        fn batches(&self) -> Vec<NewRawOtlpBatch> {
            self.batches.lock().expect("recorded batches lock").clone()
        }
    }

    #[async_trait]
    impl RawOtlpInbox for TestInbox {
        async fn append(
            &self,
            batch: NewRawOtlpBatch,
        ) -> Result<InboxAppendReceipt, RawOtlpInboxError> {
            self.append_started.notify_one();
            if self.block_append {
                self.release_append.notified().await;
            }
            match self.append_behavior {
                AppendBehavior::Capacity => {
                    return Err(RawOtlpInboxError::CapacityExceeded {
                        retained_payload_bytes: 10,
                        incoming_payload_bytes: batch.payload.len() as u64,
                        max_retained_payload_bytes: 10,
                    });
                }
                AppendBehavior::Unavailable => {
                    return Err(RawOtlpInboxError::Unavailable {
                        source: Box::new(io::Error::other("simulated storage failure")),
                    });
                }
                AppendBehavior::Success => {}
            }

            let payload_length = batch.payload.len() as u64;
            let mut batches = self.batches.lock().expect("recorded batches lock");
            batches.push(batch);
            let batch_id =
                NonZeroU64::new(batches.len() as u64).expect("recorded batch ID is nonzero");
            Ok(InboxAppendReceipt {
                inbox_batch_id: InboxBatchId::new(batch_id),
                received_at: SystemInboxClock.now(),
                payload_length,
                payload_sha256: PayloadSha256::new([0; 32]),
            })
        }

        async fn read_after(
            &self,
            _after: Option<InboxBatchId>,
            _limit: NonZeroU16,
        ) -> Result<Vec<RawOtlpInboxBatch>, RawOtlpInboxError> {
            Err(RawOtlpInboxError::CorruptData(
                "test inbox does not implement replay".to_owned(),
            ))
        }

        async fn inspect(&self) -> Result<RawOtlpInboxHealth, RawOtlpInboxError> {
            Ok(RawOtlpInboxHealth {
                retained_batch_count: self.batches().len() as u64,
                retained_payload_bytes: 0,
                oldest_retained_at: None,
                newest_retained_at: None,
                earliest_replay_at: None,
                expired_batch_count: 0,
                expired_payload_bytes: 0,
                limits: RawOtlpInboxLimits::default(),
            })
        }
    }

    #[tokio::test]
    async fn accepts_json_and_protobuf_for_all_three_signals() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let cases = [
            SignalCase {
                path: "/v1/logs",
                signal: OtlpSignal::Logs,
                json: logs_json(),
                protobuf: ExportLogsServiceRequest {
                    resource_logs: vec![ResourceLogs::default()],
                }
                .encode_to_vec(),
            },
            SignalCase {
                path: "/v1/metrics",
                signal: OtlpSignal::Metrics,
                json: metrics_json(),
                protobuf: ExportMetricsServiceRequest {
                    resource_metrics: vec![ResourceMetrics::default()],
                }
                .encode_to_vec(),
            },
            SignalCase {
                path: "/v1/traces",
                signal: OtlpSignal::Traces,
                json: traces_json(),
                protobuf: ExportTraceServiceRequest {
                    resource_spans: vec![ResourceSpans::default()],
                }
                .encode_to_vec(),
            },
        ];

        for case in &cases {
            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                case.path,
                Some("application/json; charset=utf-8"),
                None,
                case.json.to_vec(),
            )
            .await;
            assert_success(response, OtlpWireEncoding::ProtobufJson).await;
            let recorded = inbox.batches().pop().expect("recorded JSON batch");
            assert_batch(
                &recorded,
                case.signal,
                OtlpWireEncoding::ProtobufJson,
                OtlpTransportCompression::Identity,
                case.json,
            );

            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                case.path,
                Some("application/x-protobuf"),
                Some("identity"),
                case.protobuf.clone(),
            )
            .await;
            assert_success(response, OtlpWireEncoding::Protobuf).await;
            let recorded = inbox.batches().pop().expect("recorded protobuf batch");
            assert_batch(
                &recorded,
                case.signal,
                OtlpWireEncoding::Protobuf,
                OtlpTransportCompression::Identity,
                &case.protobuf,
            );
        }
    }

    #[tokio::test]
    async fn accepts_empty_export_messages() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        for path in ["/v1/logs", "/v1/metrics", "/v1/traces"] {
            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                path,
                Some("application/json"),
                None,
                b"{}".to_vec(),
            )
            .await;
            assert_success(response, OtlpWireEncoding::ProtobufJson).await;

            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                path,
                Some("application/x-protobuf"),
                None,
                Vec::new(),
            )
            .await;
            assert_success(response, OtlpWireEncoding::Protobuf).await;
        }
        assert_eq!(inbox.batches().len(), 6);
    }

    #[tokio::test]
    async fn accepts_original_protobuf_field_names_for_every_signal() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let cases = [
            (
                "/v1/logs",
                OtlpSignal::Logs,
                br#"{"resource_logs":[{"scope_logs":[{"log_records":[{"time_unix_nano":"1","body":{"string_value":"hello"}}]}]}]}"#
                    .as_slice(),
            ),
            (
                "/v1/metrics",
                OtlpSignal::Metrics,
                br#"{"resource_metrics":[{"scope_metrics":[{"metrics":[{"name":"load","gauge":{"data_points":[{"time_unix_nano":"1","as_int":"2"}]}}]}]}]}"#
                    .as_slice(),
            ),
            (
                "/v1/traces",
                OtlpSignal::Traces,
                br#"{"resource_spans":[{"scope_spans":[{"spans":[{"name":"work","start_time_unix_nano":"1","end_time_unix_nano":"2"}]}]}]}"#
                    .as_slice(),
            ),
        ];

        for (path, signal, payload) in cases {
            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                path,
                Some("application/json"),
                None,
                payload.to_vec(),
            )
            .await;
            assert_success(response, OtlpWireEncoding::ProtobufJson).await;
            let recorded = inbox.batches().pop().expect("recorded JSON batch");
            assert_batch(
                &recorded,
                signal,
                OtlpWireEncoding::ProtobufJson,
                OtlpTransportCompression::Identity,
                payload,
            );
        }
    }

    #[tokio::test]
    async fn rejects_ambiguous_protobuf_json_before_acknowledgement() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let cases = [
            (
                "/v1/logs",
                br#"{"resourceLogs":[],"resourceLogs":[]}"#.as_slice(),
            ),
            (
                "/v1/logs",
                br#"{"resourceLogs":[{"scopeLogs":[],"scope_logs":[]}]}"#.as_slice(),
            ),
            (
                "/v1/metrics",
                br#"{"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"load","gauge":{"dataPoints":[{"asInt":"1","asDouble":2.0}]}}]}]}]}"#
                    .as_slice(),
            ),
        ];

        for (path, payload) in cases {
            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                path,
                Some("application/json"),
                None,
                payload.to_vec(),
            )
            .await;
            assert_otlp_status(
                response,
                StatusCode::BAD_REQUEST,
                3,
                OtlpWireEncoding::ProtobufJson,
            )
            .await;
        }
        assert!(inbox.batches().is_empty());
    }

    #[tokio::test]
    async fn decompresses_gzip_before_validation_and_persistence() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let payload = logs_json();

        let response = send(
            inbox.clone(),
            OtlpReceiverConfig::default(),
            "/v1/logs",
            Some("application/json"),
            Some("GZip"),
            gzip(payload),
        )
        .await;

        assert_success(response, OtlpWireEncoding::ProtobufJson).await;
        let recorded = inbox.batches().pop().expect("recorded gzip batch");
        assert_batch(
            &recorded,
            OtlpSignal::Logs,
            OtlpWireEncoding::ProtobufJson,
            OtlpTransportCompression::Gzip,
            payload,
        );
    }

    #[tokio::test]
    async fn stores_mixed_authority_resources_as_one_opaque_batch() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let payload = mixed_authority_logs_json();

        let response = send(
            inbox.clone(),
            OtlpReceiverConfig::default(),
            "/v1/logs",
            Some("application/json"),
            None,
            payload.to_vec(),
        )
        .await;

        assert_success(response, OtlpWireEncoding::ProtobufJson).await;
        let batches = inbox.batches();
        assert_eq!(batches.len(), 1);
        assert_batch(
            &batches[0],
            OtlpSignal::Logs,
            OtlpWireEncoding::ProtobufJson,
            OtlpTransportCompression::Identity,
            payload,
        );
    }

    #[tokio::test]
    async fn persists_accepted_request_through_sqlite_inbox_contract() {
        let directory = tempfile::tempdir().expect("temporary inbox directory");
        let database_url = sqlite_url(&directory.path().join("inbox.db"));
        let inbox = Arc::new(
            SqliteRawOtlpInbox::connect(&database_url, RawOtlpInboxLimits::default())
                .await
                .expect("SQLite inbox"),
        );
        let payload = metrics_json();
        let response = router(inbox.clone(), OtlpReceiverConfig::default())
            .oneshot(request(
                "/v1/metrics",
                Some("application/json"),
                Some("gzip"),
                gzip(payload),
            ))
            .await
            .expect("receiver response");

        assert_success(response, OtlpWireEncoding::ProtobufJson).await;
        let batches = inbox
            .read_after(None, NonZeroU16::new(1).expect("nonzero replay limit"))
            .await
            .expect("inbox replay");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].signal, OtlpSignal::Metrics);
        assert_eq!(batches[0].wire_encoding, OtlpWireEncoding::ProtobufJson);
        assert_eq!(
            batches[0].transport_compression,
            OtlpTransportCompression::Gzip
        );
        assert_eq!(batches[0].payload, payload);
        inbox.close().await;
    }

    #[tokio::test]
    async fn enforces_limit_after_decompression() {
        let accepted_inbox = TestInbox::new(AppendBehavior::Success);
        let accepted_payload = logs_json();
        let response = send(
            accepted_inbox.clone(),
            OtlpReceiverConfig::new(accepted_payload.len()).expect("receiver limit"),
            "/v1/logs",
            Some("application/json"),
            None,
            accepted_payload.to_vec(),
        )
        .await;
        assert_success(response, OtlpWireEncoding::ProtobufJson).await;
        assert_eq!(accepted_inbox.batches().len(), 1);

        let rejected_inbox = TestInbox::new(AppendBehavior::Success);
        let response = send(
            rejected_inbox.clone(),
            OtlpReceiverConfig::new(16).expect("receiver limit"),
            "/v1/logs",
            Some("application/json"),
            Some("gzip"),
            gzip(&[b' '; 128]),
        )
        .await;

        assert_otlp_status(
            response,
            StatusCode::PAYLOAD_TOO_LARGE,
            8,
            OtlpWireEncoding::ProtobufJson,
        )
        .await;
        assert!(rejected_inbox.batches().is_empty());
    }

    #[tokio::test]
    async fn rejects_unsupported_transport_and_signal_payloads() {
        let inbox = TestInbox::new(AppendBehavior::Success);
        let cases = [
            (
                "/v1/logs",
                None,
                None,
                logs_json().to_vec(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                OtlpWireEncoding::ProtobufJson,
            ),
            (
                "/v1/logs",
                Some("text/plain"),
                None,
                logs_json().to_vec(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                OtlpWireEncoding::ProtobufJson,
            ),
            (
                "/v1/logs",
                Some("application/json"),
                Some("br"),
                logs_json().to_vec(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                OtlpWireEncoding::ProtobufJson,
            ),
            (
                "/v1/logs",
                Some("application/json"),
                None,
                br#"{"resourceMetrics":[]}"#.to_vec(),
                StatusCode::BAD_REQUEST,
                OtlpWireEncoding::ProtobufJson,
            ),
            (
                "/v1/logs",
                Some("application/x-protobuf"),
                None,
                vec![0xff],
                StatusCode::BAD_REQUEST,
                OtlpWireEncoding::Protobuf,
            ),
            (
                "/v1/logs",
                Some("application/json"),
                Some("gzip"),
                b"not-gzip".to_vec(),
                StatusCode::BAD_REQUEST,
                OtlpWireEncoding::ProtobufJson,
            ),
        ];

        for (path, content_type, content_encoding, body, expected_status, expected_encoding) in
            cases
        {
            let response = send(
                inbox.clone(),
                OtlpReceiverConfig::default(),
                path,
                content_type,
                content_encoding,
                body,
            )
            .await;
            assert_otlp_status(response, expected_status, 3, expected_encoding).await;
        }
        assert!(inbox.batches().is_empty());
    }

    #[tokio::test]
    async fn maps_capacity_and_storage_failures_to_retryable_otlp_statuses() {
        let capacity = TestInbox::new(AppendBehavior::Capacity);
        let response = send(
            capacity,
            OtlpReceiverConfig::default(),
            "/v1/logs",
            Some("application/x-protobuf"),
            None,
            ExportLogsServiceRequest::default().encode_to_vec(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/x-protobuf");
        assert_eq!(response.headers()[RETRY_AFTER], "1");
        let status = RpcStatus::decode(response_bytes(response).await.as_slice())
            .expect("protobuf google.rpc.Status");
        assert_eq!(status.code, 8);

        let unavailable = TestInbox::new(AppendBehavior::Unavailable);
        let response = send(
            unavailable,
            OtlpReceiverConfig::default(),
            "/v1/logs",
            Some("application/json"),
            None,
            br#"{"resourceLogs":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(response.headers()[RETRY_AFTER], "1");
        assert_otlp_status(
            response,
            StatusCode::SERVICE_UNAVAILABLE,
            14,
            OtlpWireEncoding::ProtobufJson,
        )
        .await;
    }

    #[tokio::test]
    async fn does_not_acknowledge_before_inbox_append_completes() {
        let inbox = TestInbox::blocking();
        let app = test_router(inbox.clone(), OtlpReceiverConfig::default());
        let request = request(
            "/v1/logs",
            Some("application/json"),
            None,
            br#"{"resourceLogs":[]}"#.to_vec(),
        );
        let response_task =
            tokio::spawn(async move { app.oneshot(request).await.expect("receiver response") });

        inbox.append_started.notified().await;
        tokio::task::yield_now().await;
        assert!(!response_task.is_finished());
        inbox.release_append.notify_one();
        let response = response_task.await.expect("receiver task");
        assert_success(response, OtlpWireEncoding::ProtobufJson).await;
    }

    struct SignalCase {
        path: &'static str,
        signal: OtlpSignal,
        json: &'static [u8],
        protobuf: Vec<u8>,
    }

    fn test_router(inbox: Arc<TestInbox>, config: OtlpReceiverConfig) -> Router {
        router(inbox, config)
    }

    async fn send(
        inbox: Arc<TestInbox>,
        config: OtlpReceiverConfig,
        path: &str,
        content_type: Option<&str>,
        content_encoding: Option<&str>,
        body: Vec<u8>,
    ) -> axum::response::Response {
        test_router(inbox, config)
            .oneshot(request(path, content_type, content_encoding, body))
            .await
            .expect("receiver response")
    }

    fn request(
        path: &str,
        content_type: Option<&str>,
        content_encoding: Option<&str>,
        body: Vec<u8>,
    ) -> Request<Body> {
        let mut builder = Request::builder().method("POST").uri(path);
        if let Some(value) = content_type {
            builder = builder.header(CONTENT_TYPE, value);
        }
        if let Some(value) = content_encoding {
            builder = builder.header(CONTENT_ENCODING, value);
        }
        builder.body(Body::from(body)).expect("valid request")
    }

    async fn assert_success(response: axum::response::Response, encoding: OtlpWireEncoding) {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], encoding.media_type());
        let body = response_bytes(response).await;
        match encoding {
            OtlpWireEncoding::Protobuf => assert!(body.is_empty()),
            OtlpWireEncoding::ProtobufJson => assert_eq!(body.as_slice(), b"{}"),
        }
    }

    async fn assert_otlp_status(
        response: axum::response::Response,
        expected_status: StatusCode,
        expected_code: i32,
        expected_encoding: OtlpWireEncoding,
    ) {
        assert_eq!(response.status(), expected_status);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            expected_encoding.media_type()
        );
        let body = response_bytes(response).await;
        match expected_encoding {
            OtlpWireEncoding::Protobuf => {
                let status =
                    RpcStatus::decode(body.as_slice()).expect("protobuf google.rpc.Status");
                assert_eq!(status.code, expected_code);
                assert!(!status.message.is_empty());
            }
            OtlpWireEncoding::ProtobufJson => {
                let body: Value = serde_json::from_slice(&body).expect("JSON status");
                assert_eq!(body["code"], i64::from(expected_code));
                assert!(
                    body["message"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                );
            }
        }
    }

    async fn response_bytes(response: axum::response::Response) -> Vec<u8> {
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("bounded response body")
            .to_vec()
    }

    fn assert_batch(
        batch: &NewRawOtlpBatch,
        signal: OtlpSignal,
        wire_encoding: OtlpWireEncoding,
        compression: OtlpTransportCompression,
        payload: &[u8],
    ) {
        assert_eq!(batch.signal, signal);
        assert_eq!(batch.wire_encoding, wire_encoding);
        assert_eq!(batch.transport_compression, compression);
        assert_eq!(
            batch.otlp_profile_version.as_str(),
            DEFAULT_OTLP_PROFILE_VERSION
        );
        assert_eq!(batch.payload, payload);
    }

    fn gzip(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(payload).expect("gzip input");
        encoder.finish().expect("gzip payload")
    }

    fn sqlite_url(path: &Path) -> String {
        format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
    }

    fn logs_json() -> &'static [u8] {
        br#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"timeUnixNano":"1770000000000000000","body":{"stringValue":"hello"},"attributes":[{"key":"bytes","value":{"bytesValue":"AQID"}}]}]}]}]}"#
    }

    fn mixed_authority_logs_json() -> &'static [u8] {
        br#"{"resourceLogs":[{"resource":{"attributes":[{"key":"flaggo.tenant","value":{"stringValue":"tenant-a"}},{"key":"flaggo.application","value":{"stringValue":"worker-a"}},{"key":"flaggo.environment","value":{"stringValue":"production"}}]},"scopeLogs":[{"scope":{"name":"worker-a.app"},"logRecords":[{"body":{"stringValue":"observation-a"}}]}]},{"resource":{"attributes":[{"key":"flaggo.tenant","value":{"stringValue":"tenant-b"}},{"key":"flaggo.application","value":{"stringValue":"worker-b"}},{"key":"flaggo.environment","value":{"stringValue":"staging"}}]},"scopeLogs":[{"scope":{"name":"worker-b.app"},"logRecords":[{"body":{"stringValue":"observation-b"}}]}]}]}"#
    }

    fn metrics_json() -> &'static [u8] {
        br#"{"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"test.metric","gauge":{"dataPoints":[{"timeUnixNano":"1770000000000000000","asDouble":0.5}]}}]}]}]}"#
    }

    fn traces_json() -> &'static [u8] {
        br#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"5b8efff798038103d269b633813fc60c","spanId":"eee19b7ec3c1b174","name":"test.span","startTimeUnixNano":"1770000000000000000","endTimeUnixNano":"1770000001000000000"}]}]}]}"#
    }
}
