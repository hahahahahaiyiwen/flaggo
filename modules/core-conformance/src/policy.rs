//! Shared product-policy corpus: callable by a consumer with no database adapter.

use flaggo_contract::{ContractResourceId, DecisionExecutable, digests, strict_json};
use flaggo_contract_store::{AcceptedContractVersion, timestamp};
use flaggo_executable_store::{
    AnalysisCandidateAdmission, AnalysisCandidateResolutionAction,
    AnalysisCandidateResolutionCommand, CandidateActivationCommand,
    ExecutableLifecycleState as State, ExecutableLifecycleToken, ExecutableStoreError,
    StoredExecutable, artifacts,
    policy::{self, LifecycleSnapshot, ResolutionPlan, ResolutionSnapshot},
};
use flaggo_expression::{ExecutableCompiler, ExpressionCompiler};
use serde_json::json;

pub const POLICY_FIXTURE_REVISION: &str = "flaggo.authority-policy-conformance/v1";

fn artifacts_fixture() -> (AcceptedContractVersion, StoredExecutable, StoredExecutable) {
    let contract = crate::contract();
    let compiler = ExecutableCompiler::default();
    let digest = digests::contract_digest(&contract, Some(&ExpressionCompiler::default())).unwrap();
    let time = timestamp::parse_exact("2026-01-02T03:04:05.1234567+00:00").unwrap();
    let make = |rules| {
        let source: DecisionExecutable = strict_json::deserialize(
            &serde_json::to_vec(&json!({"contractDigest":digest,"rules":rules})).unwrap(),
        )
        .unwrap();
        let compiled = compiler.compile(&contract, &source).unwrap();
        StoredExecutable {
            executable_digest: compiled.executable_digest,
            executable: compiled.executable,
            checked_executable: compiled.checked_executable,
            state: State::Candidate,
            state_version: 0,
            created_at: time,
            activated_at: None,
            provenance: None,
        }
    };
    let default = make(json!([]));
    let authored =
        make(json!([{"name":"learned","when":{"expression":"true"},"return":{"value":400}}]));
    (
        AcceptedContractVersion {
            contract_digest: digest,
            accepted_at: time,
            contract,
        },
        default,
        authored,
    )
}

pub fn c03_c05_policy() {
    let (version, default, authored) = artifacts_fixture();
    policy::validate_publication(
        &version,
        &default,
        Some(&authored),
        Some(&ExpressionCompiler::default()),
    )
    .unwrap();
    let steps = |active| {
        policy::plan_bootstrap(&default, Some(&authored), active)
            .iter()
            .map(|value| value.executable_digest.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        steps(None),
        [
            default.executable_digest.clone(),
            authored.executable_digest.clone()
        ]
    );
    assert_eq!(
        steps(Some(default.executable_digest.as_str())).as_slice(),
        std::slice::from_ref(&authored.executable_digest)
    );
    assert!(
        steps(Some("learned-active")).is_empty(),
        "redeploy must preserve learned authority"
    );
    assert!(policy::validate_publication(&version, &authored, None, None).is_err());
    let token = ExecutableLifecycleToken {
        executable_digest: default.executable_digest.clone(),
        state_version: 3,
    };
    let basic = policy::plan_activation(
        &version.contract_digest,
        &default.executable_digest,
        &version.contract_digest,
        Some(&token),
        None,
    )
    .unwrap();
    assert!(!basic.changed);
    assert_eq!(basic.state_version, 3);
    assert!(matches!(
        policy::plan_activation(
            &version.contract_digest,
            &authored.executable_digest,
            &version.contract_digest,
            Some(&token),
            Some(None)
        ),
        Err(ExecutableStoreError::ActivationConflict { .. })
    ));

    let admission = AnalysisCandidateAdmission {
        workspace_id: "workspace".into(),
        cycle_id: "cycle".into(),
        attempt_id: "attempt".into(),
        resource: ContractResourceId::try_from(&version.contract).unwrap(),
        contract_digest: version.contract_digest.clone(),
        executable_digest: authored.executable_digest.clone(),
        analysis_manifest_digest: format!("sha256:{}", "a".repeat(64)),
        created_at: authored.created_at,
    };
    policy::validate_admission(&authored, &admission).unwrap();
    assert!(
        policy::plan_admission(&admission, None, None)
            .unwrap()
            .is_none()
    );
    let retry = policy::plan_admission(&admission, Some(&admission), Some(State::Active))
        .unwrap()
        .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.created_at, admission.created_at);
    let mut conflicting = admission.clone();
    conflicting.attempt_id = "different-attempt".into();
    assert!(matches!(
        policy::plan_admission(&conflicting, Some(&admission), Some(State::Candidate)),
        Err(ExecutableStoreError::AdmissionConflict { .. })
    ));
    assert!(matches!(
        policy::plan_admission(&admission, None, Some(State::Active)),
        Err(ExecutableStoreError::LifecycleConflict { .. })
    ));

    let manual = CandidateActivationCommand {
        resource: admission.resource.clone(),
        contract_digest: admission.contract_digest.clone(),
        executable_digest: admission.executable_digest.clone(),
        expected_candidate_state_version: 7,
        expected_active_executable: token.clone(),
    };
    let candidate = LifecycleSnapshot {
        state: State::Candidate,
        state_version: 7,
    };
    let planned = policy::plan_candidate_activation(
        &manual,
        &version.contract_digest,
        candidate,
        Some(&version.contract_digest),
        Some(&token),
    )
    .unwrap();
    assert_eq!(
        planned.state_version, 8,
        "manual activation takes max(active,candidate)+1"
    );
    let aba = ExecutableLifecycleToken {
        state_version: 5,
        ..token.clone()
    };
    assert!(matches!(
        policy::plan_candidate_activation(
            &manual,
            &version.contract_digest,
            candidate,
            Some(&version.contract_digest),
            Some(&aba)
        ),
        Err(ExecutableStoreError::ActivationConflict { .. })
    ));
    assert!(matches!(
        policy::plan_candidate_activation(
            &manual,
            &version.contract_digest,
            candidate,
            Some("replacement"),
            Some(&token)
        ),
        Err(ExecutableStoreError::CurrentContractConflict { .. })
    ));

    let mut command = AnalysisCandidateResolutionCommand {
        admission: admission.clone(),
        expected_candidate_state_version: 7,
        expected_active_executable: Some(token.clone()),
        action: AnalysisCandidateResolutionAction::Activate,
    };
    let mut snapshot = ResolutionSnapshot {
        candidate,
        created_at: admission.created_at,
        current_contract: Some(&version.contract_digest),
        newest_candidate: Some(&authored.executable_digest),
        active: Some(&token),
    };
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::Activate {
            previous: Some(token.clone()),
            state_version: 4
        },
        "automatic resolution preserves its separate active-token version rule"
    );
    snapshot.active = Some(&aba);
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::Yielded
    );
    command.action = AnalysisCandidateResolutionAction::Reject;
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::Reject { state_version: 8 }
    );
    snapshot.newest_candidate = Some("newer-candidate");
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::Yielded
    );
    snapshot.current_contract = Some("replacement");
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::SupersedeStale { state_version: 8 }
    );
    snapshot.created_at += chrono::Duration::seconds(1);
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::Yielded
    );
    snapshot.candidate.state = State::Inactive;
    assert_eq!(
        policy::plan_resolution(&command, &snapshot).unwrap(),
        ResolutionPlan::AlreadyResolved { state_version: 7 }
    );
    assert!(matches!(
        policy::next_version(i64::MAX),
        Err(ExecutableStoreError::Integrity(_))
    ));
}

pub fn persisted_artifact_integrity() {
    let (version, _, authored) = artifacts_fixture();
    let bytes = strict_json::serialize(&authored.executable).unwrap();
    let checked = strict_json::serialize(&authored.checked_executable).unwrap();
    let decode = |digest: &str, contract: &str, checked: &[u8]| {
        artifacts::decode_persisted_artifacts(digest, contract, &bytes, checked, None)
    };
    let valid = decode(
        &authored.executable_digest,
        &version.contract_digest,
        &checked,
    )
    .unwrap();
    assert_eq!(
        strict_json::serialize(&valid.checked_executable).unwrap(),
        checked
    );
    for (digest, contract, checked) in [
        (
            "wrong-digest",
            version.contract_digest.as_str(),
            checked.as_slice(),
        ),
        (
            authored.executable_digest.as_str(),
            "wrong-contract",
            checked.as_slice(),
        ),
        (
            authored.executable_digest.as_str(),
            version.contract_digest.as_str(),
            b"null".as_slice(),
        ),
        (
            authored.executable_digest.as_str(),
            version.contract_digest.as_str(),
            br#"{"rules":[]}"#.as_slice(),
        ),
    ] {
        assert!(matches!(
            decode(digest, contract, checked),
            Err(ExecutableStoreError::Integrity(_))
        ));
    }
    assert!(matches!(
        decode(&authored.executable_digest, &version.contract_digest, b"{"),
        Err(ExecutableStoreError::Contract(
            flaggo_contract::ContractError::Json(_)
        ))
    ));
    let repeated = format!(
        r#"{{"rules":null,{}"#,
        &String::from_utf8(checked.clone()).unwrap()[1..]
    );
    assert!(matches!(
        decode(
            &authored.executable_digest,
            &version.contract_digest,
            repeated.as_bytes()
        ),
        Err(ExecutableStoreError::Contract(
            flaggo_contract::ContractError::Json(_)
        ))
    ));
    let mut invalid = authored.clone();
    invalid.state = State::Active;
    assert!(matches!(
        artifacts::validate_new_executable(&invalid, None),
        Err(ExecutableStoreError::InvalidArgument { .. })
    ));
}

pub fn persisted_contract_integrity() {
    use flaggo_contract_store::{ContractStoreError, artifacts::decode_persisted_contract};
    let (version, _, _) = artifacts_fixture();
    let canonical =
        digests::canonicalize_expressions(&version.contract, &ExpressionCompiler::default())
            .unwrap();
    let key = ContractResourceId::try_from(&canonical)
        .unwrap()
        .resource_key()
        .unwrap();
    let bytes = strict_json::serialize(&canonical).unwrap();
    let decoded = decode_persisted_contract(&key, &version.contract_digest, &bytes).unwrap();
    assert_eq!(strict_json::serialize(&decoded).unwrap(), bytes);
    for (key, digest) in [
        (
            "different/resource/scope/name",
            version.contract_digest.as_str(),
        ),
        (key.as_str(), "wrong-digest"),
    ] {
        assert!(matches!(
            decode_persisted_contract(key, digest, &bytes),
            Err(ContractStoreError::Integrity(_))
        ));
    }
    let mut invalid_scope = canonical;
    invalid_scope.authority.tenant = Some(" ".into());
    assert!(matches!(
        decode_persisted_contract(
            &key,
            &version.contract_digest,
            &strict_json::serialize(&invalid_scope).unwrap()
        ),
        Err(ContractStoreError::Integrity(_))
    ));
    assert!(matches!(
        decode_persisted_contract(&key, &version.contract_digest, b"{"),
        Err(ContractStoreError::Contract(
            flaggo_contract::ContractError::Json(_)
        ))
    ));
}

/// Compare shared decisions with an adapter's committed results. No SQL or row
/// layout is assumed; fixture setup is the same as the existing authority gates.
pub async fn c05_adapter_policy_parity(fixture: &impl crate::AuthorityFixture) {
    use flaggo_contract_application::ContractLifecycle;
    let authority = fixture.client();
    let service = ContractLifecycle::new(authority.clone());
    let contract = crate::contract();
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let version = service.deploy(&resource, &contract).await.unwrap().0;
    for (cycle, action) in [
        ("reject", AnalysisCandidateResolutionAction::Reject),
        ("activate", AnalysisCandidateResolutionAction::Activate),
    ] {
        let value = if cycle == "reject" { 400 } else { 450 };
        service
            .submit(
                &resource,
                &version.contract_digest,
                crate::candidate(cycle, value),
            )
            .await
            .unwrap();
        let pending = authority
            .list_pending(1, None)
            .await
            .unwrap()
            .candidates
            .remove(0);
        let command = AnalysisCandidateResolutionCommand {
            admission: pending.admission,
            expected_candidate_state_version: pending.candidate_state_version,
            expected_active_executable: pending.active_executable,
            action,
        };
        let selected = authority
            .get_executable(&command.admission.executable_digest)
            .await
            .unwrap()
            .unwrap();
        let plan = policy::plan_resolution(
            &command,
            &ResolutionSnapshot {
                candidate: LifecycleSnapshot {
                    state: selected.state,
                    state_version: selected.state_version,
                },
                created_at: command.admission.created_at,
                current_contract: Some(&version.contract_digest),
                newest_candidate: Some(&selected.executable_digest),
                active: command.expected_active_executable.as_ref(),
            },
        )
        .unwrap();
        let actual = fixture
            .independent_client()
            .resolve(&command)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(plan.result(&selected.executable_digest, 0)).unwrap()
        );
        let selected = authority
            .get_executable(&selected.executable_digest)
            .await
            .unwrap()
            .unwrap();
        let retry = fixture
            .independent_client()
            .resolve(&command)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(retry).unwrap(),
            serde_json::to_value(
                ResolutionPlan::AlreadyResolved {
                    state_version: selected.state_version
                }
                .result(&selected.executable_digest, 0)
            )
            .unwrap()
        );
    }
    let candidate = service
        .submit(
            &resource,
            &version.contract_digest,
            crate::candidate("manual", 480),
        )
        .await
        .unwrap()
        .0;
    let active = authority
        .get_active_executable(&version.contract_digest)
        .await
        .unwrap()
        .unwrap();
    let command = CandidateActivationCommand {
        resource,
        contract_digest: version.contract_digest.clone(),
        executable_digest: candidate.executable_digest.clone(),
        expected_candidate_state_version: 0,
        expected_active_executable: ExecutableLifecycleToken {
            executable_digest: active.executable_digest,
            state_version: active.state_version,
        },
    };
    let plan = policy::plan_candidate_activation(
        &command,
        &version.contract_digest,
        LifecycleSnapshot {
            state: State::Candidate,
            state_version: 0,
        },
        Some(&version.contract_digest),
        Some(&command.expected_active_executable),
    )
    .unwrap();
    let actual = fixture
        .independent_client()
        .activate_candidate(&command)
        .await
        .unwrap();
    assert_eq!(actual.executable_digest, candidate.executable_digest);
    assert_eq!(
        actual.previous_executable_digest,
        plan.previous.executable_digest
    );
    assert_eq!(actual.state_version, plan.state_version);
    assert_eq!(actual.superseded_candidate_count, 0);
}

#[cfg(test)]
mod tests {
    #[test]
    fn portable_policy_and_artifact_corpus() {
        super::c03_c05_policy();
        super::persisted_artifact_integrity();
        super::persisted_contract_integrity();
    }
}
