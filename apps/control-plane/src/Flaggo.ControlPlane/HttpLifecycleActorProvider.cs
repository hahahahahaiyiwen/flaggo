using System.Security.Claims;
using Flaggo.Hosting;
using Flaggo.Lifecycle;
using Flaggo.Shared.Contracts;

namespace Flaggo.ControlPlane;

public sealed class HttpLifecycleActorProvider(IHttpContextAccessor contexts) : ILifecycleActorProvider
{
    public Task<LifecycleActor> GetAsync(
        string appId,
        string environment,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (contexts.HttpContext?.User.Identity is not ClaimsIdentity { IsAuthenticated: true } identity)
        {
            throw Unauthorized();
        }

        var principal = new ClaimsPrincipal(identity);
        var subject = identity.FindFirst(ClaimTypes.NameIdentifier) ?? identity.FindFirst("sub");
        if (subject is null ||
            string.IsNullOrWhiteSpace(subject.Value) || string.IsNullOrWhiteSpace(subject.Issuer) ||
            !RuntimeHttp.HasClientScope(principal, appId, environment))
        {
            throw Unauthorized();
        }

        return Task.FromResult(new LifecycleActor(
            subject.Value,
            subject.Issuer,
            appId,
            environment,
            RuntimeHttp.HasScope(principal, "polari.lifecycle:review"),
            RuntimeHttp.HasScope(principal, "polari.lifecycle:activate")));
    }

    private static LifecycleGovernanceException Unauthorized() =>
        new("actor-not-authorized", "An authenticated lifecycle actor in the requested resource scope is required.");
}
