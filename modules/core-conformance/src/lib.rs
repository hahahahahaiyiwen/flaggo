//! Outcome-oriented authority scenarios for adapter authors.
//! Each function requires fresh storage and two independently constructed clients.
//! These scoped local scenarios are not multi-host qualification.
use std::sync::Arc;

use async_trait::async_trait;
use flaggo_contract::{
    AnalysisCandidateSubmission, ContractResourceId, DecisionContract,
    DecisionExecutableActivationRequest, strict_json,
};
use flaggo_contract_application::{ContractLifecycle, ServiceError};
use flaggo_executable_store::{ContractAuthority, ExecutableLifecycleState};
use serde_json::json;

pub mod admission;
pub mod canonical;
pub mod evidence;
pub mod evidence_policy;
pub mod policy;
pub mod runtime;
pub mod workspace;

pub const FIXTURE_REVISION: &str = "flaggo.authority-conformance/v3";

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
        "ownership": {"attemptId": "attempt-1", "epoch": "1"},
        "provenance": {
            "workspaceId": format!("sha256:{}", "a".repeat(64)),
            "cycleId": cycle, "attemptId": "attempt-1",
            "analysisManifestDigest": format!("sha256:{}", "b".repeat(64))
        }
    })).unwrap()).unwrap()
}

pub async fn c07_namespace(fixture: &impl AuthorityFixture) {
    let first = ContractLifecycle::new(fixture.client());
    let second = ContractLifecycle::new(fixture.independent_client());
    let mut identities = std::collections::BTreeSet::new();
    let mut versions = Vec::new();
    for component in 0..4 {
        let mut contract = contract();
        match component {
            1 => contract.authority.tenant = Some("other-tenant".into()),
            2 => contract.authority.application = Some("other-application".into()),
            3 => contract.authority.environment = Some("other-environment".into()),
            _ => {}
        }
        let resource = ContractResourceId::try_from(&contract).unwrap();
        let version = first.deploy(&resource, &contract).await.unwrap().0;
        // A caller-chosen workspace/cycle cannot collide across resource scopes.
        first
            .submit(
                &resource,
                &version.contract_digest,
                candidate("shared-cycle", 400),
            )
            .await
            .unwrap();
        identities.insert(resource.resource_key().unwrap());
        versions.push((resource, version));
    }
    for (resource, expected) in &versions {
        assert_eq!(
            second.get(resource, None).await.unwrap().as_ref(),
            Some(expected)
        );
        assert_eq!(
            second
                .versions(resource, 100, None)
                .await
                .unwrap()
                .unwrap()
                .versions
                .len(),
            1
        );
        assert_eq!(
            second
                .catalog(&resource.scope)
                .await
                .unwrap()
                .0
                .contracts
                .len(),
            1
        );
        for (other, version) in &versions {
            if other != resource {
                assert!(
                    second
                        .get(resource, Some(&version.contract_digest))
                        .await
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
    let mut cursor = None;
    let mut observed = std::collections::BTreeSet::new();
    loop {
        let page = fixture
            .independent_client()
            .list_pending(1, cursor.as_deref())
            .await
            .unwrap();
        for candidate in page.candidates {
            assert!(
                observed.insert(candidate.admission.resource.resource_key().unwrap()),
                "scan cannot repeat resources"
            );
        }
        match page.next_resource_key {
            Some(next) => {
                assert!(cursor.as_ref().is_none_or(|previous| previous < &next));
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(
        observed, identities,
        "paging must not skip identical names in different scopes"
    );
}

pub async fn c03_publication(fixture: &impl AuthorityFixture) {
    let first = ContractLifecycle::new(fixture.client());
    let second = ContractLifecycle::new(fixture.independent_client());
    let contract = contract();
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let (a, b) = tokio::join!(
        first.deploy(&resource, &contract),
        second.deploy(&resource, &contract),
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
    let newer = second.deploy(&resource, &newer).await.unwrap().0;
    assert_eq!(
        first
            .get(&resource, None)
            .await
            .unwrap()
            .unwrap()
            .contract_digest,
        newer.contract_digest
    );
    let redeployed = first.deploy(&resource, &contract).await.unwrap();
    assert!(!redeployed.1);
    assert_eq!(second.get(&resource, None).await.unwrap().unwrap(), a);

    let mut other_owner = contract.clone();
    other_owner.authority.tenant = Some("different-tenant".into());
    assert!(matches!(
        second.deploy(&resource, &other_owner).await,
        Err(ServiceError::ResourceMismatch)
    ));
    let other_resource = ContractResourceId::try_from(&other_owner).unwrap();
    let other = second
        .deploy(&other_resource, &other_owner)
        .await
        .unwrap()
        .0;
    assert_ne!(other.contract_digest, a.contract_digest);
    assert_eq!(first.get(&resource, None).await.unwrap().unwrap(), a);
    assert!(
        first
            .get(&other_resource, Some(&a.contract_digest))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        first
            .catalog(&resource.scope)
            .await
            .unwrap()
            .0
            .contracts
            .len(),
        1
    );
    assert_eq!(
        second
            .catalog(&other_resource.scope)
            .await
            .unwrap()
            .0
            .contracts
            .len(),
        1
    );
}

pub async fn c03_atomic_failure(fixture: &impl AuthorityFixture) {
    let lifecycle = ContractLifecycle::new(fixture.client());
    let contract = contract();
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let digest = lifecycle
        .validate(&resource, &contract)
        .await
        .unwrap()
        .contract_digest
        .unwrap();
    fixture.fail_publication(true).await;
    assert!(lifecycle.deploy(&resource, &contract).await.is_err());
    let reader = fixture.independent_client();
    assert!(
        reader
            .get_contract(&resource, &digest)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .get_current_contract(&resource)
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
    let other_resource = ContractResourceId::try_from(&other_owner).unwrap();
    assert!(
        lifecycle
            .deploy(&other_resource, &other_owner)
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
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let version = first.deploy(&resource, &contract).await.unwrap().0;
    let digest = &version.contract_digest;
    let (a, b) = tokio::join!(
        first.submit(&resource, digest, candidate("same-cycle", 400)),
        second.submit(&resource, digest, candidate("same-cycle", 400)),
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
            .submit(&resource, digest, candidate("same-cycle", 450))
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
    second.deploy(&resource, &newer).await.unwrap();
    // Currentness is checked before reconciling an identical old admission.
    assert!(matches!(
        first
            .submit(&resource, digest, candidate("same-cycle", 400))
            .await,
        Err(ServiceError::Stale { .. })
    ));
    assert!(matches!(
        first
            .submit(&resource, digest, candidate("new-cycle", 450))
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
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let version = first.deploy(&resource, &contract).await.unwrap().0;
    let digest = &version.contract_digest;
    let a = first
        .submit(&resource, digest, candidate("cycle-a", 400))
        .await
        .unwrap()
        .0;
    let b = second
        .submit(&resource, digest, candidate("cycle-b", 450))
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
        first.activate(&resource, digest, &a.executable_digest, request()),
        second.activate(&resource, digest, &b.executable_digest, request()),
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
    let redeployed = first.deploy(&resource, &contract).await.unwrap().0;
    assert_eq!(
        redeployed.active_executable_digest, observed.executable_digest,
        "redeploy must not reset learning"
    );
    assert_eq!(redeployed.accepted_at, version.accepted_at);
}
