using System.Security.Claims;
using Microsoft.Extensions.Configuration;

namespace Flaggo.Hosting;

public sealed record LocalDevelopmentIdentity(
    string AppId,
    string Environment);

public static class LocalDevelopmentIdentityHosting
{
    public static LocalDevelopmentIdentity FromConfiguration(
        IConfiguration configuration)
    {
        var appId =
            configuration["Flaggo:Authentication:LocalDevelopmentAppId"] ??
            "tetris-demo";
        var environment =
            configuration[
                "Flaggo:Authentication:LocalDevelopmentEnvironment"] ??
            "dev";
        if (string.IsNullOrWhiteSpace(appId) ||
            string.IsNullOrWhiteSpace(environment))
        {
            throw new InvalidOperationException(
                "Local-development application identity must include non-empty app and environment values.");
        }

        return new LocalDevelopmentIdentity(appId, environment);
    }

    public static ClaimsPrincipal CreatePrincipal(
        LocalDevelopmentIdentity resource,
        string scope,
        string authenticationType)
    {
        var identity = new ClaimsIdentity(
        [
            new Claim(ClaimTypes.NameIdentifier, "local-development"),
            new Claim("scope", scope),
            new Claim("polari_app_id", resource.AppId),
            new Claim("polari_environment", resource.Environment),
            new Claim("polari_tenant_id", "local-development")
        ],
        authenticationType);
        return new ClaimsPrincipal(identity);
    }
}
