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
        var principal = LocalDevelopmentIdentityHosting.CreatePrincipal(
            resource,
            "polari.definitions:validate polari.definitions:apply " +
            "polari.definitions:approve " +
            "polari.lifecycle:review polari.lifecycle:activate",
            Scheme.Name);
        return Task.FromResult(
            AuthenticateResult.Success(
                new AuthenticationTicket(principal, Scheme.Name)));
    }
}
