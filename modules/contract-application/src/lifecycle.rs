use crate::{
    ContractObservability, INSTRUMENTATION_SCOPE,
    error::{Result, ServiceError},
    models::{
        ActivationResult, CandidateResult, ContractVersion, CurrentCatalog, CurrentCatalogEntry,
        ExecutableCatalog, ExecutableHistory, ExecutableListing, ManagedExecutable, VersionList,
        VersionSummary,
    },
};
use flaggo_contract::{
    AnalysisCandidateSubmission, ContractResourceId, DecisionContract,
    DecisionContractValidationResult, DecisionExecutable, DecisionExecutableActivationRequest,
    ExecutableRule, JsonValue, ResourceScope, RuleWhen,
    digests::is_sha256_digest,
    validation::{is_decision_name, validate_contract},
};
use flaggo_contract_store::AcceptedContractVersion;
use flaggo_executable_store::{
    AnalysisCandidateAdmission, AuthorityError, CandidateActivationCommand, ContractAuthority,
    ExecutableLifecycleState as LifecycleState, ExecutableLifecycleToken, ExecutableStoreError,
    StoredExecutable,
};
use flaggo_expression::{ExecutableCompilation, ExecutableCompiler, ExpressionCompiler};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{future::Future, sync::Arc};
use tracing::Span;

#[derive(Clone)]
pub struct ContractLifecycle {
    pub authority: Arc<dyn ContractAuthority>,
    pub(crate) compiler: Arc<ExecutableCompiler>,
    pub(crate) observability: ContractObservability,
    correlation: Option<String>,
}

impl ContractLifecycle {
    pub fn new(authority: Arc<dyn ContractAuthority>) -> Self {
        Self::instrumented(authority, ContractObservability::new())
    }

    pub fn instrumented(
        authority: Arc<dyn ContractAuthority>,
        observability: ContractObservability,
    ) -> Self {
        Self {
            authority,
            compiler: Arc::new(ExecutableCompiler::default()),
            observability,
            correlation: None,
        }
    }

    pub(crate) fn correlated(&self, correlation: &str) -> Self {
        Self {
            correlation: Some(correlation.into()),
            ..self.clone()
        }
    }

    fn observe<'a, T: 'a>(
        &'a self,
        operation: &'static str,
        name: &'a str,
        digest: Option<&'a str>,
        future: impl Future<Output = Result<(T, &'static str)>> + 'a,
    ) -> impl Future<Output = Result<T>> + 'a {
        self.observability
            .observe(operation, name, digest, self.correlation.as_deref(), future)
    }

    pub async fn validate(
        &self,
        resource: &ContractResourceId,
        contract: &DecisionContract,
    ) -> Result<DecisionContractValidationResult> {
        let name = &resource.name;
        self.observe("contract.validate", name, None, async {
            resource
                .resource_key()
                .map_err(|_| ServiceError::InvalidArgument)?;
            if ContractResourceId::try_from(contract).map_err(|_| ServiceError::InvalidContract)?
                != *resource
            {
                return Err(ServiceError::ResourceMismatch);
            }
            let validation = validate_contract(contract, Some(&ExpressionCompiler::default()))?;
            if let Some(digest) = &validation.contract_digest {
                Span::current().record("flaggo.contract.digest", digest);
            }
            let outcome = if validation.status == "valid" {
                "success"
            } else {
                "invalid"
            };
            Ok((validation, outcome))
        })
        .await
    }

    pub async fn deploy(
        &self,
        resource: &ContractResourceId,
        contract: &DecisionContract,
    ) -> Result<(ContractVersion, bool)> {
        let name = resource.name.as_str();
        self.observe("contract.deploy", name, None, async {
            let validation = self.validate(resource, contract).await?;
            let digest = validation
                .contract_digest
                .filter(|_| validation.status == "valid")
                .ok_or(ServiceError::InvalidContract)?;
            Span::current().record("flaggo.contract.digest", &digest);
            let pending = AcceptedContractVersion {
                contract_digest: digest.clone(),
                accepted_at: chrono::Utc::now().fixed_offset(),
                contract: contract.clone(),
            };
            let default = self.compiler.compile(
                contract,
                &DecisionExecutable {
                    contract_digest: digest.clone(),
                    kind: "rules".into(),
                    rules: vec![],
                },
            )?;
            let authored = contract
                .authored_executable
                .as_ref()
                .filter(|authored| {
                    authored.rules.iter().all(|rule| {
                        matches!(
                            rule.as_ref().map(|rule| &rule.when),
                            Some(RuleWhen::Expression(_))
                        )
                    })
                })
                .map(|authored| {
                    self.compiler.compile(
                        contract,
                        &DecisionExecutable {
                            contract_digest: digest.clone(),
                            kind: "rules".into(),
                            rules: authored
                                .rules
                                .iter()
                                .map(|rule| {
                                    rule.as_ref().map(|rule| ExecutableRule {
                                        name: rule.name.clone(),
                                        when: flaggo_contract::ExpressionWhen {
                                            expression: match &rule.when {
                                                RuleWhen::Expression(value) => Some(value.clone()),
                                                _ => None,
                                            },
                                        },
                                        result: rule.result.clone(),
                                    })
                                })
                                .collect(),
                        },
                    )
                })
                .transpose()?;
            let default = stored(default, Some(json!({"source": "default"}).into()));
            let authored =
                authored.map(|value| stored(value, Some(json!({"source": "authored"}).into())));
            let publication = self
                .authority
                .publish_ready(&pending, &default, authored.as_ref())
                .await?;
            for executable_digest in &publication.bootstrap_activations {
                self.observe("candidate.submit", name, Some(&digest), async {
                    record_candidate(executable_digest);
                    candidate_admitted(name, &digest, executable_digest);
                    Ok(((), "success"))
                })
                .await?;
                self.observe("candidate.activate", name, Some(&digest), async {
                    record_candidate(executable_digest);
                    activation_completed(name, &digest, executable_digest);
                    Ok(((), "success"))
                })
                .await?;
            }
            let active = publication.active;
            Span::current().record("flaggo.executable.digest", &active.executable_digest);
            Ok((
                (
                    project_version(&publication.accepted, &active.executable_digest),
                    publication.created,
                ),
                "success",
            ))
        })
        .await
    }

    pub async fn get(
        &self,
        resource: &ContractResourceId,
        digest: Option<&str>,
    ) -> Result<Option<ContractVersion>> {
        let name = resource.name.as_str();
        resource
            .resource_key()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("contract.read", name, digest, async {
            validate_identity(name, digest)?;
            let accepted = match digest {
                Some(digest) => self.authority.get_contract(resource, digest).await?,
                None => self.authority.get_current_contract(resource).await?,
            };
            let result = match accepted {
                Some(accepted) => self
                    .authority
                    .get_active_executable(&accepted.contract_digest)
                    .await?
                    .map(|active| project_version(&accepted, &active.executable_digest)),
                None => None,
            };
            let outcome = if result.is_some() {
                "success"
            } else {
                "not_found"
            };
            Ok((result, outcome))
        })
        .await
    }

    pub async fn catalog(&self, scope: &ResourceScope) -> Result<(CurrentCatalog, String)> {
        scope
            .validate()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("contract.catalog.read", "", None, async {
            let mut current = self.authority.list_current_contracts(scope).await?;
            for accepted in &current {
                self.require_active(&accepted.contract_digest).await?;
            }
            current.sort_by(|left, right| {
                let a = &left.contract;
                let b = &right.contract;
                let key = |contract: &DecisionContract| {
                    [
                        contract
                            .authority
                            .tenant
                            .as_deref()
                            .unwrap_or("")
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                        contract
                            .authority
                            .application
                            .as_deref()
                            .unwrap_or("")
                            .encode_utf16()
                            .collect(),
                        contract
                            .authority
                            .environment
                            .as_deref()
                            .unwrap_or("")
                            .encode_utf16()
                            .collect(),
                        contract.name.encode_utf16().collect(),
                    ]
                };
                key(a)
                    .cmp(&key(b))
                    .then(left.contract_digest.cmp(&right.contract_digest))
            });
            let mut hash = Sha256::new();
            hash.update(b"flaggo-current-contract-catalog-v2");
            for component in [&scope.tenant, &scope.application, &scope.environment] {
                hash.update(component.as_bytes());
                hash.update([0]);
            }
            let mut contracts = Vec::new();
            for accepted in current {
                hash.update(accepted.contract_digest.as_bytes());
                contracts.push(CurrentCatalogEntry {
                    contract_digest: accepted.contract_digest,
                    contract: accepted.contract,
                });
            }
            Ok((
                (
                    CurrentCatalog { contracts },
                    format!("\"catalog:{:x}\"", hash.finalize()),
                ),
                "success",
            ))
        })
        .await
    }

    pub async fn versions(
        &self,
        resource: &ContractResourceId,
        limit: i32,
        cursor: Option<&str>,
    ) -> Result<Option<VersionList>> {
        let name = resource.name.as_str();
        resource
            .resource_key()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("contract.read", name, None, async {
            validate_name(name)?;
            validate_limit(limit)?;
            let Some(current) = self.authority.get_current_contract(resource).await? else {
                return Ok((None, "not_found"));
            };
            let page = self
                .authority
                .list_contract_versions(resource, limit, cursor)
                .await?;
            let mut versions = Vec::new();
            for accepted in page.versions {
                let active = self.require_active(&accepted.contract_digest).await?;
                versions.push(VersionSummary {
                    contract_digest: accepted.contract_digest,
                    status: "ready",
                    accepted_at: utc_timestamp(accepted.accepted_at),
                    active_executable_digest: active.executable_digest,
                });
            }
            Ok((
                Some(VersionList {
                    name: name.into(),
                    current_contract_digest: current.contract_digest,
                    versions,
                    next_cursor: page.next_cursor,
                }),
                "success",
            ))
        })
        .await
    }

    pub async fn submit(
        &self,
        resource: &ContractResourceId,
        digest: &str,
        submission: AnalysisCandidateSubmission,
    ) -> Result<(CandidateResult, bool)> {
        let name = resource.name.as_str();
        resource
            .resource_key()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("candidate.submit", name, Some(digest), async {
            validate_identity(name, Some(digest))?;
            let provenance = &submission.provenance;
            for text in [
                &provenance.workspace_id,
                &provenance.cycle_id,
                &provenance.attempt_id,
            ] {
                if text.trim().is_empty()
                    || text.encode_utf16().count() > 256
                    || text.chars().any(char::is_control)
                {
                    return Err(ServiceError::InvalidArgument);
                }
            }
            validate_digest(&provenance.workspace_id)?;
            validate_digest(&provenance.analysis_manifest_digest)?;
            let accepted = self
                .authority
                .get_contract(resource, digest)
                .await?
                .ok_or(ServiceError::NotFound)?;
            let compiled = self.compiler.compile(
                &accepted.contract,
                &DecisionExecutable {
                    contract_digest: digest.into(),
                    kind: "rules".into(),
                    rules: submission.rules,
                },
            )?;
            record_candidate(&compiled.executable_digest);
            let provenance_value = json!({"source": "async-analysis",
                "workspaceId": provenance.workspace_id, "cycleId": provenance.cycle_id,
                "attemptId": provenance.attempt_id, "analysisManifestDigest": provenance.analysis_manifest_digest}).into();
            let stored = stored(compiled, Some(provenance_value));
            let admission = AnalysisCandidateAdmission {
                workspace_id: provenance.workspace_id.clone(),
                cycle_id: provenance.cycle_id.clone(),
                attempt_id: provenance.attempt_id.clone(),
                resource: resource.clone(),
                contract_digest: digest.into(),
                executable_digest: stored.executable_digest.clone(),
                analysis_manifest_digest: provenance.analysis_manifest_digest.clone(),
                created_at: stored.created_at,
            };
            let result = self
                .authority
                .admit_current(&stored, &admission)
                .await.map_err(|error| match error {
                    AuthorityError::Executable(ExecutableStoreError::CurrentContractConflict { contract_name, expected, actual }) =>
                        ServiceError::Stale { name: contract_name, requested: expected, current: actual },
                    other => other.into(),
                })?;
            candidate_admitted(name, digest, &result.executable_digest);
            let response = CandidateResult { contract_name: name.into(), contract_digest: digest.into(),
                executable_digest: result.executable_digest, lifecycle_state: state_name(result.lifecycle_state),
                created_at: result.created_at, created: result.created };
            Ok(((response, result.created), "success"))
        })
        .await
    }

    pub async fn executables(
        &self,
        resource: &ContractResourceId,
        digest: &str,
        limit: i32,
        cursor: Option<&str>,
        history: bool,
    ) -> Result<Option<ExecutableListing>> {
        let name = resource.name.as_str();
        resource
            .resource_key()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("contract.executables.read", name, Some(digest), async {
            validate_identity(name, Some(digest))?;
            validate_limit(limit)?;
            if self
                .authority
                .get_contract(resource, digest)
                .await?
                .is_none()
            {
                return Ok((None, "not_found"));
            }
            let active = if history {
                None
            } else {
                Some(self.require_active(digest).await?)
            };
            let page = self
                .authority
                .list_executables(digest, limit, cursor, history)
                .await?;
            let rows = page.executables.iter().map(project_executable).collect();
            let response = if let Some(active) = active {
                Span::current().record("flaggo.executable.digest", &active.executable_digest);
                ExecutableListing::Catalog(Box::new(ExecutableCatalog {
                    contract_name: name.into(),
                    contract_digest: digest.into(),
                    active_executable: project_executable(&active),
                    candidates: rows,
                    next_cursor: page.next_cursor,
                }))
            } else {
                ExecutableListing::History(ExecutableHistory {
                    contract_name: name.into(),
                    contract_digest: digest.into(),
                    executables: rows,
                    next_cursor: page.next_cursor,
                })
            };
            Ok((Some(response), "success"))
        })
        .await
    }

    pub async fn activate(
        &self,
        resource: &ContractResourceId,
        digest: &str,
        candidate: &str,
        request: DecisionExecutableActivationRequest,
    ) -> Result<ActivationResult> {
        let name = resource.name.as_str();
        resource
            .resource_key()
            .map_err(|_| ServiceError::InvalidArgument)?;
        self.observe("candidate.activate", name, Some(digest), async {
            record_candidate(candidate);
            validate_identity(name, Some(digest))?;
            validate_digest(candidate)?;
            validate_digest(&request.expected_active_executable_digest)?;
            if request.expected_candidate_state_version < 0
                || request.expected_active_state_version < 0
            {
                return Err(ServiceError::InvalidArgument);
            }
            if self
                .authority
                .get_contract(resource, digest)
                .await?
                .is_none()
            {
                return Err(ServiceError::NotFound);
            }
            let result = self
                .authority
                .activate_candidate(&CandidateActivationCommand {
                    resource: resource.clone(),
                    contract_digest: digest.into(),
                    executable_digest: candidate.into(),
                    expected_candidate_state_version: request.expected_candidate_state_version,
                    expected_active_executable: ExecutableLifecycleToken {
                        executable_digest: request.expected_active_executable_digest,
                        state_version: request.expected_active_state_version,
                    },
                })
                .await?;
            let active = self
                .authority
                .get_executable(&result.executable_digest)
                .await?
                .ok_or_else(|| {
                    ServiceError::Integrity(
                        "Activated executable could not be read after persistence.".into(),
                    )
                })?;
            activation_completed(name, digest, candidate);
            Ok((
                ActivationResult {
                    contract_name: name.into(),
                    contract_digest: digest.into(),
                    executable: project_executable(&active),
                    previous_executable_digest: result.previous_executable_digest,
                    superseded_candidate_count: result.superseded_candidate_count,
                },
                "success",
            ))
        })
        .await
    }

    async fn require_active(&self, digest: &str) -> Result<StoredExecutable> {
        self.authority
            .get_active_executable(digest)
            .await?
            .ok_or_else(|| {
                ServiceError::Integrity(format!(
                    "Deployed contract '{digest}' has no active executable."
                ))
            })
    }
}

fn project_version(accepted: &AcceptedContractVersion, active: &str) -> ContractVersion {
    ContractVersion {
        name: accepted.contract.name.clone(),
        contract_digest: accepted.contract_digest.clone(),
        status: "ready",
        accepted_at: utc_timestamp(accepted.accepted_at),
        active_executable_digest: active.into(),
        contract: accepted.contract.clone(),
    }
}

pub(crate) fn utc_timestamp(time: chrono::DateTime<chrono::FixedOffset>) -> String {
    let time = time.with_timezone(&chrono::Utc);
    let fraction = format!("{:07}", time.timestamp_subsec_nanos() / 100);
    let fraction = fraction.trim_end_matches('0');
    format!(
        "{}{}{}Z",
        time.format("%Y-%m-%dT%H:%M:%S"),
        if fraction.is_empty() { "" } else { "." },
        fraction
    )
}

fn project_executable(stored: &StoredExecutable) -> ManagedExecutable {
    ManagedExecutable {
        contract_digest: stored.executable.contract_digest.clone(),
        executable_digest: stored.executable_digest.clone(),
        lifecycle_state: state_name(stored.state),
        state_version: stored.state_version,
        created_at: stored.created_at,
        activated_at: stored.activated_at,
        executable: stored.executable.clone(),
        provenance: stored.provenance.clone(),
    }
}

fn stored(compilation: ExecutableCompilation, provenance: Option<JsonValue>) -> StoredExecutable {
    StoredExecutable {
        executable_digest: compilation.executable_digest,
        executable: compilation.executable,
        checked_executable: compilation.checked_executable,
        state: LifecycleState::Candidate,
        state_version: 0,
        created_at: chrono::Utc::now().fixed_offset(),
        activated_at: None,
        provenance,
    }
}

fn state_name(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Candidate => "candidate",
        LifecycleState::Active => "active",
        LifecycleState::Inactive => "inactive",
    }
}

fn validate_name(name: &str) -> Result<()> {
    if is_decision_name(name) {
        Ok(())
    } else {
        Err(ServiceError::InvalidArgument)
    }
}

fn validate_digest(digest: &str) -> Result<()> {
    if is_sha256_digest(digest) {
        Ok(())
    } else {
        Err(ServiceError::InvalidArgument)
    }
}

fn validate_identity(name: &str, digest: Option<&str>) -> Result<()> {
    validate_name(name)?;
    if let Some(digest) = digest {
        validate_digest(digest)?;
    }
    Ok(())
}

fn validate_limit(limit: i32) -> Result<()> {
    if (1..=200).contains(&limit) {
        Ok(())
    } else {
        Err(ServiceError::InvalidArgument)
    }
}

pub(crate) fn record_candidate(digest: &str) {
    Span::current().record("flaggo.candidate.digest", digest);
    Span::current().record("flaggo.executable.digest", digest);
}

fn candidate_admitted(name: &str, contract: &str, candidate: &str) {
    tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, { "event.name" = "flaggo.candidate.admitted",
        flaggo.contract.name = name, flaggo.contract.digest = contract,
        flaggo.candidate.digest = candidate, flaggo.operation.outcome = "success" }, "Candidate admitted");
}

pub(crate) fn activation_completed(name: &str, contract: &str, candidate: &str) {
    tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, { "event.name" = "flaggo.activation.completed",
        flaggo.contract.name = name, flaggo.contract.digest = contract,
        flaggo.candidate.digest = candidate, flaggo.executable.digest = candidate,
        flaggo.operation.outcome = "success" }, "Activation completed");
}
