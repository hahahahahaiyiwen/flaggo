use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisCycle, AnalysisError, AnalysisProfile, CandidateProposal,
    CandidateRecord, CycleOutcome, EvidenceCutoff,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::{
    CyclePreparation, PrepareCycleResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFileInfo,
    WorkspaceFileSystem, WorkspaceLease, WorkspaceProvider, sha256_digest, workspace_id,
};

const WORKSPACE_SCHEMA_VERSION: u32 = 1;
const VIRTUAL_ROOT: &str = "workspace";

#[derive(Clone)]
pub struct LocalWorkspaceProvider {
    root: PathBuf,
    process_claims: Arc<Mutex<HashSet<String>>>,
}

impl LocalWorkspaceProvider {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, AnalysisError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(workspace_error)?;
        let root = fs::canonicalize(root).map_err(workspace_error)?;
        Ok(Self {
            root,
            process_claims: Arc::new(Mutex::new(HashSet::new())),
        })
    }
}

#[async_trait]
impl WorkspaceProvider for LocalWorkspaceProvider {
    async fn claim(
        &self,
        contract_name: &str,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceLease>, AnalysisError> {
        validate_contract_name(contract_name)?;
        validate_segment(attempt_id, "attempt ID")?;

        let id = workspace_id(contract_name);
        {
            let mut claims = mutex_lock(&self.process_claims)?;
            if !claims.insert(id.clone()) {
                return Err(AnalysisError::WorkspaceBusy(id.clone()));
            }
        }

        let result = self.open_claim(contract_name, attempt_id, &id);
        if result.is_err() {
            mutex_lock(&self.process_claims)?.remove(&id);
        }
        result.map(|lease| Arc::new(lease) as Arc<dyn WorkspaceLease>)
    }
}

impl LocalWorkspaceProvider {
    fn open_claim(
        &self,
        contract_name: &str,
        attempt_id: &str,
        id: &str,
    ) -> Result<LocalWorkspaceLease, AnalysisError> {
        let directory_name = id.strip_prefix("sha256:").ok_or_else(|| {
            AnalysisError::workspace("workspace ID did not use the sha256 scheme")
        })?;
        let root = self.root.join(directory_name);
        fs::create_dir_all(root.join("cycles")).map_err(workspace_error)?;

        let lock_path = root.join("claim.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(workspace_error)?;
        if let Err(error) = lock_file.try_lock() {
            return match error {
                std::fs::TryLockError::WouldBlock => {
                    Err(AnalysisError::WorkspaceBusy(id.to_owned()))
                }
                std::fs::TryLockError::Error(error) => Err(workspace_error(error)),
            };
        }

        let manifest_path = root.join("workspace.json");
        if manifest_path.exists() {
            let manifest: WorkspaceManifest = read_json(&manifest_path)?;
            if manifest.schema_version != WORKSPACE_SCHEMA_VERSION
                || manifest.workspace_id != id
                || manifest.contract_name != contract_name
            {
                return Err(AnalysisError::workspace(
                    "workspace manifest does not match the requested contract",
                ));
            }
        } else {
            write_json_atomic(
                &manifest_path,
                &WorkspaceManifest {
                    schema_version: WORKSPACE_SCHEMA_VERSION,
                    workspace_id: id.to_owned(),
                    contract_name: contract_name.to_owned(),
                    created_at: Utc::now(),
                },
            )?;
        }

        write_json_atomic(
            &root.join("claim.json"),
            &ClaimRecord {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                attempt_id: attempt_id.to_owned(),
                claimed_at: Utc::now(),
            },
        )?;

        Ok(LocalWorkspaceLease {
            root,
            workspace_id: id.to_owned(),
            contract_name: contract_name.to_owned(),
            process_claims: Arc::clone(&self.process_claims),
            _lock_file: lock_file,
            state: Arc::new(Mutex::new(LeaseState::default())),
            io_gate: Arc::new(Mutex::new(())),
        })
    }
}

pub struct LocalWorkspaceLease {
    root: PathBuf,
    workspace_id: String,
    contract_name: String,
    process_claims: Arc<Mutex<HashSet<String>>>,
    _lock_file: File,
    state: Arc<Mutex<LeaseState>>,
    io_gate: Arc<Mutex<()>>,
}

impl Drop for LocalWorkspaceLease {
    fn drop(&mut self) {
        if let Ok(mut claims) = self.process_claims.lock() {
            claims.remove(&self.workspace_id);
        }
    }
}

#[derive(Default)]
struct LeaseState {
    active_cycle_id: Option<String>,
    writable: bool,
    sealed: bool,
}

#[async_trait]
impl WorkspaceLease for LocalWorkspaceLease {
    fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    fn contract_name(&self) -> &str {
        &self.contract_name
    }

    async fn prepare_cycle(
        &self,
        contract: &AnalysisContract,
        analysis_profile: &AnalysisProfile,
        attempt_id: &str,
        now: DateTime<Utc>,
    ) -> Result<PrepareCycleResult, AnalysisError> {
        validate_segment(attempt_id, "attempt ID")?;
        if contract.name != self.contract_name {
            return Err(AnalysisError::workspace(
                "contract name does not match the claimed workspace",
            ));
        }

        let _io_guard = mutex_lock(&self.io_gate)?;
        let pointer_path = self.root.join("current-cycle.json");
        let mut pointer = if pointer_path.exists() {
            read_json::<CurrentCycle>(&pointer_path)?
        } else {
            CurrentCycle::empty()
        };
        if let Some(cycle_id) = pointer.cycle_id.as_deref()
            && self.cycle_path(cycle_id).join("outcome.json").exists()
        {
            self.reconcile_terminal_cycle(cycle_id)?;
            pointer = CurrentCycle::empty();
        }
        let mut schedule = self.load_or_create_schedule(contract)?;

        if let Some(cycle_id) = pointer.cycle_id.clone() {
            let mut cycle = self.read_cycle(&cycle_id)?;
            if cycle.status == CycleStatus::Active || cycle.status == CycleStatus::Superseding {
                if cycle.contract_digest == contract.contract_digest {
                    if cycle.analysis_profile != *analysis_profile {
                        return Err(AnalysisError::workspace(
                            "recoverable cycle uses a different analysis profile",
                        ));
                    }
                    self.record_attempt(&cycle_id, attempt_id, now)?;
                    let sealed = self
                        .cycle_path(&cycle_id)
                        .join("analysis-manifest.json")
                        .exists()
                        || self
                            .cycle_path(&cycle_id)
                            .join("proposal.pending.json")
                            .exists();
                    self.set_active_state(&cycle_id, !sealed, sealed)?;
                    return Ok(PrepareCycleResult::Ready(CyclePreparation {
                        cycle: cycle.as_analysis_cycle(true),
                        eligible_at: schedule.next_eligible_at,
                    }));
                }

                self.finish_superseded_cycle(
                    &mut cycle,
                    Some(contract.contract_digest.clone()),
                    now,
                )?;
            }
            pointer.cycle_id = None;
            write_json_atomic(&pointer_path, &pointer)?;
        }

        if schedule.contract_digest != contract.contract_digest {
            schedule = ScheduleState::new(contract)?;
            write_json_atomic(&self.root.join("schedule.json"), &schedule)?;
        }
        if now < schedule.next_eligible_at {
            self.clear_active_state()?;
            return Ok(PrepareCycleResult::NotEligible {
                eligible_at: schedule.next_eligible_at,
            });
        }

        let cycle_id = Uuid::new_v4().to_string();
        let cycle_root = self.cycle_path(&cycle_id);
        let cycle = StoredCycle {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: self.workspace_id.clone(),
            cycle_id: cycle_id.clone(),
            contract_name: contract.name.clone(),
            contract_digest: contract.contract_digest.clone(),
            analysis_profile: analysis_profile.clone(),
            opened_at: now,
            status: CycleStatus::Active,
            replacement_digest: None,
        };
        let initialization = (|| {
            fs::create_dir_all(cycle_root.join("analysis")).map_err(workspace_error)?;
            fs::create_dir_all(cycle_root.join("handoffs")).map_err(workspace_error)?;
            fs::create_dir_all(cycle_root.join("audit")).map_err(workspace_error)?;
            fs::create_dir_all(cycle_root.join("attempts")).map_err(workspace_error)?;
            write_json_atomic(&cycle_root.join("contract.json"), contract)?;
            write_json_atomic(&cycle_root.join("cycle.json"), &cycle)?;
            self.record_attempt(&cycle_id, attempt_id, now)?;
            pointer.cycle_id = Some(cycle_id.clone());
            write_json_atomic(&pointer_path, &pointer)
        })();
        if let Err(error) = initialization {
            return Err(cleanup_incomplete_cycle(&cycle_root, error));
        }
        self.set_active_state(&cycle_id, true, false)?;

        Ok(PrepareCycleResult::Ready(CyclePreparation {
            cycle: cycle.as_analysis_cycle(false),
            eligible_at: schedule.next_eligible_at,
        }))
    }

    async fn filesystem(
        &self,
        cycle_id: &str,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceFileSystem>, AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        validate_segment(attempt_id, "attempt ID")?;
        self.require_active_cycle(cycle_id)?;
        Ok(Arc::new(LocalWorkspaceFileSystem {
            root: self.cycle_path(cycle_id),
            workspace_root: self.root.clone(),
            cycle_id: cycle_id.to_owned(),
            state: Arc::clone(&self.state),
            io_gate: Arc::clone(&self.io_gate),
        }))
    }

    async fn commit_cutoff(
        &self,
        cycle_id: &str,
        cutoff: DateTime<Utc>,
        watermark: u64,
    ) -> Result<EvidenceCutoff, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        let path = self.cycle_path(cycle_id).join("cutoff.json");
        let requested = EvidenceCutoff { cutoff, watermark };
        if path.exists() {
            let existing = read_json::<EvidenceCutoff>(&path)?;
            if existing != requested {
                return Err(AnalysisError::workspace(
                    "evidence cutoff is immutable once committed",
                ));
            }
            return Ok(existing);
        }
        write_json_atomic(&path, &requested)?;
        Ok(requested)
    }

    async fn cutoff(&self, cycle_id: &str) -> Result<Option<EvidenceCutoff>, AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        let path = self.cycle_path(cycle_id).join("cutoff.json");
        if path.exists() {
            read_json(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    async fn append_audit(
        &self,
        cycle_id: &str,
        category: &str,
        record: Value,
    ) -> Result<(), AnalysisError> {
        validate_segment(category, "audit category")?;
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        let directory = self.cycle_path(cycle_id).join("audit").join(category);
        fs::create_dir_all(&directory).map_err(workspace_error)?;
        write_json_atomic(
            &directory.join(format!("{}.json", Uuid::new_v4())),
            &AuditRecord {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                recorded_at: Utc::now(),
                record,
            },
        )
    }

    async fn seal_analysis_manifest(&self, cycle_id: &str) -> Result<String, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let digest = self.ensure_manifest(cycle_id)?;
        let mut state = mutex_lock(&self.state)?;
        state.writable = false;
        state.sealed = true;
        Ok(digest)
    }

    async fn prepare_candidate_proposal(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        rules: Value,
    ) -> Result<CandidateProposal, AnalysisError> {
        validate_segment(attempt_id, "attempt ID")?;
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let cycle_root = self.cycle_path(cycle_id);
        let pending_path = cycle_root.join("proposal.pending.json");
        let proposal_path = cycle_root.join("proposal.json");

        let proposal = if pending_path.exists() {
            let existing = read_json::<CandidateProposal>(&pending_path)?;
            if existing.rules != rules {
                return Err(AnalysisError::workspace(
                    "a different Candidate proposal is already committed for this cycle",
                ));
            }
            let manifest_path = cycle_root.join("analysis-manifest.json");
            if !manifest_path.exists() {
                let manifest = self.build_manifest(cycle_id)?;
                let manifest_bytes = serde_json::to_vec(&manifest).map_err(workspace_error)?;
                if sha256_digest(&manifest_bytes) != existing.analysis_manifest_digest {
                    return Err(AnalysisError::workspace(
                        "recovered analysis manifest does not match the durable proposal",
                    ));
                }
                write_bytes_atomic(&manifest_path, &manifest_bytes)?;
            }
            existing
        } else {
            self.require_writable(cycle_id)?;
            let manifest = self.build_manifest(cycle_id)?;
            let manifest_bytes = serde_json::to_vec(&manifest).map_err(workspace_error)?;
            let proposal = CandidateProposal {
                attempt_id: attempt_id.to_owned(),
                rules,
                analysis_manifest_digest: sha256_digest(&manifest_bytes),
            };
            write_json_atomic(&pending_path, &proposal)?;
            write_bytes_atomic(&cycle_root.join("analysis-manifest.json"), &manifest_bytes)?;
            proposal
        };

        self.verify_manifest(cycle_id)?;
        let actual_digest = digest_file(&cycle_root.join("analysis-manifest.json"))?;
        if actual_digest != proposal.analysis_manifest_digest {
            return Err(AnalysisError::workspace(
                "sealed analysis manifest does not match the durable proposal",
            ));
        }
        if proposal_path.exists() {
            let existing = read_json::<CandidateProposal>(&proposal_path)?;
            if existing != proposal {
                return Err(AnalysisError::workspace(
                    "Candidate proposal record conflicts with its recovery journal",
                ));
            }
        } else {
            write_json_atomic(&proposal_path, &proposal)?;
        }
        let mut state = mutex_lock(&self.state)?;
        state.writable = false;
        state.sealed = true;
        Ok(proposal)
    }

    async fn candidate_record(
        &self,
        cycle_id: &str,
    ) -> Result<Option<CandidateRecord>, AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        let path = self.cycle_path(cycle_id).join("candidate.json");
        if path.exists() {
            read_json(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    async fn record_candidate(
        &self,
        cycle_id: &str,
        candidate: &CandidateRecord,
    ) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let proposal: CandidateProposal =
            read_json(&self.cycle_path(cycle_id).join("proposal.json"))?;
        if proposal.analysis_manifest_digest != candidate.analysis_manifest_digest {
            return Err(AnalysisError::workspace(
                "Candidate response does not match the sealed analysis manifest",
            ));
        }
        let path = self.cycle_path(cycle_id).join("candidate.json");
        if path.exists() {
            if read_json::<CandidateRecord>(&path)? != *candidate {
                return Err(AnalysisError::workspace(
                    "Candidate response conflicts with the durable cycle record",
                ));
            }
            return Ok(());
        }
        write_json_atomic(&path, candidate)
    }

    async fn mark_superseding(
        &self,
        cycle_id: &str,
        replacement_digest: Option<&str>,
    ) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let mut cycle = self.read_cycle(cycle_id)?;
        cycle.status = CycleStatus::Superseding;
        cycle.replacement_digest = replacement_digest.map(ToOwned::to_owned);
        write_json_atomic(&self.cycle_path(cycle_id).join("cycle.json"), &cycle)?;
        Ok(())
    }

    async fn record_handoff(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        summary: &str,
    ) -> Result<(), AnalysisError> {
        validate_segment(attempt_id, "attempt ID")?;
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        write_json_atomic(
            &self
                .cycle_path(cycle_id)
                .join("handoffs")
                .join(format!("{attempt_id}.json")),
            &HandoffRecord {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                attempt_id: attempt_id.to_owned(),
                recorded_at: Utc::now(),
                summary: summary.to_owned(),
            },
        )
    }

    async fn record_attempt_failure(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        error: &str,
    ) -> Result<(), AnalysisError> {
        validate_segment(attempt_id, "attempt ID")?;
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        write_json_atomic(
            &self
                .cycle_path(cycle_id)
                .join("attempts")
                .join(format!("{attempt_id}-failure.json")),
            &AttemptFailure {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                attempt_id: attempt_id.to_owned(),
                recorded_at: Utc::now(),
                error: error.to_owned(),
            },
        )
    }

    async fn complete_cycle(
        &self,
        contract: &AnalysisContract,
        cycle_id: &str,
        outcome: &CycleOutcome,
        completed_at: DateTime<Utc>,
    ) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let outcome_path = self.cycle_path(cycle_id).join("outcome.json");
        let stored_contract: AnalysisContract =
            read_json(&self.cycle_path(cycle_id).join("contract.json"))?;
        if stored_contract.name != contract.name
            || stored_contract.contract_digest != contract.contract_digest
        {
            return Err(AnalysisError::workspace(
                "terminal cycle contract does not match its durable contract snapshot",
            ));
        }
        if outcome_path.exists() {
            let existing = read_json::<StoredOutcome>(&outcome_path)?;
            if existing.outcome != *outcome {
                return Err(AnalysisError::workspace(
                    "terminal cycle outcome cannot be changed",
                ));
            }
            return self.reconcile_terminal_cycle(cycle_id);
        }

        self.ensure_manifest(cycle_id)?;
        if let CycleOutcome::Candidate { candidate } = outcome {
            let stored =
                read_json::<CandidateRecord>(&self.cycle_path(cycle_id).join("candidate.json"))?;
            if stored != *candidate {
                return Err(AnalysisError::workspace(
                    "terminal Candidate does not match the durable Candidate record",
                ));
            }
        }
        write_json_atomic(
            &outcome_path,
            &StoredOutcome {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                completed_at,
                outcome: outcome.clone(),
            },
        )?;
        self.reconcile_terminal_cycle(cycle_id)
    }
}

impl LocalWorkspaceLease {
    fn cycle_path(&self, cycle_id: &str) -> PathBuf {
        self.root.join("cycles").join(cycle_id)
    }

    fn read_cycle(&self, cycle_id: &str) -> Result<StoredCycle, AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        read_json(&self.cycle_path(cycle_id).join("cycle.json"))
    }

    fn load_or_create_schedule(
        &self,
        contract: &AnalysisContract,
    ) -> Result<ScheduleState, AnalysisError> {
        let path = self.root.join("schedule.json");
        if path.exists() {
            read_json(&path)
        } else {
            let schedule = ScheduleState::new(contract)?;
            write_json_atomic(&path, &schedule)?;
            Ok(schedule)
        }
    }

    fn reconcile_terminal_cycle(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        let cycle_root = self.cycle_path(cycle_id);
        let outcome = read_json::<StoredOutcome>(&cycle_root.join("outcome.json"))?;
        let contract = read_json::<AnalysisContract>(&cycle_root.join("contract.json"))?;
        self.verify_manifest(cycle_id)?;

        if let CycleOutcome::Candidate { candidate } = &outcome.outcome {
            let stored = read_json::<CandidateRecord>(&cycle_root.join("candidate.json"))?;
            if stored != *candidate {
                return Err(AnalysisError::workspace(
                    "terminal Candidate does not match the durable Candidate record",
                ));
            }
        }

        let mut cycle = self.read_cycle(cycle_id)?;
        cycle.status = CycleStatus::Terminal;
        if let CycleOutcome::Superseded { replacement_digest } = &outcome.outcome {
            cycle.replacement_digest = replacement_digest.clone();
        }
        write_json_atomic(&cycle_root.join("cycle.json"), &cycle)?;

        let schedule = ScheduleState {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            contract_digest: contract.contract_digest.clone(),
            accepted_at: contract.accepted_at,
            last_terminal_completed_at: Some(outcome.completed_at),
            next_eligible_at: contract
                .eligible_after(outcome.completed_at)
                .map_err(|error| AnalysisError::workspace(error.to_string()))?,
        };
        write_json_atomic(&self.root.join("schedule.json"), &schedule)?;
        write_json_atomic(
            &self.root.join("current-cycle.json"),
            &CurrentCycle::empty(),
        )?;
        self.clear_active_state()
    }

    fn set_active_state(
        &self,
        cycle_id: &str,
        writable: bool,
        sealed: bool,
    ) -> Result<(), AnalysisError> {
        let mut state = mutex_lock(&self.state)?;
        state.active_cycle_id = Some(cycle_id.to_owned());
        state.writable = writable;
        state.sealed = sealed;
        Ok(())
    }

    fn clear_active_state(&self) -> Result<(), AnalysisError> {
        let mut state = mutex_lock(&self.state)?;
        state.active_cycle_id = None;
        state.writable = false;
        state.sealed = false;
        Ok(())
    }

    fn require_active_cycle(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        let state = mutex_lock(&self.state)?;
        if state.active_cycle_id.as_deref() != Some(cycle_id) {
            return Err(AnalysisError::workspace(
                "operation does not target the active claimed cycle",
            ));
        }
        Ok(())
    }

    fn require_writable(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        let state = mutex_lock(&self.state)?;
        if state.active_cycle_id.as_deref() != Some(cycle_id) || !state.writable || state.sealed {
            return Err(AnalysisError::workspace(
                "analysis workspace is no longer writable",
            ));
        }
        Ok(())
    }

    fn record_attempt(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), AnalysisError> {
        let path = self
            .cycle_path(cycle_id)
            .join("attempts")
            .join(format!("{attempt_id}.json"));
        if path.exists() {
            return Ok(());
        }
        write_json_atomic(
            &path,
            &AttemptRecord {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                attempt_id: attempt_id.to_owned(),
                started_at,
            },
        )
    }

    fn finish_superseded_cycle(
        &self,
        cycle: &mut StoredCycle,
        replacement_digest: Option<String>,
        completed_at: DateTime<Utc>,
    ) -> Result<(), AnalysisError> {
        let cycle_root = self.cycle_path(&cycle.cycle_id);
        self.ensure_manifest(&cycle.cycle_id)?;
        let outcome = StoredOutcome {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            completed_at,
            outcome: CycleOutcome::Superseded {
                replacement_digest: replacement_digest.clone(),
            },
        };
        write_json_atomic(&cycle_root.join("outcome.json"), &outcome)?;
        cycle.status = CycleStatus::Terminal;
        cycle.replacement_digest = replacement_digest;
        write_json_atomic(&cycle_root.join("cycle.json"), cycle)
    }

    fn ensure_manifest(&self, cycle_id: &str) -> Result<String, AnalysisError> {
        let path = self.cycle_path(cycle_id).join("analysis-manifest.json");
        if path.exists() {
            self.verify_manifest(cycle_id)?;
            return digest_file(&path);
        }
        let manifest = self.build_manifest(cycle_id)?;
        let bytes = serde_json::to_vec(&manifest).map_err(workspace_error)?;
        let digest = sha256_digest(&bytes);
        write_bytes_atomic(&path, &bytes)?;
        Ok(digest)
    }

    fn verify_manifest(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        let path = self.cycle_path(cycle_id).join("analysis-manifest.json");
        let stored = read_json::<AnalysisManifest>(&path)?;
        let actual = self.build_manifest(cycle_id)?;
        if stored != actual {
            return Err(AnalysisError::workspace(
                "analysis workspace changed after its manifest was sealed",
            ));
        }
        Ok(())
    }

    fn build_manifest(&self, cycle_id: &str) -> Result<AnalysisManifest, AnalysisError> {
        let root = self.cycle_path(cycle_id);
        let cycle = self.read_cycle(cycle_id)?;
        let mut files = Vec::new();
        collect_manifest_files(&root, &root, &mut files)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(AnalysisManifest {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: self.workspace_id.clone(),
            cycle_id: cycle_id.to_owned(),
            analysis_profile: cycle.analysis_profile,
            files,
        })
    }
}

struct LocalWorkspaceFileSystem {
    root: PathBuf,
    workspace_root: PathBuf,
    cycle_id: String,
    state: Arc<Mutex<LeaseState>>,
    io_gate: Arc<Mutex<()>>,
}

#[async_trait]
impl WorkspaceFileSystem for LocalWorkspaceFileSystem {
    async fn read_text(&self, path: &str) -> Result<String, AnalysisError> {
        let resolved = self.resolve(path, false)?;
        ensure_regular_file(&resolved)?;
        fs::read_to_string(resolved).map_err(workspace_error)
    }

    async fn write_text(&self, path: &str, content: &str) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let resolved = self.resolve(path, true)?;
        self.require_writable(&resolved)?;
        ensure_parent_has_no_symlinks(&self.root, &resolved)?;
        write_bytes_atomic(&resolved, content.as_bytes())
    }

    async fn append_text(&self, path: &str, content: &str) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let resolved = self.resolve(path, true)?;
        self.require_writable(&resolved)?;
        ensure_parent_has_no_symlinks(&self.root, &resolved)?;
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).map_err(workspace_error)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(resolved)
            .map_err(workspace_error)?;
        file.write_all(content.as_bytes())
            .map_err(workspace_error)?;
        file.sync_all().map_err(workspace_error)
    }

    async fn exists(&self, path: &str) -> Result<bool, AnalysisError> {
        let resolved = self.resolve(path, false)?;
        match fs::symlink_metadata(resolved) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    Err(AnalysisError::workspace(
                        "symbolic links are not exposed to analysis agents",
                    ))
                } else {
                    Ok(true)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(workspace_error(error)),
        }
    }

    async fn metadata(&self, path: &str) -> Result<WorkspaceFileInfo, AnalysisError> {
        let resolved = self.resolve(path, false)?;
        let metadata = fs::symlink_metadata(resolved).map_err(workspace_error)?;
        if metadata.file_type().is_symlink() {
            return Err(AnalysisError::workspace(
                "symbolic links are not exposed to analysis agents",
            ));
        }
        Ok(WorkspaceFileInfo {
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            modified_at: metadata
                .modified()
                .map(DateTime::<Utc>::from)
                .map_err(workspace_error)?,
        })
    }

    async fn create_directory(&self, path: &str, recursive: bool) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let resolved = self.resolve(path, true)?;
        self.require_writable(&resolved)?;
        ensure_parent_has_no_symlinks(&self.root, &resolved)?;
        if recursive {
            fs::create_dir_all(resolved).map_err(workspace_error)
        } else {
            fs::create_dir(resolved).map_err(workspace_error)
        }
    }

    async fn read_directory(&self, path: &str) -> Result<Vec<WorkspaceEntry>, AnalysisError> {
        let resolved = self.resolve(path, false)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(resolved).map_err(workspace_error)? {
            let entry = entry.map_err(workspace_error)?;
            let metadata = entry.file_type().map_err(workspace_error)?;
            if metadata.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_history_root(path) && name == self.cycle_id {
                continue;
            }
            let kind = if metadata.is_dir() {
                WorkspaceEntryKind::Directory
            } else {
                WorkspaceEntryKind::File
            };
            entries.push(WorkspaceEntry { name, kind });
        }
        if is_workspace_root(path) {
            entries.push(WorkspaceEntry {
                name: "history".to_owned(),
                kind: WorkspaceEntryKind::Directory,
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let resolved = self.resolve(path, true)?;
        self.require_writable(&resolved)?;
        let result = match fs::symlink_metadata(&resolved) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AnalysisError::workspace(
                    "symbolic links are not exposed to analysis agents",
                ));
            }
            Ok(metadata) if metadata.is_dir() && recursive => fs::remove_dir_all(resolved),
            Ok(metadata) if metadata.is_dir() => fs::remove_dir(resolved),
            Ok(_) => fs::remove_file(resolved),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => Ok(()),
            Err(error) if force && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(workspace_error(error)),
        }
    }

    async fn rename(&self, source: &str, destination: &str) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let source = self.resolve(source, true)?;
        let destination = self.resolve(destination, true)?;
        self.require_writable(&source)?;
        self.require_writable(&destination)?;
        ensure_parent_has_no_symlinks(&self.root, &destination)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(workspace_error)?;
        }
        fs::rename(source, destination).map_err(workspace_error)
    }
}

impl LocalWorkspaceFileSystem {
    fn require_writable(&self, path: &Path) -> Result<(), AnalysisError> {
        let state = mutex_lock(&self.state)?;
        let active = state.active_cycle_id.as_deref() == Some(&self.cycle_id);
        let provider_state = path
            .strip_prefix(&self.root)
            .is_ok_and(is_provider_state_path);
        if !active || (!provider_state && (!state.writable || state.sealed)) {
            return Err(AnalysisError::workspace(
                "analysis workspace is no longer writable",
            ));
        }
        Ok(())
    }

    fn resolve(&self, path: &str, writing: bool) -> Result<PathBuf, AnalysisError> {
        if path.contains('\\') {
            return Err(AnalysisError::workspace(
                "virtual workspace paths must use POSIX separators",
            ));
        }
        let mut components = Path::new(path).components();
        if matches!(components.next(), Some(Component::RootDir)) {
            // Absolute virtual paths are rooted at /workspace.
        } else {
            components = Path::new(path).components();
        }
        if components.next().and_then(component_text) != Some(VIRTUAL_ROOT) {
            return Err(AnalysisError::workspace(
                "virtual workspace paths must be rooted at /workspace",
            ));
        }

        let mut relative = PathBuf::new();
        for component in components {
            match component {
                Component::Normal(value) => relative.push(value),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(AnalysisError::workspace(
                        "virtual workspace path escapes its cycle root",
                    ));
                }
            }
        }
        if writing {
            let first = relative.components().next().and_then(component_text);
            if !matches!(first, Some("analysis" | "handoffs")) || relative.as_os_str().is_empty() {
                return Err(AnalysisError::workspace(
                    "agents may write only under /workspace/analysis or /workspace/handoffs",
                ));
            }
        }
        if relative.components().next().and_then(component_text) == Some("history") {
            let mut parts = relative.components();
            parts.next();
            let Some(cycle_id) = parts.next().and_then(component_text) else {
                return Ok(self.workspace_root.join("cycles"));
            };
            validate_segment(cycle_id, "history cycle ID")?;
            if cycle_id == self.cycle_id {
                return Err(AnalysisError::workspace(
                    "the active cycle is not part of read-only history",
                ));
            }
            let remainder = parts.map(|part| part.as_os_str()).collect::<PathBuf>();
            let provider_state = PathBuf::from("analysis").join(".copilot");
            if remainder == provider_state || remainder.starts_with(&provider_state) {
                return Err(AnalysisError::workspace(
                    "provider runtime state is not part of analysis history",
                ));
            }
            return Ok(self
                .workspace_root
                .join("cycles")
                .join(cycle_id)
                .join(remainder));
        }
        Ok(self.root.join(relative))
    }
}

fn is_provider_state_path(path: &Path) -> bool {
    path.starts_with(PathBuf::from("analysis").join(".copilot"))
}

fn is_history_root(path: &str) -> bool {
    matches!(
        path.trim_end_matches('/'),
        "/workspace/history" | "workspace/history"
    )
}

fn is_workspace_root(path: &str) -> bool {
    matches!(path.trim_end_matches('/'), "/workspace" | "workspace")
}

fn component_text(component: Component<'_>) -> Option<&str> {
    match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn collect_manifest_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<ManifestFile>,
) -> Result<(), AnalysisError> {
    for entry in fs::read_dir(directory).map_err(workspace_error)? {
        let entry = entry.map_err(workspace_error)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(workspace_error)?;
        if metadata.file_type().is_symlink() {
            return Err(AnalysisError::workspace(
                "analysis workspace contains a symbolic link",
            ));
        }
        if metadata.is_dir() {
            collect_manifest_files(root, &path, files)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(workspace_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if is_manifest_control_file(&relative) {
            continue;
        }
        let contents = fs::read(&path).map_err(workspace_error)?;
        files.push(ManifestFile {
            path: relative,
            size: contents.len() as u64,
            digest: sha256_digest(&contents),
        });
    }
    Ok(())
}

fn is_manifest_control_file(relative: &str) -> bool {
    matches!(
        relative,
        "cycle.json"
            | "analysis-manifest.json"
            | "proposal.pending.json"
            | "proposal.json"
            | "candidate.json"
            | "outcome.json"
    ) || relative.starts_with("attempts/")
        || relative.starts_with("analysis/.copilot/")
}

fn ensure_parent_has_no_symlinks(root: &Path, target: &Path) -> Result<(), AnalysisError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| AnalysisError::workspace("workspace path escaped its cycle root"))?;
    let mut current = root.to_path_buf();
    let parents = relative
        .parent()
        .map(Path::components)
        .into_iter()
        .flatten();
    for component in parents {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AnalysisError::workspace(
                    "workspace writes may not traverse symbolic links",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AnalysisError::workspace(
                    "workspace write parent is not a directory",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(workspace_error(error)),
        }
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), AnalysisError> {
    let metadata = fs::symlink_metadata(path).map_err(workspace_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AnalysisError::workspace(
            "workspace path is not a regular file",
        ));
    }
    Ok(())
}

fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), AnalysisError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(workspace_error)?;
    write_bytes_atomic(path, &bytes)
}

fn cleanup_incomplete_cycle(cycle_root: &Path, error: AnalysisError) -> AnalysisError {
    match fs::remove_dir_all(cycle_root) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => AnalysisError::workspace(format!(
            "{error}; failed to remove incomplete cycle '{}': {cleanup}",
            cycle_root.display()
        )),
    }
}

fn write_bytes_atomic(path: &Path, contents: &[u8]) -> Result<(), AnalysisError> {
    let parent = path
        .parent()
        .ok_or_else(|| AnalysisError::workspace("workspace file has no parent directory"))?;
    fs::create_dir_all(parent).map_err(workspace_error)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(workspace_error)?;
    temporary.write_all(contents).map_err(workspace_error)?;
    temporary.as_file().sync_all().map_err(workspace_error)?;
    temporary
        .persist(path)
        .map_err(|error| workspace_error(error.error))?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, AnalysisError> {
    let mut file = File::open(path).map_err(workspace_error)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).map_err(workspace_error)?;
    serde_json::from_slice(&contents).map_err(workspace_error)
}

fn digest_file(path: &Path) -> Result<String, AnalysisError> {
    fs::read(path)
        .map(|bytes| sha256_digest(&bytes))
        .map_err(workspace_error)
}

fn validate_contract_name(value: &str) -> Result<(), AnalysisError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(AnalysisError::workspace("contract name is invalid"));
    }
    Ok(())
}

fn validate_segment(value: &str, label: &str) -> Result<(), AnalysisError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AnalysisError::workspace(format!("{label} is invalid")));
    }
    Ok(())
}

fn mutex_lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, AnalysisError> {
    mutex
        .lock()
        .map_err(|_| AnalysisError::workspace("workspace lock was poisoned"))
}

fn workspace_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::workspace(error.to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceManifest {
    schema_version: u32,
    workspace_id: String,
    contract_name: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRecord {
    schema_version: u32,
    attempt_id: String,
    claimed_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleState {
    schema_version: u32,
    contract_digest: String,
    accepted_at: DateTime<Utc>,
    last_terminal_completed_at: Option<DateTime<Utc>>,
    next_eligible_at: DateTime<Utc>,
}

impl ScheduleState {
    fn new(contract: &AnalysisContract) -> Result<Self, AnalysisError> {
        Ok(Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            contract_digest: contract.contract_digest.clone(),
            accepted_at: contract.accepted_at,
            last_terminal_completed_at: None,
            next_eligible_at: contract
                .eligible_after(contract.accepted_at)
                .map_err(|error| AnalysisError::workspace(error.to_string()))?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentCycle {
    schema_version: u32,
    cycle_id: Option<String>,
}

impl CurrentCycle {
    fn empty() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            cycle_id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CycleStatus {
    Active,
    Superseding,
    Terminal,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCycle {
    schema_version: u32,
    workspace_id: String,
    cycle_id: String,
    contract_name: String,
    contract_digest: String,
    analysis_profile: AnalysisProfile,
    opened_at: DateTime<Utc>,
    status: CycleStatus,
    replacement_digest: Option<String>,
}

impl StoredCycle {
    fn as_analysis_cycle(&self, resumed: bool) -> AnalysisCycle {
        AnalysisCycle {
            workspace_id: self.workspace_id.clone(),
            cycle_id: self.cycle_id.clone(),
            contract_name: self.contract_name.clone(),
            contract_digest: self.contract_digest.clone(),
            analysis_profile: self.analysis_profile.clone(),
            opened_at: self.opened_at,
            resumed,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredOutcome {
    schema_version: u32,
    completed_at: DateTime<Utc>,
    outcome: CycleOutcome,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttemptRecord {
    schema_version: u32,
    attempt_id: String,
    started_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttemptFailure {
    schema_version: u32,
    attempt_id: String,
    recorded_at: DateTime<Utc>,
    error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HandoffRecord {
    schema_version: u32,
    attempt_id: String,
    recorded_at: DateTime<Utc>,
    summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditRecord {
    schema_version: u32,
    recorded_at: DateTime<Utc>,
    record: Value,
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisManifest {
    schema_version: u32,
    workspace_id: String,
    cycle_id: String,
    analysis_profile: AnalysisProfile,
    files: Vec<ManifestFile>,
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    path: String,
    size: u64,
    digest: String,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use flaggo_analysis_domain::AnalysisAuthority;
    use serde_json::json;

    use super::*;

    fn contract(digest_byte: char, accepted_at: DateTime<Utc>) -> AnalysisContract {
        AnalysisContract {
            name: "checkout.delay".to_owned(),
            contract_digest: format!("sha256:{}", digest_byte.to_string().repeat(64)),
            accepted_at,
            authority: AnalysisAuthority {
                tenant: "local".to_owned(),
                application: "checkout".to_owned(),
                environment: "test".to_owned(),
            },
            evaluation_interval: "PT1H".to_owned(),
            evidence_sources: Vec::new(),
            contract: json!({"name": "checkout.delay"}),
        }
    }

    fn profile() -> AnalysisProfile {
        AnalysisProfile {
            skill_name: "evidence-analysis".to_owned(),
            skill_version: "1".to_owned(),
        }
    }

    fn ready(result: PrepareCycleResult) -> CyclePreparation {
        match result {
            PrepareCycleResult::Ready(preparation) => preparation,
            PrepareCycleResult::NotEligible { eligible_at } => {
                panic!("expected ready cycle, next eligible at {eligible_at}")
            }
        }
    }

    #[tokio::test]
    async fn supports_deep_workspace_roots() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("w".repeat(180));
        let provider = LocalWorkspaceProvider::new(root).expect("workspace provider");
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.name, "00000000-0000-0000-0000-000000000000")
            .await
            .expect("deep workspace claim");

        let preparation = lease
            .prepare_cycle(
                &contract,
                &profile(),
                "00000000-0000-0000-0000-000000000000",
                accepted_at + Duration::hours(2),
            )
            .await
            .expect("prepare deep workspace cycle");
        assert!(matches!(preparation, PrepareCycleResult::Ready(_)));
    }

    #[tokio::test]
    async fn persists_recoverable_cycle_and_seals_candidate_proposal() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let provider = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + Duration::hours(2);
        let contract = contract('a', accepted_at);

        let lease = provider
            .claim(&contract.name, "attempt-1")
            .await
            .expect("first claim");
        let preparation = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .expect("prepare first cycle"),
        );
        assert!(!preparation.cycle.resumed);

        let filesystem = lease
            .filesystem(&preparation.cycle.cycle_id, "attempt-1")
            .await
            .expect("cycle filesystem");
        filesystem
            .write_text("/workspace/analysis/findings.md", "stable finding")
            .await
            .expect("write analysis");
        assert!(
            filesystem
                .write_text("/workspace/contract.json", "tamper")
                .await
                .is_err()
        );

        let cutoff = lease
            .commit_cutoff(&preparation.cycle.cycle_id, now, 42)
            .await
            .expect("commit cutoff");
        assert_eq!(cutoff.watermark, 42);
        assert!(
            lease
                .commit_cutoff(&preparation.cycle.cycle_id, now, 43)
                .await
                .is_err()
        );

        let rules = json!([{"when": true, "then": {"delayMs": 5}}]);
        let proposal = lease
            .prepare_candidate_proposal(&preparation.cycle.cycle_id, "attempt-1", rules.clone())
            .await
            .expect("prepare proposal");
        assert_eq!(proposal.attempt_id, "attempt-1");
        assert!(
            filesystem
                .write_text("/workspace/analysis/after-seal.md", "too late")
                .await
                .is_err()
        );
        filesystem
            .write_text("/workspace/analysis/.copilot/session-state.json", "{}")
            .await
            .expect("provider state remains writable until session close");

        drop(filesystem);
        drop(lease);

        let resumed_lease = provider
            .claim(&contract.name, "attempt-2")
            .await
            .expect("recovery claim");
        let resumed = ready(
            resumed_lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-2",
                    now + Duration::minutes(5),
                )
                .await
                .expect("resume cycle"),
        );
        assert!(resumed.cycle.resumed);
        assert_eq!(resumed.cycle.cycle_id, preparation.cycle.cycle_id);

        let recovered_proposal = resumed_lease
            .prepare_candidate_proposal(&resumed.cycle.cycle_id, "attempt-2", rules)
            .await
            .expect("recover proposal");
        assert_eq!(recovered_proposal, proposal);
        assert!(
            resumed_lease
                .prepare_candidate_proposal(
                    &resumed.cycle.cycle_id,
                    "attempt-2",
                    json!([{"when": false}]),
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rebuilds_manifest_when_recovering_a_pending_proposal() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let provider = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + Duration::hours(2);
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.name, "attempt-1")
            .await
            .expect("first claim");
        let preparation = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .expect("prepare cycle"),
        );
        let filesystem = lease
            .filesystem(&preparation.cycle.cycle_id, "attempt-1")
            .await
            .expect("cycle filesystem");
        filesystem
            .write_text("/workspace/analysis/findings.md", "durable finding")
            .await
            .expect("write analysis");

        let workspace_root = directory
            .path()
            .join(workspace_id(&contract.name).trim_start_matches("sha256:"));
        let cycle_root = workspace_root
            .join("cycles")
            .join(&preparation.cycle.cycle_id);
        let stored_cycle =
            read_json::<StoredCycle>(&cycle_root.join("cycle.json")).expect("read stored cycle");
        let mut files = Vec::new();
        collect_manifest_files(&cycle_root, &cycle_root, &mut files)
            .expect("collect manifest files");
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let manifest = AnalysisManifest {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: workspace_id(&contract.name),
            cycle_id: preparation.cycle.cycle_id.clone(),
            analysis_profile: stored_cycle.analysis_profile,
            files,
        };
        let manifest_bytes = serde_json::to_vec(&manifest).expect("serialize manifest");
        let rules = json!([{"when": true, "then": {"delayMs": 5}}]);
        let pending = CandidateProposal {
            attempt_id: "attempt-1".to_owned(),
            rules: rules.clone(),
            analysis_manifest_digest: sha256_digest(&manifest_bytes),
        };
        write_json_atomic(&cycle_root.join("proposal.pending.json"), &pending)
            .expect("write pending proposal journal");
        assert!(!cycle_root.join("analysis-manifest.json").exists());

        drop(filesystem);
        drop(lease);

        let resumed_lease = provider
            .claim(&contract.name, "attempt-2")
            .await
            .expect("recovery claim");
        let resumed = ready(
            resumed_lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-2",
                    now + Duration::minutes(5),
                )
                .await
                .expect("resume cycle"),
        );
        let recovered = resumed_lease
            .prepare_candidate_proposal(&resumed.cycle.cycle_id, "attempt-2", rules)
            .await
            .expect("recover proposal");

        assert_eq!(recovered, pending);
        assert_eq!(
            digest_file(&cycle_root.join("analysis-manifest.json"))
                .expect("digest recovered manifest"),
            pending.analysis_manifest_digest
        );
        assert_eq!(
            read_json::<CandidateProposal>(&cycle_root.join("proposal.json"))
                .expect("read finalized proposal"),
            pending
        );
    }

    #[tokio::test]
    async fn enforces_process_and_operating_system_claim_exclusion() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first_provider = LocalWorkspaceProvider::new(directory.path()).expect("first provider");
        let second_provider =
            LocalWorkspaceProvider::new(directory.path()).expect("second provider");

        let first = first_provider
            .claim("checkout.delay", "attempt-1")
            .await
            .expect("first claim");
        assert!(
            first_provider
                .claim("checkout.delay", "attempt-2")
                .await
                .is_err()
        );
        assert!(
            second_provider
                .claim("checkout.delay", "attempt-3")
                .await
                .is_err()
        );

        drop(first);
        second_provider
            .claim("checkout.delay", "attempt-4")
            .await
            .expect("claim after release");
    }

    #[tokio::test]
    async fn supersedes_old_digest_and_uses_terminal_completion_for_next_schedule() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let provider = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 2, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + Duration::hours(2);
        let old_contract = contract('a', accepted_at);
        let new_contract = contract('c', accepted_at);
        let lease = provider
            .claim(&old_contract.name, "attempt-1")
            .await
            .expect("claim");

        let old_cycle = ready(
            lease
                .prepare_cycle(&old_contract, &profile(), "attempt-1", now)
                .await
                .expect("old cycle"),
        );
        let new_cycle = ready(
            lease
                .prepare_cycle(&new_contract, &profile(), "attempt-2", now)
                .await
                .expect("replacement cycle"),
        );
        assert_ne!(old_cycle.cycle.cycle_id, new_cycle.cycle.cycle_id);
        assert_eq!(
            new_cycle.cycle.contract_digest,
            new_contract.contract_digest
        );

        let completed_at = now + Duration::minutes(10);
        lease
            .complete_cycle(
                &new_contract,
                &new_cycle.cycle.cycle_id,
                &CycleOutcome::NoCandidate {
                    reason: "insufficient evidence".to_owned(),
                },
                completed_at,
            )
            .await
            .expect("complete cycle");

        match lease
            .prepare_cycle(
                &new_contract,
                &profile(),
                "attempt-3",
                completed_at + Duration::minutes(59),
            )
            .await
            .expect("check cadence")
        {
            PrepareCycleResult::NotEligible { eligible_at } => {
                assert_eq!(eligible_at, completed_at + Duration::hours(1));
            }
            PrepareCycleResult::Ready(_) => panic!("cycle became eligible too early"),
        }
    }

    #[tokio::test]
    async fn projects_prior_cycles_as_read_only_history() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let provider = LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 3, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        let now = accepted_at + Duration::hours(2);
        let completed_at = now + Duration::minutes(10);
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.name, "attempt-1")
            .await
            .expect("claim");
        let first = ready(
            lease
                .prepare_cycle(&contract, &profile(), "attempt-1", now)
                .await
                .expect("prepare first cycle"),
        );
        let first_filesystem = lease
            .filesystem(&first.cycle.cycle_id, "attempt-1")
            .await
            .expect("first filesystem");
        first_filesystem
            .write_text("/workspace/analysis/learning.md", "Prior cycle learning.")
            .await
            .expect("write prior learning");
        lease
            .complete_cycle(
                &contract,
                &first.cycle.cycle_id,
                &CycleOutcome::NoCandidate {
                    reason: "collect more evidence".to_owned(),
                },
                completed_at,
            )
            .await
            .expect("complete first cycle");

        let second = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-2",
                    completed_at + Duration::hours(1),
                )
                .await
                .expect("prepare second cycle"),
        );
        let second_filesystem = lease
            .filesystem(&second.cycle.cycle_id, "attempt-2")
            .await
            .expect("second filesystem");
        assert!(
            second_filesystem
                .read_directory("/workspace")
                .await
                .expect("workspace root")
                .iter()
                .any(|entry| entry.name == "history")
        );
        let history = second_filesystem
            .read_directory("/workspace/history")
            .await
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].name, first.cycle.cycle_id);
        let prior_path = format!(
            "/workspace/history/{}/analysis/learning.md",
            first.cycle.cycle_id
        );
        assert_eq!(
            second_filesystem
                .read_text(&prior_path)
                .await
                .expect("read prior learning"),
            "Prior cycle learning."
        );
        assert!(
            second_filesystem
                .write_text(&prior_path, "tampered")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reconciles_terminal_journal_after_each_completion_write_boundary() {
        for completed_writes in 0..=2 {
            let directory = tempfile::tempdir().expect("temporary directory");
            let provider =
                LocalWorkspaceProvider::new(directory.path()).expect("workspace provider");
            let accepted_at = Utc
                .with_ymd_and_hms(2026, 3, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp");
            let now = accepted_at + Duration::hours(2);
            let completed_at = now + Duration::minutes(10);
            let contract = contract('a', accepted_at);
            let lease = provider
                .claim(&contract.name, "attempt-1")
                .await
                .expect("claim");
            let preparation = ready(
                lease
                    .prepare_cycle(&contract, &profile(), "attempt-1", now)
                    .await
                    .expect("prepare cycle"),
            );
            lease
                .seal_analysis_manifest(&preparation.cycle.cycle_id)
                .await
                .expect("seal manifest");

            let workspace_root = directory
                .path()
                .join(workspace_id(&contract.name).trim_start_matches("sha256:"));
            let cycle_root = workspace_root
                .join("cycles")
                .join(&preparation.cycle.cycle_id);
            write_json_atomic(
                &cycle_root.join("outcome.json"),
                &StoredOutcome {
                    schema_version: WORKSPACE_SCHEMA_VERSION,
                    completed_at,
                    outcome: CycleOutcome::NoCandidate {
                        reason: "insufficient evidence".to_owned(),
                    },
                },
            )
            .expect("write terminal journal");
            if completed_writes >= 1 {
                let mut cycle =
                    read_json::<StoredCycle>(&cycle_root.join("cycle.json")).expect("read cycle");
                cycle.status = CycleStatus::Terminal;
                write_json_atomic(&cycle_root.join("cycle.json"), &cycle)
                    .expect("write terminal cycle");
            }
            if completed_writes >= 2 {
                write_json_atomic(
                    &workspace_root.join("schedule.json"),
                    &ScheduleState {
                        schema_version: WORKSPACE_SCHEMA_VERSION,
                        contract_digest: contract.contract_digest.clone(),
                        accepted_at: contract.accepted_at,
                        last_terminal_completed_at: Some(completed_at),
                        next_eligible_at: completed_at + Duration::hours(1),
                    },
                )
                .expect("write terminal schedule");
            }
            drop(lease);

            let resumed_lease = provider
                .claim(&contract.name, "attempt-2")
                .await
                .expect("recovery claim");
            match resumed_lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-2",
                    completed_at + Duration::minutes(30),
                )
                .await
                .expect("reconcile completion")
            {
                PrepareCycleResult::NotEligible { eligible_at } => {
                    assert_eq!(eligible_at, completed_at + Duration::hours(1));
                }
                PrepareCycleResult::Ready(_) => {
                    panic!("terminal cycle became eligible during recovery")
                }
            }

            assert_eq!(
                read_json::<StoredCycle>(&cycle_root.join("cycle.json"))
                    .expect("read reconciled cycle")
                    .status,
                CycleStatus::Terminal
            );
            assert!(
                read_json::<CurrentCycle>(&workspace_root.join("current-cycle.json"))
                    .expect("read current pointer")
                    .cycle_id
                    .is_none()
            );
            let schedule = read_json::<ScheduleState>(&workspace_root.join("schedule.json"))
                .expect("read reconciled schedule");
            assert_eq!(schedule.last_terminal_completed_at, Some(completed_at));
            assert_eq!(schedule.next_eligible_at, completed_at + Duration::hours(1));
        }
    }
}
