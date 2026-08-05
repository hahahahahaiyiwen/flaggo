using System.Security.Claims;
using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.DataPlane;

public static class ManagementHttp
{
    public static FlaggoProblem? AuthorizeBundle(
        HttpContext context,
        JsonElement bundle)
    {
        if (bundle.ValueKind != JsonValueKind.Object ||
            !bundle.TryGetProperty("application", out var application) ||
            application.ValueKind != JsonValueKind.Object ||
            !application.TryGetProperty("id", out var appId) ||
            appId.ValueKind != JsonValueKind.String ||
            !application.TryGetProperty("environment", out var environment) ||
            environment.ValueKind != JsonValueKind.String)
        {
            return RuntimeHttp.Problem(
                context,
                400,
                "invalid-bundle-scope",
                "A bundle application id and environment are required for authorization.");
        }

        return AuthorizeScope(
            context,
            appId.GetString()!,
            environment.GetString()!);
    }

    public static FlaggoProblem? AuthorizeScope(
        HttpContext context,
        string appId,
        string environment) =>
        RuntimeHttp.HasClientScope(context.User, appId, environment)
            ? null
            : RuntimeHttp.Problem(
                context,
                403,
                "resource-scope-mismatch",
                "The token is not authorized for the requested application and environment.");

    public static ApprovalActor Actor(HttpContext context)
    {
        var subject = context.User.FindFirstValue(ClaimTypes.NameIdentifier)
            ?? context.User.FindFirstValue("sub")
            ?? throw new InvalidOperationException(
                "An authenticated approval subject is required.");
        var displayName = context.User.FindFirstValue(ClaimTypes.Name);
        return new ApprovalActor(subject, displayName);
    }

    public static IResult Problem(
        HttpContext context,
        DefinitionLifecycleException exception) =>
        RuntimeHttp.ProblemResult(
            context,
            RuntimeHttp.Problem(
                context,
                exception.Status,
                exception.Code,
                exception.Message,
                exception.Issues));

    public static string ContentDigest(string bundleDigest)
    {
        var bytes = Convert.FromHexString(bundleDigest["sha256:".Length..]);
        return $"sha-256=:{Convert.ToBase64String(bytes)}:";
    }
}
