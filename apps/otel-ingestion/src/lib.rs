mod config;

pub use config::{
    DATABASE_URL_ENVIRONMENT_VARIABLE, DEFAULT_DATABASE_URL, DEFAULT_LISTEN_ADDRESS,
    INBOX_HARD_RETENTION_SECONDS_ENVIRONMENT_VARIABLE,
    INBOX_MAX_PAYLOAD_BYTES_ENVIRONMENT_VARIABLE, LISTEN_ADDRESS_ENVIRONMENT_VARIABLE,
    MAXIMUM_DECOMPRESSED_REQUEST_BYTES_ENVIRONMENT_VARIABLE, configured_database_url,
    configured_inbox_limits, configured_listen_address, configured_receiver, parse_inbox_limits,
    parse_listen_address, parse_receiver_config,
};
pub use flaggo_otlp_http::{
    DEFAULT_MAXIMUM_DECOMPRESSED_REQUEST_BYTES, INSTRUMENTATION_SCOPE, OtlpIngestionObservability,
    OtlpReceiverConfig, SERVICE_NAME, instrumented_router, router,
};
