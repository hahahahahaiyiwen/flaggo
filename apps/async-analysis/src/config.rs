use std::{
    env,
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
    time::Duration,
};

pub const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_DATABASE_URL";
pub const CATALOG_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_CONTRACT_CATALOG_URL";
pub const CONTRACT_SERVICE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_CONTRACT_SERVICE_URL";
pub const WORKSPACE_ROOT_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_WORKSPACE_ROOT";
pub const COPILOT_HOME_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_COPILOT_HOME";
pub const MAX_AGENTS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_MAX_AGENTS";
pub const POLL_INTERVAL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_POLL_INTERVAL_MS";
pub const MODEL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_MODEL";
pub const GITHUB_TOKEN_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_GITHUB_TOKEN";
pub const LOG_SESSION_EVENTS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_LOG_SESSION_EVENTS";
pub const QUERY_MAX_ROWS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_QUERY_MAX_ROWS";
pub const QUERY_MAX_BYTES_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_QUERY_MAX_BYTES";
pub const QUERY_TIMEOUT_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_QUERY_TIMEOUT_MS";
pub const RUN_TIMEOUT_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS";
pub const YIELD_GRACE_ENVIRONMENT_VARIABLE: &str = "FLAGGO_ANALYSIS_YIELD_GRACE_SECONDS";

const DEFAULT_DATABASE_URL: &str = "sqlite://flaggo.db";
const DEFAULT_CONTRACT_SERVICE_URL: &str = "http://127.0.0.1:5000";
const DEFAULT_WORKSPACE_ROOT: &str = "./flaggo-analysis-workspaces";
const DEFAULT_COPILOT_HOME: &str = "./flaggo-analysis-copilot";
const DEFAULT_MAX_AGENTS: usize = 1;
const DEFAULT_POLL_INTERVAL_MILLISECONDS: u64 = 5_000;
const DEFAULT_QUERY_MAX_ROWS: u32 = 1_000;
const DEFAULT_QUERY_MAX_BYTES: usize = 1_048_576;
const DEFAULT_QUERY_TIMEOUT_MILLISECONDS: u64 = 10_000;
const DEFAULT_RUN_TIMEOUT_SECONDS: u64 = 1_200;
const DEFAULT_YIELD_GRACE_SECONDS: u64 = 30;

pub struct AnalysisServiceConfig {
    pub database_url: String,
    pub catalog_url: Option<String>,
    pub contract_service_url: String,
    pub workspace_root: PathBuf,
    pub copilot_home: PathBuf,
    pub skill_root: PathBuf,
    pub model: Option<String>,
    pub github_token: Option<String>,
    pub log_session_events: bool,
    pub max_agents: NonZeroUsize,
    pub poll_interval: Duration,
    pub query_max_rows: NonZeroU32,
    pub query_max_bytes: usize,
    pub query_timeout: Duration,
    pub run_timeout: Duration,
    pub yield_grace_period: Duration,
}

impl AnalysisServiceConfig {
    pub fn from_environment() -> Result<Self, String> {
        let model = optional_nonempty(MODEL_ENVIRONMENT_VARIABLE)?;
        let max_agents = NonZeroUsize::new(parse_integer(
            MAX_AGENTS_ENVIRONMENT_VARIABLE,
            DEFAULT_MAX_AGENTS,
        )?)
        .ok_or_else(|| format!("{MAX_AGENTS_ENVIRONMENT_VARIABLE} must be greater than zero"))?;
        let query_max_rows = NonZeroU32::new(parse_integer(
            QUERY_MAX_ROWS_ENVIRONMENT_VARIABLE,
            DEFAULT_QUERY_MAX_ROWS,
        )?)
        .ok_or_else(|| {
            format!("{QUERY_MAX_ROWS_ENVIRONMENT_VARIABLE} must be greater than zero")
        })?;
        let query_max_bytes = parse_integer(
            QUERY_MAX_BYTES_ENVIRONMENT_VARIABLE,
            DEFAULT_QUERY_MAX_BYTES,
        )?;
        if query_max_bytes == 0 {
            return Err(format!(
                "{QUERY_MAX_BYTES_ENVIRONMENT_VARIABLE} must be greater than zero"
            ));
        }

        let skill_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills");
        if !skill_root.is_dir() {
            return Err(format!(
                "analysis skill directory does not exist: {}",
                skill_root.display()
            ));
        }
        Ok(Self {
            database_url: nonempty_or_default(
                DATABASE_URL_ENVIRONMENT_VARIABLE,
                DEFAULT_DATABASE_URL,
            )?,
            catalog_url: optional_nonempty(CATALOG_URL_ENVIRONMENT_VARIABLE)?,
            contract_service_url: nonempty_or_default(
                CONTRACT_SERVICE_URL_ENVIRONMENT_VARIABLE,
                DEFAULT_CONTRACT_SERVICE_URL,
            )?,
            workspace_root: PathBuf::from(nonempty_or_default(
                WORKSPACE_ROOT_ENVIRONMENT_VARIABLE,
                DEFAULT_WORKSPACE_ROOT,
            )?),
            copilot_home: PathBuf::from(nonempty_or_default(
                COPILOT_HOME_ENVIRONMENT_VARIABLE,
                DEFAULT_COPILOT_HOME,
            )?),
            skill_root,
            model,
            github_token: optional_nonempty(GITHUB_TOKEN_ENVIRONMENT_VARIABLE)?,
            log_session_events: parse_bool(LOG_SESSION_EVENTS_ENVIRONMENT_VARIABLE, false)?,
            max_agents,
            poll_interval: duration_millis(
                POLL_INTERVAL_ENVIRONMENT_VARIABLE,
                DEFAULT_POLL_INTERVAL_MILLISECONDS,
            )?,
            query_max_rows,
            query_max_bytes,
            query_timeout: duration_millis(
                QUERY_TIMEOUT_ENVIRONMENT_VARIABLE,
                DEFAULT_QUERY_TIMEOUT_MILLISECONDS,
            )?,
            run_timeout: duration_seconds(
                RUN_TIMEOUT_ENVIRONMENT_VARIABLE,
                DEFAULT_RUN_TIMEOUT_SECONDS,
            )?,
            yield_grace_period: duration_seconds(
                YIELD_GRACE_ENVIRONMENT_VARIABLE,
                DEFAULT_YIELD_GRACE_SECONDS,
            )?,
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

fn parse_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| format!("{name} must be true or false: {error}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(format!("{name} is not valid Unicode: {error}")),
    }
}

fn parse_integer<T>(name: &str, default: T) -> Result<T, String>
where
    T: std::str::FromStr + Copy,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| format!("{name} must be a positive integer: {error}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(format!("{name} is not valid Unicode: {error}")),
    }
}

fn duration_millis(name: &str, default: u64) -> Result<Duration, String> {
    let value = parse_integer(name, default)?;
    if value == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(Duration::from_millis(value))
}

fn duration_seconds(name: &str, default: u64) -> Result<Duration, String> {
    let value = parse_integer(name, default)?;
    if value == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(Duration::from_secs(value))
}
