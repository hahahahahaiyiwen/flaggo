use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::post,
};
use flaggo_contract::{ContractError, EvidenceSource, RuntimeInput, strict_json};
use flaggo_service_hosting::{CORRELATION_HEADER, HttpJsonError, read_json, with_http_contract};
use serde_json::Value;
use tower::ServiceExt as _;

fn headers(content_type: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", content_type.parse().unwrap());
    headers
}

#[tokio::test]
async fn media_type_matches_the_reference_and_does_not_accept_json_suffixes() {
    for content_type in [
        "application/json",
        "Application/JSON",
        "application/json; charset=utf-8",
        "application/json; charset=ignored",
        "application/json;",
    ] {
        let value: Value = read_json(&headers(content_type), Body::from("{}"))
            .await
            .unwrap();
        assert_eq!(value, serde_json::json!({}));
    }
    for content_type in ["text/plain", "application/problem+json", ""] {
        assert!(
            matches!(
                read_json::<Value>(&headers(content_type), Body::from("{}")).await,
                Err(HttpJsonError::Invalid(problem)) if problem.status == StatusCode::UNSUPPORTED_MEDIA_TYPE
            ),
            "{content_type}"
        );
    }
    assert!(matches!(
        read_json::<Value>(&HeaderMap::new(), Body::empty()).await,
        Err(HttpJsonError::Invalid(problem)) if problem.status == StatusCode::UNSUPPORTED_MEDIA_TYPE
    ));
}

#[tokio::test]
async fn bounds_known_and_streamed_body_sizes_at_the_exact_document_limit() {
    let mut body = b"{}".to_vec();
    body.resize(strict_json::MAXIMUM_DOCUMENT_BYTES, b' ');
    read_json::<Value>(&headers("application/json"), Body::from(body.clone()))
        .await
        .unwrap();
    body.push(b' ');
    for content_length in [None, Some(body.len())] {
        let mut headers = headers("application/json");
        if let Some(length) = content_length {
            headers.insert("content-length", length.into());
        }
        assert!(matches!(
            read_json::<Value>(&headers, Body::from(body.clone())).await,
            Err(HttpJsonError::Invalid(problem))
                if problem.status == StatusCode::BAD_REQUEST
                && problem.detail == "Request body exceeds the maximum size."
        ));
    }
}

#[tokio::test]
async fn only_json_faults_become_invalid_body_problems() {
    for body in [
        "{",
        "null",
        r#"{"attributes":{},"unknown":0}"#,
        r#"{"attributes":{},"attributes":{}}"#,
    ] {
        assert!(
            matches!(
                read_json::<RuntimeInput>(&headers("application/json"), Body::from(body)).await,
                Err(HttpJsonError::Invalid(problem))
                    if problem.status == StatusCode::BAD_REQUEST
                    && problem.detail.starts_with("Request body is invalid: ")
            ),
            "{body}"
        );
    }
    assert!(matches!(
        read_json::<Value>(&headers("application/json"), Body::from(r#""\ud800""#)).await,
        Err(HttpJsonError::Native(ContractError::InvalidUnicode(_)))
    ));
    assert!(matches!(
        read_json::<EvidenceSource>(
            &headers("application/json"),
            Body::from(r#"{"stream":"x"}"#)
        )
        .await,
        Err(HttpJsonError::Native(
            ContractError::MissingTypeDiscriminator
        ))
    ));
}

#[tokio::test]
async fn problem_status_pages_preserve_allow_and_correlation_without_inventing_a_route() {
    let router = with_http_contract(
        Router::new().route("/example", post(|| async { "ok" })),
        "test",
        "0.1.0",
        &["/example"],
    );
    for (path, status, kind) in [
        ("/missing", StatusCode::NOT_FOUND, "resource-not-found"),
        (
            "/example",
            StatusCode::METHOD_NOT_ALLOWED,
            "method-not-allowed",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(CORRELATION_HEADER, "test-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[CORRELATION_HEADER], "test-123");
        assert_eq!(
            response.headers()["content-type"],
            "application/problem+json"
        );
        if status == StatusCode::METHOD_NOT_ALLOWED {
            assert_eq!(response.headers()["allow"], "POST");
        }
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value["type"], format!("https://flaggo.dev/problems/{kind}"));
        assert_eq!(value["instance"], path);
        assert_eq!(value["status"], status.as_u16());
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
}

#[tokio::test]
async fn validates_each_correlation_id_at_ascii_and_length_boundaries() {
    let router = with_http_contract(Router::new(), "test", "0.1.0", &[]);
    for (value, valid) in [
        ("a".repeat(128), true),
        ("!".to_owned(), true),
        ("a".repeat(129), false),
        ("has space".to_owned(), false),
        (String::new(), false),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/missing")
                    .header(CORRELATION_HEADER, &value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let actual = response.headers()[CORRELATION_HEADER].to_str().unwrap();
        if valid {
            assert_eq!(actual, value);
        } else {
            assert_eq!(actual.len(), 32);
            assert!(actual.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
}
