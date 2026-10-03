mod config;

use std::error::Error;

use chrono::Utc;
use config::MaterializerConfig;
use flaggo_evidence_materializer::{
    CatalogFetch, CompiledContractCatalog, EvidenceMaterializer, HttpContractCatalogProvider,
};
use flaggo_evidence_store::{EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::SqliteRawOtlpInbox;
use serde_json::json;
use tokio::{
    signal,
    time::{MissedTickBehavior, interval},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
        .map(|url| HttpContractCatalogProvider::new(url, config.catalog_bearer_token.clone()))
        .transpose()?;
    if let Some(provider) = &provider {
        refresh_catalog(provider, &materializer, &mut catalog, &mut catalog_etag).await;
    }
    let evidence_store = store.inspect().await?;

    println!(
        "{}",
        json!({
            "activeRoutes": catalog.active_source_counts().len(),
            "currentContracts": catalog.current_contracts().len(),
            "evidenceStore": {
                "conflictCount": evidence_store.conflict_count,
                "diagnosticCount": evidence_store.diagnostic_count,
                "hasCachedCatalog": evidence_store.has_cached_catalog,
                "observationCount": evidence_store.observation_count,
                "provenanceCount": evidence_store.provenance_count
            },
            "event": "materializer.started"
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
                    println!("{}", json!({
                        "batchesRead": result.batches_read,
                        "conflictsCreated": result.conflicts_created,
                        "diagnosticsCreated": result.diagnostics_created,
                        "duplicateObservations": result.duplicate_observations,
                        "event": "materializer.batch_page_committed",
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
