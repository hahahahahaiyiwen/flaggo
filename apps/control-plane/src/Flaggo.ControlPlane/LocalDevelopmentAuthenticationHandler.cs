using System.Security.Claims;
using System.Text.Encodings.Web;
using Flaggo.Hosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.Extensions.Options;

namespace Flaggo.ControlPlane;

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
                "polari.definitions:validate polari.definitions:apply " +
                "polari.definitions:approve"),
            new Claim("polari_app_id", resource.AppId),
            new Claim("polari_environment", resource.Environment),
            new Claim("polari_tenant_id", "local-development")
        ],
        Scheme.Name);
        return Task.FromResult(
            AuthenticateResult.Success(
                new AuthenticationTicket(new ClaimsPrincipal(identity), Scheme.Name)));
    }
}
