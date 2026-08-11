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
}
