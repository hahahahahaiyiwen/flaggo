using System.Security.Claims;
using Flaggo.Contract;

namespace Flaggo.ServiceHosting;

public static class FlaggoClaimTypes
{
    public const string Application = "flaggo_application";
    public const string Environment = "flaggo_environment";
}

public static class FlaggoClaims
{
    public static bool HasScope(ClaimsPrincipal principal, string requiredScope)
    {
        ArgumentNullException.ThrowIfNull(principal);
        ArgumentException.ThrowIfNullOrWhiteSpace(requiredScope);

        return principal.Claims
            .Where(claim => claim.Type is "scope" or "scp")
            .SelectMany(claim => claim.Value.Split(
                ' ',
                StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            .Contains(requiredScope, StringComparer.Ordinal);
    }

    public static bool TryGetAuthorityScope(
        ClaimsPrincipal principal,
        out DecisionScope scope)
    {
        ArgumentNullException.ThrowIfNull(principal);

        var application = SingleNonEmptyClaim(principal, FlaggoClaimTypes.Application);
        var environment = SingleNonEmptyClaim(principal, FlaggoClaimTypes.Environment);
        if (application is null || environment is null)
        {
            scope = default;
            return false;
        }

        scope = new DecisionScope(application, environment);
        return true;
    }

    private static string? SingleNonEmptyClaim(ClaimsPrincipal principal, string claimType)
    {
        var values = principal.FindAll(claimType)
            .Select(claim => claim.Value)
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Distinct(StringComparer.Ordinal)
            .Take(2)
            .ToArray();
        return values.Length == 1 ? values[0] : null;
    }
}
