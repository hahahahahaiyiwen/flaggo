using System.Security.Cryptography;
using System.Security.Claims;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using System.Globalization;
using Microsoft.Net.Http.Headers;
using Flaggo.Shared.Contracts;

namespace Flaggo.DataPlane;

public static class RuntimeHttp
{
    private static readonly object CorrelationIdItemKey = new();

    private static readonly Regex Sha256DigestPattern = new(
        "^sha256:[0-9a-f]{64}$",
        RegexOptions.CultureInvariant);

    public static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        PropertyNameCaseInsensitive = false
    };

    public static async Task<(T? Value, byte[] Body, FlaggoProblem? Error)> ReadJsonAsync<T>(
        HttpContext context,
        CancellationToken cancellationToken)
    {
        if (!MediaTypeHeaderValue.TryParse(
                context.Request.ContentType,
                out var contentType) ||
            !string.Equals(
                contentType.MediaType.Value,
                "application/json",
                StringComparison.OrdinalIgnoreCase))
        {
            return (
                default,
                [],
                Problem(
                    context,
                    415,
                    "unsupported-media-type",
                    "Content-Type must be application/json."));
        }

        try
        {
            using var memory = new MemoryStream();
            await context.Request.Body.CopyToAsync(memory, cancellationToken);
            var body = memory.ToArray();
            EnsureNoDuplicateProperties(body);
            EnsureRequestNullability<T>(body);
            var value = JsonSerializer.Deserialize<T>(body, JsonOptions);
            return (value, body, null);
        }
        catch (JsonException error)
        {
            return (
                default,
                [],
                Problem(
                    context,
                    400,
                    "malformed-json",
                    $"Request body does not match the runtime contract: {error.Message}"));
        }
    }

    public static string Fingerprint(byte[] json, string decisionKey)
    {
        using var document = JsonDocument.Parse(json.ToArray());
        using var memory = new MemoryStream();
        var requestIdentity = Encoding.UTF8.GetBytes(
            $"POST\n/v1/decisions/{{decisionKey}}:decide\ndecisionKey={decisionKey}\nv1\napplication/json\n");
        memory.Write(requestIdentity);
        using (var writer = new Utf8JsonWriter(
                   memory,
                   new JsonWriterOptions { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping }))
        {
            WriteCanonical(writer, document.RootElement);
        }

        return Convert.ToHexString(SHA256.HashData(memory.ToArray())).ToLowerInvariant();
    }

    public static FlaggoProblem Problem(
        HttpContext context,
        int status,
        string code,
        string detail,
        IReadOnlyList<ProblemIssue>? issues = null,
        int? retryAfterSeconds = null,
        ClientFallbackEligibility? clientFallback = null) =>
        new(
            $"https://flaggo.dev/problems/{code}",
            status,
            code,
            string.Join(
                ' ',
                code.Split('-').Select(
                    (part, index) => index == 0
                        ? char.ToUpperInvariant(part[0]) + part[1..]
                        : part)),
            detail,
            context.Request.Path,
            GetCorrelationId(context),
            issues,
            retryAfterSeconds,
            clientFallback);

    public static IResult ProblemResult(HttpContext context, FlaggoProblem problem) =>
        Results.Json(
            problem,
            JsonOptions,
            "application/problem+json",
            problem.Status);

    public static bool HasScope(ClaimsPrincipal principal, string requiredScope) =>
        principal.Claims
            .Where(claim => claim.Type is "scope" or "scp")
            .SelectMany(claim => claim.Value.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            .Contains(requiredScope, StringComparer.Ordinal);

    public static bool HasClientScope(
        ClaimsPrincipal principal,
        string appId,
        string environment) =>
        principal.FindAll("polari_app_id").Any(
            claim => string.Equals(claim.Value, appId, StringComparison.Ordinal)) &&
        principal.FindAll("polari_environment").Any(
            claim => string.Equals(claim.Value, environment, StringComparison.Ordinal));

    public static bool IsRuntimeContextValue(JsonElement value) =>
        value.ValueKind is
            JsonValueKind.String or
            JsonValueKind.True or
            JsonValueKind.False or
            JsonValueKind.Null ||
        value.ValueKind == JsonValueKind.Number &&
        value.TryGetDouble(out var number) &&
        double.IsFinite(number);

    public static bool IsValidSignalInput(SignalInput? input) =>
        input?.Signal is not null &&
        !string.IsNullOrWhiteSpace(input.Signal.Key) &&
        IsRuntimeContextValue(input.Value);

    public static void SetCorrelationId(HttpContext context, string correlationId)
    {
        context.Items[CorrelationIdItemKey] = correlationId;
        context.Response.Headers["X-Flaggo-Correlation-Id"] = correlationId;
    }

    public static void RestoreCorrelationId(HttpContext context)
    {
        if (context.Items.TryGetValue(CorrelationIdItemKey, out var value) &&
            value is string correlationId)
        {
            context.Response.Headers["X-Flaggo-Correlation-Id"] = correlationId;
        }
    }

    private static string GetCorrelationId(HttpContext context)
    {
        if (context.Items.TryGetValue(CorrelationIdItemKey, out var value) &&
            value is string correlationId)
        {
            return correlationId;
        }

        return context.Response.Headers["X-Flaggo-Correlation-Id"].ToString();
    }

    public static bool IsSha256Digest(string? value) =>
        value is not null && Sha256DigestPattern.IsMatch(value);

    private static void EnsureNoDuplicateProperties(ReadOnlySpan<byte> json)
    {
        var reader = new Utf8JsonReader(json);
        var objectProperties = new Stack<HashSet<string>>();
        while (reader.Read())
        {
            switch (reader.TokenType)
            {
                case JsonTokenType.StartObject:
                    objectProperties.Push(new HashSet<string>(StringComparer.Ordinal));
                    break;
                case JsonTokenType.EndObject:
                    objectProperties.Pop();
                    break;
                case JsonTokenType.PropertyName:
                    var propertyName = reader.GetString()!;
                    if (!objectProperties.Peek().Add(propertyName))
                    {
                        throw new JsonException(
                            $"Duplicate JSON property '{propertyName}' is not allowed.");
                    }

                    break;
            }
        }
    }

    private static void EnsureRequestNullability<T>(ReadOnlySpan<byte> json)
    {
        using var document = JsonDocument.Parse(json.ToArray());
        if (document.RootElement.ValueKind != JsonValueKind.Object)
        {
            return;
        }

            if (typeof(T) == typeof(DecideRequest))
            {
                RejectNullProperties(
                    document.RootElement,
                    "expectedContract",
                    "runtimeContext",
                    "runtimeTarget",
                    "inputs",
                    "client");
                RejectNestedNullProperties(
                    document.RootElement,
                    "expectedContract",
                    "definitionId",
                    "contractDigest",
                    "revision",
                    "bundleDigest",
                    "buildId",
                    "deploymentId",
                    "artifactDigest");
                RejectNestedNullProperties(
                    document.RootElement,
                    "client",
                    "appId",
                    "environment",
                    "sdk",
                    "sdkVersion");
                RejectNestedNullProperties(document.RootElement, "runtimeTarget", "type", "id");
                if (document.RootElement.TryGetProperty("inputs", out var inputs) &&
                    inputs.ValueKind == JsonValueKind.Array)
                {
                    foreach (var input in inputs.EnumerateArray())
                    {
                        if (input.ValueKind == JsonValueKind.Null)
                        {
                            throw new JsonException("Input entries cannot be null.");
                        }

                        RejectNullProperties(input, "signal");
                        RejectNestedNullProperties(input, "signal", "key");
                    }
                }
            }
            else if (typeof(T) == typeof(ExposureConfirmationRequest))
            {
                RejectNullProperties(document.RootElement, "confirmToken", "appliedAt");
            }
        }

    private static void RejectNestedNullProperties(
            JsonElement parent,
            string objectProperty,
            params string[] propertyNames)
        {
            if (parent.TryGetProperty(objectProperty, out var nested) &&
                nested.ValueKind == JsonValueKind.Object)
            {
                RejectNullProperties(nested, propertyNames);
            }
        }

    private static void RejectNullProperties(JsonElement element, params string[] propertyNames)
        {
            foreach (var propertyName in propertyNames)
            {
                if (element.TryGetProperty(propertyName, out var value) &&
                    value.ValueKind == JsonValueKind.Null)
                {
                    throw new JsonException($"Property '{propertyName}' cannot be null.");
                }
            }
    }

    private static void WriteCanonical(Utf8JsonWriter writer, JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                writer.WriteStartObject();
                foreach (var property in element.EnumerateObject().OrderBy(
                             property => property.Name,
                             StringComparer.Ordinal))
                {
                    writer.WritePropertyName(property.Name);
                    WriteCanonical(writer, property.Value);
                }

                writer.WriteEndObject();
                break;
            case JsonValueKind.Array:
                writer.WriteStartArray();
                foreach (var item in element.EnumerateArray())
                {
                    WriteCanonical(writer, item);
                }

                writer.WriteEndArray();
                break;
            case JsonValueKind.Number:
                writer.WriteRawValue(
                    FormatCanonicalNumber(element.GetDouble()),
                    skipInputValidation: true);
                break;
            default:
                element.WriteTo(writer);
                break;
        }
    }

    private static string FormatCanonicalNumber(double value)
    {
        if (!double.IsFinite(value))
        {
            throw new JsonException("Non-finite numbers are not valid JSON.");
        }

        if (value == 0)
        {
            return "0";
        }

        var roundTrip = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentIndex = roundTrip.IndexOfAny(['E', 'e']);
        var absolute = Math.Abs(value);
        if (exponentIndex < 0)
        {
            return roundTrip;
        }

        var mantissa = roundTrip[..exponentIndex];
        var exponent = int.Parse(
            roundTrip[(exponentIndex + 1)..],
            NumberStyles.AllowLeadingSign,
            CultureInfo.InvariantCulture);
        if (absolute >= 1e-6 && absolute < 1e21)
        {
            return ExpandDecimal(mantissa, exponent);
        }

        var normalizedExponent = exponent >= 0 ? $"+{exponent}" : exponent.ToString(
            CultureInfo.InvariantCulture);
        return $"{mantissa.ToLowerInvariant()}e{normalizedExponent}";
    }

    private static string ExpandDecimal(string mantissa, int exponent)
    {
        var negative = mantissa.StartsWith('-');
        var unsigned = negative ? mantissa[1..] : mantissa;
        var decimalIndex = unsigned.IndexOf('.');
        var digits = decimalIndex < 0
            ? unsigned
            : unsigned.Remove(decimalIndex, 1);
        var originalDecimalPosition = decimalIndex < 0 ? unsigned.Length : decimalIndex;
        var decimalPosition = originalDecimalPosition + exponent;
        string expanded;
        if (decimalPosition <= 0)
        {
            expanded = $"0.{new string('0', -decimalPosition)}{digits}";
        }
        else if (decimalPosition >= digits.Length)
        {
            expanded = $"{digits}{new string('0', decimalPosition - digits.Length)}";
        }
        else
        {
            expanded = $"{digits[..decimalPosition]}.{digits[decimalPosition..]}";
        }

        return negative ? $"-{expanded}" : expanded;
    }
}
