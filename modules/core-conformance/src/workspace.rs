//! Path-free workflow outcomes. Local lock tests do not qualify distributed lease expiry.
use std::sync::Arc;

use chrono::{Duration, Utc};
use flaggo_analysis_domain::{AnalysisContract, AnalysisProfile};
use flaggo_analysis_workspace::{
    ExecutionRequest, ExecutionReservation, ExecutionState, HandoffPublication, PrepareCycleResult,
    WorkspaceProvider,
};
use serde_json::json;

pub const FIXTURE_REVISION: &str = "flaggo.workspace-conformance/v1";

pub trait WorkspaceFixture {
    fn client(&self) -> Arc<dyn WorkspaceProvider>;
    fn independent_client(&self) -> Arc<dyn WorkspaceProvider>;
}

pub async fn c10_c12_fenced_publication(fixture: &impl WorkspaceFixture) {
    let contract = AnalysisContract {
        name: "shared-name".into(),
        contract_digest: format!("sha256:{}", "a".repeat(64)),
        accepted_at: Utc::now() - Duration::hours(2),
        authority: flaggo_contract::ResourceScope::new(
            "tenant".into(),
            "app".into(),
            "test".into(),
        )
        .unwrap(),
        evaluation_interval: "PT1H".into(),
        evidence_sources: vec![],
        contract: json!({"name": "shared-name"}),
    };
    let resource = contract.resource().unwrap();
    let profile = AnalysisProfile {
        skill_names: vec![],
    };
    let first = fixture.client().claim(&resource, "first").await.unwrap();
    assert!(
        fixture
            .independent_client()
            .claim(&resource, "concurrent")
            .await
            .is_err()
    );
    let PrepareCycleResult::Ready(preparation) = first
        .prepare_cycle(&contract, &profile, "first", Utc::now())
        .await
        .unwrap()
    else {
        panic!("expected eligible cycle")
    };
    let cycle = &preparation.cycle.cycle_id;
    let filesystem = first.filesystem(cycle, "first").await.unwrap();
    let payload = serde_json::to_vec(&HandoffPublication {
        summary: "reviewable".into(),
        metadata: Some(json!({"outcome": "draftReady", "revision": 0})),
    })
    .unwrap();
    let artifact = first.put_artifact(cycle, "first", &payload).await.unwrap();
    assert_eq!(first.read_artifact(&artifact).await.unwrap(), payload);
    assert!(
        first.latest_handoff(cycle).await.unwrap().is_none(),
        "upload is not publication"
    );
    assert_eq!(
        first.put_artifact(cycle, "first", &payload).await.unwrap(),
        artifact
    );
    let mut invalid = artifact.clone();
    invalid.byte_length += 1;
    assert!(first.read_artifact(&invalid).await.is_err());
    assert!(first.commit_handoff(&invalid, None).await.is_err());
    invalid = artifact.clone();
    invalid.resource.scope.tenant = "different".into();
    assert!(first.read_artifact(&invalid).await.is_err());
    assert!(
        first
            .commit_handoff(&artifact, Some("absent"))
            .await
            .is_err()
    );
    assert!(first.latest_handoff(cycle).await.unwrap().is_none());
    first.commit_handoff(&artifact, None).await.unwrap();
    first.commit_handoff(&artifact, None).await.unwrap();
    let published = first.latest_handoff(cycle).await.unwrap().unwrap();
    assert_eq!(published.attempt_id, "first");
    let orphan = first
        .put_artifact(cycle, "unpublished", &payload)
        .await
        .unwrap();
    let source = first
        .put_artifact(cycle, "first", b"print(1)")
        .await
        .unwrap();
    let request = ExecutionRequest {
        execution_id: "lost-reply".into(),
        source,
        source_directory: String::new(),
        profile: "conformance/v1".into(),
        timeout_ms: 1000,
        output_limit_bytes: 1000,
    };
    assert!(matches!(
        first.reserve_execution(&request).await.unwrap(),
        ExecutionReservation::Granted(_)
    ));
    assert!(matches!(
        first.reserve_execution(&request).await.unwrap(),
        ExecutionReservation::Existing(_)
    ));
    let result = first
        .put_artifact(cycle, "first", br#"{"status":"succeeded"}"#)
        .await
        .unwrap();
    let completed = first
        .complete_execution(cycle, &request.execution_id, &result)
        .await
        .unwrap();
    assert_eq!(
        first
            .complete_execution(cycle, &request.execution_id, &result)
            .await
            .unwrap(),
        completed
    );
    let mut conflicting = request.clone();
    conflicting.timeout_ms += 1;
    assert!(first.reserve_execution(&conflicting).await.is_err());
    let mut pending = request.clone();
    pending.execution_id = "interrupted".into();
    assert!(matches!(
        first.reserve_execution(&pending).await.unwrap(),
        ExecutionReservation::Granted(_)
    ));
    assert!(first.commit_handoff(&orphan, None).await.is_err());
    assert_eq!(
        first.latest_handoff(cycle).await.unwrap(),
        Some(published.clone())
    );

    let epoch = first.claim_token().epoch;
    first.release().await.unwrap();
    let second = fixture
        .independent_client()
        .claim(&resource, "replacement")
        .await
        .unwrap();
    assert!(second.claim_token().epoch > epoch);
    let PrepareCycleResult::Ready(recovered) = second
        .prepare_cycle(&contract, &profile, "replacement", Utc::now())
        .await
        .unwrap()
    else {
        panic!("expected recovery")
    };
    assert_eq!(recovered.cycle.cycle_id, *cycle);
    assert!(recovered.cycle.resumed);
    assert_eq!(
        second
            .execution_receipt(cycle, &request.execution_id)
            .await
            .unwrap(),
        Some(completed)
    );
    let uncertain = second
        .execution_receipt(cycle, &pending.execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(uncertain.state, ExecutionState::Pending);
    assert!(
        matches!(
            second.reserve_execution(&pending).await.unwrap(),
            ExecutionReservation::Existing(_)
        ),
        "recovery must not grant an uncertain execution again"
    );
    assert!(
        second
            .complete_execution(cycle, &pending.execution_id, &result)
            .await
            .is_err(),
        "a replacement cannot publish a result under the old execution fence"
    );
    assert!(
        first
            .complete_execution(cycle, &pending.execution_id, &result)
            .await
            .is_err()
    );
    assert_eq!(second.latest_handoff(cycle).await.unwrap(), Some(published));
    assert_eq!(second.read_artifact(&orphan).await.unwrap(), payload);
    assert!(first.commit_handoff(&orphan, Some("first")).await.is_err());
    assert!(first.put_artifact(cycle, "stale", &payload).await.is_err());
    assert!(
        filesystem
            .read_text("/workspace/contract.json")
            .await
            .is_err()
    );
    assert!(
        filesystem
            .write_text("/workspace/analysis/stale.txt", "stale")
            .await
            .is_err()
    );
    second.commit_handoff(&orphan, Some("first")).await.unwrap();
    assert_eq!(
        second
            .latest_handoff(cycle)
            .await
            .unwrap()
            .unwrap()
            .attempt_id,
        "unpublished"
    );
    assert!(
        second.commit_handoff(&artifact, None).await.is_err(),
        "old retries cannot roll back newer workflow state"
    );
    second.release().await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use flaggo_analysis_workspace::LocalWorkspaceProvider;

    struct LocalFixture(tempfile::TempDir);

    impl WorkspaceFixture for LocalFixture {
        fn client(&self) -> Arc<dyn WorkspaceProvider> {
            Arc::new(LocalWorkspaceProvider::new(self.0.path()).unwrap())
        }
        fn independent_client(&self) -> Arc<dyn WorkspaceProvider> {
            self.client()
        }
    }

    #[tokio::test]
    async fn local_adapter_satisfies_fenced_publication_outcomes() {
        c10_c12_fenced_publication(&LocalFixture(tempfile::tempdir().unwrap())).await;
    }
}
