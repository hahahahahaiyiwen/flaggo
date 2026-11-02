use chrono::{DateTime, FixedOffset};
use flaggo_contract::{DecisionContract, DecisionExecutable, JsonValue};
use flaggo_contract_store::timestamp;
use serde::{Serialize, Serializer};

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractVersion {
    pub name: String,
    pub contract_digest: String,
    pub status: &'static str,
    pub accepted_at: String,
    pub active_executable_digest: String,
    pub contract: DecisionContract,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub contract_digest: String,
    pub status: &'static str,
    pub accepted_at: String,
    pub active_executable_digest: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    pub name: String,
    pub current_contract_digest: String,
    pub versions: Vec<VersionSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CurrentCatalog {
    pub contracts: Vec<CurrentCatalogEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCatalogEntry {
    pub contract_digest: String,
    pub contract: DecisionContract,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateResult {
    pub contract_name: String,
    pub contract_digest: String,
    pub executable_digest: String,
    pub lifecycle_state: &'static str,
    #[serde(with = "timestamp")]
    pub created_at: DateTime<FixedOffset>,
    pub created: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedExecutable {
    pub contract_digest: String,
    pub executable_digest: String,
    pub lifecycle_state: &'static str,
    pub state_version: i64,
    #[serde(with = "timestamp")]
    pub created_at: DateTime<FixedOffset>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_time"
    )]
    pub activated_at: Option<DateTime<FixedOffset>>,
    pub executable: DecisionExecutable,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<JsonValue>,
}

fn optional_time<S: Serializer>(
    value: &Option<DateTime<FixedOffset>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => timestamp::serialize(value, serializer),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableCatalog {
    pub contract_name: String,
    pub contract_digest: String,
    pub active_executable: ManagedExecutable,
    pub candidates: Vec<ManagedExecutable>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableHistory {
    pub contract_name: String,
    pub contract_digest: String,
    pub executables: Vec<ManagedExecutable>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ExecutableListing {
    Catalog(Box<ExecutableCatalog>),
    History(ExecutableHistory),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationResult {
    pub contract_name: String,
    pub contract_digest: String,
    pub executable: ManagedExecutable,
    pub previous_executable_digest: String,
    pub superseded_candidate_count: u64,
}
