use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::Body,
    extract::{Extension, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    routing::{get, post},
};
use flaggo_contract::ResourceScope;
use flaggo_service_hosting::{
    Access, CORRELATION_HEADER, Operation, Principal, RequestAdmission, RequestAdmissionError,
    RequestSecurity, SecurityError, with_http_contract, with_resource_security,
};
use tower::ServiceExt;

const ROUTE: &str = "/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}/candidates";
const ROUTES: &[&str] = &[ROUTE, "/health/live"];

struct Policy;

#[async_trait::async_trait]
impl RequestSecurity for Policy {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, SecurityError> {
        let subject = match headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
        {
            Some("Bearer writer") => "writer",
            Some("Bearer reader") => "reader",
            Some("Bearer unavailable") => {
                return Err(SecurityError::Unavailable(
                    "injected identity outage".into(),
                ));
            }
            _ => {
                return Err(SecurityError::Unauthenticated {
                    challenge: HeaderValue::from_static("Bearer"),
                });
            }
        };
        Ok(Principal {
            subject: subject.into(),
        })
    }

    async fn authorize(
        &self,
        principal: &Principal,
        scope: &ResourceScope,
        name: Option<&str>,
        operation: Operation,
    ) -> Result<Access, SecurityError> {
        if scope.tenant == "unavailable" {
            return Err(SecurityError::Unavailable(
                "injected authority outage".into(),
            ));
        }
        if scope.tenant != "visible"
            || scope.application != "app"
            || scope.environment != "test"
            || name != Some("decision")
        {
            return Ok(Access::Hidden);
        }
        Ok(
            if principal.subject == "writer" && operation == Operation::SubmitCandidate {
                Access::Allowed
            } else {
                Access::Forbidden
            },
        )
    }
}

fn app(calls: Arc<AtomicUsize>) -> Router {
    secured_app(calls, Arc::new(Policy))
}

fn secured_app(calls: Arc<AtomicUsize>, security: Arc<dyn RequestSecurity>) -> Router {
    async fn admitted(
        State(calls): State<Arc<AtomicUsize>>,
        Extension(principal): Extension<Principal>,
    ) -> StatusCode {
        assert_eq!(principal.subject, "writer");
        calls.fetch_add(1, Ordering::SeqCst);
        StatusCode::CREATED
    }
    with_http_contract(
        with_resource_security(
            Router::new()
                .route(ROUTE, post(admitted))
                .route("/health/live", get(|| async { StatusCode::OK }))
                .with_state(calls),
            security,
        ),
        "flaggo.security-test",
        "1",
        ROUTES,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AdmissionCall {
    principal: Principal,
    scope: ResourceScope,
    name: Option<String>,
    operation: Operation,
}

struct RecordingAdmission {
    inner: Arc<dyn RequestSecurity>,
    result: Result<RequestAdmission, RequestAdmissionError>,
    authenticated: AtomicUsize,
    authorized: Mutex<Vec<AdmissionCall>>,
    admitted: Mutex<Vec<AdmissionCall>>,
    pause_first: bool,
}

impl RecordingAdmission {
    fn new(
        inner: Arc<dyn RequestSecurity>,
        result: Result<RequestAdmission, RequestAdmissionError>,
    ) -> Self {
        Self {
            inner,
            result,
            authenticated: AtomicUsize::new(0),
            authorized: Mutex::new(Vec::new()),
            admitted: Mutex::new(Vec::new()),
            pause_first: false,
        }
    }
}

#[async_trait::async_trait]
impl RequestSecurity for RecordingAdmission {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, SecurityError> {
        self.authenticated.fetch_add(1, Ordering::SeqCst);
        self.inner.authenticate(headers).await
    }

    async fn authorize(
        &self,
        principal: &Principal,
        scope: &ResourceScope,
        name: Option<&str>,
        operation: Operation,
    ) -> Result<Access, SecurityError> {
        self.authorized.lock().unwrap().push(AdmissionCall {
            principal: principal.clone(),
            scope: scope.clone(),
            name: name.map(str::to_owned),
            operation,
        });
        self.inner
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
        let first = {
            let mut calls = self.admitted.lock().unwrap();
            calls.push(AdmissionCall {
                principal: principal.clone(),
                scope: scope.clone(),
                name: name.map(str::to_owned),
                operation,
            });
            calls.len() == 1
        };
        if first && self.pause_first {
            std::future::pending::<()>().await;
        }
        self.result
    }
}

fn limited() -> Result<RequestAdmission, RequestAdmissionError> {
    Ok(RequestAdmission::Limited {
        retry_after_seconds: NonZeroU32::new(17).unwrap(),
    })
}

#[tokio::test]
async fn denials_never_invoke_even_a_failing_admission_authority() {
    for result in [limited(), Err(RequestAdmissionError::Unavailable)] {
        let security = Arc::new(RecordingAdmission::new(Arc::new(Policy), result));
        let calls = Arc::new(AtomicUsize::new(0));
        let health = secured_app(calls.clone(), security.clone())
            .oneshot(Request::get("/HEALTH/LIVE/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(security.authenticated.load(Ordering::SeqCst), 0);
        assert!(security.authorized.lock().unwrap().is_empty());
        assert!(security.admitted.lock().unwrap().is_empty());
        for (tenant, token, status, kind) in [
            (
                "visible",
                "invalid",
                StatusCode::UNAUTHORIZED,
                "authentication-required",
            ),
            (
                "visible",
                "unavailable",
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization-unavailable",
            ),
            (
                "hidden",
                "writer",
                StatusCode::NOT_FOUND,
                "resource-not-found",
            ),
            (
                "visible",
                "reader",
                StatusCode::FORBIDDEN,
                "operation-forbidden",
            ),
            (
                "unavailable",
                "writer",
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization-unavailable",
            ),
        ] {
            let path = format!(
                "/v4/tenants/{tenant}/applications/app/environments/test/decision-contracts/decision/candidates"
            );
            let response = secured_app(calls.clone(), security.clone())
                .oneshot(
                    Request::post(&path)
                        .header("authorization", format!("Bearer {token}"))
                        .header(CORRELATION_HEADER, "denied-attempt")
                        .body(Body::from("invalid body"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[CORRELATION_HEADER], "denied-attempt");
            assert!(!response.headers().contains_key("retry-after"));
            if status == StatusCode::UNAUTHORIZED {
                assert_eq!(response.headers()["www-authenticate"], "Bearer");
            }
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let problem: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                problem["type"],
                format!("https://flaggo.dev/problems/{kind}")
            );
            assert_eq!(problem["status"], status.as_u16());
            assert_eq!(problem["instance"], path);
            assert!(!String::from_utf8_lossy(&bytes).contains("injected"));
        }
        assert!(security.admitted.lock().unwrap().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn admission_runs_once_and_uses_shared_problem_and_correlation_contract() {
    for (result, status, kind) in [
        (
            limited(),
            StatusCode::TOO_MANY_REQUESTS,
            "request-quota-exceeded",
        ),
        (
            Err(RequestAdmissionError::Unavailable),
            StatusCode::SERVICE_UNAVAILABLE,
            "request-admission-unavailable",
        ),
        (Ok(RequestAdmission::Accepted), StatusCode::CREATED, ""),
    ] {
        let security = Arc::new(RecordingAdmission::new(Arc::new(Policy), result));
        let calls = Arc::new(AtomicUsize::new(0));
        let path = "/V4/TENANTS/visible/APPLICATIONS/app/ENVIRONMENTS/test/DECISION-CONTRACTS/decision/CANDIDATES/";
        let response = secured_app(calls.clone(), security.clone())
            .oneshot(
                Request::post(path)
                    .header("authorization", "Bearer writer")
                    .header(CORRELATION_HEADER, "authorized-attempt")
                    .body(Body::from("invalid body"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[CORRELATION_HEADER], "authorized-attempt");
        assert!(!response.headers().contains_key("www-authenticate"));
        if status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(response.headers()["retry-after"], "17");
        } else {
            assert!(!response.headers().contains_key("retry-after"));
        }
        if status != StatusCode::CREATED {
            assert_eq!(
                response.headers()["content-type"],
                "application/problem+json"
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let problem: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                problem["type"],
                format!("https://flaggo.dev/problems/{kind}")
            );
            assert_eq!(problem["status"], status.as_u16());
            assert_eq!(problem["instance"], path);
            assert!(problem["title"].is_string());
            assert!(problem["detail"].is_string());
            assert_eq!(problem.as_object().unwrap().len(), 5);
        }
        let expected = vec![AdmissionCall {
            principal: Principal {
                subject: "writer".into(),
            },
            scope: ResourceScope::new("visible".into(), "app".into(), "test".into()).unwrap(),
            name: Some("decision".into()),
            operation: Operation::SubmitCandidate,
        }];
        assert_eq!(*security.admitted.lock().unwrap(), expected);
        assert_eq!(*security.authorized.lock().unwrap(), expected);
        assert_eq!(security.authenticated.load(Ordering::SeqCst), 1);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(status == StatusCode::CREATED)
        );
    }
}

#[tokio::test]
async fn cancellation_never_reverses_an_attempt_and_retry_invokes_admission_again() {
    for cancel_during_admission in [false, true] {
        let mut security =
            RecordingAdmission::new(Arc::new(Policy), Ok(RequestAdmission::Accepted));
        security.pause_first = cancel_during_admission;
        let security = Arc::new(security);
        let handlers = Arc::new(AtomicUsize::new(0));
        let router = with_http_contract(
            with_resource_security(
                Router::new().route(
                    ROUTE,
                    post({
                        let handlers = handlers.clone();
                        move || {
                            let handlers = handlers.clone();
                            async move {
                                handlers.fetch_add(1, Ordering::SeqCst);
                                std::future::pending::<StatusCode>().await
                            }
                        }
                    }),
                ),
                security.clone(),
            ),
            "flaggo.security-test",
            "1",
            ROUTES,
        );
        let path = "/v4/tenants/visible/applications/app/environments/test/decision-contracts/decision/candidates";
        let request = Request::post(path)
            .header("authorization", "Bearer writer")
            .body(Body::empty())
            .unwrap();
        let mut future = Box::pin(router.oneshot(request));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(security.admitted.lock().unwrap().len(), 1);
        assert_eq!(
            handlers.load(Ordering::SeqCst),
            usize::from(!cancel_during_admission)
        );
        drop(future);
        assert_eq!(security.admitted.lock().unwrap().len(), 1);
        let retry = secured_app(handlers.clone(), security.clone())
            .oneshot(
                Request::post(path)
                    .header("authorization", "Bearer writer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retry.status(), StatusCode::CREATED);
        assert_eq!(security.admitted.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn authenticates_independently_hides_foreign_resources_and_fails_closed() {
    let calls = Arc::new(AtomicUsize::new(0));
    for (tenant, authorization, expected) in [
        ("visible", None, StatusCode::UNAUTHORIZED),
        ("visible", Some("Bearer invalid"), StatusCode::UNAUTHORIZED),
        ("hidden", Some("Bearer writer"), StatusCode::NOT_FOUND),
        ("visible", Some("Bearer reader"), StatusCode::FORBIDDEN),
        (
            "unavailable",
            Some("Bearer writer"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        ("visible", Some("Bearer writer"), StatusCode::CREATED),
    ] {
        let mut request = Request::post(format!(
            "/v4/tenants/{tenant}/applications/app/environments/test/decision-contracts/decision/candidates",
        ));
        if let Some(authorization) = authorization {
            request = request.header("authorization", authorization);
        }
        let response = app(calls.clone())
            .oneshot(request.body(Body::from("untrusted declaration")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected != StatusCode::CREATED {
            assert_eq!(
                response.headers()["content-type"],
                "application/problem+json"
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let problem: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(problem["status"], expected.as_u16());
            assert!(!String::from_utf8_lossy(&bytes).contains("injected authority outage"));
        }
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "denied requests cannot reach mutation handlers"
    );
}

#[tokio::test]
async fn normalized_routes_keep_authorization_and_health_semantics() {
    let calls = Arc::new(AtomicUsize::new(0));
    let health = app(calls.clone())
        .oneshot(Request::get("/HEALTH/LIVE/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let response = app(calls.clone()).oneshot(Request::post(
        "/V4/TENANTS/visible/APPLICATIONS/app/ENVIRONMENTS/test/DECISION-CONTRACTS/decision/CANDIDATES/",
    ).header("authorization", "Bearer writer").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let denied = app(calls).oneshot(Request::post(
        "/v4/tenants/visible/applications/app/environments/test/decision-contracts/decision/candidates",
    ).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(denied.headers()["www-authenticate"], "Bearer");
}
