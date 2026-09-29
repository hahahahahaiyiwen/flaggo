using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Diagnostics;
using Microsoft.Data.Sqlite;

const string ingestPolicy = "IngestEvidence";
const string evidenceWriteScope = "flaggo.evidence:write";
const string serviceName = "flaggo-otel-ingestion";

var builder = WebApplication.CreateBuilder(args);
var connectionString = builder.Configuration.GetConnectionString("Flaggo")
    ?? "Data Source=flaggo.db";

builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton<IEvidenceStore>(_ => new SqliteEvidenceStore(connectionString));
builder.Services.AddSingleton<ITelemetryFilter, FlaggoSignalTelemetryFilter>();
builder.Services.AddSingleton<OtlpLogIngestionPipeline>();
builder.Services.AddFlaggoAuthentication(
    builder.Configuration,
    builder.Environment,
    evidenceWriteScope);
builder.Services.AddAuthorization(options =>
{
    options.AddPolicy(
        ingestPolicy,
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context => FlaggoClaims.HasScope(context.User, evidenceWriteScope)));
});
builder.Services.AddFlaggoAuthorizationProblemResults();

var app = builder.Build();

app.UseFlaggoCorrelationIds();
app.UseExceptionHandler(new ExceptionHandlerOptions
{
    AllowStatusCode404Response = true,
    ExceptionHandler = WriteExceptionAsync
});
app.UseFlaggoProblemStatusPages();
app.UseAuthentication();
app.UseAuthorization();

app.MapPost(
        "/v1/logs",
        async (
            HttpContext context,
            OtlpLogIngestionPipeline pipeline,
            CancellationToken cancellationToken) =>
        {
            if (!FlaggoClaims.TryGetAuthorityScope(context.User, out var scope))
            {
                return ProblemResults.Create(
                    context,
                    StatusCodes.Status401Unauthorized,
                    ProblemTypes.AuthenticationRequired,
                    "Authentication required",
                    "The credential must identify exactly one application and environment.");
            }

            var body = await HttpJson.ReadAsync<JsonElement>(
                context.Request,
                cancellationToken);
            var result = await pipeline.IngestAsync(scope, body, cancellationToken);
            return Results.Json(result, StrictJson.Options);
        })
    .RequireAuthorization(ingestPolicy);

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
    string? Signal = null,
    string? ObservationId = null);

internal interface ITelemetryFilter
{
    TelemetryFilterDecision Evaluate(JsonElement logRecord);
}

internal sealed class FlaggoSignalTelemetryFilter : ITelemetryFilter
{
    private static readonly HashSet<string> SupportedSignals = new(StringComparer.Ordinal)
    {
        "decision.received",
        "outcome.observed"
    };

    public TelemetryFilterDecision Evaluate(JsonElement logRecord)
    {
        var attributes = OtlpAttributes.Read(logRecord);
        if (!OtlpAttributes.TryGetString(attributes, "flaggo.signal", out var signal))
        {
            return new TelemetryFilterDecision(TelemetryFilterAction.Ignore);
        }

        if (!SupportedSignals.Contains(signal))
        {
            throw new TelemetryFilterException(
                $"Unsupported Flaggo telemetry signal '{signal}'.");
        }

        var observationId =
            OtlpAttributes.OptionalString(attributes, "flaggo.observation.id")
            ?? OtlpAttributes.OptionalString(attributes, "flaggo.decision.id")
            ?? StableObservationId(logRecord);
        return new TelemetryFilterDecision(
            TelemetryFilterAction.Accept,
            signal,
            observationId);
    }

    private static string StableObservationId(JsonElement logRecord)
    {
        var payload = JsonSerializer.Serialize(logRecord, StrictJson.Options);
        var hash = SHA256.HashData(Encoding.UTF8.GetBytes(payload));
        return "otel:" + Convert.ToHexString(hash).ToLowerInvariant();
    }
}

internal sealed class OtlpLogIngestionPipeline(
    IEvidenceStore store,
    ITelemetryFilter filter,
    TimeProvider timeProvider)
{
    public async Task<OtlpIngestionResult> IngestAsync(
        DecisionScope scope,
        JsonElement root,
        CancellationToken cancellationToken)
    {
        if (root.ValueKind != JsonValueKind.Object
            || !root.TryGetProperty("resourceLogs", out var resourceLogs)
            || resourceLogs.ValueKind != JsonValueKind.Array)
        {
            throw new OtlpIngestionException("OTLP logs payload must contain resourceLogs[].");
        }

        var ignored = 0;
        var receivedAt = timeProvider.GetUtcNow();
        var buffer = new List<EvidenceTelemetryRecord>();
        foreach (var logRecord in EnumerateLogRecords(resourceLogs))
        {
            var decision = filter.Evaluate(logRecord);
            if (decision.Action == TelemetryFilterAction.Ignore)
            {
                ignored += 1;
                continue;
            }

            if (decision.Signal is null || decision.ObservationId is null)
            {
                throw new OtlpIngestionException(
                    "Accepted telemetry must include a signal and observation ID.");
            }

            buffer.Add(new EvidenceTelemetryRecord(
                scope,
                decision.ObservationId,
                decision.Signal,
                logRecord.Clone(),
                ObservedAt(logRecord, receivedAt),
                receivedAt));
        }

        var write = await store.PutTelemetryBatchAsync(buffer, cancellationToken);
        return new OtlpIngestionResult(
            Accepted: buffer.Count,
            Ignored: ignored,
            Created: write.Created,
            Duplicates: write.Existing);
    }

    private static IEnumerable<JsonElement> EnumerateLogRecords(JsonElement resourceLogs)
    {
        foreach (var resourceLog in resourceLogs.EnumerateArray())
        {
            if (!resourceLog.TryGetProperty("scopeLogs", out var scopeLogs)
                || scopeLogs.ValueKind != JsonValueKind.Array)
            {
                continue;
            }

            foreach (var scopeLog in scopeLogs.EnumerateArray())
            {
                if (!scopeLog.TryGetProperty("logRecords", out var logRecords)
                    || logRecords.ValueKind != JsonValueKind.Array)
                {
                    continue;
                }

                foreach (var logRecord in logRecords.EnumerateArray())
                {
                    if (logRecord.ValueKind != JsonValueKind.Object)
                    {
                        throw new OtlpIngestionException(
                            "OTLP log records must be JSON objects.");
                    }
                    yield return logRecord;
                }
            }
        }
    }

    private static DateTimeOffset ObservedAt(
        JsonElement logRecord,
        DateTimeOffset fallback)
    {
        if (logRecord.TryGetProperty("timeUnixNano", out var timeUnixNano))
        {
            return FromUnixNanos(timeUnixNano);
        }
        if (logRecord.TryGetProperty("observedTimeUnixNano", out var observedTimeUnixNano))
        {
            return FromUnixNanos(observedTimeUnixNano);
        }
        return fallback;
    }

    private static DateTimeOffset FromUnixNanos(JsonElement value)
    {
        var nanos = value.ValueKind == JsonValueKind.String
            ? long.Parse(value.GetString()!, CultureInfo.InvariantCulture)
            : value.GetInt64();
        var seconds = Math.DivRem(nanos, 1_000_000_000L, out var remainder);
        return DateTimeOffset.FromUnixTimeSeconds(seconds)
            .AddTicks(remainder / 100);
    }
}

internal static class OtlpAttributes
{
    public static Dictionary<string, JsonElement> Read(JsonElement logRecord)
    {
        var attributes = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        if (!logRecord.TryGetProperty("attributes", out var rawAttributes))
        {
            return attributes;
        }
        if (rawAttributes.ValueKind != JsonValueKind.Array)
        {
            throw new TelemetryFilterException("OTLP log attributes must be an array.");
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

    public static string? OptionalString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name)
    {
        if (!attributes.TryGetValue(name, out var value))
        {
            return null;
        }
        if (value.ValueKind != JsonValueKind.String)
        {
            throw new TelemetryFilterException($"Attribute '{name}' must be a string.");
        }
        return value.GetString();
    }

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
