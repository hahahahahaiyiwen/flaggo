mod config;

use std::error::Error;

use chrono::Utc;
use config::MaterializerConfig;
use flaggo_evidence_materializer::{
    CatalogFetch, CompiledContractCatalog, EvidenceMaterializer, HttpContractCatalogProvider,
    MaterializerHealth,
};
use flaggo_evidence_store::{EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::SqliteRawOtlpInbox;
use serde_json::{Value, json};
use tokio::{
    signal,
    time::{MissedTickBehavior, interval},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    match run().await {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "error": error.to_string(),
                    "event": "materializer.failed"
                })
            );
            Err(error)
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
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

    println!(
        "{}",
        json!({
            "activeRoutes": catalog.active_source_counts().len(),
            "currentContracts": catalog.current_contracts().len(),
            "evidenceStore": {
                "conflictCount": health.evidence_store.conflict_count,
                "diagnosticCount": health.evidence_store.diagnostic_count,
                "hasCachedCatalog": health.evidence_store.has_cached_catalog,
                "observationCount": health.evidence_store.observation_count,
                "provenanceCount": health.evidence_store.provenance_count
            },
            "event": "materializer.started",
            "materialization": materialization_health_json(&health, Utc::now())
        })
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
                    eprintln!("{}", json!({
                        "error": error.to_string(),
                        "event": "materializer.shutdown_handler_failed"
                    }));
                }
                break;
            }
            _ = work.tick() => {
                let result = materializer.run_once(&catalog).await?;
                if result.batches_read > 0 {
                    let health = materializer.inspect().await?;
                    println!("{}", json!({
                        "batchesRead": result.batches_read,
                        "conflictsCreated": result.conflicts_created,
                        "diagnosticsCreated": result.diagnostics_created,
                        "duplicateObservations": result.duplicate_observations,
                        "event": "materializer.batch_page_committed",
                        "materialization": materialization_health_json(&health, Utc::now()),
                        "observationsCreated": result.observations_created,
                        "provenanceCreated": result.provenance_created
                    }));
                }
            }
            _ = catalogs.tick(), if provider.is_some() => {
                refresh_catalog(
                    provider.as_ref().expect("guard requires a provider"),
                    &materializer,
                    &mut catalog,
                    &mut catalog_etag
                ).await;
            }
        }
    }

    inbox_for_shutdown.close().await;
    store_for_shutdown.close().await;
    Ok(())
}

fn materialization_health_json(
    health: &MaterializerHealth,
    observed_at: chrono::DateTime<Utc>,
) -> Value {
    let observed_at_unix_nano = u64::try_from(
        observed_at
            .timestamp_nanos_opt()
            .expect("current UTC time must fit Unix nanoseconds"),
    )
    .expect("current UTC time must follow the Unix epoch");
    let evidence_freshness_milliseconds = health
        .evidence_store
        .newest_observed_at_unix_nano
        .map(|newest| observed_at_unix_nano.saturating_sub(newest) / 1_000_000);
    let oldest_pending_age_milliseconds = health
        .oldest_pending_received_at
        .map(|oldest| u64::try_from((observed_at - oldest).num_milliseconds()).unwrap_or(0));

    json!({
        "checkpointBatchId": health.checkpoint_batch_id,
        "evidenceFreshnessMilliseconds": evidence_freshness_milliseconds,
        "newestEvidenceObservedAtUnixNano": health
            .evidence_store
            .newest_observed_at_unix_nano
            .map(|value| value.to_string()),
        "newestPendingReceivedAt": health
            .newest_pending_received_at
            .map(|value| value.to_rfc3339()),
        "observedAt": observed_at.to_rfc3339(),
        "oldestPendingAgeMilliseconds": oldest_pending_age_milliseconds,
        "oldestPendingReceivedAt": health
            .oldest_pending_received_at
            .map(|value| value.to_rfc3339()),
        "pendingBatchCount": health.pending_batch_count
    })
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
    match provider.fetch(current_etag.as_deref()).await {
        Ok(CatalogFetch::NotModified) => {}
        Ok(CatalogFetch::Updated { catalog, etag }) => {
            match materializer
                .activate_catalog(&catalog, etag.clone(), Utc::now())
                .await
            {
                Ok(()) => {
                    println!(
                        "{}",
                        json!({
                            "activeRoutes": catalog.active_source_counts().len(),
                            "currentContracts": catalog.current_contracts().len(),
                            "event": "materializer.catalog_activated"
                        })
                    );
                    *current = catalog;
                    *current_etag = Some(etag);
                }
                Err(error) => {
                    eprintln!(
                        "{}",
                        json!({
                            "error": error.to_string(),
                            "event": "materializer.catalog_activation_failed"
                        })
                    );
                }
            }
        }
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "error": error.to_string(),
                    "event": "materializer.catalog_refresh_failed",
                    "retainedCatalog": current_etag.is_some()
                })
            );
        }
    }
}
