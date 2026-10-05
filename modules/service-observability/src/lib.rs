use std::{
    env,
    error::Error,
    fmt,
    net::{IpAddr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};

use opentelemetry::{
    InstrumentationScope, KeyValue, global, metrics::Meter, trace::TracerProvider as _,
};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    Resource,
    logs::SdkLoggerProvider,
    metrics::{PeriodicReader, SdkMeterProvider},
    propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use url::Url;

const SIGNALS: [&str; 3] = ["TRACES", "METRICS", "LOGS"];
const FORBIDDEN_RESOURCE_ATTRIBUTES: [&str; 3] =
    ["flaggo.tenant", "flaggo.application", "flaggo.environment"];

pub struct ServiceObservability {
    meter_provider: SdkMeterProvider,
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl ServiceObservability {
    pub fn initialize(
        service_name: &'static str,
        instrumentation_scope: &'static str,
        service_version: &'static str,
    ) -> Result<Self, ObservabilityError> {
        validate_exporter_configuration()?;
        validate_resource_attributes()?;

        global::set_text_map_propagator(TraceContextPropagator::new());
        let resource = Resource::builder()
            .with_service_name(service_name)
            .with_attributes([
                KeyValue::new("service.namespace", "flaggo"),
                KeyValue::new("service.version", service_version),
                KeyValue::new("service.instance.id", service_instance_id()),
            ])
            .build();
        let scope = InstrumentationScope::builder(instrumentation_scope)
            .with_version(service_version)
            .build();

        let tracer_provider = if exporter_configured("TRACES") {
            let exporter = SpanExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .build()
                .map_err(ObservabilityError::Exporter)?;
            Some(
                SdkTracerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_resource(resource.clone())
                    .build(),
            )
        } else {
            None
        };
        let tracer = tracer_provider
            .as_ref()
            .map(|provider| provider.tracer_with_scope(scope.clone()));

        let logger_provider = if exporter_configured("LOGS") {
            let exporter = LogExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .build()
                .map_err(ObservabilityError::Exporter)?;
            Some(
                SdkLoggerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_resource(resource.clone())
                    .build(),
            )
        } else {
            None
        };

        let meter_provider = if exporter_configured("METRICS") {
            let exporter = MetricExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .build()
                .map_err(ObservabilityError::Exporter)?;
            let reader = PeriodicReader::builder(exporter).build();
            SdkMeterProvider::builder()
                .with_reader(reader)
                .with_resource(resource)
                .build()
        } else {
            SdkMeterProvider::builder().with_resource(resource).build()
        };
        global::set_meter_provider(meter_provider.clone());

        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let console = tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_ansi(false)
            .with_target(true);
        let trace_layer = tracer.map(|tracer| {
            tracing_opentelemetry::layer()
                .with_tracer(tracer)
                .with_location(false)
        });
        let log_layer = logger_provider
            .as_ref()
            .map(OpenTelemetryTracingBridge::new);
        tracing_subscriber::registry()
            .with(filter)
            .with(console)
            .with(trace_layer)
            .with(log_layer)
            .try_init()
            .map_err(|error| ObservabilityError::Subscriber(error.to_string()))?;

        Ok(Self {
            meter_provider,
            tracer_provider,
            logger_provider,
        })
    }

    pub fn meter(instrumentation_scope: &'static str, service_version: &'static str) -> Meter {
        global::meter_provider().meter_with_scope(
            InstrumentationScope::builder(instrumentation_scope)
                .with_version(service_version)
                .build(),
        )
    }

    pub fn shutdown(self) -> Result<(), ObservabilityError> {
        let mut failures = Vec::new();
        if let Some(provider) = self.logger_provider
            && let Err(error) = provider.shutdown()
        {
            failures.push(format!("logs: {error}"));
        }
        if let Some(provider) = self.tracer_provider
            && let Err(error) = provider.shutdown()
        {
            failures.push(format!("traces: {error}"));
        }
        if let Err(error) = self.meter_provider.shutdown() {
            failures.push(format!("metrics: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ObservabilityError::Shutdown(failures.join("; ")))
        }
    }
}

pub fn reject_own_receiver(receiver: SocketAddr) -> Result<(), ObservabilityError> {
    for name in configured_endpoint_names() {
        let Some(value) = env::var_os(name) else {
            continue;
        };
        let value = value
            .into_string()
            .map_err(|_| ObservabilityError::InvalidEnvironment(name))?;
        if endpoint_targets_receiver(name, &value, receiver)? {
            return Err(ObservabilityError::RecursiveExport(name));
        }
    }
    Ok(())
}

fn validate_exporter_configuration() -> Result<(), ObservabilityError> {
    validate_protocol("OTEL_EXPORTER_OTLP_PROTOCOL")?;
    validate_endpoint("OTEL_EXPORTER_OTLP_ENDPOINT")?;
    for signal in SIGNALS {
        validate_protocol(signal_protocol_name(signal))?;
        validate_endpoint(signal_endpoint_name(signal))?;
    }
    Ok(())
}

fn validate_resource_attributes() -> Result<(), ObservabilityError> {
    let Some(value) = env::var_os("OTEL_RESOURCE_ATTRIBUTES") else {
        return Ok(());
    };
    let value = value
        .into_string()
        .map_err(|_| ObservabilityError::InvalidEnvironment("OTEL_RESOURCE_ATTRIBUTES"))?;
    if let Some(name) = forbidden_resource_attribute(&value) {
        return Err(ObservabilityError::ForbiddenResourceAttribute(
            name.to_owned(),
        ));
    }
    Ok(())
}

fn forbidden_resource_attribute(value: &str) -> Option<&str> {
    value.split(',').find_map(|attribute| {
        let name = attribute
            .split_once('=')
            .map_or(attribute, |(name, _)| name)
            .trim();
        FORBIDDEN_RESOURCE_ATTRIBUTES
            .contains(&name)
            .then_some(name)
    })
}

fn validate_protocol(name: &'static str) -> Result<(), ObservabilityError> {
    let Some(value) = env::var_os(name) else {
        return Ok(());
    };
    let value = value
        .into_string()
        .map_err(|_| ObservabilityError::InvalidEnvironment(name))?;
    if value.eq_ignore_ascii_case("http/protobuf") {
        Ok(())
    } else {
        Err(ObservabilityError::UnsupportedProtocol(name))
    }
}

fn validate_endpoint(name: &'static str) -> Result<(), ObservabilityError> {
    let Some(value) = env::var_os(name) else {
        return Ok(());
    };
    let value = value
        .into_string()
        .map_err(|_| ObservabilityError::InvalidEnvironment(name))?;
    parse_endpoint(name, &value).map(|_| ())
}

fn parse_endpoint(name: &'static str, value: &str) -> Result<Url, ObservabilityError> {
    let endpoint = Url::parse(value).map_err(|_| ObservabilityError::InvalidEndpoint(name))?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || endpoint.host_str().is_none()
    {
        return Err(ObservabilityError::InvalidEndpoint(name));
    }
    Ok(endpoint)
}

fn endpoint_targets_receiver(
    name: &'static str,
    value: &str,
    receiver: SocketAddr,
) -> Result<bool, ObservabilityError> {
    let endpoint = parse_endpoint(name, value)?;
    let endpoint_port = endpoint
        .port_or_known_default()
        .ok_or(ObservabilityError::InvalidEndpoint(name))?;
    if endpoint_port != receiver.port() {
        return Ok(false);
    }

    let host = endpoint.host_str().expect("validated endpoint has a host");
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(receiver.ip().is_loopback());
    }
    Ok(host
        .parse::<IpAddr>()
        .is_ok_and(|address| address == receiver.ip()))
}

fn exporter_configured(signal: &str) -> bool {
    env::var_os(signal_endpoint_name(signal)).is_some()
        || env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some()
}

fn configured_endpoint_names() -> impl Iterator<Item = &'static str> {
    [
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
    ]
    .into_iter()
}

const fn signal_endpoint_name(signal: &str) -> &'static str {
    match signal.as_bytes() {
        b"TRACES" => "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        b"METRICS" => "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        b"LOGS" => "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        _ => unreachable!(),
    }
}

const fn signal_protocol_name(signal: &str) -> &'static str {
    match signal.as_bytes() {
        b"TRACES" => "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
        b"METRICS" => "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
        b"LOGS" => "OTEL_EXPORTER_OTLP_LOGS_PROTOCOL",
        _ => unreachable!(),
    }
}

fn service_instance_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp:x}", std::process::id())
}

#[derive(Debug)]
pub enum ObservabilityError {
    Exporter(opentelemetry_otlp::ExporterBuildError),
    ForbiddenResourceAttribute(String),
    InvalidEndpoint(&'static str),
    InvalidEnvironment(&'static str),
    RecursiveExport(&'static str),
    Shutdown(String),
    Subscriber(String),
    UnsupportedProtocol(&'static str),
}

impl fmt::Display for ObservabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exporter(error) => {
                write!(formatter, "OTLP exporter configuration failed: {error}")
            }
            Self::ForbiddenResourceAttribute(name) => {
                write!(formatter, "OTEL_RESOURCE_ATTRIBUTES must not define {name}")
            }
            Self::InvalidEndpoint(name) => write!(
                formatter,
                "{name} must be an absolute HTTP(S) URL without credentials or a fragment"
            ),
            Self::InvalidEnvironment(name) => {
                write!(formatter, "{name} must contain valid Unicode")
            }
            Self::RecursiveExport(name) => write!(
                formatter,
                "{name} must not target the OTel Ingestion receiver itself"
            ),
            Self::Shutdown(error) => write!(formatter, "telemetry shutdown failed: {error}"),
            Self::Subscriber(error) => {
                write!(
                    formatter,
                    "telemetry subscriber initialization failed: {error}"
                )
            }
            Self::UnsupportedProtocol(name) => {
                write!(formatter, "{name} must be 'http/protobuf' when configured")
            }
        }
    }
}

impl Error for ObservabilityError {}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{endpoint_targets_receiver, forbidden_resource_attribute};

    const ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

    #[test]
    fn identifies_recursive_loopback_exports() {
        let receiver = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4318);

        assert!(
            endpoint_targets_receiver(ENDPOINT, "http://localhost:4318", receiver,)
                .expect("valid endpoint")
        );
        assert!(
            endpoint_targets_receiver(ENDPOINT, "http://127.0.0.1:4318/v1/traces", receiver,)
                .expect("valid endpoint")
        );
    }

    #[test]
    fn permits_distinct_export_targets() {
        let receiver = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4318);

        assert!(
            !endpoint_targets_receiver(ENDPOINT, "http://127.0.0.1:4319", receiver,)
                .expect("valid endpoint")
        );
    }

    #[test]
    fn rejects_authority_resource_attributes() {
        assert_eq!(
            forbidden_resource_attribute(
                "deployment.environment.name=local,flaggo.tenant=tenant-a"
            ),
            Some("flaggo.tenant")
        );
    }
}
