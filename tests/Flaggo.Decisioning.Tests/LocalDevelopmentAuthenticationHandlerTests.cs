using System.Security.Claims;
using System.Text.Encodings.Web;
using Flaggo.Hosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using ControlPlaneHandler =
    Flaggo.ControlPlane.LocalDevelopmentAuthenticationHandler;
using DataPlaneHandler =
    Flaggo.DataPlane.LocalDevelopmentAuthenticationHandler;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalDevelopmentAuthenticationHandlerTests
{
    private const string Scheme = "LocalDevelopment";

    [Fact]
    public async Task ControlPlane_UsesConfiguredResourceAndManagementScope()
    {
        var result = await AuthenticateAsync(
            new ControlPlaneHandler(
                new TestOptionsMonitor<AuthenticationSchemeOptions>(
                    new AuthenticationSchemeOptions()),
                NullLoggerFactory.Instance,
                UrlEncoder.Default,
                ConfiguredIdentity()),
            typeof(ControlPlaneHandler));

        AssertPrincipal(
            result,
            "polari.definitions:validate polari.definitions:apply " +
            "polari.definitions:approve");
    }

    [Fact]
    public async Task DataPlane_UsesConfiguredResourceAndRuntimeScope()
    {
        var result = await AuthenticateAsync(
            new DataPlaneHandler(
                new TestOptionsMonitor<AuthenticationSchemeOptions>(
                    new AuthenticationSchemeOptions()),
                NullLoggerFactory.Instance,
                UrlEncoder.Default,
                ConfiguredIdentity()),
            typeof(DataPlaneHandler));

        AssertPrincipal(
            result,
            "polari.decisions:decide polari.exposures:confirm");
    }

    [Theory]
    [InlineData("Flaggo:Authentication:LocalDevelopmentAppId")]
    [InlineData("Flaggo:Authentication:LocalDevelopmentEnvironment")]
    public void ConfiguredResource_RejectsEmptyValues(string key)
    {
        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(
                new Dictionary<string, string?>
                {
                    [key] = " "
                })
            .Build();

        Assert.Throws<InvalidOperationException>(
            () => LocalDevelopmentIdentityHosting.FromConfiguration(
                configuration));
    }

    private static IConfiguration ConfiguredIdentity()
    {
        return new ConfigurationBuilder()
            .AddInMemoryCollection(
                new Dictionary<string, string?>
                {
                    ["Flaggo:Authentication:LocalDevelopmentAppId"] =
                        "configured-app",
                    ["Flaggo:Authentication:LocalDevelopmentEnvironment"] =
                        "configured-environment"
                })
            .Build();
    }

    private static async Task<AuthenticateResult> AuthenticateAsync(
        IAuthenticationHandler handler,
        Type handlerType)
    {
        await handler.InitializeAsync(
            new AuthenticationScheme(Scheme, Scheme, handlerType),
            new DefaultHttpContext());
        return await handler.AuthenticateAsync();
    }

    private static void AssertPrincipal(
        AuthenticateResult result,
        string expectedScope)
    {
        Assert.True(result.Succeeded);
        var ticket = Assert.IsType<AuthenticationTicket>(result.Ticket);
        Assert.Equal(Scheme, ticket.AuthenticationScheme);
        var identity = Assert.IsType<ClaimsIdentity>(ticket.Principal.Identity);
        Assert.Equal(Scheme, identity.AuthenticationType);
        Assert.Equal(
        [
            (ClaimTypes.NameIdentifier, "local-development"),
            ("scope", expectedScope),
            ("polari_app_id", "configured-app"),
            ("polari_environment", "configured-environment"),
            ("polari_tenant_id", "local-development")
        ],
        ticket.Principal.Claims
            .Select(claim => (claim.Type, claim.Value))
            .ToArray());
    }

    private sealed class TestOptionsMonitor<TOptions>(TOptions currentValue)
        : IOptionsMonitor<TOptions>
    {
        public TOptions CurrentValue => currentValue;

        public TOptions Get(string? name)
        {
            return currentValue;
        }

        public IDisposable? OnChange(
            Action<TOptions, string?> listener)
        {
            return null;
        }
    }
}
