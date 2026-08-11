using System.Security.Claims;
using System.Text.Encodings.Web;
using Flaggo.Hosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.Extensions.Options;

namespace Flaggo.DataPlane;

public sealed class LocalDevelopmentAuthenticationHandler(
    IOptionsMonitor<AuthenticationSchemeOptions> options,
    ILoggerFactory logger,
    UrlEncoder encoder,
    IConfiguration configuration)
    : AuthenticationHandler<AuthenticationSchemeOptions>(options, logger, encoder)
{
    protected override Task<AuthenticateResult> HandleAuthenticateAsync()
    {
        var resource =
            LocalDevelopmentIdentityHosting.FromConfiguration(configuration);
        var identity = new ClaimsIdentity(
        [
            new Claim(ClaimTypes.NameIdentifier, "local-development"),
            new Claim(
                "scope",
                "polari.decisions:decide polari.exposures:confirm"),
            new Claim("polari_app_id", resource.AppId),
            new Claim("polari_environment", resource.Environment),
            new Claim("polari_tenant_id", "local-development")
        ],
        Scheme.Name);
        var principal = new ClaimsPrincipal(identity);
        return Task.FromResult(
            AuthenticateResult.Success(
                new AuthenticationTicket(principal, Scheme.Name)));
    }
}
