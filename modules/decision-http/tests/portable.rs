use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use flaggo_contract::{
    ContractResourceId, DecisionContract, DecisionExecutable, ResourceScope, strict_json,
};
use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, ContractVersion, ContractVersionReader,
    ReadinessProbe, StoreReadError,
};
use flaggo_expression::ExecutableCompiler;
use flaggo_service_hosting::{
    Access, CORRELATION_HEADER, Operation, Principal, RequestAdmission, RequestAdmissionError,
    RequestSecurity, SecurityError, TrustedLocal,
};
use serde_json::{Value, json};
use tower::ServiceExt as _;

#[derive(Clone)]
struct Source {
    contract: Arc<ContractVersion>,
    active: Arc<Mutex<Arc<ActiveExecutable>>>,
    unavailable: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}

impl ContractVersionReader for Source {
    async fn get_contract(
        &self,
        resource: &ContractResourceId,
        digest: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(
            (*resource == ContractResourceId::try_from(&self.contract.contract).unwrap()
                && digest == self.contract.contract_digest)
                .then(|| self.contract.clone()),
        )
    }
}

impl ActiveExecutableReader for Source {
    async fn get_active(
        &self,
        _: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        if self.unavailable.load(Ordering::Acquire) {
            return Err(StoreReadError::Dependency(
                std::io::Error::other("authority unavailable").into(),
            ));
        }
        Ok(Some(self.active.lock().unwrap().clone()))
    }
}

impl ReadinessProbe for Source {
    async fn is_available(&self, _: Cancellation) -> Result<bool, StoreReadError> {
        Ok(!self.unavailable.load(Ordering::Acquire))
    }
}

fn executable(contract: &DecisionContract, digest: &str, result: u32) -> Arc<ActiveExecutable> {
    let executable: DecisionExecutable = strict_json::deserialize(&serde_json::to_vec(&json!({
        "contractDigest": digest, "kind": "rules",
        "rules": [{"name": "choose", "when": {"expression": "true"}, "return": {"value": result}}]
    })).unwrap()).unwrap();
    let compiled = ExecutableCompiler::default()
        .compile(contract, &executable)
        .unwrap();
    Arc::new(ActiveExecutable {
        executable_digest: compiled.executable_digest,
        executable: compiled.executable,
        checked_executable: compiled.checked_executable,
    })
}

#[tokio::test]
async fn injected_readers_observe_activation_and_never_serve_cached_authority_on_failure() {
    let contract: DecisionContract = strict_json::deserialize(
        br#"{
        "authority":{"tenant":"t","application":"a","environment":"e"},
        "name":"portable.example","expression_syntax":"flaggo.cel/v1",
        "attributes":[],"result":{"schema":{"type":"integer"},"default":99}
    }"#,
    )
    .unwrap();
    let digest = flaggo_contract::digests::contract_digest(&contract, None).unwrap();
    let first = executable(&contract, &digest, 100);
    let second = executable(&contract, &digest, 200);
    let source = Source {
        contract: Arc::new(ContractVersion {
            contract_digest: digest.clone(),
            contract,
        }),
        active: Arc::new(Mutex::new(first)),
        unavailable: Arc::new(AtomicBool::new(false)),
        reads: Arc::new(AtomicUsize::new(0)),
    };
    let router = flaggo_decision_http::router(
        source.clone(),
        source.clone(),
        Arc::new(flaggo_service_hosting::TrustedLocal),
    );
    let path = format!(
        "/v4/tenants/t/applications/a/environments/e/decision-contracts/portable.example/versions/{digest}/decisions"
    );
    for (status, expected) in [
        (StatusCode::OK, Some(100)),
        (StatusCode::OK, Some(200)),
        (StatusCode::SERVICE_UNAVAILABLE, None),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&path)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"attributes":{"_random":0.5}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap())
                .unwrap();
        if let Some(expected) = expected {
            assert_eq!(body["result"], expected);
        } else {
            assert_eq!(
                body["type"],
                "https://flaggo.dev/problems/dependency-unavailable"
            );
        }

        if expected == Some(100) {
            *source.active.lock().unwrap() = second.clone();
        } else {
            source.unavailable.store(true, Ordering::Release);
        }
    }
    let response = router
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

struct Admission {
    result: Result<RequestAdmission, RequestAdmissionError>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl RequestSecurity for Admission {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, SecurityError> {
        TrustedLocal.authenticate(headers).await
    }

    async fn authorize(
        &self,
        principal: &Principal,
        scope: &ResourceScope,
        name: Option<&str>,
        operation: Operation,
    ) -> Result<Access, SecurityError> {
        TrustedLocal
            .authorize(principal, scope, name, operation)
            .await
    }

    async fn admit_authorized(
        &self,
        principal: &Principal,
        scope: &ResourceScope,
        name: Option<&str>,
        operation: Operation,
    ) -> Result<RequestAdmission, RequestAdmissionError> {
        assert_eq!(principal.subject, "trusted-local");
        assert_eq!(
            scope,
            &ResourceScope::new("t".into(), "a".into(), "e".into()).unwrap()
        );
        assert_eq!(name, Some("portable.example"));
        assert_eq!(operation, Operation::Evaluate);
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result
    }
}

#[tokio::test]
async fn request_admission_precedes_runtime_body_validation_and_authority_reads() {
    let contract: DecisionContract = strict_json::deserialize(
        br#"{
                "authority":{"tenant":"t","application":"a","environment":"e"},
                "name":"portable.example","expression_syntax":"flaggo.cel/v1",
                "attributes":[],"result":{"schema":{"type":"integer"},"default":99}
            }"#,
    )
    .unwrap();
    let digest = flaggo_contract::digests::contract_digest(&contract, None).unwrap();
    let active = executable(&contract, &digest, 100);
    let source = Source {
        contract: Arc::new(ContractVersion {
            contract_digest: digest.clone(),
            contract,
        }),
        active: Arc::new(Mutex::new(active)),
        unavailable: Arc::new(AtomicBool::new(false)),
        reads: Arc::new(AtomicUsize::new(0)),
    };
    let missing = format!("sha256:{}", "0".repeat(64));
    for (result, admission_status, admission_kind) in [
        (
            Ok(RequestAdmission::Limited {
                retry_after_seconds: std::num::NonZeroU32::new(9).unwrap(),
            }),
            Some(StatusCode::TOO_MANY_REQUESTS),
            "request-quota-exceeded",
        ),
        (
            Err(RequestAdmissionError::Unavailable),
            Some(StatusCode::SERVICE_UNAVAILABLE),
            "request-admission-unavailable",
        ),
        (Ok(RequestAdmission::Accepted), None, ""),
    ] {
        let security = Arc::new(Admission {
            result,
            calls: AtomicUsize::new(0),
        });
        let router = flaggo_decision_http::router(source.clone(), source.clone(), security.clone());
        for path in ["/HEALTH/LIVE/", "/health/ready"] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(security.calls.load(Ordering::SeqCst), 0);
        for (index, (requested_digest, body, downstream_status, downstream_kind, reads)) in [
            (
                &missing,
                r#"{"attributes":{"_random":0.5}}"#,
                StatusCode::NOT_FOUND,
                "contract-version-not-found",
                1,
            ),
            (&digest, "{", StatusCode::BAD_REQUEST, "invalid-request", 0),
            (
                &digest,
                r#"{"attributes":{}}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-runtime-input",
                1,
            ),
            (
                &digest,
                r#"{"attributes":{"_random":0.5}}"#,
                StatusCode::OK,
                "",
                1,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            source.reads.store(0, Ordering::SeqCst);
            let path = format!(
                "/v4/tenants/t/applications/a/environments/e/decision-contracts/portable.example/versions/{requested_digest}/decisions"
            );
            let response = router
                .clone()
                .oneshot(
                    Request::post(&path)
                        .header("content-type", "application/json")
                        .header(CORRELATION_HEADER, "runtime-admission")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            let expected = admission_status.unwrap_or(downstream_status);
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[CORRELATION_HEADER], "runtime-admission");
            if expected == StatusCode::TOO_MANY_REQUESTS {
                assert_eq!(response.headers()["retry-after"], "9");
            } else {
                assert!(!response.headers().contains_key("retry-after"));
            }
            assert_eq!(security.calls.load(Ordering::SeqCst), index + 1);
            assert_eq!(
                source.reads.load(Ordering::SeqCst),
                if admission_status.is_some() { 0 } else { reads }
            );
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            if expected != StatusCode::OK {
                let kind = if admission_status.is_some() {
                    admission_kind
                } else {
                    downstream_kind
                };
                assert_eq!(body["type"], format!("https://flaggo.dev/problems/{kind}"));
                assert_eq!(body["status"], expected.as_u16());
                assert_eq!(body["instance"], path);
            }
        }
    }
}
