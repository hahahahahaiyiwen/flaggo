mod config;
mod observability;

use std::{error::Error, time::Instant};

use chrono::Utc;
use config::MaterializerConfig;
use flaggo_evidence_materializer::{
    CatalogFetch, CompiledContractCatalog, EvidenceMaterializer, HttpContractCatalogProvider,
};
use flaggo_evidence_store::{EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::SqliteRawOtlpInbox;
use flaggo_service_observability::ServiceObservability;
use observability::{INSTRUMENTATION_SCOPE, MaterializerObservability, SERVICE_NAME};
use tokio::{
    signal,
    time::{MissedTickBehavior, interval},
};
use tracing::Instrument as _;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let telemetry = ServiceObservability::initialize(
        SERVICE_NAME,
        INSTRUMENTATION_SCOPE,
        env!("CARGO_PKG_VERSION"),
    )?;
    let result = run(MaterializerObservability::new()).await;
    if let Err(error) = &result {
        tracing::event!(
            target: INSTRUMENTATION_SCOPE,
            tracing::Level::ERROR,
            {
                "event.name" = "flaggo.service.failed",
                "flaggo.operation.outcome" = "failure",
                "flaggo.failure.category" = "internal",
                "error.type" = std::any::type_name_of_val(error.as_ref()),
            },
            "Evidence Materializer failed: {}",
            error
        );
    }
    telemetry.shutdown()?;
    result
}

async fn run(observability: MaterializerObservability) -> Result<(), Box<dyn Error>> {
    let config = MaterializerConfig::from_environment()?;
    let inbox = SqliteRawOtlpInbox::connect_reader(&config.database_url).await?;
    let store = SqliteEvidenceStore::connect(&config.database_url).await?;
    let inbox_for_shutdown = inbox.clone();
    let store_for_shutdown = store.clone();
    let materializer = EvidenceMaterializer::new(inbox, store.clone(), config.read_limit);
    let (mut catalog, mut catalog_etag) = match store.load_catalog().await? {
        Some(stored) => (
            CompiledContractCatalog::compile(stored.payload)?,
            Some(stored.etag),
        ),
        None => (CompiledContractCatalog::empty(), None),
    };
    let provider = config
        .catalog_url
        .as_deref()
        .map(HttpContractCatalogProvider::new)
        .transpose()?;
    if let Some(provider) = &provider {
        refresh_catalog(provider, &materializer, &mut catalog, &mut catalog_etag).await;
    }
    let health = materializer.inspect().await?;
    observability.record_health(&health);
    let startup_health = MaterializationSnapshot::new(&health, Utc::now());
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.started",
            "flaggo.operation.outcome" = "success",
            "flaggo.materializer.active_route_count" =
                catalog.active_source_counts().len(),
            "flaggo.materializer.current_contract_count" =
                catalog.current_contracts().len(),
            "flaggo.materializer.evidence.conflict_count" =
                health.evidence_store.conflict_count,
            "flaggo.materializer.evidence.diagnostic_count" =
                health.evidence_store.diagnostic_count,
            "flaggo.materializer.evidence.has_cached_catalog" =
                health.evidence_store.has_cached_catalog,
            "flaggo.materializer.evidence.observation_count" =
                health.evidence_store.observation_count,
            "flaggo.materializer.evidence.provenance_count" =
                health.evidence_store.provenance_count,
            "flaggo.materializer.checkpoint_batch_id" =
                %startup_health.checkpoint_batch_id,
            "flaggo.materializer.pending_batch_count" =
                startup_health.pending_batch_count,
            "flaggo.materializer.observed_at" = %startup_health.observed_at,
            "flaggo.materializer.oldest_pending_received_at" =
                %startup_health.oldest_pending_received_at,
            "flaggo.materializer.newest_pending_received_at" =
                %startup_health.newest_pending_received_at,
            "flaggo.materializer.oldest_pending_age_ms" =
                startup_health.oldest_pending_age_milliseconds,
            "flaggo.materializer.newest_evidence_observed_at_unix_nano" =
                %startup_health.newest_evidence_observed_at_unix_nano,
            "flaggo.materializer.evidence_freshness_ms" =
                startup_health.evidence_freshness_milliseconds,
        },
        "Evidence Materializer started"
    );

    let mut work = interval(config.poll_interval);
    work.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut catalogs = interval(config.catalog_interval);
    catalogs.set_missed_tick_behavior(MissedTickBehavior::Delay);
    catalogs.tick().await;
    loop {
        tokio::select! {
            shutdown = signal::ctrl_c() => {
                if let Err(error) = shutdown {
                    tracing::event!(
                        target: INSTRUMENTATION_SCOPE,
                        tracing::Level::ERROR,
                        {
                            "flaggo.failure.category" = "internal",
                            "error.type" = std::any::type_name_of_val(&error),
                        },
                        "Evidence Materializer shutdown handler failed: {}",
                        error
                    );
                }
                break;
            }
            _ = work.tick() => {
                let span = tracing::info_span!(
                    target: INSTRUMENTATION_SCOPE,
                    "flaggo.materializer.page",
                    flaggo.operation.name = "materializer.page",
                    flaggo.operation.outcome = tracing::field::Empty,
                    flaggo.failure.category = tracing::field::Empty,
                    flaggo.batch.id = tracing::field::Empty,
                );
                let started = Instant::now();
                match materializer.run_once(&catalog).instrument(span.clone()).await {
                    Ok(result) => {
                        let health = materializer.inspect().await?;
                        let outcome = observability.record_page(
                            result,
                            &health,
                            started.elapsed(),
                        );
                        span.record("flaggo.operation.outcome", outcome);
                        if let Some(batch_id) = health.checkpoint_batch_id {
                            span.record("flaggo.batch.id", batch_id.to_string());
                        }
                        if result.batches_read > 0 {
                            let snapshot =
                                MaterializationSnapshot::new(&health, Utc::now());
                            tracing::event!(
                                target: INSTRUMENTATION_SCOPE,
                                tracing::Level::INFO,
                                {
                                    "event.name" = "flaggo.materializer.page.committed",
                                    "flaggo.operation.outcome" = "success",
                                    "flaggo.batch.id" = health
                                        .checkpoint_batch_id
                                        .map(|value| value.to_string())
                                        .unwrap_or_default(),
                                    "flaggo.materializer.batches_read" =
                                        result.batches_read,
                                    "flaggo.materializer.conflicts_created" =
                                        result.conflicts_created,
                                    "flaggo.materializer.diagnostics_created" =
                                        result.diagnostics_created,
                                    "flaggo.materializer.duplicate_observations" =
                                        result.duplicate_observations,
                                    "flaggo.materializer.observations_created" =
                                        result.observations_created,
                                    "flaggo.materializer.provenance_created" =
                                        result.provenance_created,
                                    "flaggo.materializer.checkpoint_batch_id" =
                                        %snapshot.checkpoint_batch_id,
                                    "flaggo.materializer.pending_batch_count" =
                                        snapshot.pending_batch_count,
                                    "flaggo.materializer.observed_at" =
                                        %snapshot.observed_at,
                                    "flaggo.materializer.oldest_pending_received_at" =
                                        %snapshot.oldest_pending_received_at,
                                    "flaggo.materializer.newest_pending_received_at" =
                                        %snapshot.newest_pending_received_at,
                                    "flaggo.materializer.oldest_pending_age_ms" =
                                        snapshot.oldest_pending_age_milliseconds,
                                    "flaggo.materializer.newest_evidence_observed_at_unix_nano" =
                                        %snapshot.newest_evidence_observed_at_unix_nano,
                                    "flaggo.materializer.evidence_freshness_ms" =
                                        snapshot.evidence_freshness_milliseconds,
                                },
                                "Evidence Materializer page committed"
                            );
                        }
                    }
                    Err(error) => {
                        observability.record_failure(started.elapsed(), "dependency");
                        span.record("flaggo.operation.outcome", "failure");
                        span.record("flaggo.failure.category", "dependency");
                        return Err(Box::new(error));
                    }
                }
            }
            _ = catalogs.tick(), if provider.is_some() => {
                refresh_catalog(
                    provider.as_ref().expect("guard requires a provider"),
                    &materializer,
                    &mut catalog,
                    &mut catalog_etag,
                ).await;
            }
        }
    }

    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopping",
            "flaggo.operation.outcome" = "success",
        },
        "Evidence Materializer stopping"
    );
    inbox_for_shutdown.close().await;
    store_for_shutdown.close().await;
    tracing::event!(
        target: INSTRUMENTATION_SCOPE,
        tracing::Level::INFO,
        {
            "event.name" = "flaggo.service.stopped",
            "flaggo.operation.outcome" = "success",
        },
        "Evidence Materializer stopped"
    );
    Ok(())
}

struct MaterializationSnapshot {
    checkpoint_batch_id: String,
    pending_batch_count: u64,
    observed_at: String,
    oldest_pending_received_at: String,
    newest_pending_received_at: String,
    oldest_pending_age_milliseconds: u64,
    newest_evidence_observed_at_unix_nano: String,
    evidence_freshness_milliseconds: u64,
}

impl MaterializationSnapshot {
    fn new(
        health: &flaggo_evidence_materializer::MaterializerHealth,
        observed_at: chrono::DateTime<Utc>,
    ) -> Self {
        let observed_at_unix_nano = u64::try_from(
            observed_at
                .timestamp_nanos_opt()
                .expect("current UTC time must fit Unix nanoseconds"),
        )
        .expect("current UTC time must follow the Unix epoch");
        Self {
            checkpoint_batch_id: health
                .checkpoint_batch_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            pending_batch_count: health.pending_batch_count,
            observed_at: observed_at.to_rfc3339(),
            oldest_pending_received_at: health
                .oldest_pending_received_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            newest_pending_received_at: health
                .newest_pending_received_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            oldest_pending_age_milliseconds: health
                .oldest_pending_received_at
                .map(|oldest| u64::try_from((observed_at - oldest).num_milliseconds()).unwrap_or(0))
                .unwrap_or(0),
            newest_evidence_observed_at_unix_nano: health
                .evidence_store
                .newest_observed_at_unix_nano
                .map(|value| value.to_string())
                .unwrap_or_default(),
            evidence_freshness_milliseconds: health
                .evidence_store
                .newest_observed_at_unix_nano
                .map(|newest| observed_at_unix_nano.saturating_sub(newest) / 1_000_000)
                .unwrap_or(0),
        }
    }
}

async fn refresh_catalog<I, S>(
    provider: &HttpContractCatalogProvider,
    materializer: &EvidenceMaterializer<I, S>,
    current: &mut CompiledContractCatalog,
    current_etag: &mut Option<String>,
) where
    I: flaggo_raw_otlp_inbox::RawOtlpInbox,
    S: EvidenceStore,
{
    let span = tracing::info_span!(
        target: INSTRUMENTATION_SCOPE,
        "flaggo.materializer.catalog.refresh",
        otel.kind = "client",
        flaggo.operation.name = "materializer.catalog.refresh",
        flaggo.operation.outcome = tracing::field::Empty,
        flaggo.failure.category = tracing::field::Empty,
    );
    async {
        match provider.fetch(current_etag.as_deref()).await {
            Ok(CatalogFetch::NotModified) => {
                tracing::Span::current().record("flaggo.operation.outcome", "success");
            }
            Ok(CatalogFetch::Updated { catalog, etag }) => match materializer
                .activate_catalog(&catalog, etag.clone(), Utc::now())
                .await
            {
                Ok(()) => {
                    tracing::Span::current().record("flaggo.operation.outcome", "success");
                    tracing::event!(
                        target: INSTRUMENTATION_SCOPE,
                        tracing::Level::INFO,
                        {
                            "event.name" = "flaggo.materializer.catalog.activated",
                            "flaggo.operation.outcome" = "success",
                            "flaggo.materializer.active_route_count" =
                                catalog.active_source_counts().len(),
                            "flaggo.materializer.current_contract_count" =
                                catalog.current_contracts().len(),
                        },
                        "Evidence Materializer catalog activated"
                    );
                    *current = catalog;
                    *current_etag = Some(etag);
                }
                Err(error) => {
                    tracing::Span::current().record("flaggo.operation.outcome", "failure");
                    tracing::Span::current().record("flaggo.failure.category", "dependency");
                    tracing::event!(
                        target: INSTRUMENTATION_SCOPE,
                        tracing::Level::WARN,
                        {
                            "event.name" = "flaggo.materializer.catalog.refresh_failed",
                            "flaggo.operation.outcome" = "failure",
                            "flaggo.failure.category" = "dependency",
                            "flaggo.materializer.catalog_retained" =
                                current_etag.is_some(),
                        },
                        "Evidence Materializer catalog activation failed: {}",
                        error
                    );
                }
            },
            Err(error) => {
                tracing::Span::current().record("flaggo.operation.outcome", "unavailable");
                tracing::Span::current().record("flaggo.failure.category", "dependency");
                tracing::event!(
                    target: INSTRUMENTATION_SCOPE,
                    tracing::Level::WARN,
                    {
                        "event.name" = "flaggo.materializer.catalog.refresh_failed",
                        "flaggo.operation.outcome" = "unavailable",
                        "flaggo.failure.category" = "dependency",
                        "flaggo.materializer.catalog_retained" =
                            current_etag.is_some(),
                    },
                    "Evidence Materializer catalog refresh failed: {}",
                    error
                );
            }
        }
    }
    .instrument(span)
    .await;
}
