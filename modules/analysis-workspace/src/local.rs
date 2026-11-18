use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flaggo_analysis_domain::{
    AnalysisContract, AnalysisCycle, AnalysisError, AnalysisProfile, CandidateProposal,
    CandidateRecord, CycleLimits, CycleOutcome, WorkflowHandoff,
};
use flaggo_contract::{CandidateOwnership, ContractResourceId};
use flaggo_executable_store::{
    AdmissionFence, AdmissionGuard, AnalysisCandidateAdmission, AuthorityResult,
    ExecutableStoreError,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::{
    CyclePreparation, ExecutionReceipt, ExecutionRequest, ExecutionReservation, ExecutionState,
    HandoffPublication, PrepareCycleResult, WorkspaceArtifact, WorkspaceEntry, WorkspaceEntryKind,
    WorkspaceFileInfo, WorkspaceFileSystem, WorkspaceLease, WorkspaceProvider, sha256_digest,
    workspace_id,
};

const WORKSPACE_SCHEMA_VERSION: u32 = 1;
const VIRTUAL_ROOT: &str = "workspace";
type LocalClaims = Arc<Mutex<HashMap<String, Weak<LocalWorkspaceLease>>>>;

#[derive(Clone)]
pub struct LocalWorkspaceProvider {
    root: PathBuf,
    process_claims: LocalClaims,
}

impl LocalWorkspaceProvider {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, AnalysisError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(workspace_error)?;
        let root = fs::canonicalize(root).map_err(workspace_error)?;
        Ok(Self {
            root,
            process_claims: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Explicitly bind a legacy name-only directory without rewriting sealed history.
    pub fn bind_legacy_resource(
        &self,
        resource: &ContractResourceId,
    ) -> Result<bool, AnalysisError> {
        resource.resource_key().map_err(workspace_error)?;
        let legacy_id = sha256_digest(resource.name.as_bytes());
        let root = self.root.join(legacy_id.trim_start_matches("sha256:"));
        let manifest: WorkspaceManifest = read_json(&root.join("workspace.json"))?;
        if manifest.workspace_id != legacy_id || manifest.contract_name != resource.name {
            return Err(AnalysisError::workspace(
                "legacy workspace identity does not match",
            ));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("claim.lock"))
            .map_err(workspace_error)?;
        lock.try_lock().map_err(|error| {
            AnalysisError::workspace(format!("legacy workspace must be idle: {error}"))
        })?;
        for entry in fs::read_dir(root.join("cycles")).map_err(workspace_error)? {
            let entry = entry.map_err(workspace_error)?;
            if entry.file_type().map_err(workspace_error)?.is_symlink() {
                return Err(AnalysisError::workspace(
                    "legacy cycle cannot be a symbolic link",
                ));
            }
            if entry.file_type().map_err(workspace_error)?.is_dir() {
                let contract: AnalysisContract = read_json(&entry.path().join("contract.json"))?;
                if contract.resource()? != *resource {
                    return Err(AnalysisError::workspace(
                        "legacy history contains a different resource scope",
                    ));
                }
            }
        }
        let binding = ResourceBinding {
            resource: resource.clone(),
            workspace_id: legacy_id,
        };
        let path = root.join("resource.json");
        if path.exists() && read_json::<ResourceBinding>(&path)? != binding {
            return Err(AnalysisError::workspace(
                "legacy workspace is already bound to another resource",
            ));
        }
        let alias = self.binding_path(resource);
        if alias.exists() {
            if read_json::<ResourceBinding>(&alias)? != binding {
                return Err(AnalysisError::workspace(
                    "resource binding conflicts with existing history",
                ));
            }
            return Ok(false);
        }
        let scoped_root = self
            .root
            .join(workspace_id(resource).trim_start_matches("sha256:"));
        if scoped_root.exists() {
            return Err(AnalysisError::workspace(
                "scoped workspace already exists; histories cannot be silently merged",
            ));
        }
        write_json_atomic(&path, &binding)?;
        fs::create_dir_all(self.root.join("resource-bindings")).map_err(workspace_error)?;
        write_json_atomic(&alias, &binding)?;
        Ok(true)
    }

    fn binding_path(&self, resource: &ContractResourceId) -> PathBuf {
        self.root.join("resource-bindings").join(format!(
            "{}.json",
            workspace_id(resource).trim_start_matches("sha256:")
        ))
    }

    fn resolve_resource(&self, resource: &ContractResourceId) -> Result<String, AnalysisError> {
        let legacy_id = sha256_digest(resource.name.as_bytes());
        let alias = self.binding_path(resource);
        if alias.exists() {
            let binding: ResourceBinding = read_json(&alias)?;
            if binding.resource != *resource || binding.workspace_id != legacy_id {
                return Err(AnalysisError::workspace("invalid scoped workspace binding"));
            }
            return Ok(legacy_id);
        }
        let legacy_root = self.root.join(legacy_id.trim_start_matches("sha256:"));
        if legacy_root.join("workspace.json").exists() {
            let binding_path = legacy_root.join("resource.json");
            if !binding_path.exists()
                || read_json::<ResourceBinding>(&binding_path)?.resource == *resource
            {
                return Err(AnalysisError::workspace(
                    "legacy workspace requires explicit resource binding before use",
                ));
            }
        }
        Ok(workspace_id(resource))
    }

    /// Resolve a live local claim for trusted-local execution, never a remote lease.
    pub fn analysis_directory(
        &self,
        workspace: &Arc<dyn WorkspaceLease>,
        cycle_id: &str,
    ) -> Result<PathBuf, AnalysisError> {
        let lease = mutex_lock(&self.process_claims)?
            .get(workspace.workspace_id())
            .and_then(Weak::upgrade)
            .ok_or_else(|| {
                AnalysisError::workspace("execution requires a live local workspace claim")
            })?;
        let claimed: Arc<dyn WorkspaceLease> = lease.clone();
        if !Arc::ptr_eq(&claimed, workspace) {
            return Err(AnalysisError::workspace(
                "execution workspace does not belong to this local provider",
            ));
        }
        lease.require_active_cycle(cycle_id)?;
        lease.require_writable(cycle_id)?;
        Ok(lease.cycle_path(cycle_id).join("analysis"))
    }
}

#[async_trait]
impl WorkspaceProvider for LocalWorkspaceProvider {
    async fn claim(
        &self,
        resource: &ContractResourceId,
        attempt_id: &str,
    ) -> Result<Arc<dyn WorkspaceLease>, AnalysisError> {
        resource.resource_key().map_err(workspace_error)?;
        validate_segment(attempt_id, "attempt ID")?;

        let id = self.resolve_resource(resource)?;
        {
            let mut claims = mutex_lock(&self.process_claims)?;
            if claims.contains_key(&id) {
                return Err(AnalysisError::WorkspaceBusy(id.clone()));
            }
            claims.insert(id.clone(), Weak::new());
        }

        match self.open_claim(resource, attempt_id, &id) {
            Ok(lease) => {
                let lease = Arc::new(lease);
                mutex_lock(&self.process_claims)?.insert(id, Arc::downgrade(&lease));
                Ok(lease)
            }
            Err(error) => {
                mutex_lock(&self.process_claims)?.remove(&id);
                Err(error)
            }
        }
    }
}

impl LocalWorkspaceProvider {
    fn open_claim(
        &self,
        resource: &ContractResourceId,
        attempt_id: &str,
        id: &str,
    ) -> Result<LocalWorkspaceLease, AnalysisError> {
        let contract_name = &resource.name;
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
        let _authority_guard = authority_gate(&root)?;

        let binding = ResourceBinding {
            resource: resource.clone(),
            workspace_id: id.into(),
        };
        let binding_path = root.join("resource.json");
        if binding_path.exists() {
            if read_json::<ResourceBinding>(&binding_path)? != binding {
                return Err(AnalysisError::workspace(
                    "workspace resource binding does not match",
                ));
            }
        } else {
            write_json_atomic(&binding_path, &binding)?;
        }

        let manifest_path = root.join("workspace.json");
        if manifest_path.exists() {
            let manifest: WorkspaceManifest = read_json(&manifest_path)?;
            if manifest.schema_version != WORKSPACE_SCHEMA_VERSION
                || manifest.workspace_id != id
                || manifest.contract_name != *contract_name
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

        let epoch = if root.join("claim.json").exists() {
            let previous: ClaimRecord = read_json(&root.join("claim.json"))?;
            if previous.schema_version != WORKSPACE_SCHEMA_VERSION {
                return Err(AnalysisError::workspace(
                    "unsupported claim record revision",
                ));
            }
            previous
                .epoch
                .checked_add(1)
                .ok_or_else(|| AnalysisError::workspace("workspace fencing epoch exhausted"))?
        } else {
            1
        };
        write_json_atomic(
            &root.join("claim.json"),
            &ClaimRecord {
                schema_version: WORKSPACE_SCHEMA_VERSION,
                epoch,
                attempt_id: attempt_id.to_owned(),
                claimed_at: Utc::now(),
                active: true,
                cycle_id: None,
            },
        )?;

        Ok(LocalWorkspaceLease {
            resource: resource.clone(),
            epoch,
            attempt_id: attempt_id.into(),
            root,
            workspace_id: id.to_owned(),
            contract_name: contract_name.to_owned(),
            process_claims: Arc::clone(&self.process_claims),
            lock_file: Mutex::new(Some(lock_file)),
            state: Arc::new(Mutex::new(LeaseState {
                valid: true,
                ..Default::default()
            })),
            io_gate: Arc::new(Mutex::new(())),
        })
    }
}

pub struct LocalWorkspaceLease {
    resource: ContractResourceId,
    epoch: u64,
    attempt_id: String,
    root: PathBuf,
    workspace_id: String,
    contract_name: String,
    process_claims: LocalClaims,
    lock_file: Mutex<Option<File>>,
    state: Arc<Mutex<LeaseState>>,
    io_gate: Arc<Mutex<()>>,
}

impl Drop for LocalWorkspaceLease {
    fn drop(&mut self) {
        if let Err(error) = self.release_claim() {
            tracing::error!(%error, "Local workspace claim release failed");
            // A dropped lease must revoke in-process capabilities even when an
            // in-flight admission prevents acknowledging durable release.
            if let Err(error) = self.invalidate_local_claim() {
                tracing::error!(%error, "Local workspace capability revocation failed");
            }
        }
    }
}

#[derive(Default)]
struct LeaseState {
    valid: bool,
    active_cycle_id: Option<String>,
    writable: bool,
    sealed: bool,
}

#[async_trait]
impl WorkspaceLease for LocalWorkspaceLease {
    fn resource(&self) -> &ContractResourceId {
        &self.resource
    }

    fn claim_token(&self) -> crate::WorkspaceClaim {
        crate::WorkspaceClaim {
            resource: self.resource.clone(),
            workspace_id: self.workspace_id.clone(),
            attempt_id: self.attempt_id.clone(),
            epoch: self.epoch,
        }
    }

    async fn release(&self) -> Result<(), AnalysisError> {
        self.release_claim()
    }

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
        if contract.resource()? != self.resource {
            return Err(AnalysisError::workspace(
                "contract name does not match the claimed workspace",
            ));
        }

        let _io_guard = mutex_lock(&self.io_gate)?;
        let _authority_guard = authority_gate(&self.root)?;
        validate_claim(&self.root, &self.state, self.epoch)?;
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
                    return Ok(PrepareCycleResult::Ready(Box::new(CyclePreparation {
                        cycle: cycle.as_analysis_cycle(&self.resource.scope, true),
                        eligible_at: schedule.next_eligible_at,
                    })));
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
            latest_handoff: None,
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

        Ok(PrepareCycleResult::Ready(Box::new(CyclePreparation {
            cycle: cycle.as_analysis_cycle(&self.resource.scope, false),
            eligible_at: schedule.next_eligible_at,
        })))
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
            epoch: self.epoch,
            root: self.cycle_path(cycle_id),
            workspace_root: self.root.clone(),
            cycle_id: cycle_id.to_owned(),
            state: Arc::clone(&self.state),
            io_gate: Arc::clone(&self.io_gate),
        }))
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
                return Err(AnalysisError::ProposalConflict(
                    "a different Candidate proposal is already committed for this cycle".to_owned(),
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
        let _io_guard = mutex_lock(&self.io_gate)?;
        validate_claim(&self.root, &self.state, self.epoch)?;
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
        let cycle = self.read_cycle(cycle_id)?;
        if candidate.contract_name != self.resource.name
            || candidate.contract_digest != cycle.contract_digest
            || proposal.analysis_manifest_digest != candidate.analysis_manifest_digest
        {
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
        let _authority_guard = authority_gate(&self.root)?;
        self.require_active_cycle(cycle_id)?;
        let mut cycle = self.read_cycle(cycle_id)?;
        cycle.status = CycleStatus::Superseding;
        cycle.replacement_digest = replacement_digest.map(ToOwned::to_owned);
        write_json_atomic(&self.cycle_path(cycle_id).join("cycle.json"), &cycle)?;
        Ok(())
    }

    async fn put_artifact(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        bytes: &[u8],
    ) -> Result<WorkspaceArtifact, AnalysisError> {
        validate_segment(attempt_id, "attempt ID")?;
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        let artifact = WorkspaceArtifact {
            resource: self.resource.clone(),
            workspace_id: self.workspace_id.clone(),
            cycle_id: cycle_id.into(),
            attempt_id: attempt_id.into(),
            digest: sha256_digest(bytes),
            byte_length: u64::try_from(bytes.len()).map_err(workspace_error)?,
        };
        let path = self.artifact_path(&artifact)?;
        ensure_parent_has_no_symlinks(&self.root, &path)?;
        if path.exists() {
            self.read_artifact_bytes(&artifact)?;
        } else {
            write_bytes_atomic(&path, bytes)?;
        }
        Ok(artifact)
    }

    async fn read_artifact(&self, artifact: &WorkspaceArtifact) -> Result<Vec<u8>, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.read_artifact_bytes(artifact)
    }

    async fn reserve_execution(
        &self,
        request: &ExecutionRequest,
    ) -> Result<ExecutionReservation, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let cycle_id = &request.source.cycle_id;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        self.read_artifact_bytes(&request.source)?;
        validate_segment(&request.execution_id, "execution ID")?;
        if request.profile.is_empty() || request.timeout_ms == 0 || request.output_limit_bytes == 0
        {
            return Err(AnalysisError::InvalidState(
                "execution requires a profile and positive bounds".into(),
            ));
        }
        let path = self.execution_path(cycle_id, &request.execution_id)?;
        if path.exists() {
            let existing: ExecutionReceipt = read_json(&path)?;
            if existing.request != *request {
                return Err(AnalysisError::InvalidState(
                    "execution idempotency key conflicts with its recorded request".into(),
                ));
            }
            return Ok(ExecutionReservation::Existing(existing));
        }
        let receipt = ExecutionReceipt {
            request: request.clone(),
            epoch: self.epoch,
            state: ExecutionState::Pending,
        };
        write_json_atomic(&path, &receipt)?;
        Ok(ExecutionReservation::Granted(receipt))
    }

    async fn execution_receipt(
        &self,
        cycle_id: &str,
        execution_id: &str,
    ) -> Result<Option<ExecutionReceipt>, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let path = self.execution_path(cycle_id, execution_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let receipt: ExecutionReceipt = read_json(&path)?;
        if receipt.request.execution_id != execution_id
            || receipt.request.source.cycle_id != cycle_id
        {
            return Err(AnalysisError::InvalidState(
                "execution receipt identity mismatch".into(),
            ));
        }
        self.read_artifact_bytes(&receipt.request.source)?;
        if let ExecutionState::Completed { result } = &receipt.state {
            self.read_artifact_bytes(result)?;
        }
        Ok(Some(receipt))
    }

    async fn complete_execution(
        &self,
        cycle_id: &str,
        execution_id: &str,
        result: &WorkspaceArtifact,
    ) -> Result<ExecutionReceipt, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        self.require_writable(cycle_id)?;
        self.read_artifact_bytes(result)?;
        let path = self.execution_path(cycle_id, execution_id)?;
        let mut receipt: ExecutionReceipt = read_json(&path)?;
        if receipt.epoch != self.epoch
            || receipt.request.execution_id != execution_id
            || receipt.request.source.cycle_id != cycle_id
            || result.cycle_id != cycle_id
            || result.attempt_id != receipt.request.source.attempt_id
        {
            return Err(AnalysisError::InvalidState(
                "execution result has a stale fence or mismatched identity".into(),
            ));
        }
        let completed = ExecutionState::Completed {
            result: result.clone(),
        };
        if receipt.state != ExecutionState::Pending && receipt.state != completed {
            return Err(AnalysisError::InvalidState(
                "execution result conflicts with its committed receipt".into(),
            ));
        }
        receipt.state = completed;
        write_json_atomic(&path, &receipt)?;
        Ok(receipt)
    }

    async fn commit_handoff(
        &self,
        artifact: &WorkspaceArtifact,
        expected_previous: Option<&str>,
    ) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        let publication: HandoffPublication =
            serde_json::from_slice(&self.read_artifact_bytes(artifact)?)
                .map_err(workspace_error)?;
        let HandoffPublication { summary, metadata } = publication;
        let cycle_id = &artifact.cycle_id;
        let attempt_id = &artifact.attempt_id;
        self.require_writable(cycle_id)?;
        if summary.trim().is_empty()
            || metadata.as_ref().is_some_and(|value| !value.is_object())
            || serde_json::to_vec(&metadata)
                .map_err(workspace_error)?
                .len()
                > 16_384
        {
            return Err(AnalysisError::InvalidState(
                "handoff requires a non-empty summary and at most 16 KiB of object metadata"
                    .to_owned(),
            ));
        }
        let mut cycle = self.read_cycle(cycle_id)?;
        let path = self
            .cycle_path(cycle_id)
            .join("handoffs")
            .join(format!("{attempt_id}.json"));
        if path.exists() {
            let existing: HandoffRecord = read_json(&path)?;
            if existing.summary != summary
                || existing.metadata != metadata
                || existing.cycle_id.as_deref() != Some(cycle_id)
                || existing.contract_digest.as_deref() != Some(cycle.contract_digest.as_str())
                || existing.artifact.as_ref() != Some(artifact)
                || existing.previous_handoff.as_deref() != expected_previous
                || (cycle.latest_handoff.as_deref() != Some(attempt_id)
                    && cycle.latest_handoff != existing.previous_handoff)
            {
                return Err(AnalysisError::InvalidState(
                    "committed handoff cannot be changed or republished out of order".to_owned(),
                ));
            }
        } else {
            if cycle.latest_handoff.as_deref() != expected_previous {
                return Err(AnalysisError::InvalidState(
                    "workflow publication revision conflict".into(),
                ));
            }
            write_json_atomic(
                &path,
                &HandoffRecord {
                    schema_version: WORKSPACE_SCHEMA_VERSION,
                    attempt_id: attempt_id.to_owned(),
                    recorded_at: Utc::now(),
                    summary: summary.to_owned(),
                    metadata: metadata.clone(),
                    cycle_id: Some(cycle_id.to_owned()),
                    contract_digest: Some(cycle.contract_digest.clone()),
                    previous_handoff: cycle.latest_handoff.clone(),
                    artifact: Some(artifact.clone()),
                },
            )?;
        }
        if metadata.is_some() {
            cycle.latest_handoff = Some(attempt_id.to_owned());
            write_json_atomic(&self.cycle_path(cycle_id).join("cycle.json"), &cycle)?;
        }
        Ok(())
    }

    async fn latest_handoff(
        &self,
        cycle_id: &str,
    ) -> Result<Option<WorkflowHandoff>, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let cycle = self.read_cycle(cycle_id)?;
        let Some(attempt_id) = cycle.latest_handoff else {
            return Ok(None);
        };
        validate_segment(&attempt_id, "handoff attempt ID")?;
        let path = self
            .cycle_path(cycle_id)
            .join("handoffs")
            .join(format!("{attempt_id}.json"));
        let contents = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AnalysisError::InvalidState("latest workflow handoff record is missing".to_owned())
            } else {
                workspace_error(error)
            }
        })?;
        let record: HandoffRecord = serde_json::from_slice(&contents).map_err(|error| {
            AnalysisError::InvalidState(format!(
                "latest workflow handoff record is corrupt: {error}"
            ))
        })?;
        if record.schema_version != WORKSPACE_SCHEMA_VERSION
            || record.attempt_id != attempt_id
            || record.cycle_id.as_deref() != Some(cycle_id)
            || record.contract_digest.as_deref() != Some(cycle.contract_digest.as_str())
        {
            return Err(AnalysisError::InvalidState(
                "latest handoff does not match its bound cycle and attempt".to_owned(),
            ));
        }
        if let Some(artifact) = &record.artifact {
            let publication: HandoffPublication =
                serde_json::from_slice(&self.read_artifact_bytes(artifact)?)
                    .map_err(workspace_error)?;
            if artifact.attempt_id != attempt_id
                || publication.summary != record.summary
                || publication.metadata != record.metadata
            {
                return Err(AnalysisError::InvalidState(
                    "committed handoff differs from its immutable artifact".into(),
                ));
            }
        }
        let metadata = record.metadata.filter(Value::is_object).ok_or_else(|| {
            AnalysisError::InvalidState("latest workflow handoff has no object metadata".to_owned())
        })?;
        Ok(Some(WorkflowHandoff {
            attempt_id,
            summary: record.summary,
            metadata,
        }))
    }

    async fn pending_proposal(
        &self,
        cycle_id: &str,
    ) -> Result<Option<CandidateProposal>, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        let path = self.cycle_path(cycle_id).join("proposal.pending.json");
        if path.exists() {
            read_json(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    async fn reserve_attempt(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        limits: &CycleLimits,
        timeout: Duration,
    ) -> Result<Duration, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        self.require_active_cycle(cycle_id)?;
        validate_segment(attempt_id, "attempt ID")?;
        if limits.max_attempts == 0 || limits.execution_seconds == 0 || timeout.is_zero() {
            return Err(AnalysisError::InvalidState(
                "cycle execution limits must be positive".to_owned(),
            ));
        }
        let path = self.cycle_path(cycle_id).join("budget.json");
        let mut budget = if path.exists() {
            read_json::<CycleBudget>(&path)?
        } else {
            CycleBudget {
                limits: limits.clone(),
                attempts: 0,
                execution_millis: 0,
                reservation: None,
            }
        };
        if budget.limits != *limits {
            return Err(AnalysisError::InvalidState(
                "cycle limits differ from their durable configuration".to_owned(),
            ));
        }
        let total = limits.execution_seconds.checked_mul(1_000).ok_or_else(|| {
            AnalysisError::InvalidState("cycle time budget overflows milliseconds".to_owned())
        })?;
        if budget.attempts >= limits.max_attempts || budget.execution_millis >= total {
            return Err(AnalysisError::LimitExceeded(
                "attempt count or accumulated execution time exhausted".to_owned(),
            ));
        }
        let requested = u64::try_from(timeout.as_millis()).map_err(workspace_error)?;
        let allowance = requested.min(total - budget.execution_millis);
        if allowance == 0 {
            return Err(AnalysisError::InvalidState(
                "attempt timeout is below one millisecond".to_owned(),
            ));
        }
        // An unsettled prior reservation remains charged after a crash.
        budget.attempts += 1;
        budget.execution_millis += allowance;
        budget.reservation = Some(AttemptReservation {
            attempt_id: attempt_id.to_owned(),
            millis: allowance,
        });
        write_json_atomic(&path, &budget)?;
        Ok(Duration::from_millis(allowance))
    }

    async fn settle_attempt(
        &self,
        cycle_id: &str,
        attempt_id: &str,
        elapsed: Duration,
    ) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        validate_claim(&self.root, &self.state, self.epoch)?;
        validate_segment(cycle_id, "cycle ID")?;
        let path = self.cycle_path(cycle_id).join("budget.json");
        let mut budget: CycleBudget = read_json(&path)?;
        let reservation = budget.reservation.take().ok_or_else(|| {
            AnalysisError::InvalidState("attempt has no execution reservation".to_owned())
        })?;
        if reservation.attempt_id != attempt_id {
            return Err(AnalysisError::InvalidState(
                "execution reservation belongs to another attempt".to_owned(),
            ));
        }
        let elapsed = u64::try_from(elapsed.as_millis())
            .map_err(workspace_error)?
            .min(reservation.millis);
        budget.execution_millis = budget
            .execution_millis
            .checked_sub(reservation.millis - elapsed)
            .ok_or_else(|| {
                AnalysisError::InvalidState(
                    "execution reservation exceeds the charged budget".to_owned(),
                )
            })?;
        write_json_atomic(&path, &budget)
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
        let _authority_guard = authority_gate(&self.root)?;
        self.require_active_cycle(cycle_id)?;
        let outcome_path = self.cycle_path(cycle_id).join("outcome.json");
        let stored_contract: AnalysisContract =
            read_json(&self.cycle_path(cycle_id).join("contract.json"))?;
        if stored_contract.resource()? != contract.resource()?
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
    fn execution_path(&self, cycle_id: &str, execution_id: &str) -> Result<PathBuf, AnalysisError> {
        validate_segment(cycle_id, "cycle ID")?;
        validate_segment(execution_id, "execution ID")?;
        Ok(self
            .cycle_path(cycle_id)
            .join("execution-receipts")
            .join(format!("{execution_id}.json")))
    }

    fn artifact_path(&self, artifact: &WorkspaceArtifact) -> Result<PathBuf, AnalysisError> {
        self.require_active_cycle(&artifact.cycle_id)?;
        validate_segment(&artifact.attempt_id, "artifact attempt ID")?;
        let digest = artifact
            .digest
            .strip_prefix("sha256:")
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| AnalysisError::workspace("invalid artifact digest"))?;
        if artifact.resource != self.resource || artifact.workspace_id != self.workspace_id {
            return Err(AnalysisError::workspace(
                "artifact belongs to a different resource or workspace",
            ));
        }
        Ok(self
            .cycle_path(&artifact.cycle_id)
            .join("artifacts")
            .join(&artifact.attempt_id)
            .join(digest))
    }

    fn read_artifact_bytes(&self, artifact: &WorkspaceArtifact) -> Result<Vec<u8>, AnalysisError> {
        let path = self.artifact_path(artifact)?;
        ensure_parent_has_no_symlinks(&self.root, &path)?;
        ensure_regular_file(&path)?;
        let bytes = fs::read(path).map_err(workspace_error)?;
        if u64::try_from(bytes.len()).map_err(workspace_error)? != artifact.byte_length
            || sha256_digest(&bytes) != artifact.digest
        {
            return Err(AnalysisError::workspace(
                "immutable artifact integrity mismatch",
            ));
        }
        Ok(bytes)
    }

    fn release_claim(&self) -> Result<(), AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
        if !mutex_lock(&self.state)?.valid {
            return Ok(());
        }
        let _authority_guard = authority_gate(&self.root)?;
        let mut claim: ClaimRecord = read_json(&self.root.join("claim.json"))?;
        if claim.epoch != self.epoch {
            return Err(AnalysisError::workspace("workspace fencing epoch is stale"));
        }
        claim.active = false;
        claim.cycle_id = None;
        write_json_atomic(&self.root.join("claim.json"), &claim)?;
        self.invalidate_local_claim()
    }

    fn invalidate_local_claim(&self) -> Result<(), AnalysisError> {
        {
            let mut state = mutex_lock(&self.state)?;
            state.valid = false;
            state.writable = false;
            state.active_cycle_id = None;
        }
        mutex_lock(&self.lock_file)?.take();
        let mut claims = mutex_lock(&self.process_claims)?;
        if claims
            .get(&self.workspace_id)
            .is_some_and(|claim| std::ptr::eq(claim.as_ptr(), self))
        {
            claims.remove(&self.workspace_id);
        }
        Ok(())
    }

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
        let mut claim: ClaimRecord = read_json(&self.root.join("claim.json"))?;
        claim.cycle_id = Some(cycle_id.to_owned());
        write_json_atomic(&self.root.join("claim.json"), &claim)?;
        let mut state = mutex_lock(&self.state)?;
        state.active_cycle_id = Some(cycle_id.to_owned());
        state.writable = writable;
        state.sealed = sealed;
        Ok(())
    }

    fn clear_active_state(&self) -> Result<(), AnalysisError> {
        let mut claim: ClaimRecord = read_json(&self.root.join("claim.json"))?;
        claim.cycle_id = None;
        write_json_atomic(&self.root.join("claim.json"), &claim)?;
        let mut state = mutex_lock(&self.state)?;
        state.active_cycle_id = None;
        state.writable = false;
        state.sealed = false;
        Ok(())
    }

    fn require_active_cycle(&self, cycle_id: &str) -> Result<(), AnalysisError> {
        validate_claim(&self.root, &self.state, self.epoch)?;
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
    epoch: u64,
    root: PathBuf,
    workspace_root: PathBuf,
    cycle_id: String,
    state: Arc<Mutex<LeaseState>>,
    io_gate: Arc<Mutex<()>>,
}

#[async_trait]
impl WorkspaceFileSystem for LocalWorkspaceFileSystem {
    async fn read_text(&self, path: &str) -> Result<String, AnalysisError> {
        let _io_guard = mutex_lock(&self.io_gate)?;
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
        let _io_guard = mutex_lock(&self.io_gate)?;
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
        let _io_guard = mutex_lock(&self.io_gate)?;
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
        let _io_guard = mutex_lock(&self.io_gate)?;
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
        validate_claim(&self.workspace_root, &self.state, self.epoch)?;
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
            | "budget.json"
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceBinding {
    resource: ContractResourceId,
    workspace_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRecord {
    schema_version: u32,
    #[serde(default)]
    epoch: u64,
    attempt_id: String,
    claimed_at: DateTime<Utc>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    cycle_id: Option<String>,
}

struct LocalAdmissionGuard {
    _file: File,
}

impl AdmissionGuard for LocalAdmissionGuard {}

fn authority_gate(root: &Path) -> Result<File, AnalysisError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("authority.lock"))
        .map_err(workspace_error)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => {
            AnalysisError::WorkspaceBusy("workspace authority mutation in progress".into())
        }
        std::fs::TryLockError::Error(error) => workspace_error(error),
    })?;
    Ok(file)
}

impl AdmissionFence for LocalWorkspaceProvider {
    fn acquire(
        &self,
        admission: &AnalysisCandidateAdmission,
        ownership: &CandidateOwnership,
    ) -> AuthorityResult<Box<dyn AdmissionGuard>> {
        let verified = (|| -> Result<Option<File>, AnalysisError> {
            admission.resource.resource_key().map_err(workspace_error)?;
            validate_segment(&admission.cycle_id, "cycle ID")?;
            validate_segment(&ownership.attempt_id, "attempt ID")?;
            let id = self.resolve_resource(&admission.resource)?;
            if id != admission.workspace_id {
                return Ok(None);
            }
            let root = self.root.join(id.trim_start_matches("sha256:"));
            if !root.join("claim.json").exists() {
                return Ok(None);
            }
            let gate = authority_gate(&root)?;
            let claim: ClaimRecord = read_json(&root.join("claim.json"))?;
            if claim.schema_version != WORKSPACE_SCHEMA_VERSION
                || !claim.active
                || claim.epoch == 0
                || claim.epoch.to_string() != ownership.epoch
                || claim.attempt_id != ownership.attempt_id
                || claim.cycle_id.as_deref() != Some(&admission.cycle_id)
            {
                return Ok(None);
            }
            // A crashed process cannot authorize delayed requests with its
            // durable record. The lifetime lock must still have a live owner.
            let live = OpenOptions::new()
                .read(true)
                .write(true)
                .open(root.join("claim.lock"))
                .map_err(workspace_error)?;
            match live.try_lock() {
                Ok(()) => return Ok(None),
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(error)) => return Err(workspace_error(error)),
            }
            let binding: ResourceBinding = read_json(&root.join("resource.json"))?;
            let pointer: CurrentCycle = read_json(&root.join("current-cycle.json"))?;
            let cycle_root = root.join("cycles").join(&admission.cycle_id);
            let cycle: StoredCycle = read_json(&cycle_root.join("cycle.json"))?;
            let contract: AnalysisContract = read_json(&cycle_root.join("contract.json"))?;
            if binding.resource != admission.resource
                || binding.workspace_id != admission.workspace_id
                || pointer.cycle_id.as_deref() != Some(&admission.cycle_id)
                || cycle.status != CycleStatus::Active
                || cycle.workspace_id != admission.workspace_id
                || cycle.cycle_id != admission.cycle_id
                || cycle.contract_digest != admission.contract_digest
                || contract.resource()? != admission.resource
                || contract.contract_digest != admission.contract_digest
                || cycle_root.join("outcome.json").exists()
            {
                return Ok(None);
            }
            let proposal: CandidateProposal = read_json(&cycle_root.join("proposal.pending.json"))?;
            if proposal.attempt_id != admission.attempt_id
                || proposal.analysis_manifest_digest != admission.analysis_manifest_digest
            {
                return Ok(None);
            }
            Ok(Some(gate))
        })()
        .map_err(|error| ExecutableStoreError::Dependency(Box::new(error)))?;
        let file = verified.ok_or(ExecutableStoreError::OwnershipConflict)?;
        Ok(Box::new(LocalAdmissionGuard { _file: file }))
    }
}

fn validate_claim(root: &Path, state: &Mutex<LeaseState>, epoch: u64) -> Result<(), AnalysisError> {
    if !mutex_lock(state)?.valid {
        return Err(AnalysisError::workspace(
            "workspace claim has been released",
        ));
    }
    let record: ClaimRecord = read_json(&root.join("claim.json"))?;
    if record.schema_version != WORKSPACE_SCHEMA_VERSION || record.epoch != epoch {
        return Err(AnalysisError::workspace("workspace fencing epoch is stale"));
    }
    Ok(())
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
    #[serde(default)]
    latest_handoff: Option<String>,
}

impl StoredCycle {
    fn as_analysis_cycle(
        &self,
        scope: &flaggo_analysis_domain::AnalysisAuthority,
        resumed: bool,
    ) -> AnalysisCycle {
        AnalysisCycle {
            scope: scope.clone(),
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HandoffRecord {
    schema_version: u32,
    attempt_id: String,
    recorded_at: DateTime<Utc>,
    summary: String,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    cycle_id: Option<String>,
    #[serde(default)]
    contract_digest: Option<String>,
    #[serde(default)]
    previous_handoff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact: Option<WorkspaceArtifact>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CycleBudget {
    limits: CycleLimits,
    attempts: u32,
    execution_millis: u64,
    reservation: Option<AttemptReservation>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttemptReservation {
    attempt_id: String,
    millis: u64,
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
            skill_names: vec![
                "analysis-cycle-protocol".to_owned(),
                "understand-decision-contract".to_owned(),
                "qualitative-analysis".to_owned(),
                "author-executable".to_owned(),
            ],
        }
    }

    fn ready(result: PrepareCycleResult) -> CyclePreparation {
        match result {
            PrepareCycleResult::Ready(preparation) => *preparation,
            PrepareCycleResult::NotEligible { eligible_at } => {
                panic!("expected ready cycle, next eligible at {eligible_at}")
            }
        }
    }

    #[tokio::test]
    async fn scoped_workspaces_are_independent_and_released_capabilities_are_fenced() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let now = Utc::now();
        let contract = contract('a', now - Duration::hours(2));
        let resource = contract.resource().unwrap();
        let lease = provider.claim(&resource, "first").await.unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "first", now)
                .await
                .unwrap(),
        )
        .cycle;
        let filesystem = lease.filesystem(&cycle.cycle_id, "first").await.unwrap();
        filesystem
            .write_text("/workspace/analysis/value.txt", "original")
            .await
            .unwrap();
        let first = lease.claim_token();

        let mut other_resource = resource.clone();
        other_resource.scope.tenant = "another".into();
        let independent = provider
            .claim(&other_resource, "independent")
            .await
            .unwrap();
        assert_ne!(lease.workspace_id(), independent.workspace_id());
        assert!(
            independent
                .prepare_cycle(&contract, &profile(), "independent", now)
                .await
                .is_err()
        );
        drop(independent);

        lease.release().await.unwrap();
        let replacement = provider.claim(&resource, "replacement").await.unwrap();
        assert!(replacement.claim_token().epoch > first.epoch);
        let resumed = ready(
            replacement
                .prepare_cycle(&contract, &profile(), "replacement", now)
                .await
                .unwrap(),
        )
        .cycle;
        assert_eq!(resumed.cycle_id, cycle.cycle_id);
        assert!(
            filesystem
                .read_text("/workspace/analysis/value.txt")
                .await
                .is_err()
        );
        assert!(
            filesystem
                .write_text("/workspace/analysis/value.txt", "stale")
                .await
                .is_err()
        );
        assert!(
            lease
                .prepare_cycle(&contract, &profile(), "first", now)
                .await
                .is_err()
        );
        assert!(
            lease
                .record_handoff(&cycle.cycle_id, "first", "stale", None, None)
                .await
                .is_err()
        );
        assert!(
            lease
                .append_audit(&cycle.cycle_id, "stale", json!({}))
                .await
                .is_err()
        );
        assert!(
            lease
                .reserve_attempt(
                    &cycle.cycle_id,
                    "first",
                    &CycleLimits::default(),
                    std::time::Duration::from_secs(1)
                )
                .await
                .is_err()
        );
        assert!(
            lease
                .complete_cycle(
                    &contract,
                    &cycle.cycle_id,
                    &CycleOutcome::NoChange {
                        reason: "stale".into()
                    },
                    now
                )
                .await
                .is_err()
        );
        drop(lease);
        assert!(
            provider.claim(&resource, "third").await.is_err(),
            "dropping a stale lease must not remove its replacement"
        );
        let current = replacement
            .filesystem(&cycle.cycle_id, "replacement")
            .await
            .unwrap();
        assert_eq!(
            current
                .read_text("/workspace/analysis/value.txt")
                .await
                .unwrap(),
            "original"
        );
        replacement.release().await.unwrap();
        assert!(
            current
                .write_text("/workspace/analysis/value.txt", "released")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn explicit_legacy_binding_preserves_workspace_identity_and_sealed_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let now = Utc::now();
        let contract = contract('a', now - Duration::hours(2));
        let resource = contract.resource().unwrap();
        let legacy_id = sha256_digest(resource.name.as_bytes());
        let lease = provider
            .open_claim(&resource, "legacy", &legacy_id)
            .unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(&contract, &profile(), "legacy", now)
                .await
                .unwrap(),
        )
        .cycle;
        lease
            .filesystem(&cycle.cycle_id, "legacy")
            .await
            .unwrap()
            .write_text(
                "/workspace/analysis/provenance.txt",
                "historical source stays historical",
            )
            .await
            .unwrap();
        let digest = lease.seal_analysis_manifest(&cycle.cycle_id).await.unwrap();
        let root = lease.root.clone();
        let manifest_path = root
            .join("cycles")
            .join(&cycle.cycle_id)
            .join("analysis-manifest.json");
        let sealed = fs::read(&manifest_path).unwrap();
        drop(lease);
        fs::remove_file(root.join("resource.json")).unwrap();
        write_json_atomic(
            &root.join("claim.json"),
            &json!({
                "schemaVersion": 1, "attemptId": "legacy", "claimedAt": now,
            }),
        )
        .unwrap();
        assert!(provider.claim(&resource, "before-migration").await.is_err());
        let mut wrong = resource.clone();
        wrong.scope.tenant = "wrong".into();
        assert!(provider.bind_legacy_resource(&wrong).is_err());
        assert!(provider.bind_legacy_resource(&resource).unwrap());
        assert!(!provider.bind_legacy_resource(&resource).unwrap());
        let migrated = provider.claim(&resource, "recovered").await.unwrap();
        assert_eq!(migrated.workspace_id(), legacy_id);
        let resumed = ready(
            migrated
                .prepare_cycle(&contract, &profile(), "recovered", now)
                .await
                .unwrap(),
        )
        .cycle;
        assert!(resumed.resumed);
        assert_eq!(resumed.scope, resource.scope);
        assert_eq!(
            migrated
                .seal_analysis_manifest(&cycle.cycle_id)
                .await
                .unwrap(),
            digest
        );
        assert_eq!(fs::read(manifest_path).unwrap(), sealed);
    }

    #[tokio::test]
    async fn exhausted_fencing_epoch_is_never_reused() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let resource = contract('a', Utc::now()).resource().unwrap();
        let lease = provider.claim(&resource, "first").await.unwrap();
        let claim_path = directory
            .path()
            .join(lease.workspace_id().trim_start_matches("sha256:"))
            .join("claim.json");
        lease.release().await.unwrap();
        let mut record: ClaimRecord = read_json(&claim_path).unwrap();
        record.epoch = u64::MAX;
        write_json_atomic(&claim_path, &record).unwrap();
        assert!(provider.claim(&resource, "overflow").await.is_err());
        assert_eq!(
            read_json::<ClaimRecord>(&claim_path).unwrap().epoch,
            u64::MAX
        );
    }

    #[tokio::test]
    async fn local_execution_projection_requires_the_exact_live_writable_claim() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path().join("first")).unwrap();
        let other_provider = LocalWorkspaceProvider::new(directory.path().join("other")).unwrap();
        let accepted_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-1")
            .await
            .unwrap();
        assert!(provider.analysis_directory(&lease, "unprepared").is_err());
        let cycle = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-1",
                    accepted_at + Duration::hours(2),
                )
                .await
                .unwrap(),
        )
        .cycle;
        assert!(
            provider
                .analysis_directory(&lease, &cycle.cycle_id)
                .unwrap()
                .is_dir()
        );
        assert!(provider.analysis_directory(&lease, "wrong-cycle").is_err());

        let other_lease = other_provider
            .claim(&contract.resource().unwrap(), "attempt-1")
            .await
            .unwrap();
        assert_eq!(lease.workspace_id(), other_lease.workspace_id());
        assert!(
            provider
                .analysis_directory(&other_lease, &cycle.cycle_id)
                .is_err(),
            "matching identity alone cannot select another local lease"
        );
        lease.seal_analysis_manifest(&cycle.cycle_id).await.unwrap();
        assert!(
            provider
                .analysis_directory(&lease, &cycle.cycle_id)
                .is_err()
        );
        drop(lease);
        assert!(
            provider
                .claim(&contract.resource().unwrap(), "attempt-2")
                .await
                .is_ok(),
            "the execution projection must not retain ownership after lease drop"
        );
    }

    #[tokio::test]
    async fn commits_handoff_pointer_without_advancing_on_checkpoint_or_orphan() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let accepted_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-1")
            .await
            .unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-1",
                    accepted_at + Duration::hours(2),
                )
                .await
                .unwrap(),
        )
        .cycle;
        let metadata = json!({"outcome": "draftReady", "revision": 0});
        lease
            .record_handoff(
                &cycle.cycle_id,
                "attempt-1",
                "draft",
                Some(metadata.clone()),
                None,
            )
            .await
            .unwrap();
        lease
            .record_handoff(
                &cycle.cycle_id,
                "attempt-1",
                "draft",
                Some(metadata.clone()),
                None,
            )
            .await
            .unwrap();
        assert!(matches!(
            lease
                .record_handoff(
                    &cycle.cycle_id,
                    "attempt-1",
                    "changed",
                    Some(metadata.clone()),
                    None,
                )
                .await,
            Err(AnalysisError::InvalidState(_))
        ));
        lease
            .record_handoff(
                &cycle.cycle_id,
                "attempt-2",
                "checkpoint",
                None,
                Some("attempt-1"),
            )
            .await
            .unwrap();
        let root = directory
            .path()
            .join(workspace_id(&contract.resource().unwrap()).trim_start_matches("sha256:"))
            .join("cycles")
            .join(&cycle.cycle_id);
        let orphan = HandoffRecord {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            attempt_id: "attempt-orphan".to_owned(),
            recorded_at: Utc::now(),
            summary: "unpublished".to_owned(),
            metadata: Some(json!({"outcome": "requestChange"})),
            cycle_id: Some(cycle.cycle_id.clone()),
            contract_digest: Some(contract.contract_digest.clone()),
            previous_handoff: Some("attempt-1".to_owned()),
            artifact: None,
        };
        write_json_atomic(&root.join("handoffs").join("attempt-orphan.json"), &orphan).unwrap();
        drop(lease);
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-3")
            .await
            .unwrap();
        let mut changed_profile = profile();
        changed_profile.skill_names.reverse();
        let resumed = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &changed_profile,
                    "attempt-3",
                    accepted_at + Duration::hours(3),
                )
                .await
                .unwrap(),
        );
        assert_eq!(resumed.cycle.cycle_id, cycle.cycle_id);
        assert_eq!(
            lease
                .latest_handoff(&cycle.cycle_id)
                .await
                .unwrap()
                .unwrap()
                .metadata,
            metadata
        );
        let mut corrupt: HandoffRecord =
            read_json(&root.join("handoffs").join("attempt-1.json")).unwrap();
        corrupt.contract_digest = Some("wrong".to_owned());
        write_json_atomic(&root.join("handoffs").join("attempt-1.json"), &corrupt).unwrap();
        assert!(matches!(
            lease.latest_handoff(&cycle.cycle_id).await,
            Err(AnalysisError::InvalidState(_))
        ));
        fs::remove_file(root.join("handoffs").join("attempt-1.json")).unwrap();
        assert!(matches!(
            lease.latest_handoff(&cycle.cycle_id).await,
            Err(AnalysisError::InvalidState(_))
        ));
    }

    #[tokio::test]
    async fn durable_attempt_budget_refunds_execution_but_charges_crash_reservations() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let accepted_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-1")
            .await
            .unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-1",
                    accepted_at + Duration::hours(2),
                )
                .await
                .unwrap(),
        )
        .cycle;
        let limits = CycleLimits {
            max_attempts: 8,
            max_revisions: 1,
            execution_seconds: 10,
        };
        let seconds = std::time::Duration::from_secs;
        assert_eq!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-1", &limits, seconds(8))
                .await
                .unwrap(),
            seconds(8)
        );
        lease
            .settle_attempt(&cycle.cycle_id, "attempt-1", seconds(2))
            .await
            .unwrap();
        assert_eq!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-2", &limits, seconds(6))
                .await
                .unwrap(),
            seconds(6)
        );
        drop(lease); // A crash leaves the second reservation fully charged.
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-3")
            .await
            .unwrap();
        lease
            .prepare_cycle(
                &contract,
                &profile(),
                "attempt-3",
                accepted_at + Duration::hours(3),
            )
            .await
            .unwrap();
        assert_eq!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-3", &limits, seconds(8))
                .await
                .unwrap(),
            seconds(2)
        );
        assert!(matches!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-4", &limits, seconds(1))
                .await,
            Err(AnalysisError::LimitExceeded(_))
        ));
        lease
            .settle_attempt(&cycle.cycle_id, "attempt-3", seconds(1))
            .await
            .unwrap();
        let changed = CycleLimits {
            max_attempts: 9,
            ..limits.clone()
        };
        assert!(matches!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-4", &changed, seconds(1))
                .await,
            Err(AnalysisError::InvalidState(_))
        ));
        lease
            .reserve_attempt(&cycle.cycle_id, "attempt-4", &limits, seconds(1))
            .await
            .unwrap();
        lease
            .settle_attempt(&cycle.cycle_id, "attempt-4", seconds(2))
            .await
            .unwrap();
        assert!(matches!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-5", &limits, seconds(1))
                .await,
            Err(AnalysisError::LimitExceeded(_))
        ));
    }

    #[tokio::test]
    async fn failed_attempts_count_and_budget_updates_do_not_invalidate_sealed_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let provider = LocalWorkspaceProvider::new(directory.path()).unwrap();
        let accepted_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
        let contract = contract('a', accepted_at);
        let lease = provider
            .claim(&contract.resource().unwrap(), "attempt-1")
            .await
            .unwrap();
        let cycle = ready(
            lease
                .prepare_cycle(
                    &contract,
                    &profile(),
                    "attempt-1",
                    accepted_at + Duration::hours(2),
                )
                .await
                .unwrap(),
        )
        .cycle;
        let limits = CycleLimits {
            max_attempts: 2,
            ..CycleLimits::default()
        };
        let seconds = std::time::Duration::from_secs;
        lease
            .reserve_attempt(&cycle.cycle_id, "attempt-1", &limits, seconds(1))
            .await
            .unwrap();
        lease
            .record_attempt_failure(&cycle.cycle_id, "attempt-1", "failed")
            .await
            .unwrap();
        lease
            .settle_attempt(&cycle.cycle_id, "attempt-1", seconds(0))
            .await
            .unwrap();
        lease
            .reserve_attempt(&cycle.cycle_id, "attempt-2", &limits, seconds(1))
            .await
            .unwrap();
        let rules = json!([{"when": true, "then": 1}]);
        let proposal = lease
            .prepare_candidate_proposal(&cycle.cycle_id, "attempt-2", rules.clone())
            .await
            .unwrap();
        lease
            .settle_attempt(&cycle.cycle_id, "attempt-2", seconds(0))
            .await
            .unwrap();
        assert_eq!(
            lease
                .prepare_candidate_proposal(&cycle.cycle_id, "attempt-3", rules)
                .await
                .unwrap(),
            proposal
        );
        assert!(matches!(
            lease
                .reserve_attempt(&cycle.cycle_id, "attempt-3", &limits, seconds(1))
                .await,
            Err(AnalysisError::LimitExceeded(_))
        ));
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
            .claim(
                &contract.resource().unwrap(),
                "00000000-0000-0000-0000-000000000000",
            )
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
            .claim(&contract.resource().unwrap(), "attempt-1")
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

        assert!(!filesystem.exists("/workspace/cutoff.json").await.unwrap());

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
            .claim(&contract.resource().unwrap(), "attempt-2")
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
            .claim(&contract.resource().unwrap(), "attempt-1")
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
            .join(workspace_id(&contract.resource().unwrap()).trim_start_matches("sha256:"));
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
            workspace_id: workspace_id(&contract.resource().unwrap()),
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
            .claim(&contract.resource().unwrap(), "attempt-2")
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
            .claim(&contract('a', Utc::now()).resource().unwrap(), "attempt-1")
            .await
            .expect("first claim");
        assert!(
            first_provider
                .claim(&contract('a', Utc::now()).resource().unwrap(), "attempt-2")
                .await
                .is_err()
        );
        assert!(
            second_provider
                .claim(&contract('a', Utc::now()).resource().unwrap(), "attempt-3")
                .await
                .is_err()
        );

        drop(first);
        second_provider
            .claim(&contract('a', Utc::now()).resource().unwrap(), "attempt-4")
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
            .claim(&old_contract.resource().unwrap(), "attempt-1")
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
            .claim(&contract.resource().unwrap(), "attempt-1")
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
                .claim(&contract.resource().unwrap(), "attempt-1")
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
                .join(workspace_id(&contract.resource().unwrap()).trim_start_matches("sha256:"));
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
                .claim(&contract.resource().unwrap(), "attempt-2")
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
