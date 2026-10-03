use std::num::{NonZeroU16, NonZeroU64};

use flaggo_evidence_store::{
    EvidenceStore, EvidenceStoreCommitResult, EvidenceStoreError, ForwardMaterializationKey,
    MaterializerVersions, StoredContractCatalog,
};
use flaggo_raw_otlp_inbox::{InboxBatchId, RawOtlpInbox, RawOtlpInboxError};
use thiserror::Error;

use crate::{
    CompiledContractCatalog, DECODER_VERSION, IDENTITY_VERSION, MATERIALIZER_VERSION,
    PROJECTION_VERSION, ROUTING_VERSION,
    projector::{ProjectionRoutes, project_batch},
};

pub struct EvidenceMaterializer<I, S> {
    inbox: I,
    store: S,
    read_limit: NonZeroU16,
    versions: MaterializerVersions,
}

impl<I, S> EvidenceMaterializer<I, S>
where
    I: RawOtlpInbox,
    S: EvidenceStore,
{
    pub fn new(inbox: I, store: S, read_limit: NonZeroU16) -> Self {
        Self {
            inbox,
            store,
            read_limit,
            versions: MaterializerVersions {
                materializer: MATERIALIZER_VERSION.to_owned(),
                decoder: DECODER_VERSION.to_owned(),
                identity: IDENTITY_VERSION.to_owned(),
                projection: PROJECTION_VERSION.to_owned(),
                routing: ROUTING_VERSION.to_owned(),
            },
        }
    }

    pub async fn activate_catalog(
        &self,
        current: &CompiledContractCatalog,
        next: &CompiledContractCatalog,
        etag: String,
        fetched_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<CatalogActivationResult, MaterializerError> {
        let newly_active_routes = next.newly_active_routes(current);
        let mut replay = MaterializerRunResult::default();
        if !newly_active_routes.is_empty() {
            let mut after = None;
            loop {
                let batches = self.inbox.read_after(after, self.read_limit).await?;
                if batches.is_empty() {
                    break;
                }
                replay.batches_read +=
                    u64::try_from(batches.len()).expect("batch page length must fit in u64");
                for batch in batches {
                    after = Some(batch.inbox_batch_id);
                    let commit = project_batch(
                        &batch,
                        ProjectionRoutes::Replay(&newly_active_routes),
                        &self.versions,
                    );
                    replay.add_commit(self.store.commit(commit).await?);
                }
            }
        }
        self.store
            .save_catalog(StoredContractCatalog {
                etag,
                payload: next.payload().to_vec(),
                fetched_at,
            })
            .await?;
        Ok(CatalogActivationResult {
            routes_activated: u64::try_from(newly_active_routes.len())
                .expect("route count must fit in u64"),
            replay,
        })
    }

    pub async fn run_once(
        &self,
        catalog: &CompiledContractCatalog,
    ) -> Result<MaterializerRunResult, MaterializerError> {
        let key = ForwardMaterializationKey {
            versions: self.versions.clone(),
        };
        let checkpoint = self.store.forward_checkpoint(&key).await?;
        let after = checkpoint.and_then(NonZeroU64::new).map(InboxBatchId::new);
        let batches = self.inbox.read_after(after, self.read_limit).await?;
        let mut result = MaterializerRunResult {
            batches_read: u64::try_from(batches.len()).expect("batch page length must fit in u64"),
            ..MaterializerRunResult::default()
        };
        for batch in batches {
            let commit = project_batch(&batch, ProjectionRoutes::Forward(catalog), &self.versions);
            result.add_commit(self.store.commit(commit).await?);
        }
        Ok(result)
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CatalogActivationResult {
    pub routes_activated: u64,
    pub replay: MaterializerRunResult,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaterializerRunResult {
    pub batches_read: u64,
    pub observations_created: u64,
    pub duplicate_observations: u64,
    pub provenance_created: u64,
    pub diagnostics_created: u64,
    pub conflicts_created: u64,
}

impl MaterializerRunResult {
    fn add_commit(&mut self, commit: EvidenceStoreCommitResult) {
        self.observations_created += commit.observations_created;
        self.duplicate_observations += commit.duplicate_observations;
        self.provenance_created += commit.provenance_created;
        self.diagnostics_created += commit.diagnostics_created;
        self.conflicts_created += commit.conflicts_created;
    }
}

#[derive(Debug, Error)]
pub enum MaterializerError {
    #[error("raw OTLP inbox failure: {0}")]
    Inbox(#[from] RawOtlpInboxError),
    #[error("evidence store failure: {0}")]
    Store(#[from] EvidenceStoreError),
}
