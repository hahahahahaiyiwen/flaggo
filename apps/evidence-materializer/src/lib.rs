mod builtin;
mod candidate;
mod catalog;
mod decoder;
mod materializer;
mod projector;
mod provider;
mod route;

pub use catalog::{CompiledContractCatalog, ContractCatalogError};
pub use decoder::{DecodedBatch, DecoderDiagnostic, decode_batch};
pub use materializer::{
    EvidenceMaterializer, MaterializerError, MaterializerHealth, MaterializerRunResult,
};
pub use provider::{CatalogFetch, CatalogProviderError, HttpContractCatalogProvider};
pub use route::{CurrentContractKey, MaterializationRoute};

pub const MATERIALIZER_VERSION: &str = "2";
pub const DECODER_VERSION: &str = "3";
pub const IDENTITY_VERSION: &str = "4";
pub const PROJECTION_VERSION: &str = "2";
pub const ROUTING_VERSION: &str = "1";
