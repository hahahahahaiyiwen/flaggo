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
            IEvidenceStore store,
            TimeProvider timeProvider,
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
            var result = await OtlpLogIngestor.IngestAsync(
                scope,
                body,
                store,
                timeProvider.GetUtcNow(),
                cancellationToken);
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
    if (exception is not null and not HttpContractException and not OtlpIngestionException)
    {
        logger.LogError(exception, "OTel ingestion request failed.");
    }
    await result.ExecuteAsync(context);
}

public partial class Program;

internal sealed record OtlpIngestionResult(
    int Accepted,
    int Ignored,
    int DecisionObservationsCreated,
    int OutcomeObservationsCreated,
    int Duplicates);

internal sealed class OtlpIngestionException(string message) : Exception(message);

internal static class OtlpLogIngestor
{
    public static async Task<OtlpIngestionResult> IngestAsync(
        DecisionScope scope,
        JsonElement root,
        IEvidenceStore store,
        DateTimeOffset observedFallback,
        CancellationToken cancellationToken)
    {
        if (root.ValueKind != JsonValueKind.Object
            || !root.TryGetProperty("resourceLogs", out var resourceLogs)
            || resourceLogs.ValueKind != JsonValueKind.Array)
        {
            throw new OtlpIngestionException("OTLP logs payload must contain resourceLogs[].");
        }

        var accepted = 0;
        var ignored = 0;
        var decisionsCreated = 0;
        var outcomesCreated = 0;
        var duplicates = 0;

        foreach (var logRecord in EnumerateLogRecords(resourceLogs))
        {
            var attributes = ReadAttributes(logRecord);
            if (!TryGetString(attributes, "flaggo.signal", out var signal))
            {
                ignored += 1;
                continue;
            }

            switch (signal)
            {
                case "decision.received":
                {
                    var result = await store.PutDecisionAsync(
                        CreateDecision(scope, logRecord, attributes, observedFallback),
                        cancellationToken);
                    accepted += 1;
                    if (result == EvidenceObservationWriteResult.Created)
                    {
                        decisionsCreated += 1;
                    }
                    else
                    {
                        duplicates += 1;
                    }
                    break;
                }

                case "outcome.observed":
                {
                    var result = await store.PutOutcomeAsync(
                        CreateOutcome(scope, logRecord, attributes, observedFallback),
                        cancellationToken);
                    accepted += 1;
                    if (result == EvidenceObservationWriteResult.Created)
                    {
                        outcomesCreated += 1;
                    }
                    else
                    {
                        duplicates += 1;
                    }
                    break;
                }

                default:
                    throw new OtlpIngestionException(
                        $"Unsupported Flaggo telemetry signal '{signal}'.");
            }
        }

        return new OtlpIngestionResult(
            accepted,
            ignored,
            decisionsCreated,
            outcomesCreated,
            duplicates);
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
                    yield return logRecord;
                }
            }
        }
    }

    private static DecisionObservation CreateDecision(
        DecisionScope scope,
        JsonElement logRecord,
        IReadOnlyDictionary<string, JsonElement> attributes,
        DateTimeOffset observedFallback)
    {
        var decisionId = RequiredString(attributes, "flaggo.decision.id");
        var resultJson = RequiredString(attributes, "flaggo.result.json");
        using var result = JsonDocument.Parse(resultJson);
        return new DecisionObservation(
            scope,
            decisionId,
            RequiredString(attributes, "flaggo.contract.name"),
            RequiredString(attributes, "flaggo.contract.digest"),
            RequiredString(attributes, "flaggo.executable.digest"),
            result.RootElement.Clone(),
            RequiredString(attributes, "flaggo.result.hash"),
            RequiredString(attributes, "flaggo.evaluation.source"),
            OptionalString(attributes, "flaggo.evaluation.rule"),
            CorrelationAttributes(attributes),
            ObservedAt(logRecord, observedFallback));
    }

    private static OutcomeObservation CreateOutcome(
        DecisionScope scope,
        JsonElement logRecord,
        IReadOnlyDictionary<string, JsonElement> attributes,
        DateTimeOffset observedFallback)
    {
        var valueJson = RequiredString(attributes, "flaggo.evidence.value.json");
        using var value = JsonDocument.Parse(valueJson);
        var observation = new OutcomeObservation(
            scope,
            OptionalString(attributes, "flaggo.observation.id")
                ?? StableObservationId(logRecord, attributes),
            RequiredString(attributes, "flaggo.evidence.binding"),
            value.RootElement.Clone(),
            OptionalString(attributes, "flaggo.decision.id"),
            OptionalString(attributes, "flaggo.contract.name"),
            OptionalString(attributes, "flaggo.contract.digest"),
            CorrelationAttributes(attributes),
            ObservedAt(logRecord, observedFallback));
        return observation;
    }

    private static Dictionary<string, JsonElement> ReadAttributes(JsonElement logRecord)
    {
        var attributes = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        if (!logRecord.TryGetProperty("attributes", out var rawAttributes))
        {
            return attributes;
        }
        if (rawAttributes.ValueKind != JsonValueKind.Array)
        {
            throw new OtlpIngestionException("OTLP log attributes must be an array.");
        }
        foreach (var attribute in rawAttributes.EnumerateArray())
        {
            if (!attribute.TryGetProperty("key", out var key)
                || key.ValueKind != JsonValueKind.String
                || !attribute.TryGetProperty("value", out var value))
            {
                throw new OtlpIngestionException("OTLP attributes require key and value.");
            }
            attributes[key.GetString()!] = ReadAttributeValue(value);
        }
        return attributes;
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
        throw new OtlpIngestionException(
            "Only OTLP stringValue, boolValue, intValue, and doubleValue attributes are supported.");
    }

    private static IReadOnlyDictionary<string, JsonElement> CorrelationAttributes(
        IReadOnlyDictionary<string, JsonElement> attributes)
    {
        const string prefix = "flaggo.correlation.";
        var correlation = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var (name, value) in attributes)
        {
            if (name.StartsWith(prefix, StringComparison.Ordinal))
            {
                correlation[name[prefix.Length..]] = value.Clone();
            }
        }
        return correlation;
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

    private static string RequiredString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name) =>
        OptionalString(attributes, name)
        ?? throw new OtlpIngestionException($"Missing required attribute '{name}'.");

    private static string? OptionalString(
        IReadOnlyDictionary<string, JsonElement> attributes,
        string name)
    {
        if (!attributes.TryGetValue(name, out var value))
        {
            return null;
        }
        if (value.ValueKind != JsonValueKind.String)
        {
            throw new OtlpIngestionException($"Attribute '{name}' must be a string.");
        }
        return value.GetString();
    }

    private static bool TryGetString(
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
            throw new OtlpIngestionException($"Attribute '{name}' must be a string.");
        }
        value = element.GetString()!;
        return true;
    }

    private static string StableObservationId(
        JsonElement logRecord,
        IReadOnlyDictionary<string, JsonElement> attributes)
    {
        var payload = JsonSerializer.Serialize(new
        {
            time = logRecord.TryGetProperty("timeUnixNano", out var time)
                ? time.ToString()
                : null,
            binding = OptionalString(attributes, "flaggo.evidence.binding"),
            value = OptionalString(attributes, "flaggo.evidence.value.json"),
            decision = OptionalString(attributes, "flaggo.decision.id"),
            contract = OptionalString(attributes, "flaggo.contract.digest")
        }, StrictJson.Options);
        var hash = SHA256.HashData(Encoding.UTF8.GetBytes(payload));
        return "otel:" + Convert.ToHexString(hash).ToLowerInvariant();
    }
}
