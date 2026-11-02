//! Real OTLP/HTTP capture shared by the independently hosted Rust service gates.
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
        trace::v1::ExportTraceServiceRequest,
    },
    common::v1::{KeyValue, any_value},
};
use prost::Message as _;

#[derive(Default, Debug)]
pub struct Capture {
    pub traces: Vec<ExportTraceServiceRequest>,
    pub metrics: Vec<ExportMetricsServiceRequest>,
    pub logs: Vec<ExportLogsServiceRequest>,
}

pub struct Collector {
    pub endpoint: String,
    pub received: Arc<Mutex<Capture>>,
    server: tokio::task::JoinHandle<()>,
}

impl Collector {
    pub async fn start() -> Self {
        let received = Arc::new(Mutex::new(Capture::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/{signal}", post(collect))
            .with_state(received.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            endpoint,
            received,
            server,
        }
    }

    pub async fn close(self) {
        self.server.abort();
        assert!(self.server.await.unwrap_err().is_cancelled());
    }
}

async fn collect(
    State(received): State<Arc<Mutex<Capture>>>,
    Path(signal): Path<String>,
    headers: HeaderMap,
    payload: Bytes,
) -> StatusCode {
    assert_eq!(headers["content-type"], "application/x-protobuf");
    let mut received = received.lock().unwrap();
    match signal.as_str() {
        "traces" => received
            .traces
            .push(ExportTraceServiceRequest::decode(payload).unwrap()),
        "metrics" => received
            .metrics
            .push(ExportMetricsServiceRequest::decode(payload).unwrap()),
        "logs" => received
            .logs
            .push(ExportLogsServiceRequest::decode(payload).unwrap()),
        _ => panic!("unexpected signal {signal}"),
    }
    StatusCode::OK
}

pub fn string(attributes: &[KeyValue], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|value| value.key == name)
        .and_then(|value| match value.value.as_ref()?.value.as_ref()? {
            any_value::Value::StringValue(value) => Some(value.clone()),
            _ => None,
        })
}
