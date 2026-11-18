//! Outcome-oriented authority scenarios for adapter authors.
//! Each function requires fresh storage and two independently constructed clients.
//! These local scenarios are not multi-host qualification or scoped/authenticated v4 tests.
use std::sync::Arc;

use async_trait::async_trait;
use flaggo_contract::{
    AnalysisCandidateSubmission, DecisionContract, DecisionExecutableActivationRequest, strict_json,
};
use flaggo_contract_application::{ContractLifecycle, ServiceError};
use flaggo_executable_store::{ContractAuthority, ExecutableLifecycleState};
use serde_json::json;

pub mod evidence;

pub const FIXTURE_REVISION: &str = "flaggo.authority-conformance/v1";

#[async_trait]
pub trait AuthorityFixture {
    fn client(&self) -> Arc<dyn ContractAuthority>;
    fn independent_client(&self) -> Arc<dyn ContractAuthority>;
    /// Fail current publication after contract/artifact staging, at the commit boundary.
    async fn fail_publication(&self, enabled: bool);
}

pub fn contract() -> DecisionContract {
    #[derive(serde::Deserialize)]
    struct Fixture {
        request: Request,
    }
    #[derive(serde::Deserialize)]
    struct Request {
        body: DecisionContract,
    }
    let fixture: Fixture = strict_json::deserialize(include_bytes!(
        "../../../contracts/fixtures/management/decision-contract/01-validate-valid.json"
    ))
    .unwrap();
    fixture.request.body
}

fn candidate(cycle: &str, value: u32) -> AnalysisCandidateSubmission {
    strict_json::deserialize(&serde_json::to_vec(&json!({
        "rules": [{"name": "learned", "when": {"expression": "true"}, "return": {"value": value}}],
        "provenance": {
            "workspaceId": format!("sha256:{}", "a".repeat(64)),
            "cycleId": cycle, "attemptId": "attempt-1",
            "analysisManifestDigest": format!("sha256:{}", "b".repeat(64))
        }
    })).unwrap()).unwrap()
}

pub async fn c03_publication(fixture: &impl AuthorityFixture) {
    let first = ContractLifecycle::new(fixture.client());
    let second = ContractLifecycle::new(fixture.independent_client());
    let contract = contract();
    let (a, b) = tokio::join!(
        first.deploy(&contract.name, &contract),
        second.deploy(&contract.name, &contract),
    );
    let (a, created_a) = a.unwrap();
    let (b, created_b) = b.unwrap();
    assert_ne!(created_a, created_b, "exactly one same-version insertion");
    assert_eq!(a, b, "retry preserves acceptance identity and activation");
    let active = fixture
        .independent_client()
        .get_active_executable(&a.contract_digest)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.executable_digest, a.active_executable_digest);
    assert_eq!(active.state, ExecutableLifecycleState::Active);

    let mut newer = contract.clone();
    newer.result.default = json!(600).into();
    let newer = second.deploy(&newer.name, &newer).await.unwrap().0;
    assert_eq!(
        first
            .get(&contract.name, None)
            .await
            .unwrap()
            .unwrap()
            .contract_digest,
        newer.contract_digest
    );
    let redeployed = first.deploy(&contract.name, &contract).await.unwrap();
    assert!(!redeployed.1);
    assert_eq!(second.get(&contract.name, None).await.unwrap().unwrap(), a);

    let mut other_owner = contract.clone();
    other_owner.authority.tenant = Some("different-tenant".into());
    assert!(matches!(
        second.deploy(&contract.name, &other_owner).await,
        Err(ServiceError::ContractStore(
            flaggo_contract_store::ContractStoreError::AuthorityConflict { .. }
        ))
    ));
}

pub async fn c03_atomic_failure(fixture: &impl AuthorityFixture) {
    let lifecycle = ContractLifecycle::new(fixture.client());
    let contract = contract();
    let digest = lifecycle
        .validate(&contract.name, &contract)
        .await
        .unwrap()
        .contract_digest
        .unwrap();
    fixture.fail_publication(true).await;
    assert!(lifecycle.deploy(&contract.name, &contract).await.is_err());
    let reader = fixture.independent_client();
    assert!(
        reader
            .get_contract(&contract.name, &digest)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .get_current_contract(&contract.name)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .get_active_executable(&digest)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .list_executables(&digest, 200, None, true)
            .await
            .unwrap()
            .executables
            .is_empty()
    );
    fixture.fail_publication(false).await;
    let mut other_owner = contract.clone();
    other_owner.authority.tenant = Some("different-tenant".into());
    assert!(
        lifecycle
            .deploy(&other_owner.name, &other_owner)
            .await
            .unwrap()
            .1,
        "failed publication must not reserve a name"
    );
}

pub async fn c04_admission(fixture: &impl AuthorityFixture) {
    let first = ContractLifecycle::new(fixture.client());
    let second = ContractLifecycle::new(fixture.independent_client());
    let contract = contract();
    let version = first.deploy(&contract.name, &contract).await.unwrap().0;
    let digest = &version.contract_digest;
    let (a, b) = tokio::join!(
        first.submit(&contract.name, digest, candidate("same-cycle", 400)),
        second.submit(&contract.name, digest, candidate("same-cycle", 400)),
    );
    let (a, created_a) = a.unwrap();
    let (b, created_b) = b.unwrap();
    assert_ne!(created_a, created_b);
    assert_eq!(a.executable_digest, b.executable_digest);
    assert_eq!(a.created_at, b.created_at);
    let before = fixture
        .client()
        .list_executables(digest, 200, None, true)
        .await
        .unwrap()
        .executables
        .len();
    assert!(
        second
            .submit(&contract.name, digest, candidate("same-cycle", 450))
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .client()
            .list_executables(digest, 200, None, true)
            .await
            .unwrap()
            .executables
            .len(),
        before
    );
    let mut newer = contract.clone();
    newer.result.default = json!(600).into();
    second.deploy(&contract.name, &newer).await.unwrap();
    // v3 checks currentness even before reconciling an identical old admission.
    assert!(matches!(
        first
            .submit(&contract.name, digest, candidate("same-cycle", 400))
            .await,
        Err(ServiceError::Stale { .. })
    ));
    assert!(matches!(
        first
            .submit(&contract.name, digest, candidate("new-cycle", 450))
            .await,
        Err(ServiceError::Stale { .. })
    ));
    assert_eq!(
        fixture
            .client()
            .list_executables(digest, 200, None, true)
            .await
            .unwrap()
            .executables
            .len(),
        before
    );
}

pub async fn c05_activation(fixture: &impl AuthorityFixture) {
    let first = ContractLifecycle::new(fixture.client());
    let second = ContractLifecycle::new(fixture.independent_client());
    let contract = contract();
    let version = first.deploy(&contract.name, &contract).await.unwrap().0;
    let digest = &version.contract_digest;
    let a = first
        .submit(&contract.name, digest, candidate("cycle-a", 400))
        .await
        .unwrap()
        .0;
    let b = second
        .submit(&contract.name, digest, candidate("cycle-b", 450))
        .await
        .unwrap()
        .0;
    let active = fixture
        .client()
        .get_active_executable(digest)
        .await
        .unwrap()
        .unwrap();
    let request = || DecisionExecutableActivationRequest {
        expected_candidate_state_version: 0,
        expected_active_executable_digest: active.executable_digest.clone(),
        expected_active_state_version: active.state_version,
    };
    let (a_result, b_result) = tokio::join!(
        first.activate(&contract.name, digest, &a.executable_digest, request()),
        second.activate(&contract.name, digest, &b.executable_digest, request()),
    );
    assert_ne!(a_result.is_ok(), b_result.is_ok(), "one activation wins");
    let winner = a_result.or(b_result).unwrap();
    let observed = fixture
        .independent_client()
        .get_active_executable(digest)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        observed.executable_digest,
        winner.executable.executable_digest
    );
    assert!(observed.state_version > active.state_version);
    let redeployed = first.deploy(&contract.name, &contract).await.unwrap().0;
    assert_eq!(
        redeployed.active_executable_digest, observed.executable_digest,
        "redeploy must not reset learning"
    );
    assert_eq!(redeployed.accepted_at, version.accepted_at);
}
