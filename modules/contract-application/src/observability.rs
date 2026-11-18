use crate::{INSTRUMENTATION_SCOPE, error::Result};
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Histogram, Meter},
};
use std::{future::Future, time::Instant};
use tracing::{Instrument as _, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

#[derive(Clone)]
pub struct ContractObservability {
    operations: Counter<u64>,
    duration: Histogram<f64>,
    submissions: Counter<u64>,
    activations: Counter<u64>,
}

impl Default for ContractObservability {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractObservability {
    pub fn new() -> Self {
        Self::with_meter(flaggo_service_observability::ServiceObservability::meter(
            INSTRUMENTATION_SCOPE,
            env!("CARGO_PKG_VERSION"),
        ))
    }

    pub fn with_meter(meter: Meter) -> Self {
        Self {
            operations: meter
                .u64_counter("flaggo.contract.operations")
                .with_unit("{operation}")
                .build(),
            duration: meter
                .f64_histogram("flaggo.contract.operation.duration")
                .with_unit("s")
                .build(),
            submissions: meter
                .u64_counter("flaggo.candidate.submissions")
                .with_unit("{submission}")
                .build(),
            activations: meter
                .u64_counter("flaggo.activation.attempts")
                .with_unit("{attempt}")
                .build(),
        }
    }

    pub(crate) fn observe<'a, T: 'a>(
        &'a self,
        operation: &'static str,
        name: &'a str,
        digest: Option<&'a str>,
        correlation: Option<&'a str>,
        future: impl Future<Output = Result<(T, &'static str)>> + 'a,
    ) -> impl Future<Output = Result<T>> + 'a {
        // Bound stack use when deployment nests validation and activation operations.
        self.observe_classified(
            operation,
            name,
            digest,
            correlation,
            Box::pin(async { future.await.map(|(value, outcome)| (value, outcome, None)) }),
        )
    }

    pub(crate) async fn observe_classified<T>(
        &self,
        operation: &'static str,
        name: &str,
        digest: Option<&str>,
        correlation: Option<&str>,
        future: impl Future<Output = Result<(T, &'static str, Option<&'static str>)>>,
    ) -> Result<T> {
        macro_rules! span {
            ($name:literal) => { tracing::info_span!(target: INSTRUMENTATION_SCOPE, $name,
                flaggo.operation.name = operation,
                flaggo.operation.outcome = tracing::field::Empty,
                flaggo.failure.category = tracing::field::Empty,
                flaggo.contract.name = name,
                flaggo.contract.digest = tracing::field::Empty,
                flaggo.candidate.digest = tracing::field::Empty,
                flaggo.executable.digest = tracing::field::Empty,
                flaggo.request.correlation_id = tracing::field::Empty,
                error.type = tracing::field::Empty,
            ) };
        }
        let span = match operation {
            "contract.validate" => span!("flaggo.contract.validate"),
            "contract.deploy" => span!("flaggo.contract.deploy"),
            "contract.catalog.read" => span!("flaggo.contract.catalog.read"),
            "contract.executables.read" => span!("flaggo.contract.executables.read"),
            "candidate.submit" => span!("flaggo.candidate.submit"),
            "candidate.activate" => span!("flaggo.candidate.activate"),
            _ => span!("flaggo.contract.read"),
        };
        if let Some(digest) = digest {
            span.record("flaggo.contract.digest", digest);
        }
        if let Some(correlation) = correlation {
            span.record("flaggo.request.correlation_id", correlation);
        }
        let mut guard = Operation {
            telemetry: self,
            operation,
            span: span.clone(),
            started: Instant::now(),
            completed: false,
        };
        async {
            match future.await {
                Ok((value, outcome, category)) => {
                    guard.finish(outcome, category);
                    Ok(value)
                }
                Err(error) => {
                    let (outcome, category) = error.classification();
                    guard.span.record("error.type", std::any::type_name_of_val(&error));
                    guard.finish(outcome, Some(category));
                    if operation == "candidate.submit" || operation == "candidate.activate" {
                        tracing::event!(target: INSTRUMENTATION_SCOPE, tracing::Level::WARN, {
                            "event.name" = if operation == "candidate.submit" { "flaggo.candidate.rejected" } else { "flaggo.activation.rejected" },
                            flaggo.contract.name = name, flaggo.contract.digest = digest,
                            flaggo.operation.outcome = outcome, flaggo.failure.category = category,
                            error = %error }, "Contract lifecycle operation rejected");
                    }
                    Err(error)
                }
            }
        }.instrument(span).await
    }
}

struct Operation<'a> {
    telemetry: &'a ContractObservability,
    operation: &'static str,
    span: Span,
    started: Instant,
    completed: bool,
}

impl Operation<'_> {
    fn finish(&mut self, outcome: &'static str, category: Option<&'static str>) {
        self.completed = true;
        self.span.record("flaggo.operation.outcome", outcome);
        let mut attributes = vec![KeyValue::new("flaggo.operation.outcome", outcome)];
        if let Some(category) = category {
            attributes.push(KeyValue::new("flaggo.failure.category", category));
            self.span.record("flaggo.failure.category", category);
            self.span
                .set_status(opentelemetry::trace::Status::error(category));
        }
        match self.operation {
            "candidate.submit" => self.telemetry.submissions.add(1, &attributes),
            "candidate.activate" => self.telemetry.activations.add(1, &attributes),
            _ => {}
        }
        attributes.push(KeyValue::new("flaggo.operation.name", self.operation));
        self.telemetry.operations.add(1, &attributes);
        self.telemetry
            .duration
            .record(self.started.elapsed().as_secs_f64(), &attributes);
    }
}

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.finish("cancelled", Some("cancelled"));
        }
    }
}
