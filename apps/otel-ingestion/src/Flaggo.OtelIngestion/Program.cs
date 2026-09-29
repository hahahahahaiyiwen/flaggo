using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Diagnostics;
using Microsoft.AspNetCore.RequestDecompression;
using Microsoft.Data.Sqlite;
using Microsoft.Net.Http.Headers;

const string serviceName = "flaggo-otel-ingestion";
const long defaultMaximumRequestBodyBytes = 67_108_864;

var builder = WebApplication.CreateBuilder(args);
var connectionString = builder.Configuration.GetConnectionString("Flaggo")
    ?? "Data Source=flaggo.db";
var maximumRequestBodyBytes = builder.Configuration.GetValue<long?>(
    "Flaggo:OtlpIngestion:MaximumRequestBodyBytes")
    ?? defaultMaximumRequestBodyBytes;
if (maximumRequestBodyBytes < 1)
{
    throw new InvalidOperationException(
        "Flaggo:OtlpIngestion:MaximumRequestBodyBytes must be positive.");
}

builder.WebHost.ConfigureKestrel(options =>
    options.Limits.MaxRequestBodySize = maximumRequestBodyBytes);
builder.Services.AddRequestDecompression();
builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton(new OtlpJsonReader(maximumRequestBodyBytes));
builder.Services.AddSingleton<IEvidenceStore>(_ => new SqliteEvidenceStore(connectionString));
builder.Services.AddSingleton<ITelemetryFilter, CandidateTelemetryFilter>();
builder.Services.AddSingleton<OtlpIngestionPipeline>();

var app = builder.Build();

app.UseFlaggoCorrelationIds();
app.UseExceptionHandler(new ExceptionHandlerOptions
{
    AllowStatusCode404Response = true,
    ExceptionHandler = WriteExceptionAsync
});
app.UseRequestDecompression();
app.UseFlaggoProblemStatusPages();

MapOtlpEndpoint(app, "/v1/logs", OtlpTelemetryType.Logs);
MapOtlpEndpoint(app, "/v1/metrics", OtlpTelemetryType.Metrics);
MapOtlpEndpoint(app, "/v1/traces", OtlpTelemetryType.Traces);

app.MapGet(
    "/health/live",
    (TimeProvider timeProvider) => Results.Json(
        new LivenessResult
        {
            Service = serviceName,
            Version = typeof(Program).Assembly.GetName().Version?.ToString() ?? "0.0.0",
            ObservedAt = timeProvider.GetUtcNow()
        },
        StrictJson.Options));

app.MapGet(
    "/health/ready",
    async (
        IEvidenceStore evidenceStore,
        TimeProvider timeProvider,
        CancellationToken cancellationToken) =>
    {
        var checks = new[]
        {
            new ReadinessCheck
            {
                Name = "evidence-store",
                Status = await evidenceStore.IsAvailableAsync(cancellationToken)
                    ? "up"
                    : "down",
                Required = true
            }
        };
        var ready = checks.All(check => check.Status == "up");
        return Results.Json(
            new ReadinessResult
            {
                Status = ready ? "ready" : "not-ready",
                ObservedAt = timeProvider.GetUtcNow(),
                Checks = checks
            },
            StrictJson.Options,
            statusCode: ready
                ? StatusCodes.Status200OK
                : StatusCodes.Status503ServiceUnavailable);
    });

await app.Services.GetRequiredService<IEvidenceStore>().InitializeAsync();
app.Run();

static void MapOtlpEndpoint(WebApplication app, string route, OtlpTelemetryType type)
{
    app.MapPost(
        route,
        async (
            HttpContext context,
            OtlpJsonReader reader,
            OtlpIngestionPipeline pipeline,
            CancellationToken cancellationToken) =>
        {
            var body = await reader.ReadAsync(
                context.Request,
                cancellationToken);
            var result = await pipeline.IngestAsync(
                type,
                body,
                cancellationToken);
            return Results.Json(new { }, StrictJson.Options);
        });
}

static async Task WriteExceptionAsync(HttpContext context)
{
    var exception = context.Features.Get<IExceptionHandlerFeature>()?.Error;
    var logger = context.RequestServices.GetRequiredService<ILogger<Program>>();
    IResult result = exception switch
    {
        HttpContractException contractException =>
            ProblemResults.FromException(context, contractException),
        TelemetryFilterException filterException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid telemetry",
                filterException.Message),
        OtlpIngestionException ingestionException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid telemetry",
                ingestionException.Message),
        ArgumentException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid request",
                "The telemetry payload is invalid."),
        SqliteException or IOException or TimeoutException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status503ServiceUnavailable,
                ProblemTypes.DependencyUnavailable,
                "Dependency unavailable",
                "A required ingestion dependency is unavailable.",
                retryAfterSeconds: 1),
        _ =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.InternalError,
                "Request failed",
                "The request failed.")
    };
    if (exception is not null
        and not HttpContractException
        and not TelemetryFilterException
        and not OtlpIngestionException)
    {
        logger.LogError(exception, "OTel ingestion request failed.");
    }
    await result.ExecuteAsync(context);
}

public partial class Program;

internal static class OtlpJson
{
    public const int MaximumDepth = 64;

    public static JsonSerializerOptions Options { get; } = new(JsonSerializerDefaults.Web)
    {
        AllowTrailingCommas = false,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        MaxDepth = MaximumDepth,
        PropertyNameCaseInsensitive = false,
        ReadCommentHandling = JsonCommentHandling.Disallow
    };
}

internal sealed class OtlpJsonReader(long maximumDocumentBytes)
{
    public async ValueTask<JsonElement> ReadAsync(
        HttpRequest request,
        CancellationToken cancellationToken)
    {
        if (!MediaTypeHeaderValue.TryParse(request.ContentType, out var contentType)
            || !string.Equals(
                contentType.MediaType.Value,
                "application/json",
                StringComparison.OrdinalIgnoreCase))
        {
            throw new HttpContractException(
                StatusCodes.Status415UnsupportedMediaType,
                ProblemTypes.UnsupportedMediaType,
                "Unsupported media type",
                "Content-Type must be application/json.");
        }

        if (request.ContentLength > maximumDocumentBytes)
        {
            throw PayloadTooLarge();
        }

        await using var body = new MemoryStream();
        var buffer = new byte[65_536];
        while (true)
        {
            var read = await request.Body.ReadAsync(buffer, cancellationToken);
            if (read == 0)
            {
                break;
            }
            if (body.Length + read > maximumDocumentBytes)
            {
                throw PayloadTooLarge();
            }
            await body.WriteAsync(buffer.AsMemory(0, read), cancellationToken);
        }

        try
        {
            using var document = JsonDocument.Parse(
                body.ToArray(),
                new JsonDocumentOptions
                {
                    AllowTrailingCommas = false,
                    CommentHandling = JsonCommentHandling.Disallow,
                    MaxDepth = OtlpJson.MaximumDepth
                });
            return document.RootElement.Clone();
        }
        catch (JsonException exception)
        {
            throw new HttpContractException(
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid request",
                $"Request body is invalid: {exception.Message}",
                exception);
        }
    }

    private static HttpContractException PayloadTooLarge() =>
        new(
            StatusCodes.Status413PayloadTooLarge,
            ProblemTypes.InvalidRequest,
            "Payload too large",
            "OTLP request body exceeds the configured decompressed-size limit.");
}

internal enum OtlpTelemetryType
{
    Logs,
    Metrics,
    Traces
}

internal sealed record OtlpIngestionResult(
    int Accepted,
    int Ignored,
    int Created,
    int Duplicates);

internal sealed class OtlpIngestionException(string message) : Exception(message);

internal sealed class TelemetryFilterException(string message) : Exception(message);

internal enum TelemetryFilterAction
{
    Accept,
    Ignore
}

internal sealed record TelemetryFilterDecision(
    TelemetryFilterAction Action,
    string Signal,
    string ObservationId,
    DateTimeOffset? ObservedAt = null);

internal sealed record OtlpCandidate(
    OtlpTelemetryType Type,
    JsonElement Resource,
    JsonElement Payload,
    JsonElement Envelope);

internal interface ITelemetryFilter
{
    TelemetryFilterDecision Evaluate(OtlpCandidate candidate);
}

internal sealed class CandidateTelemetryFilter : ITelemetryFilter
{
    public TelemetryFilterDecision Evaluate(OtlpCandidate candidate)
    {
        var signal = candidate.Type switch
        {
            OtlpTelemetryType.Logs => LogSignal(candidate.Payload),
            OtlpTelemetryType.Metrics => MetricSignal(candidate.Payload),
            OtlpTelemetryType.Traces => SpanSignal(candidate.Payload),
            _ => throw new TelemetryFilterException("Unsupported OTLP telemetry type.")
        };
        var observationId = ExplicitObservationId(candidate.Payload)
            ?? SpanObservationId(candidate)
            ?? StableObservationId(candidate.Type, candidate.Envelope);
        return new TelemetryFilterDecision(
            TelemetryFilterAction.Accept,
            signal,
            observationId,
            ObservedAt(candidate));
    }

    private static string LogSignal(JsonElement logRecord)
    {
        var attributes = OtlpAttributes.Read(logRecord);
        if (OtlpAttributes.TryGetString(attributes, "flaggo.signal", out var flaggoSignal))
        {
            return flaggoSignal;
        }
        if (logRecord.TryGetProperty("eventName", out var eventName)
            && eventName.ValueKind == JsonValueKind.String)
        {
            return $"log:{eventName.GetString()}";
        }
        return "log";
    }

    private static string MetricSignal(JsonElement metric)
    {
        if (metric.TryGetProperty("name", out var name)
            && name.ValueKind == JsonValueKind.String
            && !string.IsNullOrWhiteSpace(name.GetString()))
        {
            return $"metric:{name.GetString()}";
        }
        return "metric";
    }

    private static string SpanSignal(JsonElement span)
    {
        if (span.TryGetProperty("name", out var name)
            && name.ValueKind == JsonValueKind.String
            && !string.IsNullOrWhiteSpace(name.GetString()))
        {
            return $"span:{name.GetString()}";
        }
        return "span";
    }

    private static string? ExplicitObservationId(JsonElement payload)
    {
        var attributes = OtlpAttributes.Read(payload);
        if (OtlpAttributes.TryGetOptionalString(
                attributes,
                "flaggo.observation.id",
                out var observationId))
        {
            return observationId;
        }
        return null;
    }

    private static string? SpanObservationId(OtlpCandidate candidate)
    {
        if (candidate.Type != OtlpTelemetryType.Traces
            || !candidate.Payload.TryGetProperty("traceId", out var traceId)
            || traceId.ValueKind != JsonValueKind.String
            || string.IsNullOrWhiteSpace(traceId.GetString())
            || !candidate.Payload.TryGetProperty("spanId", out var spanId)
            || spanId.ValueKind != JsonValueKind.String
            || string.IsNullOrWhiteSpace(spanId.GetString()))
        {
            return null;
        }
        return $"traces:{traceId.GetString()}:{spanId.GetString()}";
    }

    private static DateTimeOffset? ObservedAt(OtlpCandidate candidate)
    {
        if (candidate.Payload.TryGetProperty("timeUnixNano", out var timeUnixNano))
        {
            return OtlpTime.FromUnixNanos(timeUnixNano);
        }
        if (candidate.Payload.TryGetProperty("observedTimeUnixNano", out var observedTimeUnixNano))
        {
            return OtlpTime.FromUnixNanos(observedTimeUnixNano);
        }
        if (candidate.Payload.TryGetProperty("startTimeUnixNano", out var startTimeUnixNano))
        {
            return OtlpTime.FromUnixNanos(startTimeUnixNano);
        }
        return FirstDataPointTime(candidate.Payload);
    }

    private static DateTimeOffset? FirstDataPointTime(JsonElement metric)
    {
        foreach (var kind in new[] { "gauge", "sum", "histogram" })
        {
            if (!metric.TryGetProperty(kind, out var pointsContainer)
                || !pointsContainer.TryGetProperty("dataPoints", out var dataPoints)
                || dataPoints.ValueKind != JsonValueKind.Array)
            {
                continue;
            }
            foreach (var dataPoint in dataPoints.EnumerateArray())
            {
                if (dataPoint.TryGetProperty("timeUnixNano", out var timeUnixNano))
                {
                    return OtlpTime.FromUnixNanos(timeUnixNano);
                }
                if (dataPoint.TryGetProperty(
                        "startTimeUnixNano",
                        out var startTimeUnixNano))
                {
                    return OtlpTime.FromUnixNanos(startTimeUnixNano);
                }
            }
        }
        return null;
    }

    private static string StableObservationId(OtlpTelemetryType type, JsonElement envelope)
    {
        var serialized = JsonSerializer.Serialize(envelope, OtlpJson.Options);
        var hash = SHA256.HashData(Encoding.UTF8.GetBytes(serialized));
        return $"{TelemetryTypeName(type)}:"
            + Convert.ToHexString(hash).ToLowerInvariant();
    }

    private static string TelemetryTypeName(OtlpTelemetryType type) => type switch
    {
        OtlpTelemetryType.Logs => "logs",
        OtlpTelemetryType.Metrics => "metrics",
        OtlpTelemetryType.Traces => "traces",
        _ => throw new TelemetryFilterException("Unsupported OTLP telemetry type.")
    };
}

internal sealed class OtlpIngestionPipeline(
    IEvidenceStore store,
    ITelemetryFilter filter,
    TimeProvider timeProvider)
{
    public async Task<OtlpIngestionResult> IngestAsync(
        OtlpTelemetryType type,
        JsonElement root,
        CancellationToken cancellationToken)
    {
        if (root.ValueKind != JsonValueKind.Object)
        {
            throw new OtlpIngestionException("OTLP payload must be a JSON object.");
        }

        var ignored = 0;
        var receivedAt = timeProvider.GetUtcNow();
        var buffer = new List<EvidenceTelemetryRecord>();
        foreach (var candidate in EnumerateCandidates(type, root))
        {
            var scope = ScopeFrom(candidate.Resource);
            if (scope is null)
            {
                ignored += 1;
                continue;
            }
            var decision = filter.Evaluate(candidate);
            if (decision.Action == TelemetryFilterAction.Ignore)
            {
                ignored += 1;
                continue;
            }

            buffer.Add(new EvidenceTelemetryRecord(
                scope.Value,
                decision.ObservationId,
                TelemetryTypeName(candidate.Type),
                decision.Signal,
                candidate.Envelope.Clone(),
                decision.ObservedAt ?? receivedAt,
                receivedAt));
        }

        var write = await store.PutTelemetryBatchAsync(buffer, cancellationToken);
        return new OtlpIngestionResult(
            Accepted: buffer.Count,
            Ignored: ignored,
            Created: write.Created,
            Duplicates: write.Existing);
    }

    private static IEnumerable<OtlpCandidate> EnumerateCandidates(
        OtlpTelemetryType type,
        JsonElement root) =>
        type switch
        {
            OtlpTelemetryType.Logs => EnumerateNested(
                root,
                "resourceLogs",
                "scopeLogs",
                "logRecords",
                type),
            OtlpTelemetryType.Metrics => EnumerateNested(
                root,
                "resourceMetrics",
                "scopeMetrics",
                "metrics",
                type),
            OtlpTelemetryType.Traces => EnumerateNested(
                root,
                "resourceSpans",
                "scopeSpans",
                "spans",
                type),
            _ => throw new OtlpIngestionException("Unsupported OTLP telemetry type.")
        };

    private static IEnumerable<OtlpCandidate> EnumerateNested(
        JsonElement root,
        string resourceCollectionName,
        string scopeCollectionName,
        string recordCollectionName,
        OtlpTelemetryType type)
    {
        if (!root.TryGetProperty(resourceCollectionName, out var resourceItems)
            || resourceItems.ValueKind != JsonValueKind.Array)
        {
            throw new OtlpIngestionException(
                $"OTLP payload must contain {resourceCollectionName}[].");
        }

        foreach (var resourceItem in resourceItems.EnumerateArray())
        {
            var resource = ObjectPropertyOrEmpty(resourceItem, "resource");
            var resourceSchemaUrl = OptionalStringProperty(resourceItem, "schemaUrl");
            if (!resourceItem.TryGetProperty(scopeCollectionName, out var scopeItems)
                || scopeItems.ValueKind != JsonValueKind.Array)
            {
                continue;
            }

            foreach (var scopeItem in scopeItems.EnumerateArray())
            {
                var instrumentationScope = ObjectPropertyOrEmpty(scopeItem, "scope");
                var scopeSchemaUrl = OptionalStringProperty(scopeItem, "schemaUrl");
                if (!scopeItem.TryGetProperty(recordCollectionName, out var records)
                    || records.ValueKind != JsonValueKind.Array)
                {
                    continue;
                }

                foreach (var record in records.EnumerateArray())
                {
                    if (record.ValueKind != JsonValueKind.Object)
                    {
                        throw new OtlpIngestionException(
                            $"OTLP {recordCollectionName} entries must be JSON objects.");
                    }
                    yield return new OtlpCandidate(
                        type,
                        resource,
                        record,
                        CreateEnvelope(
                            resource,
                            resourceSchemaUrl,
                            instrumentationScope,
                            scopeSchemaUrl,
                            record));
                }
            }
        }
    }

    private static JsonElement CreateEnvelope(
        JsonElement resource,
        string? resourceSchemaUrl,
        JsonElement instrumentationScope,
        string? scopeSchemaUrl,
        JsonElement record) =>
        JsonSerializer.SerializeToElement(
            new
            {
                resource,
                resourceSchemaUrl,
                scope = instrumentationScope,
                scopeSchemaUrl,
                record
            },
            OtlpJson.Options);

    private static JsonElement ObjectPropertyOrEmpty(
        JsonElement owner,
        string name)
    {
        if (!owner.TryGetProperty(name, out var value))
        {
            return JsonSerializer.SerializeToElement(new { });
        }
        if (value.ValueKind != JsonValueKind.Object)
        {
            throw new OtlpIngestionException($"OTLP {name} must be a JSON object.");
        }
        return value;
    }

    private static string? OptionalStringProperty(
        JsonElement owner,
        string name)
    {
        if (!owner.TryGetProperty(name, out var value))
        {
            return null;
        }
        if (value.ValueKind != JsonValueKind.String)
        {
            throw new OtlpIngestionException($"OTLP {name} must be a string.");
        }
        return value.GetString();
    }

    private static DecisionScope? ScopeFrom(JsonElement resource)
    {
        var attributes = OtlpAttributes.Read(resource);
        var application =
            OtlpAttributes.OptionalString(attributes, "flaggo.application")
            ?? OtlpAttributes.OptionalString(attributes, "service.name");
        var environment =
            OtlpAttributes.OptionalString(attributes, "flaggo.environment")
            ?? OtlpAttributes.OptionalString(attributes, "deployment.environment.name")
            ?? OtlpAttributes.OptionalString(attributes, "deployment.environment")
            ?? "default";
        return string.IsNullOrWhiteSpace(application)
            ? null
            : new DecisionScope(application, environment);
    }

    private static string TelemetryTypeName(OtlpTelemetryType type) => type switch
    {
        OtlpTelemetryType.Logs => "logs",
        OtlpTelemetryType.Metrics => "metrics",
        OtlpTelemetryType.Traces => "traces",
        _ => throw new OtlpIngestionException("Unsupported OTLP telemetry type.")
    };
}

internal static class OtlpAttributes
{
    public static Dictionary<string, JsonElement> Read(JsonElement owner)
    {
        var attributes = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        if (owner.ValueKind != JsonValueKind.Object
            || !owner.TryGetProperty("attributes", out var rawAttributes))
        {
            return attributes;
        }
        if (rawAttributes.ValueKind != JsonValueKind.Array)
        {
            throw new TelemetryFilterException("OTLP attributes must be an array.");
        }
        foreach (var attribute in rawAttributes.EnumerateArray())
        {
            if (!attribute.TryGetProperty("key", out var key)
                || key.ValueKind != JsonValueKind.String
                || !attribute.TryGetProperty("value", out var value))
            {
                throw new TelemetryFilterException("OTLP attributes require key and value.");
            }
            attributes[key.GetString()!] = ReadAttributeValue(value);
        }
        return attributes;
    }

    public static bool TryGetString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name,
        out string value)
    {
        value = string.Empty;
        if (!attributes.TryGetValue(name, out var element))
        {
            return false;
        }
        if (element.ValueKind != JsonValueKind.String)
        {
            throw new TelemetryFilterException($"Attribute '{name}' must be a string.");
        }
        value = element.GetString()!;
        return true;
    }

    public static bool TryGetOptionalString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name,
        out string? value)
    {
        value = null;
        if (!attributes.TryGetValue(name, out var element))
        {
            return false;
        }
        if (element.ValueKind != JsonValueKind.String)
        {
            throw new TelemetryFilterException($"Attribute '{name}' must be a string.");
        }
        value = element.GetString();
        return !string.IsNullOrWhiteSpace(value);
    }

    public static string? OptionalString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name) =>
        TryGetOptionalString(attributes, name, out var value)
            ? value
            : null;

    private static JsonElement ReadAttributeValue(JsonElement value)
    {
        if (value.TryGetProperty("stringValue", out var stringValue))
        {
            return JsonSerializer.SerializeToElement(stringValue.GetString());
        }
        if (value.TryGetProperty("boolValue", out var boolValue))
        {
            return JsonSerializer.SerializeToElement(boolValue.GetBoolean());
        }
        if (value.TryGetProperty("intValue", out var intValue))
        {
            return intValue.ValueKind == JsonValueKind.String
                ? JsonSerializer.SerializeToElement(long.Parse(
                    intValue.GetString()!,
                    CultureInfo.InvariantCulture))
                : JsonSerializer.SerializeToElement(intValue.GetInt64());
        }
        if (value.TryGetProperty("doubleValue", out var doubleValue))
        {
            return JsonSerializer.SerializeToElement(doubleValue.GetDouble());
        }
        throw new TelemetryFilterException(
            "Only OTLP stringValue, boolValue, intValue, and doubleValue attributes are supported.");
    }
}

internal static class OtlpTime
{
    public static DateTimeOffset FromUnixNanos(JsonElement value)
    {
        var nanos = value.ValueKind == JsonValueKind.String
            ? long.Parse(value.GetString()!, CultureInfo.InvariantCulture)
            : value.GetInt64();
        var seconds = Math.DivRem(nanos, 1_000_000_000L, out var remainder);
        return DateTimeOffset.FromUnixTimeSeconds(seconds)
            .AddTicks(remainder / 100);
    }
}
