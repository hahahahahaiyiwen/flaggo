mod support;

use std::time::Duration;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use support::Fixture;
use tower::ServiceExt as _;

async fn request(router: Router, path: &str, body: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .header("x-flaggo-correlation-id", "http-test")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["x-flaggo-correlation-id"], "http-test");
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    (status, value)
}

#[tokio::test]
async fn real_stores_return_rule_and_default_without_selecting_current_or_writing_runtime_state() {
    let fixture = Fixture::new("attributes.x > 0", json!({"value": 7})).await;
    let router = fixture.router();
    for (x, result, evaluation) in [
        (1, 7, json!({"source": "rule", "rule": "choose"})),
        (0, 0, json!({"source": "default"})),
    ] {
        let (status, actual) = request(
            router.clone(),
            &fixture.path(),
            &format!(r#"{{"attributes":{{"x":{x},"_random":0.5}}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            actual,
            json!({
                "contractDigest": fixture.accepted.contract_digest,
                "executableDigest": fixture.executable_digest,
                "result": result,
                "evaluation": evaluation,
            })
        );
    }
    assert!(
        fixture
            .contracts
            .get_current("http.parity", None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM decision_executables")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        1
    );
    fixture.close().await;
}

#[tokio::test]
async fn maps_body_identity_input_not_found_and_not_active_in_reference_order() {
    let fixture = Fixture::new("true", json!({"value": 7})).await;
    let path = fixture.path();
    for (uri, body, status, kind) in [
        (
            path.replace("http.parity", "bad%20name"),
            r#"{"attributes":{}}"#,
            400,
            "invalid-request",
        ),
        (
            path.replace("http.parity", "missing"),
            r#"{"attributes":{}}"#,
            404,
            "contract-version-not-found",
        ),
        (
            path.clone(),
            r#"{"attributes":{"x":11}}"#,
            422,
            "invalid-runtime-input",
        ),
        (
            path.clone(),
            r#"{"attributes":{},"unknown":0}"#,
            400,
            "invalid-request",
        ),
        (
            path.clone(),
            r#"{"attributes":{"x":"\ud800"}}"#,
            500,
            "internal-error",
        ),
    ] {
        let (actual_status, value) = request(fixture.router(), &uri, body).await;
        assert_eq!(actual_status.as_u16(), status, "{uri}: {value}");
        assert_eq!(value["type"], format!("https://flaggo.dev/problems/{kind}"));
        assert_eq!(value["instance"], uri);
    }
    sqlx::query("UPDATE decision_executables SET lifecycle_state='inactive'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&path)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"attributes":{"x":1,"_random":0.5}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
    fixture.close().await;
}

#[tokio::test]
async fn differentiates_checked_integrity_native_json_and_dependency_failures() {
    let fixture = Fixture::new("true", json!({"value": 7})).await;
    for (stored, kind) in [
        ("null", "executable-integrity-failure"),
        ("{", "internal-error"),
    ] {
        sqlx::query("UPDATE decision_executables SET checked_executable_json=CAST(? AS BLOB)")
            .bind(stored)
            .execute(&fixture.pool)
            .await
            .unwrap();
        let (status, value) = request(
            fixture.router(),
            &fixture.path(),
            r#"{"attributes":{"x":1,"_random":0.5}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(value["type"], format!("https://flaggo.dev/problems/{kind}"));
    }
    fixture.executables.close().await;
    let (status, value) = request(
        fixture.router(),
        &fixture.path(),
        r#"{"attributes":{"x":1,"_random":0.5}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        value["type"],
        "https://flaggo.dev/problems/dependency-unavailable"
    );
    fixture.close().await;
}

#[tokio::test]
async fn maps_evaluation_and_result_validation_failures_without_runtime_fallback() {
    for (expression, kind) in [
        ("1 / attributes.x", "evaluation-failed"),
        ("attributes.x", "invalid-runtime-result"),
    ] {
        let fixture = Fixture::new("true", json!({"expression": expression})).await;
        let input = if expression.starts_with("1 /") { 0 } else { -1 };
        let (status, value) = request(
            fixture.router(),
            &fixture.path(),
            &format!(r#"{{"attributes":{{"x":{input},"_random":0.5}}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{value}");
        assert_eq!(value["type"], format!("https://flaggo.dev/problems/{kind}"));
        assert!(value.get("result").is_none());
        fixture.close().await;
    }
}

#[tokio::test]
async fn health_reports_only_required_store_checks_and_survives_a_failed_schema_probe() {
    let fixture = Fixture::new("true", json!({"value": 7})).await;
    let live = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(live.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(live.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(value["service"], "flaggo-decision-service");
    assert!(chrono::DateTime::parse_from_rfc3339(value["observedAt"].as_str().unwrap()).is_ok());
    for ready in [true, false] {
        if !ready {
            sqlx::query(
                "UPDATE flaggo_schema_versions SET version=999 WHERE component='executable-store'",
            )
            .execute(&fixture.pool)
            .await
            .unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if ready {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        );
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value["status"], if ready { "ready" } else { "not-ready" });
        assert_eq!(
            value["checks"],
            json!([
                {"name": "contract-store", "status": "up", "required": true},
                {"name": "executable-store", "status": if ready { "up" } else { "down" }, "required": true}
            ])
        );
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires Release builds of the unchanged .NET Contract and Decision services"]
async fn actual_dotnet_http_and_sqlite_producer_interoperability() {
    use flaggo_contract::strict_json;
    use support::process::Server;

    let fixture = Fixture::new("attributes.x > 0", json!({"value": 7})).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let dotnet_database = fixture.directory.path().join("dotnet.db");
    let contract = Server::dotnet(
        "contract",
        &dotnet_database,
        &fixture.directory.path().join("contract.log"),
    )
    .await;
    let deployed = client
        .put(format!(
            "{}/v3/decision-contracts/http.parity",
            contract.base
        ))
        .header("content-type", "application/json")
        .body(strict_json::serialize(&fixture.accepted.contract).unwrap())
        .send()
        .await
        .unwrap();
    let status = deployed.status();
    let deployed: Value = deployed.json().await.unwrap();
    assert_eq!(status.as_u16(), 201, "{deployed}");
    assert_eq!(deployed["contractDigest"], fixture.accepted.contract_digest);
    drop(contract);

    let mut compared = 0;
    for (producer, database) in [
        ("rust", fixture.directory.path().join("http.db")),
        ("dotnet", dotnet_database),
    ] {
        let before = compared;
        let reference = Server::dotnet(
            "decision",
            &database,
            &fixture.directory.path().join(format!("{producer}-net.log")),
        )
        .await;
        let replacement = Server::rust(
            &database,
            &fixture
                .directory
                .path()
                .join(format!("{producer}-rust.log")),
            None,
        )
        .await;
        let cases = [
            (
                "escaped-literal",
                "POST",
                fixture.path().replace("/v3/", "/v%33/"),
                "application/json",
                r#"{"attributes":{"x":1,"_random":0.5}}"#,
            ),
            (
                "rule",
                "POST",
                fixture.path(),
                "application/json",
                r#"{"attributes":{"x":1,"_random":0.5}}"#,
            ),
            (
                "default",
                "POST",
                fixture.path(),
                "Application/JSON; charset=utf-8",
                r#"{"attributes":{"x":0,"_random":0.5}}"#,
            ),
            (
                "missing-attribute",
                "POST",
                fixture.path(),
                "application/json",
                r#"{"attributes":{"_random":0.5}}"#,
            ),
            (
                "invalid-input",
                "POST",
                fixture.path(),
                "application/json",
                r#"{"attributes":{"x":11,"_random":0.5}}"#,
            ),
            (
                "unknown-contract",
                "POST",
                fixture.path().replace("http.parity", "missing"),
                "application/json",
                r#"{"attributes":{}}"#,
            ),
            (
                "invalid-identity",
                "POST",
                fixture.path().replace("http.parity", "bad%20name"),
                "application/json",
                r#"{"attributes":{}}"#,
            ),
            (
                "invalid-digest",
                "POST",
                fixture
                    .path()
                    .replace(&fixture.accepted.contract_digest, "bad"),
                "application/json",
                r#"{"attributes":{}}"#,
            ),
            (
                "invalid-json",
                "POST",
                fixture.path(),
                "application/json",
                "{",
            ),
            (
                "missing-property",
                "POST",
                fixture.path(),
                "application/json",
                "{}",
            ),
            (
                "duplicate-property",
                "POST",
                fixture.path(),
                "application/json",
                r#"{"attributes":{},"attributes":{}}"#,
            ),
            (
                "invalid-unicode",
                "POST",
                fixture.path(),
                "application/json",
                r#"{"attributes":{"x":"\ud800"}}"#,
            ),
            ("wrong-media", "POST", fixture.path(), "text/plain", "{}"),
            (
                "json-suffix",
                "POST",
                fixture.path(),
                "application/problem+json",
                "{}",
            ),
            (
                "empty-parameter",
                "POST",
                fixture.path(),
                "application/json;",
                r#"{"attributes":{"x":1,"_random":0.5}}"#,
            ),
            ("wrong-method", "GET", fixture.path(), "", ""),
            ("missing-route", "GET", "/missing".to_owned(), "", ""),
            (
                "trailing-slash",
                "POST",
                format!("{}/", fixture.path()),
                "application/json",
                r#"{"attributes":{"x":1,"_random":0.5}}"#,
            ),
            (
                "case-insensitive-route",
                "POST",
                fixture
                    .path()
                    .replace("/v3/decision-contracts/", "/V3/Decision-Contracts/")
                    .replace("/versions/", "/Versions/")
                    .replace("/decisions", "/Decisions"),
                "application/json",
                r#"{"attributes":{"x":1,"_random":0.5}}"#,
            ),
            ("live", "GET", "/health/live".to_owned(), "", ""),
            ("ready", "GET", "/health/ready".to_owned(), "", ""),
        ];
        for (name, method, path, content_type, body) in cases {
            let mut observed = Vec::new();
            for server in [&reference, &replacement] {
                let mut request = client
                    .request(method.parse().unwrap(), format!("{}{path}", server.base))
                    .header("x-flaggo-correlation-id", "actual-http-parity");
                if !content_type.is_empty() {
                    request = request
                        .header("content-type", content_type)
                        .body(body.to_owned());
                }
                let response = request.send().await.unwrap();
                let status = response.status().as_u16();
                let media = response.headers().get("content-type").map(|value| {
                    value
                        .to_str()
                        .unwrap()
                        .split(';')
                        .next()
                        .unwrap()
                        .to_owned()
                });
                let retry = response.headers().get("retry-after").cloned();
                assert_eq!(
                    response.headers()["x-flaggo-correlation-id"],
                    "actual-http-parity"
                );
                let mut value: Value = response.json().await.unwrap();
                if value.get("observedAt").is_some() {
                    assert!(
                        chrono::DateTime::parse_from_rfc3339(value["observedAt"].as_str().unwrap())
                            .is_ok()
                    );
                    value.as_object_mut().unwrap().remove("observedAt");
                }
                if name == "live" {
                    assert!(value["version"].is_string());
                    value.as_object_mut().unwrap().remove("version");
                }
                if let Some(detail) = value.get("detail").and_then(Value::as_str)
                    && detail.starts_with("Request body is invalid: ")
                {
                    // JSON exceptions expose native serializer diagnostics; compare the public category/prefix.
                    value["detail"] =
                        Value::String("Request body is invalid: <native diagnostic>".into());
                }
                observed.push((status, media, retry, value));
            }
            let expected_status = match name {
                "rule"
                | "default"
                | "missing-attribute"
                | "empty-parameter"
                | "escaped-literal"
                | "trailing-slash"
                | "case-insensitive-route"
                | "live"
                | "ready" => 200,
                "invalid-input" => 422,
                "unknown-contract" | "missing-route" => 404,
                "invalid-unicode" => 500,
                "wrong-media" | "json-suffix" => 415,
                "wrong-method" => 405,
                "invalid-identity" | "invalid-digest" | "invalid-json" | "missing-property"
                | "duplicate-property" => 400,
                _ => panic!("missing reference expectation: {name}"),
            };
            assert_eq!(observed[0].0, expected_status, "{producer}: {name}");
            if matches!(name, "rule" | "default" | "missing-attribute") {
                assert_eq!(observed[0].3["result"], if name == "rule" { 7 } else { 0 });
                assert_eq!(
                    observed[0].3["evaluation"]["source"],
                    if name == "rule" { "rule" } else { "default" }
                );
            }
            assert_eq!(observed[0], observed[1], "{producer}: {name}");
            compared += 1;
        }
        for streamed in [false, true] {
            for extra in [0, 1] {
                let mut results = Vec::new();
                for server in [&reference, &replacement] {
                    let mut body = br#"{"attributes":{"x":1,"_random":0.5}}"#.to_vec();
                    body.resize(
                        flaggo_contract::strict_json::MAXIMUM_DOCUMENT_BYTES + extra,
                        b' ',
                    );
                    let body = if streamed {
                        reqwest::Body::wrap_stream(futures_util::stream::iter([Ok::<
                            _,
                            std::io::Error,
                        >(
                            body
                        )]))
                    } else {
                        reqwest::Body::from(body)
                    };
                    let response = client
                        .post(format!("{}{}", server.base, fixture.path()))
                        .header("content-type", "application/json")
                        .body(body)
                        .send()
                        .await
                        .unwrap();
                    assert_eq!(
                        response.status().as_u16(),
                        if extra == 0 { 200 } else { 400 }
                    );
                    results.push(response.json::<Value>().await.unwrap());
                }
                assert_eq!(
                    results[0], results[1],
                    "{producer}: boundary streamed={streamed} extra={extra}"
                );
                compared += 1;
            }
        }
        for supplied in [
            None,
            Some("a".repeat(128)),
            Some("a".repeat(129)),
            Some("has space".to_owned()),
        ] {
            for server in [&reference, &replacement] {
                let mut request = client.get(format!("{}/health/live", server.base)).header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                );
                if let Some(value) = &supplied {
                    request = request.header("x-flaggo-correlation-id", value);
                }
                let response = request.send().await.unwrap();
                assert_eq!(response.status().as_u16(), 200);
                let expected = supplied
                    .as_deref()
                    .filter(|value| value.len() == 128)
                    .unwrap_or("4bf92f3577b34da6a3ce929d0e0e4736");
                assert_eq!(response.headers()["x-flaggo-correlation-id"], expected);
                let _: Value = response.json().await.unwrap();
            }
            compared += 1;
        }
        drop(replacement);
        drop(reference);
        println!(
            "FLAGGO_HTTP_PARITY {}",
            json!({"format": "flaggo.http-parity/v1", "service": "decision",
                "producer": producer, "comparisons": compared - before})
        );
    }
    assert_eq!(compared, 58);
    fixture.close().await;
}

#[test]
fn invalid_configuration_emits_a_failure_without_opening_the_database() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("must-not-exist.db");
    let output = support::process::rust_command()
        .env("FLAGGO_DECISION_LISTEN_ADDRESS", "invalid-address")
        .env("FLAGGO_DECISION_DATABASE_PATH", &database)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!database.exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let event = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["event.name"] == "flaggo.service.failed")
        .expect("startup failure event");
    assert_eq!(event["flaggo.operation.outcome"], "failure");
    assert_eq!(event["flaggo.failure.category"], "configuration");
}

#[test]
fn legacy_configuration_requires_explicit_rust_settings_without_creating_state() {
    let directory = tempfile::tempdir().unwrap();
    support::process::assert_legacy_configuration_rejected(
        support::process::rust_command,
        directory.path(),
        "FLAGGO_DECISION_DATABASE_PATH",
        "FLAGGO_DECISION_LISTEN_ADDRESS",
    );
}

#[tokio::test]
async fn unavailable_telemetry_backend_does_not_change_runtime_results() {
    let fixture = Fixture::new("true", json!({"value": 7})).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (observed, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let backend = tokio::spawn(async move {
        let receiver = Router::new().fallback(move || {
            let observed = observed.clone();
            async move {
                observed.send(()).unwrap();
                StatusCode::SERVICE_UNAVAILABLE
            }
        });
        axum::serve(listener, receiver).await.unwrap();
    });
    let server = support::process::Server::rust(
        &fixture.directory.path().join("http.db"),
        &fixture.directory.path().join("unavailable-exporter.log"),
        Some(&endpoint),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(10), requests.recv())
        .await
        .unwrap()
        .expect("actual export attempt");
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("{}{}", server.base, fixture.path()))
        .header("content-type", "application/json")
        .body(r#"{"attributes":{"x":1,"_random":0.5}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let decision: Value = response.json().await.unwrap();
    assert_eq!(decision["result"], 7);
    assert_eq!(decision["executableDigest"], fixture.executable_digest);
    drop(server);
    backend.abort();
    assert!(backend.await.unwrap_err().is_cancelled());
    fixture.close().await;
}

#[tokio::test]
async fn actual_process_exports_correlated_spans_logs_and_bounded_metric_dimensions() {
    use opentelemetry_proto::tonic::metrics::v1::{metric, number_data_point};
    use std::{
        collections::BTreeMap,
        time::{Duration, Instant},
    };
    use support::{
        capture::{Collector, string},
        process::Server,
    };

    let fixture = Fixture::new("attributes.x > 0", json!({"value": 7})).await;
    let collector = Collector::start().await;
    let server = Server::rust(
        &fixture.directory.path().join("http.db"),
        &fixture.directory.path().join("telemetry.log"),
        Some(&collector.endpoint),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    for (name, x, media, status) in [
        ("http.parity", 1, "application/json", 200),
        ("http.parity", 0, "application/json", 200),
        ("http.parity", 1, "text/plain", 415),
        ("missing", 1, "application/json", 404),
    ] {
        let response = client
            .post(format!(
                "{}{}",
                server.base,
                fixture.path().replace("http.parity", name)
            ))
            .header("content-type", media)
            .header(
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            .header("x-flaggo-correlation-id", "exported-http-test")
            .body(format!(r#"{{"attributes":{{"x":{x},"_random":0.5}}}}"#))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(
            response.headers()["x-flaggo-correlation-id"],
            "exported-http-test"
        );
        let _: Value = response.json().await.unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let complete = {
            let captured = collector.received.lock().unwrap();
            let evaluations = captured
                .traces
                .iter()
                .flat_map(|export| &export.resource_spans)
                .flat_map(|resource| &resource.scope_spans)
                .flat_map(|scope| &scope.spans)
                .filter(|span| span.name == "flaggo.decision.evaluate")
                .count();
            let counter_complete = captured.metrics.iter().flat_map(|export| &export.resource_metrics)
                .flat_map(|resource| &resource.scope_metrics).flat_map(|scope| &scope.metrics)
                .any(|value| value.name == "flaggo.decision.evaluations"
                    && matches!(&value.data, Some(metric::Data::Sum(sum)) if sum.data_points.len() == 4));
            let started = captured
                .logs
                .iter()
                .flat_map(|export| &export.resource_logs)
                .flat_map(|resource| &resource.scope_logs)
                .flat_map(|scope| &scope.log_records)
                .any(|record| {
                    string(&record.attributes, "event.name").as_deref()
                        == Some("flaggo.service.started")
                });
            evaluations == 4 && counter_complete && started
        };
        if complete {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Incomplete emitted telemetry: {:#?}",
            collector.received.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    {
        let captured = collector.received.lock().unwrap();
        let resources = captured
            .traces
            .iter()
            .flat_map(|value| &value.resource_spans)
            .map(|value| value.resource.as_ref().unwrap())
            .chain(
                captured
                    .metrics
                    .iter()
                    .flat_map(|value| &value.resource_metrics)
                    .map(|value| value.resource.as_ref().unwrap()),
            )
            .chain(
                captured
                    .logs
                    .iter()
                    .flat_map(|value| &value.resource_logs)
                    .map(|value| value.resource.as_ref().unwrap()),
            );
        for resource in resources {
            assert_eq!(
                string(&resource.attributes, "service.version").as_deref(),
                Some(env!("CARGO_PKG_VERSION"))
            );
            assert_eq!(
                string(&resource.attributes, "service.name").as_deref(),
                Some("flaggo-decision-service")
            );
            assert_eq!(
                string(&resource.attributes, "service.namespace").as_deref(),
                Some("flaggo")
            );
            assert!(string(&resource.attributes, "service.instance.id").is_some());
            assert!(resource.attributes.iter().all(|value| !matches!(
                value.key.as_str(),
                "flaggo.tenant" | "flaggo.application" | "flaggo.environment"
            )));
        }
        for scope in captured
            .logs
            .iter()
            .flat_map(|export| &export.resource_logs)
            .flat_map(|resource| &resource.scope_logs)
        {
            if scope.log_records.iter().any(|record| {
                string(&record.attributes, "event.name").as_deref()
                    == Some("flaggo.service.started")
            }) {
                assert_eq!(
                    scope.scope.as_ref().unwrap().name,
                    "flaggo.decision-service"
                );
                assert_eq!(
                    scope.scope.as_ref().unwrap().version,
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
        for scope in captured
            .traces
            .iter()
            .flat_map(|export| &export.resource_spans)
            .flat_map(|resource| &resource.scope_spans)
        {
            assert_eq!(
                scope.scope.as_ref().unwrap().name,
                "flaggo.decision-service"
            );
            assert_eq!(
                scope.scope.as_ref().unwrap().version,
                env!("CARGO_PKG_VERSION")
            );
        }
        let spans = captured
            .traces
            .iter()
            .flat_map(|export| &export.resource_spans)
            .flat_map(|resource| &resource.scope_spans)
            .flat_map(|scope| &scope.spans)
            .collect::<Vec<_>>();
        for span in spans
            .iter()
            .filter(|span| span.name == "flaggo.decision.evaluate")
        {
            assert_eq!(
                span.trace_id,
                [
                    0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e,
                    0x0e, 0x47, 0x36
                ]
            );
            assert_eq!(
                string(&span.attributes, "flaggo.request.correlation_id").as_deref(),
                Some("exported-http-test")
            );
            assert_eq!(
                string(&span.attributes, "flaggo.contract.digest").as_deref(),
                Some(fixture.accepted.contract_digest.as_str())
            );
            let parent = spans
                .iter()
                .find(|parent| parent.span_id == span.parent_span_id)
                .expect("HTTP server parent");
            assert_eq!(parent.kind, 2);
            assert_eq!(
                parent.parent_span_id,
                [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7]
            );
            assert_eq!(parent.trace_id, span.trace_id);
            if string(&span.attributes, "flaggo.operation.outcome").as_deref() == Some("success") {
                assert_eq!(
                    string(&span.attributes, "flaggo.executable.digest").as_deref(),
                    Some(fixture.executable_digest.as_str())
                );
            } else {
                assert_eq!(span.status.as_ref().unwrap().code, 2);
            }
        }
        let mut counters = BTreeMap::new();
        let mut durations = BTreeMap::new();
        let mut http_duration = false;
        for scope in captured
            .metrics
            .iter()
            .flat_map(|export| &export.resource_metrics)
            .flat_map(|resource| &resource.scope_metrics)
        {
            assert_eq!(
                scope.scope.as_ref().unwrap().name,
                "flaggo.decision-service"
            );
            assert_eq!(
                scope.scope.as_ref().unwrap().version,
                env!("CARGO_PKG_VERSION")
            );
            for value in &scope.metrics {
                if value.name == "http.server.request.duration" {
                    http_duration = true;
                    assert_eq!(value.unit, "s");
                    if let Some(metric::Data::Histogram(histogram)) = &value.data {
                        for point in &histogram.data_points {
                            assert!(point.attributes.iter().all(|value| matches!(
                                value.key.as_str(),
                                "http.request.method" | "http.response.status_code" | "http.route"
                            )));
                            if let Some(route) = string(&point.attributes, "http.route") {
                                assert!(
                                    !route.contains("http.parity") && !route.contains("sha256:")
                                );
                            }
                        }
                    }
                    continue;
                }
                let points = match &value.data {
                    Some(metric::Data::Sum(sum)) => {
                        assert_eq!(value.name, "flaggo.decision.evaluations");
                        assert_eq!(value.unit, "{evaluation}");
                        sum.data_points
                            .iter()
                            .map(|point| {
                                let Some(number_data_point::Value::AsInt(count)) = point.value
                                else {
                                    panic!("integer counter")
                                };
                                (&point.attributes, count as u64)
                            })
                            .collect::<Vec<_>>()
                    }
                    Some(metric::Data::Histogram(histogram)) => {
                        assert_eq!(value.name, "flaggo.decision.evaluation.duration");
                        assert_eq!(value.unit, "s");
                        histogram
                            .data_points
                            .iter()
                            .map(|point| {
                                assert!(point.sum.is_some_and(|sum| sum >= 0.0));
                                (&point.attributes, point.count)
                            })
                            .collect::<Vec<_>>()
                    }
                    _ => panic!("unexpected metric {value:?}"),
                };
                for (attributes, count) in points {
                    assert!(attributes.iter().all(|value| matches!(
                        value.key.as_str(),
                        "flaggo.operation.outcome"
                            | "flaggo.failure.category"
                            | "flaggo.evaluation.source"
                    )));
                    let key = (
                        string(attributes, "flaggo.operation.outcome").unwrap(),
                        string(attributes, "flaggo.failure.category"),
                        string(attributes, "flaggo.evaluation.source"),
                    );
                    let target = if value.name == "flaggo.decision.evaluations" {
                        &mut counters
                    } else {
                        &mut durations
                    };
                    target.insert(key, count);
                }
            }
        }
        assert!(http_duration);
        let expected = BTreeMap::from([
            (("success".into(), None, Some("rule".into())), 1),
            (("success".into(), None, Some("default".into())), 1),
            (("invalid".into(), Some("validation".into()), None), 1),
            (("not_found".into(), Some("not_found".into()), None), 1),
        ]);
        assert_eq!(counters, expected);
        assert_eq!(durations, expected);
    }
    drop(server);
    collector.close().await;
    fixture.close().await;
}
