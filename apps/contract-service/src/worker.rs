use crate::{
    ContractLifecycle, INSTRUMENTATION_SCOPE,
    error::{Result, ServiceError},
    lifecycle::{activation_completed, record_candidate},
};
use flaggo_executable_store::{
    AnalysisCandidateActivationSnapshot, AnalysisCandidateResolutionAction as Action,
    AnalysisCandidateResolutionCommand, AnalysisCandidateResolutionOutcome as Outcome,
    ExecutableLifecycleState,
};
use std::time::Duration;

pub const POLL_INTERVAL_VARIABLE: &str = "FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS";

pub fn poll_interval(value: Option<&str>) -> std::result::Result<Duration, String> {
    let value = match value {
        Some(value) if !value.trim().is_empty() => value,
        _ => return Ok(Duration::from_millis(5_000)),
    };
    let invalid = || format!("{POLL_INTERVAL_VARIABLE} must be a positive integer.");
    if !value.bytes().all(|value| value.is_ascii_digit()) {
        return Err(invalid());
    }
    let milliseconds = value.parse::<i64>().map_err(|_| invalid())?;
    // TimeSpan's positive millisecond range, also safely representable by Tokio's timer.
    if !(1..=922_337_203_685_477).contains(&milliseconds) {
        return Err(invalid());
    }
    Ok(Duration::from_millis(milliseconds as u64))
}

pub struct ActivationWorker {
    lifecycle: ContractLifecycle,
    next_contract_name: Option<String>,
}

impl ActivationWorker {
    pub fn new(lifecycle: ContractLifecycle) -> Self {
        Self {
            lifecycle,
            next_contract_name: None,
        }
    }

    pub async fn run_once(&mut self) -> Result<()> {
        let page = self
            .lifecycle
            .executables
            .list_pending(100, self.next_contract_name.as_deref(), None)
            .await?;
        self.next_contract_name = page.next_contract_name;
        for snapshot in page.candidates {
            let admission = &snapshot.admission;
            let result = self.lifecycle.observability.observe_classified(
                "candidate.activate", &admission.contract_name, Some(&admission.contract_digest), None,
                async {
                    record_candidate(&admission.executable_digest);
                    let (action, category) = self.assess(&snapshot).await?;
                    let result = self.lifecycle.executables.resolve(&AnalysisCandidateResolutionCommand {
                        admission: admission.clone(), expected_candidate_state_version: snapshot.candidate_state_version,
                        expected_active_executable: snapshot.active_executable.clone(), action,
                    }, None).await?;
                    let outcome = match result.outcome {
                        Outcome::Activated => "success",
                        Outcome::Rejected => "rejected",
                        Outcome::Superseded => "superseded",
                        Outcome::Yielded | Outcome::AlreadyResolved => "yielded",
                    };
                    let category = if result.outcome == Outcome::Rejected { category } else { None };
                    if result.outcome == Outcome::Activated {
                        activation_completed(&admission.contract_name, &admission.contract_digest, &admission.executable_digest);
                    } else {
                        tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::INFO, {
                            "event.name" = "flaggo.activation.resolved",
                            flaggo.contract.name = admission.contract_name,
                            flaggo.contract.digest = admission.contract_digest,
                            flaggo.candidate.digest = admission.executable_digest,
                            flaggo.executable.digest = admission.executable_digest,
                            flaggo.operation.outcome = outcome,
                            flaggo.failure.category = category,
                            flaggo.activation.superseded_candidate_count = result.superseded_candidate_count,
                            }, "Candidate resolved");
                    }
                    Ok(((), outcome, category))
                },
            ).await;
            if let Err(error) = result {
                tracing::warn!(target: INSTRUMENTATION_SCOPE, %error,
                    flaggo.contract.name = admission.contract_name,
                    flaggo.candidate.digest = admission.executable_digest,
                    "Candidate processing failed; continuing scan");
            }
        }
        Ok(())
    }

    pub async fn run(
        mut self,
        interval: Duration,
        mut stopping: tokio::sync::watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                biased;
                _ = stopping.changed() => return,
                _ = tokio::time::sleep(interval) => {}
            }
            tokio::select! {
                biased;
                _ = stopping.changed() => return,
                result = self.run_once() => if let Err(error) = result { scan_failed(&error); }
            }
        }
    }

    async fn assess(
        &self,
        snapshot: &AnalysisCandidateActivationSnapshot,
    ) -> Result<(Action, Option<&'static str>)> {
        let result = self.assess_inner(snapshot).await;
        match result {
            Err(ServiceError::Compilation(
                flaggo_expression::ExecutableCompilationError::Validation(_)
                | flaggo_expression::ExecutableCompilationError::Expression(_)
                | flaggo_expression::ExecutableCompilationError::Contract(
                    flaggo_contract::ContractError::Serialization(_)
                    | flaggo_contract::ContractError::InvalidModel(_),
                ),
            )) => Ok((Action::Reject, Some("validation"))),
            Err(ServiceError::Materialization(_) | ServiceError::Integrity(_)) => {
                Ok((Action::Reject, Some("integrity")))
            }
            Err(ServiceError::ExecutableStore(
                flaggo_executable_store::ExecutableStoreError::Integrity(_)
                | flaggo_executable_store::ExecutableStoreError::Contract(
                    flaggo_contract::ContractError::Json(_),
                ),
            )) => Ok((Action::Reject, Some("integrity"))),
            Err(
                ServiceError::ContractStore(
                    flaggo_contract_store::ContractStoreError::Integrity(_)
                    | flaggo_contract_store::ContractStoreError::Contract(
                        flaggo_contract::ContractError::Json(_),
                    ),
                )
                | ServiceError::Compilation(flaggo_expression::ExecutableCompilationError::Contract(
                    flaggo_contract::ContractError::Json(_),
                )),
            ) => Ok((Action::Reject, Some("integrity"))),
            other => other,
        }
    }

    async fn assess_inner(
        &self,
        snapshot: &AnalysisCandidateActivationSnapshot,
    ) -> Result<(Action, Option<&'static str>)> {
        let admission = &snapshot.admission;
        let Some(accepted) = self
            .lifecycle
            .contracts
            .get(&admission.contract_name, &admission.contract_digest, None)
            .await?
        else {
            return Ok((Action::Reject, Some("not_found")));
        };
        if accepted
            .contract
            .learning
            .as_ref()
            .map(|learning| learning.policy.mode.as_str())
            != Some("auto-activation")
        {
            return Ok((Action::Reject, Some("validation")));
        }
        let Some(candidate) = self
            .lifecycle
            .executables
            .get(&admission.executable_digest, None)
            .await?
        else {
            return Ok((Action::Reject, Some("not_found")));
        };
        if candidate.state != ExecutableLifecycleState::Candidate
            || candidate.state_version != snapshot.candidate_state_version
        {
            return Ok((Action::Activate, None));
        }
        if candidate.executable.contract_digest != admission.contract_digest {
            return Ok((Action::Reject, Some("integrity")));
        }
        let compiled = self
            .lifecycle
            .compiler
            .compile(&accepted.contract, &candidate.executable)?;
        if compiled.executable_digest != candidate.executable_digest {
            return Ok((Action::Reject, Some("integrity")));
        }
        // Compiler bytes are not a cross-language identity. Independently validate the
        // original checked artifact, including source binding, types and cost bounds.
        self.lifecycle.compiler.materialize(
            &accepted.contract,
            &accepted.contract_digest,
            &candidate.executable,
            &candidate.executable_digest,
            &candidate.checked_executable,
        )?;
        Ok((Action::Activate, None))
    }
}

pub fn scan_failed(error: &ServiceError) {
    let (outcome, category) = error.classification();
    tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::ERROR, { "event.name" = "flaggo.activation.scan_failed",
        flaggo.operation.outcome = outcome, flaggo.failure.category = category,
        error = %error }, "Activation scan failed; will retry");
}
