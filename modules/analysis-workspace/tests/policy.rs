//! Public-consumer tests: also run with --no-default-features.
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisProfile, CandidateRecord, CycleLimits, CycleOutcome,
};
use flaggo_analysis_workspace::{
    ExecutionRequest, ExecutionReservation, ExecutionState, HandoffPublication, WorkspaceArtifact,
    WorkspaceClaim, policy::*, sha256_digest,
};
use flaggo_contract::ResourceScope;
use serde_json::json;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 31, 12, 0, 0).unwrap()
}

fn contract() -> AnalysisContract {
    AnalysisContract {
        name: "example".into(),
        contract_digest: "sha256:contract".into(),
        accepted_at: now(),
        authority: ResourceScope::new("tenant".into(), "app".into(), "test".into()).unwrap(),
        evaluation_interval: "P1M".into(),
        evidence_sources: vec![],
        contract: json!({"name": "example"}),
    }
}

fn profile() -> AnalysisProfile {
    AnalysisProfile {
        skill_names: vec!["one".into(), "two".into()],
    }
}

fn cycle() -> StoredCycle {
    StoredCycle::new("workspace", "cycle", &contract(), &profile(), now())
}

fn artifact(attempt_id: &str, content: &[u8]) -> WorkspaceArtifact {
    WorkspaceArtifact {
        resource: contract().resource().unwrap(),
        workspace_id: "workspace".into(),
        cycle_id: "cycle".into(),
        attempt_id: attempt_id.into(),
        digest: sha256_digest(content),
        byte_length: content.len() as u64,
    }
}

fn manifest() -> AnalysisManifest {
    let mut files = vec![
        ManifestFile {
            path: "contract.json".into(),
            size: 3,
            digest: "sha256:c".into(),
        },
        ManifestFile {
            path: "analysis/z.py".into(),
            size: 2,
            digest: "sha256:b".into(),
        },
        ManifestFile {
            path: "analysis/A.py".into(),
            size: 1,
            digest: "sha256:a".into(),
        },
    ];
    for path in [
        "cycle.json",
        "budget.json",
        "analysis-manifest.json",
        "proposal.pending.json",
        "proposal.json",
        "candidate.json",
        "outcome.json",
        "attempts/old.json",
        "attempts/old-failure.json",
        "analysis/.copilot/session/state",
    ] {
        files.push(ManifestFile {
            path: path.into(),
            size: 999,
            digest: "excluded".into(),
        });
    }
    AnalysisManifest::new("workspace", "cycle", profile(), files)
}

const LEGACY_MANIFEST: &[u8] = br#"{"schemaVersion":1,"workspaceId":"workspace","cycleId":"cycle","analysisProfile":{"skillNames":["one","two"]},"files":[{"path":"analysis/A.py","size":1,"digest":"sha256:a"},{"path":"analysis/z.py","size":2,"digest":"sha256:b"},{"path":"contract.json","size":3,"digest":"sha256:c"}]}"#;
const LEGACY_DIGEST: &str =
    "sha256:919be2f3e52967e937b83830ef6096531d685916e0009ffa580fcbc01439c091";

#[test]
fn manifest_preserves_legacy_bytes_digest_membership_and_serde_acceptance() {
    let actual = manifest();
    assert_eq!(actual.encode().unwrap(), LEGACY_MANIFEST);
    assert_eq!(sha256_digest(LEGACY_MANIFEST), LEGACY_DIGEST);
    let stored = AnalysisManifest::decode(LEGACY_MANIFEST).unwrap();
    stored.verify(&actual).unwrap();
    for included in [
        "attempts.json",
        "analysis/.copilot",
        "analysis/.copilot-other/x",
        "analysis/cycle.json",
        "audit/x.json",
        "handoffs/x.json",
        "artifacts/x/hash",
        "execution-receipts/x.json",
    ] {
        assert!(!is_manifest_control_file(included), "{included}");
    }
    for mutate in 0..7 {
        let mut changed = actual.clone();
        match mutate {
            0 => changed.schema_version += 1,
            1 => changed.workspace_id.push('x'),
            2 => changed.cycle_id.push('x'),
            3 => changed.analysis_profile.skill_names.reverse(),
            4 => changed.files[0].size += 1,
            5 => changed.files[0].digest.push('x'),
            _ => changed.files[0].path.push('x'),
        }
        assert!(stored.verify(&changed).is_err());
    }
    assert!(AnalysisManifest::decode(b"{}").is_err());
    let mut extended = serde_json::to_value(&stored).unwrap();
    extended["legacyExtension"] = json!(true);
    AnalysisManifest::decode(&serde_json::to_vec(&extended).unwrap())
        .unwrap()
        .verify(&stored)
        .unwrap();
}

#[test]
fn proposal_codec_and_recovery_retain_original_attempt_and_reject_conflicts() {
    let proposal = plan_proposal(None, "first", json!([]), LEGACY_MANIFEST).unwrap();
    let expected = concat!(
        "{\n  \"attemptId\": \"first\",\n  \"rules\": [],\n  \"analysisManifestDigest\": ",
        "\"sha256:919be2f3e52967e937b83830ef6096531d685916e0009ffa580fcbc01439c091\"\n}"
    );
    assert_eq!(encode_proposal(&proposal).unwrap(), expected.as_bytes());
    assert_eq!(decode_proposal(expected.as_bytes()).unwrap(), proposal);
    let recovered =
        plan_proposal(Some(&proposal), "replacement", json!([]), LEGACY_MANIFEST).unwrap();
    assert_eq!(recovered, proposal);
    assert!(plan_proposal(Some(&proposal), "replacement", json!([1]), LEGACY_MANIFEST).is_err());
    let pretty = serde_json::to_vec_pretty(&manifest()).unwrap();
    assert!(plan_proposal(Some(&proposal), "replacement", json!([]), &pretty).is_err());
    verify_proposal_manifest(&proposal, LEGACY_MANIFEST, false).unwrap();
    assert!(verify_proposal_manifest(&proposal, &pretty, false).is_err());
    let mut changed = proposal.clone();
    changed.attempt_id = "replacement".into();
    assert!(verify_proposal_record(&proposal, &changed).is_err());
    verify_proposal_record(&proposal, &proposal).unwrap();
    assert!(decode_proposal(b"{").is_err());
}

#[test]
fn preparation_preserves_sealed_resume_superseding_and_terminal_authority() {
    let cycle = cycle();
    for status in [CycleStatus::Active, CycleStatus::Superseding] {
        let mut stored = cycle.clone();
        stored.status = status;
        assert_eq!(
            plan_current_cycle(Some(&stored), &cycle.contract_digest, false),
            CurrentCyclePlan::Resume
        );
        for (manifest, proposal) in [(true, false), (false, true), (true, true)] {
            let access = CycleAccess::resumed("cycle", manifest || proposal);
            assert!(access.sealed);
            assert!(!access.writable);
            access.require_active("cycle").unwrap();
            assert!(access.require_writable("cycle").is_err());
        }
        assert_eq!(
            plan_current_cycle(Some(&stored), "new-digest", false),
            CurrentCyclePlan::Supersede
        );
        assert_eq!(
            plan_current_cycle(Some(&stored), &cycle.contract_digest, true),
            CurrentCyclePlan::ReconcileTerminal
        );
    }
    assert_eq!(
        plan_current_cycle(None, &cycle.contract_digest, true),
        CurrentCyclePlan::ReconcileTerminal
    );
    let terminal = cycle.terminal(&CycleOutcome::NoChange {
        reason: "done".into(),
    });
    assert_eq!(
        plan_current_cycle(Some(&terminal), &cycle.contract_digest, false),
        CurrentCyclePlan::Clear
    );
    assert_eq!(
        cycle
            .as_analysis_cycle(&contract().authority, true)
            .analysis_profile,
        profile()
    );
    let access = CycleAccess::resumed("cycle", false);
    assert!(access.require_active("other").is_err());
    assert!(access.require_writable("other").is_err());
    access.require_writable("cycle").unwrap();
}

#[test]
fn scheduling_uses_calendar_interval_and_terminal_journal_timestamp() {
    let contract = contract();
    let schedule = ScheduleState::new(&contract).unwrap();
    let february = Utc.with_ymd_and_hms(2026, 2, 28, 12, 0, 0).unwrap();
    assert_eq!(schedule.next_eligible_at, february);
    assert!(
        !plan_eligibility(&schedule, &contract, now())
            .unwrap()
            .eligible
    );
    assert!(
        plan_eligibility(&schedule, &contract, february)
            .unwrap()
            .eligible
    );
    let outcome = CycleOutcome::NoChange {
        reason: "done".into(),
    };
    let journal = plan_outcome(None, &outcome, february).unwrap();
    let replay = plan_outcome(Some(&journal), &outcome, now()).unwrap();
    assert_eq!(replay, journal);
    assert!(
        plan_outcome(
            Some(&journal),
            &CycleOutcome::Failure {
                reason: "changed".into()
            },
            now()
        )
        .is_err()
    );
    let completed = ScheduleState::completed(&contract, replay.completed_at).unwrap();
    assert_eq!(
        completed.next_eligible_at,
        Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap()
    );
    let mut replacement = contract.clone();
    replacement.contract_digest = "replacement".into();
    let plan = plan_eligibility(&completed, &replacement, february).unwrap();
    assert!(plan.replace_schedule && plan.eligible);
    assert_eq!(plan.schedule.last_terminal_completed_at, None);
    assert_eq!(plan.schedule.next_eligible_at, february);
    assert!(validate_terminal_contract(&contract, &replacement).is_err());
    replacement = contract.clone();
    replacement.authority.tenant = "other".into();
    assert!(validate_terminal_contract(&contract, &replacement).is_err());
    validate_terminal_contract(&contract, &contract).unwrap();
    replacement = contract.clone();
    replacement.evaluation_interval = "invalid".into();
    assert!(ScheduleState::new(&replacement).is_err());
    assert!(ScheduleState::completed(&replacement, now()).is_err());
}

#[test]
fn candidate_receipt_binding_and_terminal_receipt_are_immutable() {
    let proposal = plan_proposal(None, "first", json!([]), LEGACY_MANIFEST).unwrap();
    let candidate = CandidateRecord {
        contract_name: contract().name,
        contract_digest: contract().contract_digest,
        executable_digest: "sha256:executable".into(),
        created_at: now(),
        created: true,
        analysis_manifest_digest: LEGACY_DIGEST.into(),
    };
    validate_candidate_binding(
        &candidate.contract_name,
        &candidate.contract_digest,
        &proposal,
        &candidate,
    )
    .unwrap();
    assert_eq!(
        plan_candidate_record(None, &candidate).unwrap(),
        CandidateRecordPlan::Append
    );
    assert_eq!(
        plan_candidate_record(Some(&candidate), &candidate).unwrap(),
        CandidateRecordPlan::Unchanged
    );
    for mutate in 0..6 {
        let mut changed = candidate.clone();
        match mutate {
            0 => changed.contract_name.push('x'),
            1 => changed.contract_digest.push('x'),
            2 => changed.analysis_manifest_digest.push('x'),
            3 => changed.executable_digest.push('x'),
            4 => changed.created = false,
            _ => changed.created_at += chrono::Duration::seconds(1),
        }
        if mutate < 3 {
            assert!(
                validate_candidate_binding(
                    &candidate.contract_name,
                    &candidate.contract_digest,
                    &proposal,
                    &changed
                )
                .is_err()
            );
        }
        assert!(plan_candidate_record(Some(&candidate), &changed).is_err());
        assert!(
            validate_terminal_candidate(
                &CycleOutcome::Candidate { candidate: changed },
                Some(&candidate)
            )
            .is_err()
        );
    }
    let outcome = CycleOutcome::Candidate {
        candidate: candidate.clone(),
    };
    validate_terminal_candidate(&outcome, Some(&candidate)).unwrap();
    assert!(validate_terminal_candidate(&outcome, None).is_err());
    validate_terminal_candidate(
        &CycleOutcome::NoCandidate {
            reason: "none".into(),
        },
        None,
    )
    .unwrap();
}

#[test]
fn handoff_replays_reconcile_pointer_without_rolling_back_newer_attempts() {
    let mut cycle = cycle();
    let publication = HandoffPublication {
        summary: "ready".into(),
        metadata: Some(json!({"step": 1})),
    };
    let artifact = artifact("first", &serde_json::to_vec(&publication).unwrap());
    let plan = plan_handoff(&publication, &artifact, &cycle, None, None, now()).unwrap();
    assert_eq!(plan.advance_to.as_deref(), Some("first"));
    let record = plan.append.unwrap();
    // Record persisted, pointer write interrupted.
    let retry = plan_handoff(&publication, &artifact, &cycle, Some(&record), None, now()).unwrap();
    assert!(retry.append.is_none());
    assert_eq!(retry.advance_to.as_deref(), Some("first"));
    cycle.latest_handoff = retry.advance_to;
    plan_handoff(&publication, &artifact, &cycle, Some(&record), None, now()).unwrap();
    assert!(
        plan_handoff(
            &publication,
            &artifact,
            &cycle,
            Some(&record),
            Some("wrong"),
            now()
        )
        .is_err()
    );
    let changed = HandoffPublication {
        summary: "different".into(),
        ..publication.clone()
    };
    assert!(plan_handoff(&changed, &artifact, &cycle, Some(&record), None, now()).is_err());
    validate_handoff_binding(&record, &cycle.cycle_id, &cycle.contract_digest, "first").unwrap();
    validate_handoff_artifact(&record, &artifact, &publication, "first").unwrap();
    assert!(validate_handoff_artifact(&record, &artifact, &changed, "first").is_err());
    assert!(
        validate_handoff_binding(
            &record,
            &cycle.cycle_id,
            &cycle.contract_digest,
            "different"
        )
        .is_err()
    );
    let mut wrong_cycle = cycle.clone();
    wrong_cycle.contract_digest.push('x');
    assert!(
        validate_handoff_binding(
            &record,
            &wrong_cycle.cycle_id,
            &wrong_cycle.contract_digest,
            "first"
        )
        .is_err()
    );
    assert_eq!(
        workflow_handoff(record.clone()).unwrap().metadata,
        json!({"step": 1})
    );
    cycle.latest_handoff = Some("newer".into());
    assert!(plan_handoff(&publication, &artifact, &cycle, Some(&record), None, now()).is_err());
}

#[test]
fn checkpoint_does_not_advance_and_metadata_limit_is_exact() {
    let mut cycle = cycle();
    cycle.latest_handoff = Some("previous".into());
    let publication = HandoffPublication {
        summary: "checkpoint".into(),
        metadata: None,
    };
    let artifact = artifact("checkpoint", &serde_json::to_vec(&publication).unwrap());
    assert!(plan_handoff(&publication, &artifact, &cycle, None, None, now()).is_err());
    let plan = plan_handoff(
        &publication,
        &artifact,
        &cycle,
        None,
        Some("previous"),
        now(),
    )
    .unwrap();
    assert!(plan.advance_to.is_none());
    let record = plan.append.unwrap();
    assert!(workflow_handoff(record.clone()).is_err());
    assert!(
        plan_handoff(
            &publication,
            &artifact,
            &cycle,
            Some(&record),
            Some("previous"),
            now()
        )
        .unwrap()
        .advance_to
        .is_none()
    );
    for metadata in [json!([]), json!(null), json!("object required")] {
        assert!(
            validate_handoff_publication(&HandoffPublication {
                summary: "x".into(),
                metadata: Some(metadata)
            })
            .is_err()
        );
    }
    assert!(
        validate_handoff_publication(&HandoffPublication {
            summary: " \n".into(),
            metadata: None
        })
        .is_err()
    );
    let mut bounded = HandoffPublication {
        summary: "x".into(),
        metadata: Some(json!({"x": "a".repeat(16_376)})),
    };
    assert_eq!(serde_json::to_vec(&bounded.metadata).unwrap().len(), 16_384);
    validate_handoff_publication(&bounded).unwrap();
    bounded.metadata = Some(json!({"x": "a".repeat(16_377)}));
    assert!(validate_handoff_publication(&bounded).is_err());
}

#[test]
fn executions_keep_uncertain_reservations_and_original_completion_fence() {
    let request = ExecutionRequest {
        execution_id: "execution".into(),
        source: artifact("original", b"source"),
        source_directory: String::new(),
        profile: "python".into(),
        timeout_ms: 1,
        output_limit_bytes: 1,
    };
    let ExecutionReservation::Granted(pending) =
        plan_execution_reservation(&request, 1, None).unwrap()
    else {
        panic!("new reservation required")
    };
    assert_eq!(pending.state, ExecutionState::Pending);
    for epoch in [1, 2] {
        assert_eq!(
            plan_execution_reservation(&request, epoch, Some(&pending)).unwrap(),
            ExecutionReservation::Existing(pending.clone())
        );
    }
    let mut changed = request.clone();
    changed.timeout_ms += 1;
    assert!(plan_execution_reservation(&changed, 1, Some(&pending)).is_err());
    changed.timeout_ms = 0;
    assert!(plan_execution_reservation(&changed, 1, None).is_err());
    validate_execution_receipt(&pending, "cycle", "execution").unwrap();
    assert!(validate_execution_receipt(&pending, "other", "execution").is_err());
    assert!(validate_execution_receipt(&pending, "cycle", "other").is_err());
    let result = artifact("original", b"result");
    let completed = plan_execution_completion(&pending, 1, "cycle", "execution", &result).unwrap();
    assert_eq!(
        plan_execution_completion(&completed, 1, "cycle", "execution", &result).unwrap(),
        completed
    );
    assert_eq!(
        plan_execution_reservation(&request, 2, Some(&completed)).unwrap(),
        ExecutionReservation::Existing(completed.clone())
    );
    assert!(plan_execution_completion(&pending, 2, "cycle", "execution", &result).is_err());
    assert!(plan_execution_completion(&completed, 2, "cycle", "execution", &result).is_err());
    assert!(plan_execution_completion(&pending, 1, "cycle", "other", &result).is_err());
    assert!(plan_execution_completion(&pending, 1, "other", "execution", &result).is_err());
    assert!(
        plan_execution_completion(
            &pending,
            1,
            "cycle",
            "execution",
            &artifact("replacement", b"result")
        )
        .is_err()
    );
    assert!(
        plan_execution_completion(
            &completed,
            1,
            "cycle",
            "execution",
            &artifact("original", b"different")
        )
        .is_err()
    );
}

#[test]
fn attempts_charge_crashes_and_only_refund_matching_unused_reservations() {
    let limits = CycleLimits {
        max_attempts: 3,
        max_revisions: 1,
        execution_seconds: 10,
    };
    let (first, allowed) =
        plan_attempt_reservation(None, "first", &limits, Duration::from_secs(4)).unwrap();
    assert_eq!(allowed, Duration::from_secs(4));
    assert_eq!((first.attempts, first.execution_millis), (1, 4000));
    let (repeated, _) =
        plan_attempt_reservation(Some(&first), "first", &limits, Duration::from_secs(4)).unwrap();
    assert_eq!((repeated.attempts, repeated.execution_millis), (2, 8000));
    let (second, _) =
        plan_attempt_reservation(Some(&first), "second", &limits, Duration::from_secs(4)).unwrap();
    assert_eq!((second.attempts, second.execution_millis), (2, 8000));
    assert!(plan_attempt_settlement(&second, "first", Duration::ZERO).is_err());
    let settled = plan_attempt_settlement(&second, "second", Duration::from_secs(1)).unwrap();
    assert_eq!((settled.attempts, settled.execution_millis), (2, 5000));
    assert!(settled.reservation.is_none());
    assert!(plan_attempt_settlement(&settled, "second", Duration::ZERO).is_err());
    let (third, allowed) =
        plan_attempt_reservation(Some(&settled), "third", &limits, Duration::from_secs(8)).unwrap();
    assert_eq!(allowed, Duration::from_secs(5));
    assert!(
        plan_attempt_reservation(Some(&third), "fourth", &limits, Duration::from_secs(1)).is_err()
    );
    let capped = plan_attempt_settlement(&third, "third", Duration::from_secs(100)).unwrap();
    assert_eq!(capped.execution_millis, 10_000);
    let refunded = plan_attempt_settlement(&third, "third", Duration::ZERO).unwrap();
    assert!(
        plan_attempt_reservation(Some(&refunded), "fourth", &limits, Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(
        AttemptFailure::new("second", "failed", now()).attempt_id,
        "second"
    );
    assert_eq!(second.execution_millis, 8000);
}

#[test]
fn budget_rejects_configuration_changes_underflow_zero_and_overflow() {
    let limits = CycleLimits::default();
    let (budget, _) =
        plan_attempt_reservation(None, "first", &limits, Duration::from_secs(1)).unwrap();
    let mut changed = limits.clone();
    changed.max_revisions += 1;
    assert!(
        plan_attempt_reservation(Some(&budget), "next", &changed, Duration::from_secs(1)).is_err()
    );
    for timeout in [Duration::ZERO, Duration::from_nanos(999_999), Duration::MAX] {
        assert!(plan_attempt_reservation(None, "first", &limits, timeout).is_err());
    }
    for (attempts, seconds) in [(0, 1), (1, 0), (1, u64::MAX)] {
        let limits = CycleLimits {
            max_attempts: attempts,
            execution_seconds: seconds,
            max_revisions: 1,
        };
        assert!(plan_attempt_reservation(None, "first", &limits, Duration::from_secs(1)).is_err());
    }
    let mut corrupt = budget.clone();
    corrupt.execution_millis = 0;
    assert!(plan_attempt_settlement(&corrupt, "first", Duration::ZERO).is_err());
    assert!(serde_json::from_value::<CycleBudget>(json!({
        "limits": limits, "attempts": 1, "executionMillis": 1000, "reservation": null, "unknown": true
    })).is_err());
}

#[test]
fn admission_separates_live_owner_from_sealed_proposal_provenance() {
    let contract = contract();
    let resource = contract.resource().unwrap();
    let cycle = cycle();
    let claim = WorkspaceClaim {
        resource: resource.clone(),
        workspace_id: "workspace".into(),
        attempt_id: "replacement".into(),
        epoch: 2,
    };
    let record = ClaimRecord {
        schema_version: 1,
        epoch: 2,
        attempt_id: "replacement".into(),
        claimed_at: now(),
        active: true,
        cycle_id: Some("cycle".into()),
    };
    let identity = AdmissionIdentity {
        resource: &resource,
        workspace_id: "workspace",
        cycle_id: "cycle",
        contract_digest: &contract.contract_digest,
        attempt_id: "original",
        analysis_manifest_digest: LEGACY_DIGEST,
    };
    let proposal = plan_proposal(None, "original", json!([]), LEGACY_MANIFEST).unwrap();
    assert!(admission_owner_matches(
        &record,
        &claim.candidate_ownership(),
        "cycle"
    ));
    assert!(admission_proposal_matches(&identity, &proposal));
    assert!(
        admission_cycle_matches(
            &identity,
            &resource,
            "workspace",
            Some("cycle"),
            &cycle,
            &contract,
            false
        )
        .unwrap()
    );
    let superseding = cycle.superseding(Some("new"));
    assert_eq!(superseding.replacement_digest.as_deref(), Some("new"));
    assert!(
        !admission_cycle_matches(
            &identity,
            &resource,
            "workspace",
            Some("cycle"),
            &superseding,
            &contract,
            false
        )
        .unwrap()
    );
    assert!(
        !admission_cycle_matches(
            &identity,
            &resource,
            "workspace",
            Some("cycle"),
            &cycle,
            &contract,
            true
        )
        .unwrap()
    );
    assert!(
        !admission_cycle_matches(
            &identity,
            &resource,
            "workspace",
            None,
            &cycle,
            &contract,
            false
        )
        .unwrap()
    );
    let mut wrong_resource = resource.clone();
    wrong_resource.scope.tenant = "other".into();
    assert!(
        !admission_cycle_matches(
            &identity,
            &wrong_resource,
            "workspace",
            Some("cycle"),
            &cycle,
            &contract,
            false
        )
        .unwrap()
    );
    let other = plan_proposal(None, "replacement", json!([]), LEGACY_MANIFEST).unwrap();
    assert!(!admission_proposal_matches(&identity, &other));
    for mutate in 0..5 {
        let mut stale = record.clone();
        match mutate {
            0 => stale.active = false,
            1 => stale.epoch = 1,
            2 => stale.attempt_id = "original".into(),
            3 => stale.cycle_id = None,
            _ => stale.schema_version = 0,
        }
        assert!(!admission_owner_matches(
            &stale,
            &claim.candidate_ownership(),
            "cycle"
        ));
    }
    validate_claim_epoch(&record, 2).unwrap();
    assert!(validate_claim_epoch(&record, 1).is_err());
}
