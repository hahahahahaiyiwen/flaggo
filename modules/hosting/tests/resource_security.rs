use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    routing::{get, post},
};
use flaggo_contract::ResourceScope;
use flaggo_service_hosting::{
    Access, Operation, Principal, RequestSecurity, SecurityError, with_http_contract,
    with_resource_security,
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
    async fn admitted(State(calls): State<Arc<AtomicUsize>>) -> StatusCode {
        calls.fetch_add(1, Ordering::SeqCst);
        StatusCode::CREATED
    }
    with_http_contract(
        with_resource_security(
            Router::new()
                .route(ROUTE, post(admitted))
                .route("/health/live", get(|| async { StatusCode::OK }))
                .with_state(calls),
            Arc::new(Policy),
        ),
        "flaggo.security-test",
        "1",
        ROUTES,
    )
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
