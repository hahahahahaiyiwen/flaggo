using System.Diagnostics;
using System.Diagnostics.Metrics;
using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using OpenTelemetry.Logs;
using OpenTelemetry.Metrics;
using OpenTelemetry.Resources;
using OpenTelemetry.Trace;

namespace Flaggo.ServiceHosting;

public static class ServiceObservability
{
    private static readonly HashSet<string> ForbiddenResourceAttributes =
    [
        "flaggo.tenant",
        "flaggo.application",
        "flaggo.environment"
    ];
    private static readonly string[] Signals = ["TRACES", "METRICS", "LOGS"];

    public static WebApplicationBuilder AddFlaggoServiceObservability(
        this WebApplicationBuilder builder,
        string serviceName,
        string instrumentationScope,
        string serviceVersion)
    {
        ArgumentNullException.ThrowIfNull(builder);
        ArgumentException.ThrowIfNullOrWhiteSpace(serviceName);
        ArgumentException.ThrowIfNullOrWhiteSpace(instrumentationScope);
        ArgumentException.ThrowIfNullOrWhiteSpace(serviceVersion);

        ValidateExporterConfiguration();
        ValidateResourceAttributes();
        var instanceId = Guid.NewGuid().ToString("N");
        var resource = ResourceBuilder
            .CreateDefault()
            .AddService(
                serviceName,
                serviceNamespace: "flaggo",
                serviceVersion: serviceVersion,
                serviceInstanceId: instanceId);

        builder.Services.AddSingleton(provider =>
            new FlaggoInstrumentation(
                instrumentationScope,
                serviceVersion,
                provider.GetRequiredService<ILoggerFactory>()));
        builder.Services.AddHostedService<FlaggoServiceLifecycle>();

        var openTelemetry = builder.Services
            .AddOpenTelemetry()
            .ConfigureResource(resourceBuilder => resourceBuilder.AddService(
                serviceName,
                serviceNamespace: "flaggo",
                serviceVersion: serviceVersion,
                serviceInstanceId: instanceId))
            .WithTracing(tracing =>
            {
                tracing
                    .AddSource(instrumentationScope)
                    .AddAspNetCoreInstrumentation()
                    .AddHttpClientInstrumentation();
                if (IsExporterConfigured("TRACES"))
                {
                    tracing.AddOtlpExporter(options =>
                        options.Protocol = OpenTelemetry.Exporter.OtlpExportProtocol.HttpProtobuf);
                }
            })
            .WithMetrics(metrics =>
            {
                metrics
                    .AddMeter(instrumentationScope)
                    .AddAspNetCoreInstrumentation()
                    .AddHttpClientInstrumentation();
                if (IsExporterConfigured("METRICS"))
                {
                    metrics.AddOtlpExporter(options =>
                        options.Protocol = OpenTelemetry.Exporter.OtlpExportProtocol.HttpProtobuf);
                }
            });

        _ = openTelemetry;
        if (IsExporterConfigured("LOGS"))
        {
            builder.Logging.AddOpenTelemetry(logging =>
            {
                logging.IncludeFormattedMessage = true;
                logging.IncludeScopes = true;
                logging.ParseStateValues = true;
                logging.SetResourceBuilder(resource);
                logging.AddOtlpExporter(options =>
                    options.Protocol = OpenTelemetry.Exporter.OtlpExportProtocol.HttpProtobuf);
            });
        }

        return builder;
    }

    private static void ValidateExporterConfiguration()
    {
        ValidateProtocol("OTEL_EXPORTER_OTLP_PROTOCOL");
        foreach (var signal in Signals)
        {
            ValidateProtocol($"OTEL_EXPORTER_OTLP_{signal}_PROTOCOL");
        }

        ValidateEndpoint("OTEL_EXPORTER_OTLP_ENDPOINT");
        foreach (var signal in Signals)
        {
            ValidateEndpoint($"OTEL_EXPORTER_OTLP_{signal}_ENDPOINT");
        }
    }

    private static void ValidateResourceAttributes()
    {
        var attributes = Environment.GetEnvironmentVariable(
            "OTEL_RESOURCE_ATTRIBUTES");
        if (string.IsNullOrWhiteSpace(attributes))
        {
            return;
        }

        foreach (var attribute in attributes.Split(','))
        {
            var separator = attribute.IndexOf('=');
            var name = (separator >= 0 ? attribute[..separator] : attribute)
                .Trim();
            if (ForbiddenResourceAttributes.Contains(name))
            {
                throw new InvalidOperationException(
                    $"OTEL_RESOURCE_ATTRIBUTES must not define {name}.");
            }
        }
    }

    private static void ValidateProtocol(string name)
    {
        var value = Environment.GetEnvironmentVariable(name);
        if (!string.IsNullOrWhiteSpace(value)
            && !string.Equals(value, "http/protobuf", StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException(
                $"{name} must be 'http/protobuf' when configured.");
        }
    }

    private static void ValidateEndpoint(string name)
    {
        var value = Environment.GetEnvironmentVariable(name);
        if (string.IsNullOrWhiteSpace(value))
        {
            return;
        }

        if (!Uri.TryCreate(value, UriKind.Absolute, out var endpoint)
            || endpoint.Scheme is not ("http" or "https")
            || !string.IsNullOrEmpty(endpoint.UserInfo)
            || !string.IsNullOrEmpty(endpoint.Fragment))
        {
            throw new InvalidOperationException(
                $"{name} must be an absolute HTTP(S) URL without credentials or a fragment.");
        }
    }

    private static bool IsExporterConfigured(string signal) =>
        !string.IsNullOrWhiteSpace(
            Environment.GetEnvironmentVariable($"OTEL_EXPORTER_OTLP_{signal}_ENDPOINT"))
        || !string.IsNullOrWhiteSpace(
            Environment.GetEnvironmentVariable("OTEL_EXPORTER_OTLP_ENDPOINT"));
}

public sealed class FlaggoInstrumentation : IDisposable
{
    private readonly ActivitySource _activitySource;
    private readonly Meter _meter;

    public FlaggoInstrumentation(
        string instrumentationScope,
        string version,
        ILoggerFactory loggerFactory)
    {
        ScopeName = instrumentationScope;
        _activitySource = new ActivitySource(instrumentationScope, version);
        _meter = new Meter(instrumentationScope, version);
        Logger = loggerFactory.CreateLogger(instrumentationScope);
    }

    public string ScopeName { get; }

    public ILogger Logger { get; }

    public Meter Meter => _meter;

    public Activity? StartActivity(
        string name,
        ActivityKind kind = ActivityKind.Internal) =>
        _activitySource.StartActivity(name, kind);

    public void LogEvent(
        LogLevel level,
        string eventName,
        Exception? exception = null,
        IReadOnlyDictionary<string, object?>? attributes = null)
    {
        var state = new Dictionary<string, object?>
        {
            ["event.name"] = eventName
        };
        if (attributes is not null)
        {
            foreach (var attribute in attributes)
            {
                state[attribute.Key] = attribute.Value;
            }
        }

        using var scope = Logger.BeginScope(state);
        Logger.Log(
            level,
            new EventId(0, eventName),
            exception,
            eventName);
    }

    public void Dispose()
    {
        _activitySource.Dispose();
        _meter.Dispose();
    }
}

public sealed class FlaggoOperation : IDisposable
{
    private readonly Activity? _activity;
    private readonly Counter<long> _counter;
    private readonly Histogram<double> _duration;
    private readonly string? _operationName;
    private readonly Counter<long>? _additionalCounter;
    private readonly long _startedAt;
    private bool _completed;

    public FlaggoOperation(
        Activity? activity,
        Counter<long> counter,
        Histogram<double> duration,
        string? operationName = null,
        Counter<long>? additionalCounter = null)
    {
        _activity = activity;
        _counter = counter;
        _duration = duration;
        _operationName = operationName;
        _additionalCounter = additionalCounter;
        _startedAt = Stopwatch.GetTimestamp();
    }

    public Activity? Activity => _activity;

    public void Complete(
        string outcome,
        string? failureCategory = null,
        params KeyValuePair<string, object?>[] attributes)
    {
        if (_completed)
        {
            return;
        }

        _completed = true;
        _activity?.SetTag("flaggo.operation.outcome", outcome);
        if (_operationName is not null)
        {
            _activity?.SetTag("flaggo.operation.name", _operationName);
        }
        if (failureCategory is not null)
        {
            _activity?.SetTag("flaggo.failure.category", failureCategory);
            _activity?.SetStatus(ActivityStatusCode.Error, failureCategory);
        }
        foreach (var attribute in attributes)
        {
            _activity?.SetTag(attribute.Key, attribute.Value);
        }

        var tags = new TagList
        {
            { "flaggo.operation.outcome", outcome }
        };
        if (_operationName is not null)
        {
            tags.Add("flaggo.operation.name", _operationName);
        }
        if (failureCategory is not null)
        {
            tags.Add("flaggo.failure.category", failureCategory);
        }
        foreach (var attribute in attributes)
        {
            tags.Add(attribute.Key, attribute.Value);
        }

        _counter.Add(1, tags);
        if (_additionalCounter is not null)
        {
            var additionalTags = new TagList
            {
                { "flaggo.operation.outcome", outcome }
            };
            if (failureCategory is not null)
            {
                additionalTags.Add(
                    "flaggo.failure.category",
                    failureCategory);
            }
            _additionalCounter.Add(1, additionalTags);
        }
        _duration.Record(Stopwatch.GetElapsedTime(_startedAt).TotalSeconds, tags);
        _activity?.Stop();
    }

    public void Fail(
        Exception exception,
        string outcome,
        string failureCategory)
    {
        _activity?.SetTag("error.type", exception.GetType().FullName);
        Complete(outcome, failureCategory);
    }

    public void Dispose()
    {
        if (!_completed)
        {
            Complete("cancelled", "cancelled");
        }
        _activity?.Dispose();
    }
}

internal sealed class FlaggoServiceLifecycle(
    FlaggoInstrumentation instrumentation,
    IHostApplicationLifetime applicationLifetime) : IHostedService, IDisposable
{
    private CancellationTokenRegistration _stopping;
    private CancellationTokenRegistration _stopped;

    public Task StartAsync(CancellationToken cancellationToken)
    {
        _stopping = applicationLifetime.ApplicationStopping.Register(() =>
            instrumentation.LogEvent(
                LogLevel.Information,
                "flaggo.service.stopping",
                attributes: SuccessAttributes()));
        _stopped = applicationLifetime.ApplicationStopped.Register(() =>
            instrumentation.LogEvent(
                LogLevel.Information,
                "flaggo.service.stopped",
                attributes: SuccessAttributes()));
        instrumentation.LogEvent(
            LogLevel.Information,
            "flaggo.service.started",
            attributes: SuccessAttributes());
        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken) =>
        Task.CompletedTask;

    public void Dispose()
    {
        _stopping.Dispose();
        _stopped.Dispose();
    }

    private static Dictionary<string, object?> SuccessAttributes() =>
        new()
        {
            ["flaggo.operation.outcome"] = "success"
        };
}
