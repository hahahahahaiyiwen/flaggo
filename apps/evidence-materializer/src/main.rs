mod config;

use std::error::Error;

use chrono::Utc;
use config::MaterializerConfig;
use flaggo_evidence_materializer::{
    CompiledSelectorSnapshot, EvidenceMaterializer, HttpSelectorSnapshotProvider, SnapshotFetch,
};
use flaggo_evidence_store::{EvidenceStore, SqliteEvidenceStore};
use flaggo_raw_otlp_inbox::{RawOtlpInboxLimits, SqliteRawOtlpInbox};
use serde_json::json;
use tokio::{
    signal,
    time::{MissedTickBehavior, interval},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = MaterializerConfig::from_environment()?;
    let inbox =
        SqliteRawOtlpInbox::connect(&config.database_url, RawOtlpInboxLimits::default()).await?;
    let store = SqliteEvidenceStore::connect(&config.database_url).await?;
    let inbox_for_shutdown = inbox.clone();
    let store_for_shutdown = store.clone();
    let materializer = EvidenceMaterializer::new(
        inbox,
        store.clone(),
        config.selector_scope.clone(),
        config.read_limit,
    );
    let mut snapshot = match store.load_active_snapshot().await? {
        Some(stored) => CompiledSelectorSnapshot::compile(stored.payload)?,
        None => {
            let snapshot = CompiledSelectorSnapshot::built_in_only();
            materializer
                .activate_snapshot(&snapshot, Utc::now())
                .await?;
            snapshot
        }
    };
    let provider = config
        .snapshot_url
        .as_deref()
        .map(|url| HttpSelectorSnapshotProvider::new(url, config.snapshot_bearer_token.clone()))
        .transpose()?;
    if let Some(provider) = &provider {
        refresh_snapshot(provider, &materializer, &mut snapshot).await;
    }

    println!(
        "{}",
        json!({
            "application": config.selector_scope.application,
            "environment": config.selector_scope.environment,
            "event": "materializer.started",
            "snapshotDigest": snapshot.snapshot_digest
        })
    );

    let mut work = interval(config.poll_interval);
    work.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut snapshots = interval(config.snapshot_interval);
    snapshots.set_missed_tick_behavior(MissedTickBehavior::Delay);
    snapshots.tick().await;
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
                let result = materializer.run_once(&snapshot).await?;
                if result.batches_read > 0 {
                    println!("{}", json!({
                        "associationsCreated": result.associations_created,
                        "batchesRead": result.batches_read,
                        "conflictsCreated": result.conflicts_created,
                        "diagnosticsCreated": result.diagnostics_created,
                        "duplicateObservations": result.duplicate_observations,
                        "event": "materializer.batch_page_committed",
                        "observationsCreated": result.observations_created,
                        "provenanceCreated": result.provenance_created,
                        "snapshotDigest": snapshot.snapshot_digest
                    }));
                }
            }
            _ = snapshots.tick(), if provider.is_some() => {
                refresh_snapshot(
                    provider.as_ref().expect("guard requires a provider"),
                    &materializer,
                    &mut snapshot
                ).await;
            }
        }
    }

    inbox_for_shutdown.close().await;
    store_for_shutdown.close().await;
    Ok(())
}

async fn refresh_snapshot<I, S>(
    provider: &HttpSelectorSnapshotProvider,
    materializer: &EvidenceMaterializer<I, S>,
    current: &mut CompiledSelectorSnapshot,
) where
    I: flaggo_raw_otlp_inbox::RawOtlpInbox,
    S: EvidenceStore,
{
    match provider.fetch(&current.snapshot_digest).await {
        Ok(SnapshotFetch::NotModified) => {}
        Ok(SnapshotFetch::Updated(snapshot)) => {
            if let Err(error) = materializer.activate_snapshot(&snapshot, Utc::now()).await {
                eprintln!(
                    "{}",
                    json!({
                        "error": error.to_string(),
                        "event": "materializer.snapshot_activation_failed"
                    })
                );
                return;
            }
            println!(
                "{}",
                json!({
                    "event": "materializer.snapshot_activated",
                    "snapshotDigest": snapshot.snapshot_digest
                })
            );
            *current = snapshot;
        }
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "error": error.to_string(),
                    "event": "materializer.snapshot_refresh_failed",
                    "retainedSnapshotDigest": current.snapshot_digest
                })
            );
        }
    }
}
