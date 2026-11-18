//! Candidate admission ownership outcomes, independent of host paths and stores.
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use flaggo_analysis_domain::{AnalysisContract, AnalysisProfile, CandidateRecord, CycleOutcome};
use flaggo_analysis_workspace::{PrepareCycleResult, WorkspaceLease, WorkspaceProvider};
use flaggo_contract::{AnalysisCandidateSubmission, ContractResourceId};
use flaggo_contract_application::ContractLifecycle;
use flaggo_executable_store::ContractAuthority;
use serde_json::{Value, json};

pub const FIXTURE_REVISION: &str = "flaggo.admission-conformance/v1";

pub struct PreparedCandidate {
    pub lease: Arc<dyn WorkspaceLease>,
    pub contract: AnalysisContract,
    pub profile: AnalysisProfile,
    pub submission: AnalysisCandidateSubmission,
}

pub async fn prepare_candidate(
    provider: &dyn WorkspaceProvider,
    version: &Value,
    attempt_id: &str,
    rules: Value,
) -> PreparedCandidate {
    let resource = ContractResourceId::new(
        serde_json::from_value(version["contract"]["authority"].clone()).unwrap(),
        version["contract"]["name"].as_str().unwrap().into(),
    )
    .unwrap();
    let contract = AnalysisContract {
        name: resource.name.clone(),
        authority: resource.scope.clone(),
        contract_digest: version["contractDigest"].as_str().unwrap().into(),
        accepted_at: version["acceptedAt"]
            .as_str()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap(),
        evaluation_interval: version["contract"]["learning"]["policy"]["evaluate"]["interval"]
            .as_str()
            .unwrap()
            .into(),
        evidence_sources: Vec::new(),
        contract: version["contract"].clone(),
    };
    let profile = AnalysisProfile {
        skill_names: vec!["conformance".into()],
    };
    let lease = provider.claim(&resource, attempt_id).await.unwrap();
    let PrepareCycleResult::Ready(cycle) = lease
        .prepare_cycle(
            &contract,
            &profile,
            attempt_id,
            contract.accepted_at + Duration::days(1),
        )
        .await
        .unwrap()
    else {
        panic!("expected an eligible cycle")
    };
    let proposal = lease
        .prepare_candidate_proposal(&cycle.cycle.cycle_id, attempt_id, rules)
        .await
        .unwrap();
    let submission = serde_json::from_value(json!({
        "rules": proposal.rules,
        "ownership": lease.claim_token().candidate_ownership(),
        "provenance": {
            "workspaceId": lease.workspace_id(),
            "cycleId": cycle.cycle.cycle_id,
            "attemptId": proposal.attempt_id,
            "analysisManifestDigest": proposal.analysis_manifest_digest
        }
    }))
    .unwrap();
    PreparedCandidate {
        lease,
        contract,
        profile,
        submission,
    }
}

pub async fn c04_c10_fenced_admission(
    authority: Arc<dyn ContractAuthority>,
    independent_authority: Arc<dyn ContractAuthority>,
    provider: Arc<dyn WorkspaceProvider>,
) {
    let service = ContractLifecycle::new(authority);
    let independent = ContractLifecycle::new(independent_authority);
    let document = crate::contract();
    let resource = ContractResourceId::try_from(&document).unwrap();
    let version = service.deploy(&resource, &document).await.unwrap().0;
    let prepared = prepare_candidate(
        provider.as_ref(),
        &serde_json::to_value(version).unwrap(),
        "original-owner",
        json!([{"name":"learned","when":{"expression":"true"},"return":{"value":400}}]),
    )
    .await;
    let digest = &prepared.contract.contract_digest;
    prepared.lease.release().await.unwrap();
    let stale = prepared.submission.clone();
    assert_eq!(
        independent
            .submit(&resource, digest, stale.clone())
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );

    let replacement = provider
        .claim(&resource, "replacement-owner")
        .await
        .unwrap();
    let mut resumed = stale.clone();
    resumed.ownership = replacement.claim_token().candidate_ownership();
    assert_eq!(
        independent
            .submit(&resource, digest, resumed.clone())
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );
    let PrepareCycleResult::Ready(cycle) = replacement
        .prepare_cycle(
            &prepared.contract,
            &prepared.profile,
            "replacement-owner",
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap()
    else {
        panic!("expected recovery")
    };
    assert!(cycle.cycle.resumed);
    assert_eq!(cycle.cycle.cycle_id, stale.provenance.cycle_id);
    let (admitted, created) = independent
        .submit(&resource, digest, resumed.clone())
        .await
        .unwrap();
    assert!(created);
    assert_eq!(
        service
            .submit(&resource, digest, stale)
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );
    let (retry, created) = service
        .submit(&resource, digest, resumed.clone())
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(retry.executable_digest, admitted.executable_digest);
    let mut wrong_owner = resumed.clone();
    wrong_owner.ownership.attempt_id = "original-owner".into();
    assert_eq!(
        service
            .submit(&resource, digest, wrong_owner)
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );

    let record = CandidateRecord {
        contract_name: resource.name.clone(),
        contract_digest: digest.clone(),
        executable_digest: admitted.executable_digest,
        analysis_manifest_digest: resumed.provenance.analysis_manifest_digest.clone(),
        created_at: admitted.created_at.with_timezone(&Utc),
        created: admitted.created,
    };
    replacement
        .record_candidate(&cycle.cycle.cycle_id, &record)
        .await
        .unwrap();
    replacement
        .complete_cycle(
            &prepared.contract,
            &cycle.cycle.cycle_id,
            &CycleOutcome::Candidate { candidate: record },
            Utc::now(),
        )
        .await
        .unwrap();
    // Closed-cycle retries must not bypass ownership through the existing receipt.
    assert_eq!(
        service
            .submit(&resource, digest, resumed)
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );
    replacement.release().await.unwrap();
}
