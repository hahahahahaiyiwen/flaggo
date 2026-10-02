use std::num::{NonZeroU16, NonZeroU64};

use flaggo_evidence_store::{
    DecisionScope, EvidenceStore, EvidenceStoreCommitResult, EvidenceStoreError,
    MaterializationKey, MaterializerVersions, StoredSelectorSnapshot,
};
use flaggo_raw_otlp_inbox::{InboxBatchId, RawOtlpInbox, RawOtlpInboxError};
use thiserror::Error;

use crate::{
    CompiledSelectorSnapshot, DECODER_VERSION, IDENTITY_VERSION, MATERIALIZER_VERSION,
    PROJECTION_VERSION, SELECTOR_PROTOCOL_VERSION, projector::project_batch,
};

pub struct EvidenceMaterializer<I, S> {
    inbox: I,
    store: S,
    selector_scope: DecisionScope,
    read_limit: NonZeroU16,
    versions: MaterializerVersions,
}

impl<I, S> EvidenceMaterializer<I, S>
where
    I: RawOtlpInbox,
    S: EvidenceStore,
{
    pub fn new(inbox: I, store: S, selector_scope: DecisionScope, read_limit: NonZeroU16) -> Self {
        Self {
            inbox,
            store,
            selector_scope,
            read_limit,
            versions: MaterializerVersions {
                materializer: MATERIALIZER_VERSION.to_owned(),
                decoder: DECODER_VERSION.to_owned(),
                identity: IDENTITY_VERSION.to_owned(),
                projection: PROJECTION_VERSION.to_owned(),
                selector_protocol: SELECTOR_PROTOCOL_VERSION.to_owned(),
            },
        }
    }

    pub async fn activate_snapshot(
        &self,
        snapshot: &CompiledSelectorSnapshot,
        fetched_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), MaterializerError> {
        self.store
            .activate_snapshot(StoredSelectorSnapshot {
                snapshot_digest: snapshot.snapshot_digest.clone(),
                payload: snapshot.payload.clone(),
                fetched_at,
            })
            .await?;
        Ok(())
    }

    pub async fn run_once(
        &self,
        snapshot: &CompiledSelectorSnapshot,
    ) -> Result<MaterializerRunResult, MaterializerError> {
        let key = MaterializationKey {
            snapshot_digest: snapshot.snapshot_digest.clone(),
            versions: self.versions.clone(),
        };
        let checkpoint = self.store.checkpoint(&key).await?;
        let after = checkpoint.and_then(NonZeroU64::new).map(InboxBatchId::new);
        let batches = self.inbox.read_after(after, self.read_limit).await?;
        let mut result = MaterializerRunResult {
            batches_read: u64::try_from(batches.len()).expect("batch page length must fit in u64"),
            ..MaterializerRunResult::default()
        };
        for batch in batches {
            let commit = project_batch(&batch, snapshot, &self.selector_scope, &self.versions);
            let committed = self.store.commit(commit).await?;
            result.add_commit(committed);
        }
        Ok(result)
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaterializerRunResult {
    pub batches_read: u64,
    pub observations_created: u64,
    pub duplicate_observations: u64,
    pub provenance_created: u64,
    pub associations_created: u64,
    pub diagnostics_created: u64,
    pub conflicts_created: u64,
}

impl MaterializerRunResult {
    fn add_commit(&mut self, commit: EvidenceStoreCommitResult) {
        self.observations_created += commit.observations_created;
        self.duplicate_observations += commit.duplicate_observations;
        self.provenance_created += commit.provenance_created;
        self.associations_created += commit.associations_created;
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
