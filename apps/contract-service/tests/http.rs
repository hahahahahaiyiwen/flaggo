mod support;

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use flaggo_contract::DecisionContract;
use flaggo_contract_service::{router, worker::ActivationWorker};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::{Fixture, Server, contract, submission};
use tower::ServiceExt as _;

const NAME: &str = "tetris.dropInterval";
const BASE: &str = "/v3/decision-contracts/tetris.dropInterval";

async fn request(
    fixture: &Fixture,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-flaggo-correlation-id", "contract-test");
    let body = if let Some(body) = body {
        request = request.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&body).unwrap())
    } else {
        Body::empty()
    };
    let response = router(fixture.lifecycle.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["x-flaggo-correlation-id"], "contract-test");
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        headers,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

fn version_path(version: &Value) -> String {
    format!(
        "{BASE}/versions/{}",
        version["contractDigest"].as_str().unwrap()
    )
}

#[tokio::test]
async fn validation_deployment_readiness_catalog_and_paging() {
    let fixture = Fixture::new().await;
    let document = contract();
    let (status, _, validation) = request(
        &fixture,
        "POST",
        &format!("{BASE}/validate"),
        Some(document.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validation["status"], "valid");
    assert_eq!(
        validation["contractDigest"],
        "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098"
    );
    assert_eq!(
        request(&fixture, "GET", BASE, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, headers, version) = request(&fixture, "PUT", BASE, Some(document.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers["location"], version_path(&version));
    assert_eq!(version["contractDigest"], validation["contractDigest"]);
    assert_ne!(version["activeExecutableDigest"], Value::Null);
    let (status, headers, repeated) = request(&fixture, "PUT", BASE, Some(document)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!headers.contains_key("location"));
    assert_eq!(repeated, version);
    for path in [
        BASE.to_string(),
        version_path(&version),
        format!("/V3/DECISION-CONTRACTS/{NAME}/"),
    ] {
        let (status, _, body) = request(&fixture, "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, version);
    }
    let (_, _, catalog) = request(
        &fixture,
        "GET",
        "/v3/decision-contract-catalog/current",
        None,
    )
    .await;
    assert_eq!(catalog["contracts"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["contracts"][0]["contract"], version["contract"]);
    let (_, etag) = fixture.lifecycle.catalog().await.unwrap();
    for header in [etag.clone(), format!("\"other\", {etag}"), "*".into()] {
        let response = router(fixture.lifecycle.clone())
            .oneshot(
                Request::builder()
                    .uri("/v3/decision-contract-catalog/current")
                    .header("if-none-match", header)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()["etag"], etag);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let mut changed = contract();
    changed["result"]["default"] = 600.into();
    let newer = fixture.deploy(&changed).await;
    let (_, _, first) = request(&fixture, "GET", &format!("{BASE}/versions?limit=1"), None).await;
    assert_eq!(first["currentContractDigest"], newer["contractDigest"]);
    assert_eq!(first["versions"].as_array().unwrap().len(), 1);
    let cursor = first["nextCursor"].as_str().unwrap();
    let (_, _, second) = request(
        &fixture,
        "GET",
        &format!("{BASE}/versions?limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(
        second["versions"][0]["contractDigest"],
        version["contractDigest"]
    );
    assert_eq!(second["nextCursor"], Value::Null);
    for query in [
        "limit=0",
        "limit=201",
        "limit=invalid",
        "cursor=bad",
        "limit=1&limit=2",
    ] {
        assert_eq!(
            request(&fixture, "GET", &format!("{BASE}/versions?{query}"), None)
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }
    assert_eq!(
        request(&fixture, "GET", "/health/ready", None).await.2["status"],
        "ready"
    );
    assert_eq!(
        request(&fixture, "GET", "/health/live", None).await.2["service"],
        "flaggo-contract-service"
    );
    fixture.close().await;
}

#[tokio::test]
async fn candidate_admission_manual_activation_conflicts_and_history() {
    let fixture = Fixture::new().await;
    let version = fixture.deploy(&contract()).await;
    let path = version_path(&version);
    let candidate = submission("cycle-1", "attributes.board_pressure >= 0.5", 300);
    let (status, _, admitted) = request(
        &fixture,
        "POST",
        &format!("{path}/candidates"),
        Some(candidate.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(admitted["created"], true);
    assert_eq!(admitted["lifecycleState"], "candidate");
    let retry = request(
        &fixture,
        "POST",
        &format!("{path}/candidates"),
        Some(candidate.clone()),
    )
    .await;
    assert_eq!(retry.0, StatusCode::OK);
    assert_eq!(retry.2["created"], false);
    assert_eq!(retry.2["createdAt"], admitted["createdAt"]);
    assert_eq!(
        request(
            &fixture,
            "POST",
            &format!("{path}/candidates"),
            Some(submission("cycle-1", "true", 400))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, _, catalog) = request(&fixture, "GET", &format!("{path}/executables"), None).await;
    assert_eq!(catalog["candidates"].as_array().unwrap().len(), 1);
    let active = &catalog["activeExecutable"];
    assert!(active.get("checkedExecutable").is_none());
    assert!(active.get("state").is_none());
    let command = json!({"expectedActiveExecutableDigest": active["executableDigest"],
        "expectedActiveStateVersion": active["stateVersion"], "expectedCandidateStateVersion": 0});
    let activation_path = format!(
        "{path}/executables/{}/activate",
        admitted["executableDigest"].as_str().unwrap()
    );
    let (status, _, activated) =
        request(&fixture, "POST", &activation_path, Some(command.clone())).await;
    assert_eq!(status, StatusCode::OK, "{activated}");
    assert_eq!(activated["executable"]["lifecycleState"], "active");
    assert_eq!(activated["executable"]["stateVersion"], 3);
    assert_eq!(
        request(&fixture, "POST", &activation_path, Some(command))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let retry = request(
        &fixture,
        "POST",
        &format!("{path}/candidates"),
        Some(candidate),
    )
    .await;
    assert_eq!(retry.0, StatusCode::OK);
    assert_eq!(retry.2["lifecycleState"], "active");
    let (_, _, history) = request(
        &fixture,
        "GET",
        &format!("{path}/executables/history?limit=1"),
        None,
    )
    .await;
    assert_eq!(history["executables"].as_array().unwrap().len(), 1);
    assert!(history["nextCursor"].is_string());
    let cursor = history["nextCursor"].as_str().unwrap();
    let (_, _, next) = request(
        &fixture,
        "GET",
        &format!("{path}/executables/history?limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(next["executables"].as_array().unwrap().len(), 1);
    let mut newer = contract();
    newer["result"]["default"] = 600.into();
    fixture.deploy(&newer).await;
    let stale = request(
        &fixture,
        "POST",
        &format!("{path}/candidates"),
        Some(submission("stale", "true", 450)),
    )
    .await;
    assert_eq!(stale.0, StatusCode::CONFLICT);
    assert_eq!(
        stale.2["type"],
        "https://flaggo.dev/problems/stale-contract-digest"
    );
    fixture.close().await;
}

#[tokio::test]
async fn default_authority_is_ready_when_authored_rules_are_absent_or_opaque() {
    let fixture = Fixture::new().await;
    for opaque in [false, true] {
        let mut document = contract();
        document["name"] = if opaque {
            "default.opaque"
        } else {
            "default.absent"
        }
        .into();
        if opaque {
            for rule in document["authoredExecutable"]["rules"]
                .as_array_mut()
                .unwrap()
            {
                rule["when"] = json!({"condition": "the player needs assistance"});
            }
        } else {
            document
                .as_object_mut()
                .unwrap()
                .remove("authoredExecutable");
        }
        let version = fixture.deploy(&document).await;
        let active = fixture
            .lifecycle
            .executables
            .get_active(version["contractDigest"].as_str().unwrap(), None)
            .await
            .unwrap()
            .unwrap();
        assert!(active.executable.rules.is_empty());
        assert_eq!(version["activeExecutableDigest"], active.executable_digest);
        assert!(
            fixture
                .lifecycle
                .get(document["name"].as_str().unwrap(), None)
                .await
                .unwrap()
                .is_some()
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn typed_projections_preserve_raw_numbers_and_leading_source_discriminators() {
    let fixture = Fixture::new().await;
    let mut document = contract();
    document["result"]["schema"]["minimum"] = 0.into();
    let raw = serde_json::to_string(&document)
        .unwrap()
        .replace("\"default\":500", "\"default\":1e-999")
        .replace("\"value\":250", "\"value\":1e-999");
    let response = router(fixture.lifecycle.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(BASE)
                .header("content-type", "application/json")
                .body(Body::from(raw))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("\"default\":1e-999"), "{body}");
    assert!(body.contains("\"value\":1e-999"), "{body}");
    let version: Value = serde_json::from_slice(&bytes).unwrap();
    let decoded: DecisionContract = flaggo_contract::strict_json::deserialize(
        &serde_json::to_vec(&version["contract"]).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded.name, NAME);
    for path in [
        BASE.to_string(),
        version_path(&version),
        "/v3/decision-contract-catalog/current".into(),
        format!("{}/executables", version_path(&version)),
        format!("{}/executables/history", version_path(&version)),
    ] {
        let response = router(fixture.lifecycle.clone())
            .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("\"value\":1e-999"), "{path}: {body}");
    }
    fixture.close().await;
}

#[tokio::test]
async fn native_faults_provenance_validation_and_dependency_failures() {
    let fixture = Fixture::new().await;
    let version = fixture.deploy(&contract()).await;
    let path = format!("{}/candidates", version_path(&version));
    let mut invalid = contract();
    invalid["result"]["default"] = 1.into();
    let validation = request(
        &fixture,
        "POST",
        &format!("{BASE}/validate"),
        Some(invalid.clone()),
    )
    .await;
    assert_eq!(validation.0, StatusCode::OK);
    assert_eq!(validation.2["status"], "invalid");
    assert_eq!(
        request(&fixture, "PUT", BASE, Some(invalid)).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut mismatch = contract();
    mismatch["name"] = "other".into();
    assert_eq!(
        request(&fixture, "PUT", BASE, Some(mismatch)).await.0,
        StatusCode::CONFLICT
    );
    let mut authority = contract();
    authority["authority"]["tenant"] = "other".into();
    assert_eq!(
        request(&fixture, "PUT", BASE, Some(authority)).await.2["type"],
        "https://flaggo.dev/problems/contract-name-authority-conflict"
    );
    for text in [
        " ".into(),
        "\n".into(),
        "x".repeat(257),
        "\u{1f600}".repeat(129),
    ] {
        let mut candidate = submission("cycle", "true", 300);
        candidate["provenance"]["cycleId"] = text.into();
        assert_eq!(
            request(&fixture, "POST", &path, Some(candidate)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &fixture,
            "POST",
            &path,
            Some(submission(&"\u{1f600}".repeat(128), "true", 300))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        request(
            &fixture,
            "POST",
            &path,
            Some(submission("invalid", "attributes.missing", 300))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for (body, content_type, status) in [
        ("{}", "text/plain", StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ("{}", "application/json", StatusCode::BAD_REQUEST),
        ("null", "application/json", StatusCode::BAD_REQUEST),
        (
            "{\"rules\":[],\"provenance\":{}}",
            "application/json",
            StatusCode::BAD_REQUEST,
        ),
        (
            "{\"rules\":[],\"provenance\":null}",
            "application/json",
            StatusCode::BAD_REQUEST,
        ),
        (
            "{\"rules\":[],\"rules\":[],\"provenance\":{}}",
            "application/json",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = router(fixture.lifecycle.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&path)
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{body}");
    }
    fixture.lifecycle.contracts.close().await;
    let response = request(&fixture, "GET", BASE, None).await;
    assert_eq!(response.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.1["retry-after"], "1");
    fixture.close().await;
}

#[tokio::test]
async fn concurrent_deployments_preserve_readiness_and_acceptance_time() {
    let fixture = Fixture::new().await;
    let contract: DecisionContract = serde_json::from_value(contract()).unwrap();
    let (left, right) = tokio::join!(
        fixture.lifecycle.deploy(NAME, &contract),
        fixture.lifecycle.deploy(NAME, &contract)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.1, right.1);
    assert_eq!(left.0, right.0);
    assert!(fixture.lifecycle.get(NAME, None).await.unwrap().is_some());
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM decision_executables WHERE lifecycle_state = 'active'",
    )
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    fixture.close().await;
}

#[tokio::test]
async fn worker_rejects_corruption_without_rewriting_and_continues_other_contracts() {
    for mutation in [
        "canonical",
        "checksum",
        "references",
        "rules",
        "base64",
        "json",
    ] {
        let fixture = Fixture::new().await;
        let version = fixture.deploy(&contract()).await;
        let path = version_path(&version);
        let admitted = request(
            &fixture,
            "POST",
            &format!("{path}/candidates"),
            Some(submission("corrupt", "true", 300)),
        )
        .await
        .2;
        let digest = admitted["executableDigest"].as_str().unwrap();
        let original: Vec<u8> = sqlx::query_scalar(
            "SELECT checked_executable_json FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        let mut corrupt: Value = serde_json::from_slice(&original).unwrap();
        match mutation {
            "canonical" => corrupt["rules"][0]["predicate"]["canonicalExpression"] = "false".into(),
            "checksum" => {
                corrupt["rules"][0]["predicate"]["checkedAstDigest"] =
                    format!("sha256:{}", "0".repeat(64)).into()
            }
            "references" => {
                corrupt["rules"][0]["predicate"]["referencedAttributes"] = json!(["board_pressure"])
            }
            "rules" => corrupt["rules"] = json!([]),
            "base64" => corrupt["rules"][0]["predicate"]["checkedAst"] = "!".into(),
            "json" => {}
            _ => unreachable!(),
        }
        let corrupt = if mutation == "json" {
            b"{".to_vec()
        } else {
            serde_json::to_vec(&corrupt).unwrap()
        };
        sqlx::query(
        "UPDATE decision_executables SET checked_executable_json = ? WHERE executable_digest = ?",
    )
    .bind(&corrupt)
    .bind(digest)
    .execute(&fixture.pool)
    .await
    .unwrap();
        let mut second = contract();
        second["name"] = "tetris.other".into();
        let second = fixture.deploy(&second).await;
        let second_admitted = request(
            &fixture,
            "POST",
            &format!(
                "/v3/decision-contracts/tetris.other/versions/{}/candidates",
                second["contractDigest"].as_str().unwrap()
            ),
            Some(submission("valid", "true", 400)),
        )
        .await
        .2;
        ActivationWorker::new(fixture.lifecycle.clone())
            .run_once()
            .await
            .unwrap();
        let state: String = sqlx::query_scalar(
            "SELECT lifecycle_state FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(state, "inactive", "{mutation}");
        assert_eq!(
            fixture
                .lifecycle
                .executables
                .get_active(version["contractDigest"].as_str().unwrap(), None)
                .await
                .unwrap()
                .unwrap()
                .executable_digest,
            version["activeExecutableDigest"].as_str().unwrap()
        );
        assert_eq!(
            fixture
                .lifecycle
                .executables
                .get_active(second["contractDigest"].as_str().unwrap(), None)
                .await
                .unwrap()
                .unwrap()
                .executable_digest,
            second_admitted["executableDigest"].as_str().unwrap()
        );
        let after: Vec<u8> = sqlx::query_scalar(
            "SELECT checked_executable_json FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(after, corrupt);
        fixture.close().await;
    }
}

#[tokio::test]
async fn worker_rejects_absent_policy_and_uses_atomic_currentness_fences() {
    let fixture = Fixture::new().await;
    let mut document = contract();
    document.as_object_mut().unwrap().remove("learning");
    let version = fixture.deploy(&document).await;
    let admitted = request(
        &fixture,
        "POST",
        &format!("{}/candidates", version_path(&version)),
        Some(submission("no-policy", "true", 300)),
    )
    .await
    .2;
    ActivationWorker::new(fixture.lifecycle.clone())
        .run_once()
        .await
        .unwrap();
    assert_eq!(
        fixture
            .lifecycle
            .executables
            .get(admitted["executableDigest"].as_str().unwrap(), None)
            .await
            .unwrap()
            .unwrap()
            .state,
        flaggo_executable_store::ExecutableLifecycleState::Inactive
    );
    let version = fixture.deploy(&contract()).await;
    let admitted = request(
        &fixture,
        "POST",
        &format!("{}/candidates", version_path(&version)),
        Some(submission("stale", "true", 400)),
    )
    .await
    .2;
    let mut changed = contract();
    changed["result"]["default"] = 650.into();
    fixture.deploy(&changed).await;
    ActivationWorker::new(fixture.lifecycle.clone())
        .run_once()
        .await
        .unwrap();
    assert_ne!(
        fixture
            .lifecycle
            .executables
            .get_active(version["contractDigest"].as_str().unwrap(), None)
            .await
            .unwrap()
            .unwrap()
            .executable_digest,
        admitted["executableDigest"].as_str().unwrap()
    );
    fixture.close().await;
}

#[test]
fn activation_interval_configuration_is_strict_and_bounded() {
    use flaggo_contract_service::worker::poll_interval;
    assert_eq!(poll_interval(None).unwrap(), Duration::from_secs(5));
    assert_eq!(poll_interval(Some(" \t")).unwrap(), Duration::from_secs(5));
    assert_eq!(
        poll_interval(Some("0001")).unwrap(),
        Duration::from_millis(1)
    );
    for invalid in [
        "0",
        "-1",
        "+1",
        " 1",
        "1 ",
        "1.5",
        "922337203685478",
        "18446744073709551616",
    ] {
        assert!(poll_interval(Some(invalid)).is_err(), "{invalid}");
    }
}

#[test]
fn native_timestamp_and_pool_timeout_keep_reference_http_and_telemetry_categories() {
    use flaggo_contract_service::ServiceError;
    let timestamp =
        ServiceError::ContractStore(flaggo_contract_store::ContractStoreError::Timestamp(
            flaggo_contract_store::timestamp::TimestampError("invalid round-trip timestamp".into()),
        ));
    assert_eq!(timestamp.problem().status, StatusCode::BAD_REQUEST);
    assert_eq!(timestamp.classification(), ("invalid", "validation"));
    let timeout = ServiceError::ExecutableStore(
        flaggo_executable_store::ExecutableStoreError::Database(sqlx::Error::PoolTimedOut),
    );
    assert_eq!(timeout.problem().status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(timeout.problem().retry_after_seconds, Some(1));
    assert_eq!(timeout.classification(), ("unavailable", "timeout"));
}

#[tokio::test]
async fn startup_rejects_invalid_configuration_before_opening_database() {
    let directory = tempfile::tempdir().unwrap();
    for (key, value) in [
        ("FLAGGO_CONTRACT_LISTEN_ADDRESS", "invalid"),
        ("FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS", "+1"),
    ] {
        let database = directory.path().join("must-not-exist.db");
        let output = support::rust_command()
            .env("FLAGGO_CONTRACT_DATABASE_PATH", &database)
            .env("FLAGGO_CONTRACT_LISTEN_ADDRESS", "127.0.0.1:0")
            .env("FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS", "5000")
            .env(key, value)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!database.exists());
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(log.contains("flaggo.service.failed"), "{log}");
        assert!(log.contains("configuration"), "{log}");
    }
}

#[test]
fn legacy_configuration_requires_explicit_rust_settings_without_creating_state() {
    let directory = tempfile::tempdir().unwrap();
    support::assert_legacy_configuration_rejected(
        support::rust_command,
        directory.path(),
        "FLAGGO_CONTRACT_DATABASE_PATH",
        "FLAGGO_CONTRACT_LISTEN_ADDRESS",
    );
}

#[tokio::test]
async fn real_process_activates_persisted_pending_candidates_before_readiness() {
    let fixture = Fixture::new().await;
    let version = fixture.deploy(&contract()).await;
    let admitted = request(
        &fixture,
        "POST",
        &format!("{}/candidates", version_path(&version)),
        Some(submission("restart", "true", 300)),
    )
    .await
    .2;
    let database = fixture.directory.path().join("http.db");
    let server = Server::rust(
        &database,
        &fixture.directory.path().join("rust.log"),
        None,
        "300000",
    )
    .await;
    let current: Value = client()
        .get(format!("{}{BASE}", server.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        current["activeExecutableDigest"],
        admitted["executableDigest"]
    );
    drop(server);
    let restarted = Server::rust(
        &database,
        &fixture.directory.path().join("restarted.log"),
        None,
        "300000",
    )
    .await;
    let current_after: Value = client()
        .get(format!("{}{BASE}", restarted.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current_after, current);
    drop(restarted);
    fixture.close().await;
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

async fn wait_active(client: &reqwest::Client, server: &Server, digest: &Value) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current: Value = client
            .get(format!("{}{BASE}", server.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["activeExecutableDigest"] == *digest {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Candidate did not activate: {current}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn real_otlp_contains_bounded_contract_metrics_nested_spans_and_lifecycle_logs() {
    use support::capture::{Collector, string};
    let collector = Collector::start().await;
    let fixture = Fixture::new().await;
    let mut corrupt_contract = contract();
    corrupt_contract["name"] = "tetris.corrupt".into();
    let corrupt_version = fixture.deploy(&corrupt_contract).await;
    let corrupt_candidate = fixture
        .lifecycle
        .submit(
            "tetris.corrupt",
            corrupt_version["contractDigest"].as_str().unwrap(),
            serde_json::from_value(submission("corrupt-export", "true", 300)).unwrap(),
        )
        .await
        .unwrap()
        .0;
    sqlx::query(
        "UPDATE decision_executables SET checked_executable_json = ? WHERE executable_digest = ?",
    )
    .bind(b"{".as_slice())
    .bind(&corrupt_candidate.executable_digest)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let server = Server::rust(
        &fixture.directory.path().join("http.db"),
        &fixture.directory.path().join("rust.log"),
        Some(&collector.endpoint),
        "50",
    )
    .await;
    let client = client();
    let response = client
        .put(format!("{}{BASE}", server.base))
        .header("x-flaggo-correlation-id", "contract-export")
        .header(
            "traceparent",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
        )
        .json(&contract())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let version: Value = response.json().await.unwrap();
    assert_eq!(
        client
            .post(format!("{}{BASE}/validate", server.base))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let path = version_path(&version);
    let response = client
        .post(format!("{}{path}/candidates", server.base))
        .json(&submission("export", "true", 300))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let admitted: Value = response.json().await.unwrap();
    wait_active(&client, &server, &admitted["executableDigest"]).await;
    let invalid = client
        .post(format!("{}{path}/candidates", server.base))
        .json(&submission("bad", "attributes.unknown", 300))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 422);
    for path in [
        BASE.to_string(),
        "/v3/decision-contract-catalog/current".into(),
        format!("{BASE}/versions"),
        format!("{path}/executables/history"),
    ] {
        assert!(
            client
                .get(format!("{}{path}", server.base))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        {
            let captured = collector.received.lock().unwrap();
            let names: std::collections::BTreeSet<_> = captured
                .metrics
                .iter()
                .flat_map(|request| &request.resource_metrics)
                .flat_map(|resource| &resource.scope_metrics)
                .flat_map(|scope| &scope.metrics)
                .map(|metric| metric.name.as_str())
                .collect();
            let have_logs = captured
                .logs
                .iter()
                .flat_map(|request| &request.resource_logs)
                .flat_map(|resource| &resource.scope_logs)
                .flat_map(|scope| &scope.log_records)
                .any(|record| {
                    string(&record.attributes, "event.name").as_deref()
                        == Some("flaggo.candidate.rejected")
                });
            let have_resolution = captured
                .logs
                .iter()
                .flat_map(|request| &request.resource_logs)
                .flat_map(|resource| &resource.scope_logs)
                .flat_map(|scope| &scope.log_records)
                .any(|record| {
                    string(&record.attributes, "event.name").as_deref()
                        == Some("flaggo.activation.resolved")
                });
            if names.contains("flaggo.contract.operations")
                && names.contains("flaggo.contract.operation.duration")
                && names.contains("flaggo.candidate.submissions")
                && names.contains("flaggo.activation.attempts")
                && have_logs
                && have_resolution
                && !captured.traces.is_empty()
            {
                break;
            }
        }
        assert!(Instant::now() < deadline, "Missing Contract OTLP signals");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    {
        use opentelemetry_proto::tonic::metrics::v1::metric;
        let captured = collector.received.lock().unwrap();
        let mut span_names = std::collections::BTreeSet::new();
        let mut spans = Vec::new();
        for resource in captured
            .traces
            .iter()
            .flat_map(|request| &request.resource_spans)
        {
            assert_eq!(
                string(
                    &resource.resource.as_ref().unwrap().attributes,
                    "service.name"
                )
                .as_deref(),
                Some("flaggo-contract-service")
            );
            for scope in &resource.scope_spans {
                assert_eq!(
                    scope.scope.as_ref().unwrap().name,
                    "flaggo.contract-service"
                );
                assert_eq!(
                    scope.scope.as_ref().unwrap().version,
                    env!("CARGO_PKG_VERSION")
                );
                for span in &scope.spans {
                    span_names.insert(span.name.as_str());
                    spans.push(span);
                }
            }
        }
        for name in [
            "flaggo.contract.deploy",
            "flaggo.contract.validate",
            "flaggo.candidate.submit",
            "flaggo.candidate.activate",
        ] {
            assert!(span_names.contains(name), "Missing {name}");
        }
        let deploy = spans
            .iter()
            .find(|span| span.name == "flaggo.contract.deploy")
            .unwrap();
        let validate = spans
            .iter()
            .find(|span| {
                span.name == "flaggo.contract.validate" && span.parent_span_id == deploy.span_id
            })
            .unwrap();
        assert_eq!(validate.trace_id, deploy.trace_id);
        assert_eq!(
            string(&deploy.attributes, "flaggo.request.correlation_id").as_deref(),
            Some("contract-export")
        );
        assert_eq!(
            string(&deploy.attributes, "flaggo.contract.digest"),
            version["contractDigest"].as_str().map(str::to_string)
        );
        assert_eq!(
            deploy.trace_id,
            [
                1, 35, 69, 103, 137, 171, 205, 239, 1, 35, 69, 103, 137, 171, 205, 239
            ]
        );
        let mut rejected_activation_metric = false;
        for scope in captured
            .metrics
            .iter()
            .flat_map(|request| &request.resource_metrics)
            .flat_map(|resource| &resource.scope_metrics)
        {
            assert_eq!(
                scope.scope.as_ref().unwrap().name,
                "flaggo.contract-service"
            );
            assert_eq!(
                scope.scope.as_ref().unwrap().version,
                env!("CARGO_PKG_VERSION")
            );
            for metric in &scope.metrics {
                let attributes: Vec<_> = match metric.data.as_ref().unwrap() {
                    metric::Data::Sum(sum) => sum
                        .data_points
                        .iter()
                        .map(|point| &point.attributes)
                        .collect(),
                    metric::Data::Histogram(histogram) => histogram
                        .data_points
                        .iter()
                        .map(|point| &point.attributes)
                        .collect(),
                    _ => panic!("Unexpected metric data: {}", metric.name),
                };
                for attributes in attributes {
                    if metric.name == "flaggo.activation.attempts"
                        && string(attributes, "flaggo.operation.outcome").as_deref()
                            == Some("rejected")
                        && string(attributes, "flaggo.failure.category").as_deref()
                            == Some("integrity")
                    {
                        rejected_activation_metric = true;
                    }
                    for attribute in attributes {
                        assert!(
                            matches!(
                                attribute.key.as_str(),
                                "flaggo.operation.name"
                                    | "flaggo.operation.outcome"
                                    | "flaggo.failure.category"
                                    | "http.request.method"
                                    | "http.response.status_code"
                                    | "http.route"
                            ),
                            "Unbounded dimension: {}",
                            attribute.key
                        );
                    }
                    if metric.name == "flaggo.candidate.submissions"
                        || metric.name == "flaggo.activation.attempts"
                    {
                        assert!(
                            attributes
                                .iter()
                                .all(|attribute| attribute.key != "flaggo.operation.name")
                        );
                    }
                }
            }
        }
        assert!(
            rejected_activation_metric,
            "Missing emitted integrity-rejection activation metric"
        );
        assert_eq!(
            spans
                .iter()
                .filter(|span| span.name == "flaggo.contract.validate")
                .count(),
            1,
            "Body decoding must precede lifecycle instrumentation"
        );
        let mut events = std::collections::BTreeSet::new();
        for scope in captured
            .logs
            .iter()
            .flat_map(|request| &request.resource_logs)
            .flat_map(|resource| &resource.scope_logs)
        {
            assert_eq!(
                scope.scope.as_ref().unwrap().name,
                "flaggo.contract-service"
            );
            assert_eq!(
                scope.scope.as_ref().unwrap().version,
                env!("CARGO_PKG_VERSION")
            );
            for record in &scope.log_records {
                if let Some(event) = string(&record.attributes, "event.name") {
                    events.insert(event);
                }
            }
        }
        for event in [
            "flaggo.candidate.admitted",
            "flaggo.activation.completed",
            "flaggo.candidate.rejected",
            "flaggo.activation.resolved",
        ] {
            assert!(events.contains(event), "Missing {event}");
        }
    }
    drop(server);
    collector.close().await;
    fixture.close().await;
}

#[tokio::test]
#[ignore = "Requires built Release .NET Contract and Decision Service reference hosts"]
async fn actual_dotnet_management_and_cross_producer_activation() {
    for producer in ["dotnet", "rust"] {
        let fixture = Fixture::new().await;
        let database = fixture.directory.path().join("http.db");
        let log = fixture.directory.path().join("producer.log");
        let produced = if producer == "dotnet" {
            Server::dotnet("contract", &database, &log).await
        } else {
            Server::rust(&database, &log, None, "300000").await
        };
        let client = client();
        let response = client
            .put(format!("{}{BASE}", produced.base))
            .json(&contract())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let version: Value = response.json().await.unwrap();
        assert_eq!(
            version["contractDigest"],
            "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098"
        );
        drop(produced);
        let reference = Server::dotnet(
            "contract",
            &database,
            &fixture.directory.path().join("reference.log"),
        )
        .await;
        let replacement = Server::rust(
            &database,
            &fixture.directory.path().join("replacement.log"),
            None,
            "300000",
        )
        .await;
        let path = version_path(&version);
        let missing = format!("sha256:{}", "0".repeat(64));
        let mut cases = vec![
            ("GET", BASE.to_string(), None, 200),
            ("GET", path.clone(), None, 200),
            ("GET", format!("{BASE}/versions?limit=1"), None, 200),
            (
                "GET",
                "/v3/decision-contract-catalog/current".into(),
                None,
                200,
            ),
            ("GET", format!("{path}/executables"), None, 200),
            ("GET", format!("{path}/executables/history"), None, 200),
            ("POST", format!("{BASE}/validate"), Some(contract()), 200),
            ("PUT", BASE.to_string(), Some(contract()), 200),
            ("GET", format!("{BASE}/versions/{missing}"), None, 404),
            ("GET", format!("{BASE}/versions/invalid"), None, 400),
            ("GET", format!("/V3/DECISION-CONTRACTS/{NAME}/"), None, 200),
            (
                "GET",
                format!("/v3/decision-contracts/{}/", NAME.to_ascii_lowercase()),
                None,
                404,
            ),
            ("GET", "/missing".into(), None, 404),
            ("POST", BASE.to_string(), None, 405),
        ];
        for query in [
            "limit=0",
            "limit=201",
            "limit=bad",
            "cursor=bad",
            "limit=1&limit=2",
            "limit=+1",
            "LIMIT=1",
            "limit=",
        ] {
            cases.push((
                "GET",
                format!("{BASE}/versions?{query}"),
                None,
                if ["limit=+1", "LIMIT=1"].contains(&query) {
                    200
                } else {
                    400
                },
            ));
        }
        let mut invalid = contract();
        invalid["result"]["default"] = 1.into();
        cases.push((
            "POST",
            format!("{BASE}/validate"),
            Some(invalid.clone()),
            200,
        ));
        cases.push(("PUT", BASE.into(), Some(invalid), 422));
        let mut mismatch = contract();
        mismatch["name"] = "other".into();
        cases.push(("PUT", BASE.into(), Some(mismatch), 409));
        let mut authority = contract();
        authority["authority"]["tenant"] = "other".into();
        cases.push(("PUT", BASE.into(), Some(authority), 409));
        cases.push((
            "POST",
            format!("{path}/candidates"),
            Some(submission("bad-expression", "attributes.unknown", 300)),
            422,
        ));
        cases.push((
            "POST",
            format!("{BASE}/versions/{missing}/candidates"),
            Some(submission("missing", "true", 300)),
            404,
        ));
        for bad in [" ".into(), "\u{1f600}".repeat(129)] {
            let mut candidate = submission("invalid-provenance", "true", 300);
            candidate["provenance"]["cycleId"] = bad.into();
            cases.push(("POST", format!("{path}/candidates"), Some(candidate), 400));
        }
        for body in [
            json!({"rules": [], "provenance": null}),
            json!({"rules": null, "provenance": {}}),
        ] {
            cases.push(("POST", format!("{path}/candidates"), Some(body), 400));
        }
        let active = fixture
            .lifecycle
            .executables
            .get_active(version["contractDigest"].as_str().unwrap(), None)
            .await
            .unwrap()
            .unwrap();
        let command = json!({"expectedActiveExecutableDigest": active.executable_digest,
            "expectedActiveStateVersion": active.state_version, "expectedCandidateStateVersion": 0});
        cases.push((
            "POST",
            format!("{path}/executables/{missing}/activate"),
            Some(command.clone()),
            404,
        ));
        cases.push((
            "POST",
            format!("{path}/executables/{}/activate", active.executable_digest),
            Some(command.clone()),
            409,
        ));
        let mut negative = command;
        negative["expectedCandidateStateVersion"] = (-1).into();
        cases.push((
            "POST",
            format!("{path}/executables/{missing}/activate"),
            Some(negative),
            400,
        ));
        let mut comparisons = cases.len();
        for (method, path, body, expected) in cases {
            let mut replies = Vec::new();
            for server in [&reference, &replacement] {
                let mut request = client
                    .request(method.parse().unwrap(), format!("{}{path}", server.base))
                    .header("x-flaggo-correlation-id", "management-parity");
                if let Some(body) = &body {
                    request = request.json(body);
                }
                let response = request.send().await.unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    expected,
                    "{producer} {method} {path}: {:?}",
                    response.text().await
                );
                assert_eq!(
                    response.headers()["x-flaggo-correlation-id"],
                    "management-parity"
                );
                let etag = response.headers().get("etag").cloned();
                let location = response.headers().get("location").cloned();
                let mut body: Value = response.json().await.unwrap();
                normalize_numeric_spelling(&mut body);
                normalize_native_diagnostic(&mut body);
                replies.push((etag, location, body));
            }
            assert_eq!(replies[0], replies[1], "{producer} {method} {path}");
        }
        let producer_server = if producer == "dotnet" {
            &reference
        } else {
            &replacement
        };
        let response = client
            .post(format!("{}{path}/candidates", producer_server.base))
            .json(&submission(
                "cross-producer",
                "attributes.board_pressure >= 0.5",
                300,
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let admitted: Value = response.json().await.unwrap();
        let digest = admitted["executableDigest"].as_str().unwrap();
        let before: Vec<u8> = sqlx::query_scalar(
            "SELECT checked_executable_json FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        if producer == "dotnet" {
            let stored = fixture
                .lifecycle
                .executables
                .get(digest, None)
                .await
                .unwrap()
                .unwrap();
            let accepted = fixture
                .lifecycle
                .contracts
                .get(NAME, version["contractDigest"].as_str().unwrap(), None)
                .await
                .unwrap()
                .unwrap();
            let rust = flaggo_expression::ExecutableCompiler::default()
                .compile(&accepted.contract, &stored.executable)
                .unwrap();
            assert_ne!(
                serde_json::to_value(&stored.checked_executable).unwrap(),
                serde_json::to_value(&rust.checked_executable).unwrap(),
                "This gate must exercise different valid compiler artifacts, not just replay identical Rust bytes"
            );
        }
        let mut replays = Vec::new();
        for server in [&reference, &replacement] {
            let response = client
                .post(format!("{}{path}/candidates", server.base))
                .json(&submission(
                    "cross-producer",
                    "attributes.board_pressure >= 0.5",
                    300,
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let replay: Value = response.json().await.unwrap();
            assert_eq!(replay["created"], false);
            assert_eq!(replay["createdAt"], admitted["createdAt"]);
            assert_eq!(replay["executableDigest"], admitted["executableDigest"]);
            replays.push(replay);
        }
        assert_eq!(replays[0], replays[1]);
        comparisons += 1;
        for suffix in ["executables", "executables/history"] {
            let mut replies = Vec::new();
            for server in [&reference, &replacement] {
                let response = client
                    .get(format!("{}{path}/{suffix}?limit=1", server.base))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let mut body: Value = response.json().await.unwrap();
                normalize_numeric_spelling(&mut body);
                replies.push(body);
            }
            assert_eq!(replies[0], replies[1]);
            comparisons += 1;
        }
        drop(reference);
        drop(replacement);
        let activated = Server::rust(
            &database,
            &fixture.directory.path().join("activation.log"),
            None,
            "300000",
        )
        .await;
        wait_active(&client, &activated, &admitted["executableDigest"]).await;
        let after: Vec<u8> = sqlx::query_scalar(
            "SELECT checked_executable_json FROM decision_executables WHERE executable_digest = ?",
        )
        .bind(digest)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(after, before, "Worker must not rewrite producer artifact");
        let decision = Server::dotnet(
            "decision",
            &database,
            &fixture.directory.path().join("decision.log"),
        )
        .await;
        let response = client.post(format!("{}{path}/decisions", decision.base))
            .json(&json!({"attributes": {"board_pressure": 0.7, "current_level": 1, "recent_placement_time_ms": 10,
                "recovery_failures": 0, "session_id": "test", "_random": 0.5}})).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let result: Value = response.json().await.unwrap();
        assert_eq!(result["executableDigest"], admitted["executableDigest"]);
        assert_eq!(result["result"], 300);
        let input = flaggo_contract::strict_json::deserialize(
            &serde_json::to_vec(&json!({"attributes": {
                "board_pressure": 0.7, "current_level": 1, "recent_placement_time_ms": 10,
                "recovery_failures": 0, "session_id": "test", "_random": 0.5
            }}))
            .unwrap(),
        )
        .unwrap();
        let runtime = flaggo_decision::DecisionRuntime::new(
            fixture.lifecycle.contracts.clone(),
            fixture.lifecycle.executables.clone(),
        );
        let rust_result = runtime
            .decide(
                NAME,
                version["contractDigest"].as_str().unwrap(),
                &input,
                None,
            )
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&rust_result).unwrap(), result);
        sqlx::query(
            "UPDATE decision_contract_versions SET accepted_at = ? WHERE contract_digest = ?",
        )
        .bind("invalid-timestamp")
        .bind(version["contractDigest"].as_str().unwrap())
        .execute(&fixture.pool)
        .await
        .unwrap();
        let timestamp_reference = Server::dotnet(
            "contract",
            &database,
            &fixture.directory.path().join("timestamp-reference.log"),
        )
        .await;
        let mut timestamp_replies = Vec::new();
        for server in [&timestamp_reference, &activated] {
            let response = client
                .get(format!("{}{BASE}", server.base))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 400);
            timestamp_replies.push(response.json::<Value>().await.unwrap());
        }
        assert_eq!(timestamp_replies[0], timestamp_replies[1]);
        comparisons += 1;
        drop(timestamp_reference);
        drop(decision);
        drop(activated);
        fixture.close().await;
        println!(
            "Verified {producer} producer: {comparisons} management comparisons, different compiler artifacts, unchanged persistence, Rust activation and both runtimes"
        );
        println!(
            "FLAGGO_HTTP_PARITY {}",
            json!({"format": "flaggo.http-parity/v1", "service": "contract",
                "producer": producer, "comparisons": comparisons})
        );
    }
}

fn normalize_numeric_spelling(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_numeric_spelling),
        Value::Object(object) => object.values_mut().for_each(normalize_numeric_spelling),
        Value::Number(number) => {
            if number.as_i64().is_none()
                && let Some(number) = number.as_f64()
                && number.fract() == 0.0
                && number.abs() <= 9_007_199_254_740_991.0
            {
                *value = Value::from(number as i64);
            }
        }
        _ => {}
    }
}

fn normalize_native_diagnostic(body: &mut Value) {
    if body["type"] == "https://flaggo.dev/problems/invalid-request"
        && body["detail"]
            .as_str()
            .is_some_and(|detail| detail.starts_with("Request body is invalid: "))
    {
        body["detail"] = "Request body is invalid: <native diagnostic>".into();
    }
}
