use std::time::{Duration, SystemTime};

use flaggo_evidence_materializer::{MaterializerHealth, MaterializerRunResult};
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram},
};

pub const INSTRUMENTATION_SCOPE: &str = "flaggo.evidence-materializer";
pub const SERVICE_NAME: &str = "flaggo-evidence-materializer";

#[derive(Clone)]
pub struct MaterializerObservability {
    pages: Counter<u64>,
    page_duration: Histogram<f64>,
    pending_batches: Gauge<u64>,
    oldest_pending_age: Gauge<f64>,
    evidence_freshness: Gauge<f64>,
    items: Counter<u64>,
}

impl MaterializerObservability {
    #[must_use]
    pub fn new() -> Self {
        let meter = flaggo_service_observability::ServiceObservability::meter(
            INSTRUMENTATION_SCOPE,
            env!("CARGO_PKG_VERSION"),
        );
        Self {
            pages: meter
                .u64_counter("flaggo.materializer.pages")
                .with_unit("{page}")
                .build(),
            page_duration: meter
                .f64_histogram("flaggo.materializer.page.duration")
                .with_unit("s")
                .build(),
            pending_batches: meter
                .u64_gauge("flaggo.materializer.pending.batches")
                .with_unit("{batch}")
                .build(),
            oldest_pending_age: meter
                .f64_gauge("flaggo.materializer.oldest_pending.age")
                .with_unit("s")
                .build(),
            evidence_freshness: meter
                .f64_gauge("flaggo.materializer.evidence.freshness")
                .with_unit("s")
                .build(),
            items: meter
                .u64_counter("flaggo.materializer.items")
                .with_unit("{item}")
                .build(),
        }
    }

    pub fn record_page(
        &self,
        result: MaterializerRunResult,
        health: &MaterializerHealth,
        duration: Duration,
    ) -> &'static str {
        let outcome = if result.batches_read == 0 {
            "idle"
        } else {
            "success"
        };
        let attributes = [KeyValue::new("flaggo.operation.outcome", outcome)];
        self.pages.add(1, &attributes);
        self.page_duration
            .record(duration.as_secs_f64(), &attributes);
        self.record_health(health);
        self.record_items("observation_created", result.observations_created);
        self.record_items("duplicate_observation", result.duplicate_observations);
        self.record_items("provenance_created", result.provenance_created);
        self.record_items("diagnostic_created", result.diagnostics_created);
        self.record_items("conflict_created", result.conflicts_created);
        outcome
    }

    pub fn record_failure(&self, duration: Duration, failure_category: &'static str) {
        let attributes = [
            KeyValue::new("flaggo.operation.outcome", "failure"),
            KeyValue::new("flaggo.failure.category", failure_category),
        ];
        self.pages.add(1, &attributes);
        self.page_duration
            .record(duration.as_secs_f64(), &attributes);
    }

    pub fn record_health(&self, health: &MaterializerHealth) {
        self.pending_batches.record(health.pending_batch_count, &[]);
        self.oldest_pending_age.record(
            health
                .oldest_pending_received_at
                .map(age_seconds)
                .unwrap_or(0.0),
            &[],
        );
        self.evidence_freshness.record(
            health
                .evidence_store
                .newest_observed_at_unix_nano
                .map(|value| {
                    let observed = Duration::from_nanos(value);
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .saturating_sub(observed)
                        .as_secs_f64()
                })
                .unwrap_or(0.0),
            &[],
        );
    }

    fn record_items(&self, outcome: &'static str, count: u64) {
        if count > 0 {
            self.items.add(
                count,
                &[KeyValue::new("flaggo.materializer.item.outcome", outcome)],
            );
        }
    }
}

impl Default for MaterializerObservability {
    fn default() -> Self {
        Self::new()
    }
}

fn age_seconds(value: chrono::DateTime<chrono::Utc>) -> f64 {
    SystemTime::now()
        .duration_since(value.into())
        .unwrap_or_default()
        .as_secs_f64()
}
