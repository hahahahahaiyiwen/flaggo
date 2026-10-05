use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use flaggo_analysis_domain::{AnalysisContract, AnalysisError, CycleOutcome};
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram},
};
use tracing::Span;

use crate::coordinator::CoordinatorRunOutcome;

pub const INSTRUMENTATION_SCOPE: &str = "flaggo.async-analysis";
pub const SERVICE_NAME: &str = "flaggo-async-analysis";

#[derive(Clone)]
pub struct AnalysisObservability {
    cycles: Counter<u64>,
    cycle_duration: Histogram<f64>,
    active_workers: Gauge<u64>,
    active_worker_count: Arc<AtomicU64>,
}

impl AnalysisObservability {
    #[must_use]
    pub fn new() -> Self {
        let meter = flaggo_service_observability::ServiceObservability::meter(
            INSTRUMENTATION_SCOPE,
            env!("CARGO_PKG_VERSION"),
        );
        let active_workers = meter
            .u64_gauge("flaggo.analysis.workers.active")
            .with_unit("{worker}")
            .build();
        active_workers.record(0, &[]);
        Self {
            cycles: meter
                .u64_counter("flaggo.analysis.cycles")
                .with_unit("{cycle}")
                .build(),
            cycle_duration: meter
                .f64_histogram("flaggo.analysis.cycle.duration")
                .with_unit("s")
                .build(),
            active_workers,
            active_worker_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn start_cycle(&self, contract: &AnalysisContract) -> CycleObservation {
        let span = tracing::info_span!(
            target: INSTRUMENTATION_SCOPE,
            "flaggo.analysis.cycle",
            flaggo.operation.name = "analysis.cycle",
            flaggo.operation.outcome = tracing::field::Empty,
            flaggo.contract.name = %contract.name,
            flaggo.contract.digest = %contract.contract_digest,
            flaggo.analysis.cycle.id = tracing::field::Empty,
            flaggo.analysis.attempt.id = tracing::field::Empty,
            flaggo.candidate.digest = tracing::field::Empty,
            flaggo.failure.category = tracing::field::Empty,
        );
        CycleObservation {
            observability: self.clone(),
            span,
            contract_name: contract.name.clone(),
            contract_digest: contract.contract_digest.clone(),
            cycle_id: None,
            attempt_id: None,
            active_started: None,
        }
    }

    pub(crate) fn worker_active(&self) -> ActiveWorkerGuard {
        let active = self.active_worker_count.fetch_add(1, Ordering::AcqRel) + 1;
        self.active_workers.record(active, &[]);
        ActiveWorkerGuard {
            observability: self.clone(),
        }
    }
}

impl Default for AnalysisObservability {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct CycleObservation {
    observability: AnalysisObservability,
    span: Span,
    contract_name: String,
    contract_digest: String,
    cycle_id: Option<String>,
    attempt_id: Option<String>,
    active_started: Option<Instant>,
}

impl CycleObservation {
    #[must_use]
    pub fn span(&self) -> Span {
        self.span.clone()
    }

    pub fn record_active_attempt(&mut self, cycle_id: &str, attempt_id: &str) {
        self.span.record("flaggo.analysis.cycle.id", cycle_id);
        self.span.record("flaggo.analysis.attempt.id", attempt_id);
        self.cycle_id = Some(cycle_id.to_owned());
        self.attempt_id = Some(attempt_id.to_owned());
        self.active_started = Some(Instant::now());
    }

    pub fn finish(self, result: &Result<CoordinatorRunOutcome, AnalysisError>) {
        let _entered = self.span.enter();
        let status = cycle_status(result);
        self.span.record("flaggo.operation.outcome", status.outcome);
        if let Some(candidate_digest) = status.candidate_digest {
            self.span
                .record("flaggo.candidate.digest", candidate_digest);
        }
        if let Some(category) = status.failure_category {
            self.span.record("flaggo.failure.category", category);
        }

        let mut attributes = vec![KeyValue::new("flaggo.operation.outcome", status.outcome)];
        if let Some(category) = status.failure_category {
            attributes.push(KeyValue::new("flaggo.failure.category", category));
        }
        self.observability.cycles.add(1, &attributes);
        if let Some(started) = self.active_started {
            self.observability
                .cycle_duration
                .record(started.elapsed().as_secs_f64(), &attributes);
        }

        if let Some(category) = status.failure_category {
            self.log_failed(status.outcome, category);
        } else {
            self.log_completed(status.outcome, status.candidate_digest);
        }
    }

    fn log_completed(&self, outcome: &'static str, candidate_digest: Option<&str>) {
        match (
            self.cycle_id.as_deref(),
            self.attempt_id.as_deref(),
            candidate_digest,
        ) {
            (Some(cycle_id), Some(attempt_id), Some(candidate_digest)) => {
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::INFO,
                    {
                        "event.name" = "flaggo.analysis.cycle.completed",
                        "flaggo.operation.outcome" = outcome,
                        "flaggo.contract.name" = %self.contract_name,
                        "flaggo.contract.digest" = %self.contract_digest,
                        "flaggo.analysis.cycle.id" = cycle_id,
                        "flaggo.analysis.attempt.id" = attempt_id,
                        "flaggo.candidate.digest" = candidate_digest,
                    },
                    "Async Analysis cycle completed"
                );
            }
            (Some(cycle_id), Some(attempt_id), None) => {
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::INFO,
                    {
                        "event.name" = "flaggo.analysis.cycle.completed",
                        "flaggo.operation.outcome" = outcome,
                        "flaggo.contract.name" = %self.contract_name,
                        "flaggo.contract.digest" = %self.contract_digest,
                        "flaggo.analysis.cycle.id" = cycle_id,
                        "flaggo.analysis.attempt.id" = attempt_id,
                    },
                    "Async Analysis cycle completed"
                );
            }
            _ => {
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::INFO,
                    {
                        "event.name" = "flaggo.analysis.cycle.completed",
                        "flaggo.operation.outcome" = outcome,
                        "flaggo.contract.name" = %self.contract_name,
                        "flaggo.contract.digest" = %self.contract_digest,
                    },
                    "Async Analysis cycle completed"
                );
            }
        }
    }

    fn log_failed(&self, outcome: &'static str, failure_category: &'static str) {
        match (self.cycle_id.as_deref(), self.attempt_id.as_deref()) {
            (Some(cycle_id), Some(attempt_id)) => {
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::WARN,
                    {
                        "event.name" = "flaggo.analysis.cycle.failed",
                        "flaggo.operation.outcome" = outcome,
                        "flaggo.failure.category" = failure_category,
                        "flaggo.contract.name" = %self.contract_name,
                        "flaggo.contract.digest" = %self.contract_digest,
                        "flaggo.analysis.cycle.id" = cycle_id,
                        "flaggo.analysis.attempt.id" = attempt_id,
                    },
                    "Async Analysis cycle failed"
                );
            }
            _ => {
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::WARN,
                    {
                        "event.name" = "flaggo.analysis.cycle.failed",
                        "flaggo.operation.outcome" = outcome,
                        "flaggo.failure.category" = failure_category,
                        "flaggo.contract.name" = %self.contract_name,
                        "flaggo.contract.digest" = %self.contract_digest,
                    },
                    "Async Analysis cycle failed"
                );
            }
        }
    }
}

pub(crate) struct ActiveWorkerGuard {
    observability: AnalysisObservability,
}

impl Drop for ActiveWorkerGuard {
    fn drop(&mut self) {
        let previous = self
            .observability
            .active_worker_count
            .fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "active worker count underflow");
        self.observability
            .active_workers
            .record(previous.saturating_sub(1), &[]);
    }
}

#[derive(Debug, Eq, PartialEq)]
struct CycleStatus<'a> {
    outcome: &'static str,
    candidate_digest: Option<&'a str>,
    failure_category: Option<&'static str>,
}

fn cycle_status(result: &Result<CoordinatorRunOutcome, AnalysisError>) -> CycleStatus<'_> {
    match result {
        Ok(CoordinatorRunOutcome::Idle) => success_status("idle"),
        Ok(CoordinatorRunOutcome::NotEligible { .. }) => success_status("not_eligible"),
        Ok(CoordinatorRunOutcome::Busy { .. }) => success_status("busy"),
        Ok(CoordinatorRunOutcome::Recoverable {
            failure_category, ..
        }) => CycleStatus {
            outcome: "recoverable",
            candidate_digest: None,
            failure_category: Some(failure_category),
        },
        Ok(CoordinatorRunOutcome::Completed { outcome, .. }) => match outcome {
            CycleOutcome::Candidate { candidate } => CycleStatus {
                outcome: "candidate",
                candidate_digest: Some(&candidate.executable_digest),
                failure_category: None,
            },
            CycleOutcome::NoChange { .. } => success_status("no_change"),
            CycleOutcome::NoCandidate { .. } => success_status("no_candidate"),
            CycleOutcome::Failure { .. } => CycleStatus {
                outcome: "failure",
                candidate_digest: None,
                failure_category: Some("internal"),
            },
            CycleOutcome::Superseded { .. } => success_status("superseded"),
        },
        Err(error) => CycleStatus {
            outcome: "failure",
            candidate_digest: None,
            failure_category: Some(analysis_failure_category(error)),
        },
    }
}

const fn success_status(outcome: &'static str) -> CycleStatus<'static> {
    CycleStatus {
        outcome,
        candidate_digest: None,
        failure_category: None,
    }
}

#[must_use]
pub fn analysis_failure_category(error: &AnalysisError) -> &'static str {
    match error {
        AnalysisError::WorkspaceBusy(_) => "conflict",
        AnalysisError::InvalidState(_) => "integrity",
        AnalysisError::Contract(_)
        | AnalysisError::Workspace(_)
        | AnalysisError::Agent(_)
        | AnalysisError::Evidence(_)
        | AnalysisError::Candidate(_) => "dependency",
    }
}

#[cfg(test)]
mod tests {
    use flaggo_analysis_domain::{AnalysisError, CycleOutcome};

    use super::{CycleStatus, analysis_failure_category, cycle_status};
    use crate::coordinator::CoordinatorRunOutcome;

    #[test]
    fn maps_no_change_to_its_registered_outcome() {
        let result = Ok(CoordinatorRunOutcome::Completed {
            contract_name: "checkout.delay".to_owned(),
            cycle_id: "cycle-1".to_owned(),
            outcome: CycleOutcome::NoChange {
                reason: "guardrails remain healthy".to_owned(),
            },
        });

        assert_eq!(
            cycle_status(&result),
            CycleStatus {
                outcome: "no_change",
                candidate_digest: None,
                failure_category: None,
            }
        );
    }

    #[test]
    fn preserves_recoverable_failure_categories() {
        let result = Ok(CoordinatorRunOutcome::Recoverable {
            contract_name: "checkout.delay".to_owned(),
            cycle_id: "cycle-1".to_owned(),
            reason: "provider timeout".to_owned(),
            failure_category: "timeout",
        });

        assert_eq!(
            cycle_status(&result),
            CycleStatus {
                outcome: "recoverable",
                candidate_digest: None,
                failure_category: Some("timeout"),
            }
        );
    }

    #[test]
    fn classifies_invalid_state_as_integrity_failure() {
        assert_eq!(
            analysis_failure_category(&AnalysisError::InvalidState(
                "terminal invariant violated".to_owned()
            )),
            "integrity"
        );
    }
}
