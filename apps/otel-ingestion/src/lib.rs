use std::{env, net::SocketAddr};

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:5090";
pub const LISTEN_ADDRESS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS";
pub const SERVICE_NAME: &str = "flaggo-otel-ingestion";

pub fn router() -> Router {
    Router::new().route("/health/live", get(liveness))
}

pub fn configured_listen_address() -> Result<SocketAddr, String> {
    let value = env::var(LISTEN_ADDRESS_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_LISTEN_ADDRESS.to_owned());
    parse_listen_address(&value)
}

pub fn parse_listen_address(value: &str) -> Result<SocketAddr, String> {
    value.parse::<SocketAddr>().map_err(|error| {
        format!("{LISTEN_ADDRESS_ENVIRONMENT_VARIABLE} must be an IP socket address: {error}")
    })
}

async fn liveness() -> Json<Value> {
    Json(json!({
        "service": SERVICE_NAME,
        "status": "live",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::{SERVICE_NAME, parse_listen_address, router};

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

    #[tokio::test]
    async fn reports_liveness() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 16_384)
            .await
            .expect("bounded response body");
        let payload: Value = serde_json::from_slice(&body).expect("JSON response");
        assert_eq!(payload["service"], SERVICE_NAME);
        assert_eq!(payload["status"], "live");
    }
}
