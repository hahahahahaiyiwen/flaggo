mod builtin;
mod candidate;
mod decoder;
mod materializer;
mod projector;
mod selector;
mod snapshot;

pub use decoder::{DecodedBatch, DecoderDiagnostic, decode_batch};
pub use materializer::{EvidenceMaterializer, MaterializerError, MaterializerRunResult};
pub use selector::{CompiledSelectorSnapshot, SelectorSnapshotError};
pub use snapshot::{HttpSelectorSnapshotProvider, SnapshotFetch, SnapshotProviderError};

pub const MATERIALIZER_VERSION: &str = "1";
pub const DECODER_VERSION: &str = "1";
pub const IDENTITY_VERSION: &str = "1";
pub const PROJECTION_VERSION: &str = "1";
pub const SELECTOR_PROTOCOL_VERSION: &str = "1";
