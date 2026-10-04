use std::{env, net::SocketAddr, time::Duration};

use flaggo_raw_otlp_inbox::RawOtlpInboxLimits;

pub const DATABASE_URL_ENVIRONMENT_VARIABLE: &str = "FLAGGO_DATABASE_URL";
pub const DEFAULT_DATABASE_URL: &str = "sqlite://flaggo.db";
pub const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:5090";
pub const DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES: usize = 64 * 1024 * 1024;
pub const INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_OTLP_INBOX_HARD_RETENTION_SECONDS";
pub const INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_OTLP_INBOX_MAX_PAYLOAD_BYTES";
pub const LISTEN_ADDRESS_ENVIRONMENT_VARIABLE: &str = "FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS";
pub const MAXIMUM_DECOMPRESSED_REQUEST_BYTES_ENVIRONMENT_VARIABLE: &str =
    "FLAGGO_OTLP_MAXIMUM_DECOMPRESSED_REQUEST_BYTES";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OtlpReceiverConfig {
    maximum_decompressed_request_bytes: usize,
}

impl OtlpReceiverConfig {
    pub fn new(maximum_decompressed_request_bytes: usize) -> Result<Self, String> {
        if maximum_decompressed_request_bytes == 0
            || maximum_decompressed_request_bytes == usize::MAX
        {
            return Err(
                "maximum decompressed OTLP request bytes must be between 1 and usize::MAX - 1"
                    .to_owned(),
            );
        }
        Ok(Self {
            maximum_decompressed_request_bytes,
        })
    }

    #[must_use]
    pub const fn maximum_decompressed_request_bytes(self) -> usize {
        self.maximum_decompressed_request_bytes
    }
}

impl Default for OtlpReceiverConfig {
    fn default() -> Self {
        Self {
            maximum_decompressed_request_bytes: DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES,
        }
    }
}

pub fn configured_listen_address() -> Result<SocketAddr, String> {
    let value = env::var(LISTEN_ADDRESS_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_LISTEN_ADDRESS.to_owned());
    parse_listen_address(&value)
}

pub fn configured_database_url() -> Result<String, String> {
    let value = env::var(DATABASE_URL_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
    if value.trim().is_empty() {
        return Err(format!(
            "{DATABASE_URL_ENVIRONMENT_VARIABLE} must not be empty"
        ));
    }
    Ok(value)
}

pub fn configured_inbox_limits() -> Result<RawOtlpInboxLimits, String> {
    let defaults = RawOtlpInboxLimits::default();
    let maximum = env::var(INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| defaults.max_retained_payload_bytes().to_string());
    let retention = env::var(INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| defaults.hard_retention().as_secs().to_string());
    parse_inbox_limits(&maximum, &retention)
}

pub fn configured_receiver() -> Result<OtlpReceiverConfig, String> {
    let maximum = env::var(MAXIMUM_DECOMPRESSED_REQUEST_BYTES_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES.to_string());
    parse_receiver_config(&maximum)
}

pub fn parse_listen_address(value: &str) -> Result<SocketAddr, String> {
    value.parse::<SocketAddr>().map_err(|error| {
        format!("{LISTEN_ADDRESS_ENVIRONMENT_VARIABLE} must be an IP socket address: {error}")
    })
}

pub fn parse_inbox_limits(
    maximum_payload_bytes: &str,
    hard_retention_seconds: &str,
) -> Result<RawOtlpInboxLimits, String> {
    let maximum_payload_bytes = maximum_payload_bytes.parse::<u64>().map_err(|error| {
        format!("{INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE} must be an integer: {error}")
    })?;
    let hard_retention_seconds = hard_retention_seconds.parse::<u64>().map_err(|error| {
        format!("{INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE} must be an integer: {error}")
    })?;
    RawOtlpInboxLimits::new(
        maximum_payload_bytes,
        Duration::from_secs(hard_retention_seconds),
    )
    .map_err(|error| format!("invalid raw OTLP inbox limits: {error}"))
}

pub fn parse_receiver_config(
    maximum_decompressed_request_bytes: &str,
) -> Result<OtlpReceiverConfig, String> {
    let maximum = maximum_decompressed_request_bytes
        .parse::<usize>()
        .map_err(|error| {
            format!(
                "{MAXIMUM_DECOMPRESSED_REQUEST_BYTES_ENVIRONMENT_VARIABLE} must be an integer: {error}"
            )
        })?;
    OtlpReceiverConfig::new(maximum)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES, OtlpReceiverConfig, parse_inbox_limits,
        parse_listen_address, parse_receiver_config,
    };

    #[test]
    fn parses_ip_socket_listen_addresses() {
        let address = parse_listen_address("127.0.0.1:5090").expect("valid address");

        assert_eq!(address.ip().to_string(), "127.0.0.1");
        assert_eq!(address.port(), 5090);
    }

    #[test]
    fn rejects_non_socket_listen_addresses() {
        let error = parse_listen_address("http://127.0.0.1:5090")
            .expect_err("URL must not be accepted as a socket address");

        assert!(error.contains("must be an IP socket address"));
    }

    #[test]
    fn parses_and_validates_inbox_limits() {
        let limits = parse_inbox_limits("2048", "60").expect("valid limits");
        assert_eq!(limits.max_retained_payload_bytes(), 2048);
        assert_eq!(limits.hard_retention().as_secs(), 60);

        assert!(parse_inbox_limits("0", "60").is_err());
        assert!(parse_inbox_limits("2048", "0").is_err());
        assert!(parse_inbox_limits("many", "60").is_err());
    }

    #[test]
    fn parses_and_validates_receiver_limit() {
        assert_eq!(DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES, 67_108_864);
        assert_eq!(
            OtlpReceiverConfig::default().maximum_decompressed_request_bytes(),
            67_108_864
        );

        let config = parse_receiver_config("4096").expect("valid receiver limit");
        assert_eq!(config.maximum_decompressed_request_bytes(), 4096);

        assert!(parse_receiver_config("0").is_err());
        assert!(parse_receiver_config("many").is_err());
    }
}
