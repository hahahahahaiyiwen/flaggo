use std::{env, num::NonZeroU16, time::Duration};

use flaggo_evidence_store::DecisionScope;

pub const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_DATABASE_URL";
pub const APPLICATION_ENVIRONMENT_VARIABLE: &str = "FLAGGO_APPLICATION";
pub const ENVIRONMENT_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ENVIRONMENT";
pub const SNAPSHOT_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_CONTRACT_SNAPSHOT_URL";
pub const SNAPSHOT_BEARER_TOKEN_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_CONTRACT_SNAPSHOT_BEARER_TOKEN";
pub const POLL_INTERVAL_MILLISECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_MATERIALIZER_POLL_INTERVAL_MS";
pub const SNAPSHOT_INTERVAL_SECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_MATERIALIZER_SNAPSHOT_INTERVAL_SECONDS";

const DEFAULT_DATABASE_URL: &str = "sqlite://flaggo.db";
const DEFAULT_APPLICATION: &str = "local-application";
const DEFAULT_ENVIRONMENT: &str = "development";
const DEFAULT_POLL_INTERVAL_MILLISECONDS: u64 = 250;
const DEFAULT_SNAPSHOT_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_READ_LIMIT: u16 = 100;

pub struct MaterializerConfig {
    pub database_url: String,
    pub selector_scope: DecisionScope,
    pub snapshot_url: Option<String>,
    pub snapshot_bearer_token: Option<String>,
    pub poll_interval: Duration,
    pub snapshot_interval: Duration,
    pub read_limit: NonZeroU16,
}

impl MaterializerConfig {
    pub fn from_environment() -> Result<Self, String> {
        let database_url =
            nonempty_or_default(DATABASE_URL_ENVIRONMENT_VARIABLE, DEFAULT_DATABASE_URL)?;
        let application =
            nonempty_or_default(APPLICATION_ENVIRONMENT_VARIABLE, DEFAULT_APPLICATION)?;
        let environment =
            nonempty_or_default(ENVIRONMENT_ENVIRONMENT_VARIABLE, DEFAULT_ENVIRONMENT)?;
        let snapshot_url = optional_nonempty(SNAPSHOT_URL_ENVIRONMENT_VARIABLE)?;
        let snapshot_bearer_token = optional_nonempty(SNAPSHOT_BEARER_TOKEN_ENVIRONMENT_VARIABLE)?;
        if snapshot_bearer_token.is_some() && snapshot_url.is_none() {
            return Err(format!(
                "{SNAPSHOT_BEARER_TOKEN_ENVIRONMENT_VARIABLE} requires {SNAPSHOT_URL_ENVIRONMENT_VARIABLE}"
            ));
        }
        let poll_interval = parse_duration(
            POLL_INTERVAL_MILLISECONDS_ENVIRONMENT_VARIABLE,
            DEFAULT_POLL_INTERVAL_MILLISECONDS,
            Duration::from_millis,
        )?;
        let snapshot_interval = parse_duration(
            SNAPSHOT_INTERVAL_SECONDS_ENVIRONMENT_VARIABLE,
            DEFAULT_SNAPSHOT_INTERVAL_SECONDS,
            Duration::from_secs,
        )?;

        Ok(Self {
            database_url,
            selector_scope: DecisionScope::new(application, environment)
                .map_err(|error| error.to_string())?,
            snapshot_url,
            snapshot_bearer_token,
            poll_interval,
            snapshot_interval,
            read_limit: NonZeroU16::new(DEFAULT_READ_LIMIT)
                .expect("default read limit must be nonzero"),
        })
    }
}

fn nonempty_or_default(name: &str, default: &str) -> Result<String, String> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => Err(format!("{name} must not be empty")),
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => Err(format!("{name} is not valid Unicode: {error}")),
    }
}

fn optional_nonempty(name: &str) -> Result<Option<String>, String> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => {
            Err(format!("{name} must not be empty when configured"))
        }
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(format!("{name} is not valid Unicode: {error}")),
    }
}

fn parse_duration(
    name: &str,
    default: u64,
    convert: impl FnOnce(u64) -> Duration,
) -> Result<Duration, String> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|error| format!("{name} must be a positive integer: {error}"))?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(format!("{name} is not valid Unicode: {error}")),
    };
    if value == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(convert(value))
}
