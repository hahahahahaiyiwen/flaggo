pub mod digests;
mod json_value;
pub mod models;
pub mod strict_json;
pub mod validation;
pub mod value_schema;

pub use json_value::JsonValue;
pub use models::*;

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("JSON contains invalid Unicode: {0}")]
    InvalidUnicode(#[source] serde_json::Error),
    #[error("{0}")]
    InvalidModel(String),
    #[error("expression canonicalization failed: {0}")]
    Expression(String),
}
