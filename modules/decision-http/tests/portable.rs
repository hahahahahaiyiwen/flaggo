use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use flaggo_contract::{ContractResourceId, DecisionContract, DecisionExecutable, strict_json};
use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, ContractVersion, ContractVersionReader,
    ReadinessProbe, StoreReadError,
};
use flaggo_expression::ExecutableCompiler;
use serde_json::{Value, json};
use tower::ServiceExt as _;

#[derive(Clone)]
struct Source {
    contract: Arc<ContractVersion>,
    active: Arc<Mutex<Arc<ActiveExecutable>>>,
    unavailable: Arc<AtomicBool>,
}

impl ContractVersionReader for Source {
    async fn get_contract(
        &self,
        resource: &ContractResourceId,
        digest: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
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
