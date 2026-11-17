use std::time::Instant;

use opentelemetry::{
    KeyValue,
    metrics::{Counter, Histogram, Meter},
};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

#[derive(Clone)]
pub struct DecisionObservability {
    evaluations: Counter<u64>,
    duration: Histogram<f64>,
}

impl Default for DecisionObservability {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionObservability {
    pub fn new() -> Self {
        Self::with_meter(flaggo_service_observability::ServiceObservability::meter(
            crate::INSTRUMENTATION_SCOPE,
            env!("CARGO_PKG_VERSION"),
        ))
    }

    pub fn with_meter(meter: Meter) -> Self {
        Self {
            evaluations: meter
                .u64_counter("flaggo.decision.evaluations")
                .with_unit("{evaluation}")
                .build(),
            duration: meter
                .f64_histogram("flaggo.decision.evaluation.duration")
                .with_unit("s")
                .build(),
        }
    }

    pub(crate) fn start(&self) -> EvaluationOperation<'_> {
        EvaluationOperation {
            observability: self,
            started_at: Instant::now(),
            completed: false,
        }
    }
}

pub(crate) struct EvaluationOperation<'a> {
    observability: &'a DecisionObservability,
    started_at: Instant,
    completed: bool,
}

impl EvaluationOperation<'_> {
    pub fn complete(&mut self, source: &'static str) {
        Span::current().record("flaggo.evaluation.source", source);
        self.finish("success", None, Some(source));
    }

    pub fn fail<E: std::error::Error>(
        &mut self,
        _error: &E,
        outcome: &'static str,
        category: &'static str,
    ) {
        Span::current().record("error.type", std::any::type_name::<E>());
        self.finish(outcome, Some(category), None);
    }

    fn finish(
        &mut self,
        outcome: &'static str,
        category: Option<&'static str>,
        source: Option<&'static str>,
    ) {
        if self.completed {
            return;
        }
        self.completed = true;
        let span = Span::current();
        span.record("flaggo.operation.outcome", outcome);
        let mut attributes = vec![KeyValue::new("flaggo.operation.outcome", outcome)];
        if let Some(category) = category {
            span.record("flaggo.failure.category", category);
            span.set_status(opentelemetry::trace::Status::error(category));
            attributes.push(KeyValue::new("flaggo.failure.category", category));
        }
        if let Some(source) = source {
            attributes.push(KeyValue::new("flaggo.evaluation.source", source));
        }
        self.observability.evaluations.add(1, &attributes);
        self.observability
            .duration
            .record(self.started_at.elapsed().as_secs_f64(), &attributes);
    }
}

impl Drop for EvaluationOperation<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.finish("cancelled", Some("cancelled"), None);
        }
    }
}
