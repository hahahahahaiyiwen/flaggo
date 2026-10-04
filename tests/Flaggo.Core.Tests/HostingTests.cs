using System.Security.Claims;
using Flaggo.Contract;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Http;

namespace Flaggo.Core.Tests;

public sealed class HostingTests
{
    [Fact]
    public void ReadsAuthorityScopeAndBothStandardScopeClaims()
    {
        var principal = Principal(
            new Claim(FlaggoClaimTypes.Tenant, "acme"),
            new Claim(FlaggoClaimTypes.Application, "tetris"),
            new Claim(FlaggoClaimTypes.Environment, "production"),
            new Claim("scope", "flaggo.contracts:read"),
            new Claim("scp", "flaggo.decisions:decide"));

        Assert.True(FlaggoClaims.TryGetAuthorityScope(principal, out var scope));
        Assert.Equal(new AuthorityScope("acme", "tetris", "production"), scope);
        Assert.True(FlaggoClaims.HasScope(principal, "flaggo.contracts:read"));
        Assert.True(FlaggoClaims.HasScope(principal, "flaggo.decisions:decide"));
    }

    [Fact]
    public void RejectsAmbiguousAuthorityScope()
    {
        var principal = Principal(
            new Claim(FlaggoClaimTypes.Tenant, "acme"),
            new Claim(FlaggoClaimTypes.Application, "one"),
            new Claim(FlaggoClaimTypes.Application, "two"),
            new Claim(FlaggoClaimTypes.Environment, "production"));

        Assert.False(FlaggoClaims.TryGetAuthorityScope(principal, out _));
    }

    [Fact]
    public async Task RejectsNonJsonRequestBodies()
    {
        var context = new DefaultHttpContext();
        context.Request.ContentType = "text/plain";

        var exception = await Assert.ThrowsAsync<HttpContractException>(
            async () => await HttpJson.ReadAsync<object>(context.Request, CancellationToken.None));

        Assert.Equal(StatusCodes.Status415UnsupportedMediaType, exception.Status);
        Assert.Equal(ProblemTypes.UnsupportedMediaType, exception.Type);
    }

    private static ClaimsPrincipal Principal(params Claim[] claims) =>
        new(new ClaimsIdentity(claims, "test"));
}
