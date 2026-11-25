//! Workspace workflow outcomes, with full and path-free capability coverage.
//! Local lock tests do not qualify distributed lease expiry.
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
    fenced_publication(fixture, FilesystemCoverage::Required).await;
}

/// The full corpus except filesystem acquisition and stale filesystem read/write checks.
/// Passing this scenario does not qualify the provider's filesystem capabilities.
pub async fn c10_c12_path_free_fenced_publication(fixture: &impl WorkspaceFixture) {
    fenced_publication(fixture, FilesystemCoverage::Excluded).await;
}

enum FilesystemCoverage {
    Required,
    Excluded,
}

async fn fenced_publication(
    fixture: &impl WorkspaceFixture,
    filesystem_coverage: FilesystemCoverage,
) {
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
    let filesystem = match filesystem_coverage {
        FilesystemCoverage::Required => Some(first.filesystem(cycle, "first").await.unwrap()),
        FilesystemCoverage::Excluded => None,
    };
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
    if let Some(filesystem) = filesystem {
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
    }
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
    use async_trait::async_trait;
    use chrono::DateTime;
    use flaggo_analysis_domain::{
        AnalysisError, CandidateProposal, CandidateRecord, CycleLimits, CycleOutcome,
        WorkflowHandoff,
    };
    use flaggo_analysis_workspace::{
        ExecutionReceipt, LocalWorkspaceProvider, WorkspaceArtifact, WorkspaceClaim,
        WorkspaceFileSystem, WorkspaceLease,
    };
    use flaggo_contract::ContractResourceId;
    use serde_json::Value;

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

    struct NoFilesystemFixture(LocalFixture);

    impl WorkspaceFixture for NoFilesystemFixture {
        fn client(&self) -> Arc<dyn WorkspaceProvider> {
            Arc::new(NoFilesystemProvider(self.0.client()))
        }

        fn independent_client(&self) -> Arc<dyn WorkspaceProvider> {
            Arc::new(NoFilesystemProvider(self.0.independent_client()))
        }
    }

    struct NoFilesystemProvider(Arc<dyn WorkspaceProvider>);

    #[async_trait]
    impl WorkspaceProvider for NoFilesystemProvider {
        async fn claim(
            &self,
            resource: &ContractResourceId,
            attempt_id: &str,
        ) -> Result<Arc<dyn WorkspaceLease>, AnalysisError> {
            Ok(Arc::new(NoFilesystemLease(
                self.0.claim(resource, attempt_id).await?,
            )))
        }
    }

    struct NoFilesystemLease(Arc<dyn WorkspaceLease>);

    #[async_trait]
    impl WorkspaceLease for NoFilesystemLease {
        fn resource(&self) -> &ContractResourceId {
            self.0.resource()
        }

        fn claim_token(&self) -> WorkspaceClaim {
            self.0.claim_token()
        }

        fn workspace_id(&self) -> &str {
            self.0.workspace_id()
        }

        fn contract_name(&self) -> &str {
            self.0.contract_name()
        }

        async fn release(&self) -> Result<(), AnalysisError> {
            self.0.release().await
        }

        async fn prepare_cycle(
            &self,
            contract: &AnalysisContract,
            analysis_profile: &AnalysisProfile,
            attempt_id: &str,
            now: DateTime<Utc>,
        ) -> Result<PrepareCycleResult, AnalysisError> {
            self.0
                .prepare_cycle(contract, analysis_profile, attempt_id, now)
                .await
        }

        async fn filesystem(
            &self,
            _cycle_id: &str,
            _attempt_id: &str,
        ) -> Result<Arc<dyn WorkspaceFileSystem>, AnalysisError> {
            panic!("filesystem must not be acquired");
        }

        async fn append_audit(
            &self,
            cycle_id: &str,
            category: &str,
            record: Value,
        ) -> Result<(), AnalysisError> {
            self.0.append_audit(cycle_id, category, record).await
        }

        async fn seal_analysis_manifest(&self, cycle_id: &str) -> Result<String, AnalysisError> {
            self.0.seal_analysis_manifest(cycle_id).await
        }

        async fn prepare_candidate_proposal(
            &self,
            cycle_id: &str,
            attempt_id: &str,
            rules: Value,
        ) -> Result<CandidateProposal, AnalysisError> {
            self.0
                .prepare_candidate_proposal(cycle_id, attempt_id, rules)
                .await
        }

        async fn candidate_record(
            &self,
            cycle_id: &str,
        ) -> Result<Option<CandidateRecord>, AnalysisError> {
            self.0.candidate_record(cycle_id).await
        }

        async fn record_candidate(
            &self,
            cycle_id: &str,
            candidate: &CandidateRecord,
        ) -> Result<(), AnalysisError> {
            self.0.record_candidate(cycle_id, candidate).await
        }

        async fn mark_superseding(
            &self,
            cycle_id: &str,
            replacement_digest: Option<&str>,
        ) -> Result<(), AnalysisError> {
            self.0.mark_superseding(cycle_id, replacement_digest).await
        }

        async fn put_artifact(
            &self,
            cycle_id: &str,
            attempt_id: &str,
            bytes: &[u8],
        ) -> Result<WorkspaceArtifact, AnalysisError> {
            self.0.put_artifact(cycle_id, attempt_id, bytes).await
        }

        async fn read_artifact(
            &self,
            artifact: &WorkspaceArtifact,
        ) -> Result<Vec<u8>, AnalysisError> {
            self.0.read_artifact(artifact).await
        }

        async fn commit_handoff(
            &self,
            artifact: &WorkspaceArtifact,
            expected_previous: Option<&str>,
        ) -> Result<(), AnalysisError> {
            self.0.commit_handoff(artifact, expected_previous).await
        }

        async fn reserve_execution(
            &self,
            request: &ExecutionRequest,
        ) -> Result<ExecutionReservation, AnalysisError> {
            self.0.reserve_execution(request).await
        }

        async fn execution_receipt(
            &self,
            cycle_id: &str,
            execution_id: &str,
        ) -> Result<Option<ExecutionReceipt>, AnalysisError> {
            self.0.execution_receipt(cycle_id, execution_id).await
        }

        async fn complete_execution(
            &self,
            cycle_id: &str,
            execution_id: &str,
            result: &WorkspaceArtifact,
        ) -> Result<ExecutionReceipt, AnalysisError> {
            self.0
                .complete_execution(cycle_id, execution_id, result)
                .await
        }

        async fn latest_handoff(
            &self,
            cycle_id: &str,
        ) -> Result<Option<WorkflowHandoff>, AnalysisError> {
            self.0.latest_handoff(cycle_id).await
        }

        async fn pending_proposal(
            &self,
            cycle_id: &str,
        ) -> Result<Option<CandidateProposal>, AnalysisError> {
            self.0.pending_proposal(cycle_id).await
        }

        async fn reserve_attempt(
            &self,
            cycle_id: &str,
            attempt_id: &str,
            limits: &CycleLimits,
            timeout: std::time::Duration,
        ) -> Result<std::time::Duration, AnalysisError> {
            self.0
                .reserve_attempt(cycle_id, attempt_id, limits, timeout)
                .await
        }

        async fn settle_attempt(
            &self,
            cycle_id: &str,
            attempt_id: &str,
            elapsed: std::time::Duration,
        ) -> Result<(), AnalysisError> {
            self.0.settle_attempt(cycle_id, attempt_id, elapsed).await
        }

        async fn record_attempt_failure(
            &self,
            cycle_id: &str,
            attempt_id: &str,
            error: &str,
        ) -> Result<(), AnalysisError> {
            self.0
                .record_attempt_failure(cycle_id, attempt_id, error)
                .await
        }

        async fn complete_cycle(
            &self,
            contract: &AnalysisContract,
            cycle_id: &str,
            outcome: &CycleOutcome,
            completed_at: DateTime<Utc>,
        ) -> Result<(), AnalysisError> {
            self.0
                .complete_cycle(contract, cycle_id, outcome, completed_at)
                .await
        }
    }

    #[tokio::test]
    async fn path_free_corpus_never_acquires_filesystem() {
        c10_c12_path_free_fenced_publication(&NoFilesystemFixture(LocalFixture(
            tempfile::tempdir().unwrap(),
        )))
        .await;
    }

    #[tokio::test]
    #[should_panic(expected = "filesystem must not be acquired")]
    async fn full_corpus_still_requires_filesystem() {
        c10_c12_fenced_publication(&NoFilesystemFixture(LocalFixture(
            tempfile::tempdir().unwrap(),
        )))
        .await;
    }
}
