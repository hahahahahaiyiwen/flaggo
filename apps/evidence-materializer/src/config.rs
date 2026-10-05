use std::{env, num::NonZeroU16, time::Duration};

pub const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_DATABASE_URL";
pub const CATALOG_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_CONTRACT_CATALOG_URL";
pub const POLL_INTERVAL_MILLISECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_MATERIALIZER_POLL_INTERVAL_MS";
pub const CATALOG_INTERVAL_SECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_MATERIALIZER_CATALOG_INTERVAL_SECONDS";

const DEFAULT_DATABASE_URL: &str = "sqlite://flaggo.db";
const DEFAULT_POLL_INTERVAL_MILLISECONDS: u64 = 250;
const DEFAULT_CATALOG_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_READ_LIMIT: u16 = 100;

pub struct MaterializerConfig {
    pub database_url: String,
    pub catalog_url: Option<String>,
    pub poll_interval: Duration,
    pub catalog_interval: Duration,
    pub read_limit: NonZeroU16,
}

impl MaterializerConfig {
    pub fn from_environment() -> Result<Self, String> {
        let database_url =
            nonempty_or_default(DATABASE_URL_ENVIRONMENT_VARIABLE, DEFAULT_DATABASE_URL)?;
        let catalog_url = optional_nonempty(CATALOG_URL_ENVIRONMENT_VARIABLE)?;
        let poll_interval = parse_duration(
            POLL_INTERVAL_MILLISECONDS_ENVIRONMENT_VARIABLE,
            DEFAULT_POLL_INTERVAL_MILLISECONDS,
            Duration::from_millis,
        )?;
        let catalog_interval = parse_duration(
            CATALOG_INTERVAL_SECONDS_ENVIRONMENT_VARIABLE,
            DEFAULT_CATALOG_INTERVAL_SECONDS,
            Duration::from_secs,
        )?;

        Ok(Self {
            database_url,
            catalog_url,
            poll_interval,
            catalog_interval,
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
