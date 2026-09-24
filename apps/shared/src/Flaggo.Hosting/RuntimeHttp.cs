using System.Security.Cryptography;
using System.Security.Claims;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Microsoft.Net.Http.Headers;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Microsoft.AspNetCore.Http;

namespace Flaggo.Hosting;

public static class RuntimeHttp
{
    private static readonly object CorrelationIdItemKey = new();

    private static readonly Regex Sha256DigestPattern = new(
        "\\Asha256:[0-9a-f]{64}\\z",
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
            StrictJson.Validate(body);
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
        using var document = JsonDocument.Parse(json);
        using var memory = new MemoryStream();
        var requestIdentity = Encoding.UTF8.GetBytes(
            $"POST\n/v1/decisions/{{decisionKey}}:decide\ndecisionKey={decisionKey}\nv1\napplication/json\n");
        memory.Write(requestIdentity);
        memory.Write(CanonicalJson.Canonicalize(document.RootElement));

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
        CanonicalJson.IsIeee754CompatibleNumber(value);

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

    private static void EnsureRequestNullability<T>(byte[] json)
    {
        using var document = JsonDocument.Parse(json);
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
            }
            else if (typeof(T) == typeof(ExposureConfirmationRequest))
            {
                RejectNullProperties(document.RootElement, "confirmToken", "appliedAt");
            }
            else if (typeof(T) == typeof(ApproveDefinitionBundleRequest))
            {
                RejectNullProperties(
                    document.RootElement,
                    "expectedBundleDigest",
                    "comment");
            }
            else if (typeof(T) == typeof(RejectDefinitionBundleRequest))
            {
                RejectNullProperties(
                    document.RootElement,
                    "expectedBundleDigest",
                    "reasonCode",
                    "comment");
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

}
