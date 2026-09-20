using System.Security.Claims;
using System.Text;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.ControlPlane;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Lifecycle;
using Flaggo.Policy;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Flaggo.Decisioning.Tests;

public sealed class LifecycleHostingTests
{
    [Theory]
    [InlineData("scope", "polari.lifecycle:review", true, false)]
    [InlineData("scp", "polari.lifecycle:activate", false, true)]
    [InlineData("scope", "polari.definitions:approve", false, false)]
    public async Task ActorAuthorityComesFromAuthenticatedClaims(
        string scopeType, string scope, bool canReview, bool canActivate)
    {
        var accessor = new HttpContextAccessor
        {
            HttpContext = new DefaultHttpContext { User = Principal(scope, scopeType) }
        };
        var actor = await new HttpLifecycleActorProvider(accessor)
            .GetAsync("tetris-demo", "dev", CancellationToken.None);

        Assert.Equal("subject", actor.Subject);
        Assert.Equal("test-issuer", actor.Issuer);
        Assert.Equal(canReview, actor.CanReview);
        Assert.Equal(canActivate, actor.CanActivate);
    }

    [Theory]
    [InlineData("no-context")]
    [InlineData("anonymous")]
    [InlineData("missing-subject")]
    [InlineData("wrong-app")]
    [InlineData("wrong-environment")]
    public async Task ActorMappingRejectsMissingOrCrossScopeIdentity(string scenario)
    {
        var principal = Principal();
        if (scenario == "anonymous")
        {
            principal = new ClaimsPrincipal(new ClaimsIdentity(principal.Claims));
        }
        if (scenario == "missing-subject")
        {
            principal = new ClaimsPrincipal(new ClaimsIdentity(
                principal.Claims.Where(claim => claim.Type != ClaimTypes.NameIdentifier), "test"));
        }
        var accessor = new HttpContextAccessor
        {
            HttpContext = scenario == "no-context"
                ? null
                : new DefaultHttpContext { User = principal }
        };
        var error = await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            new HttpLifecycleActorProvider(accessor).GetAsync(
                scenario == "wrong-app" ? "other-app" : "tetris-demo",
                scenario == "wrong-environment" ? "prod" : "dev",
                CancellationToken.None));
        Assert.Equal("actor-not-authorized", error.Code);
    }

    [Fact]
    public void LifecycleCompositionCannotWriteBehindABootstrapGeneration()
    {
        using var state = TestJsonFile.CreateCommitted("lifecycle-host-bootstrap");
        using var registry = new TestJsonFile("lifecycle-host-registry");
        using var factory = new LifecycleFactory(state.Path, null, registry.Path);
        factory.Services.GetRequiredService<IConfiguration>()["Flaggo:Bootstrap:LocalGenerationPath"] = state.Path;
        using var scope = factory.Services.CreateScope();
        var error = Assert.Throws<InvalidOperationException>(() =>
            scope.ServiceProvider.GetRequiredService<IProposalGovernance>());
        Assert.Contains("not a bootstrap generation", error.Message);
    }

    [Fact]
    public async Task SupplementalAnonymousClaimsCannotGrantLifecycleAuthority()
    {
        var principal = Principal("polari.definitions:approve");
        principal.AddIdentity(new ClaimsIdentity([new Claim("scope", "polari.lifecycle:activate")]));
        var accessor = new HttpContextAccessor
        {
            HttpContext = new DefaultHttpContext { User = principal }
        };
        var actor = await new HttpLifecycleActorProvider(accessor)
            .GetAsync("tetris-demo", "dev", CancellationToken.None);
        Assert.False(actor.CanActivate);
    }

    [Fact]
    public async Task ComposedHostAutomaticallyApprovesAndPublishesTheSeededTetrisStrategy()
    {
        using var state = TestJsonFile.CreateCommitted("lifecycle-host-state");
        using var evidence = TestJsonFile.CreateCommitted("lifecycle-host-evidence");
        using var registry = new TestJsonFile("lifecycle-host-registry");
        var definition = LocalRegistryHosting.DefaultDefinitions().Single(item => item.LifecycleStatus == "active");
        var identity = new GovernedDefinitionIdentity(
            definition.AppId, definition.Environment, definition.DecisionKey, definition.Identity);
        var target = new DecisionTargetRef("cohort", "new_players");
        await evidence.WriteAsync(LifecycleJson.Bytes(new ProposalEvidenceDocument(1,
        [
            new("evidence-1", identity, target, LifecycleTestData.Now.AddMinutes(-1),
                LifecycleTestData.Now.AddHours(1), new DecisionEvidenceSnapshot(0.9, 0.1, 0.8, 40))
        ])));
        using var factory = new LifecycleFactory(state.Path, evidence.Path, registry.Path);
        using var scope = factory.Services.CreateScope();
        scope.ServiceProvider.GetRequiredService<IHttpContextAccessor>().HttpContext =
            new DefaultHttpContext { User = Principal() };
        var governance = scope.ServiceProvider.GetRequiredService<IProposalGovernance>();
        var proposal = new NumericStrategyDecisionProposal(
            new DecisionProposalContext(
                "proposal", new DecisionProposalSource(DecisionProposalSourceKind.Scripted, "host-test"),
                identity, target, new GovernedStateBaseline(null, 0), "Evaluate the seeded strategy.",
                ["evidence-1"], [], LifecycleTestData.Now.AddMinutes(-1), LifecycleTestData.Now.AddHours(1)),
            JsonSerializer.SerializeToElement(800), "strategy",
            new NumericRuleStrategy("tetris.boardPressure", 0.7, 850, 750));

        var review = await governance.ReviewAsync(new("review", proposal), CancellationToken.None);
        var activation = await governance.ActivateAsync(new("activation", "review"), CancellationToken.None);
        var lifecycle = scope.ServiceProvider.GetRequiredService<IGovernedStateLifecycleStore>();
        var audit = scope.ServiceProvider.GetRequiredService<ILifecycleAuditReader>();
        var trail = await audit.ReadAsync("tetris-demo", "dev", CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Approved, review.Disposition);
        Assert.Equal(LifecycleMutationStatus.Applied, activation.Status);
        Assert.Same(lifecycle, audit);
        Assert.Equal("subject", review.Approval!.Initiator.Subject);
        Assert.Contains(trail.Records, record => record.Kind == LifecycleAuditKind.AutomaticApproval);
        Assert.Contains(trail.Records, record => record.Kind == LifecycleAuditKind.Activation);
        Assert.Equal(activation.StateId,
            (await lifecycle.GetBaselineAsync(
                new("tetris-demo", "dev", "tetris.dropInterval", target), CancellationToken.None))!.StateId);
        var runtimeState = await new LocalFileStateStore(new LocalFileStateStoreOptions(state.Path))
            .GetActiveAsync(definition.DecisionKey, definition.Identity.DefinitionId,
                definition.Identity.Revision, [target], CancellationToken.None);
        Assert.Equal(activation.StateId, runtimeState!.StateId);
    }

    [Fact]
    public async Task PolicyReadsObserveUpdatedOperatorControlsAndRejectUnknownSettings()
    {
        using var state = TestJsonFile.CreateCommitted("lifecycle-host-policy");
        using var registry = new TestJsonFile("lifecycle-host-registry");
        using var factory = new LifecycleFactory(state.Path, null, registry.Path);
        var provider = factory.Services.GetRequiredService<ILifecyclePolicyContextProvider>();
        var configuration = factory.Services.GetRequiredService<IConfiguration>();
        var definition = LifecycleTestData.Definition with
        {
            AppId = "tetris-demo",
            DecisionKey = "tetris.dropInterval"
        };
        Assert.False((await provider.GetAsync(definition, CancellationToken.None))!.OperatorControls.Paused);

        configuration["Flaggo:Lifecycle:Policies:0:OperatorControls:Paused"] = "true";
        Assert.True((await provider.GetAsync(definition, CancellationToken.None))!.OperatorControls.Paused);

        configuration["Flaggo:Lifecycle:Policies:0:OperatorControls:UnexpectedPermission"] = "true";
        await Assert.ThrowsAsync<InvalidOperationException>(() =>
            provider.GetAsync(definition, CancellationToken.None));
    }

    [Fact]
    public void MissingStateConfigurationDoesNotCreateAnInMemoryWriter()
    {
        using var registry = new TestJsonFile("lifecycle-host-no-state");
        using var factory = new LifecycleFactory(null, null, registry.Path);
        using var scope = factory.Services.CreateScope();
        var error = Assert.Throws<InvalidOperationException>(() =>
            scope.ServiceProvider.GetRequiredService<IProposalGovernance>());
        Assert.Contains("Flaggo:State:LocalFilePath", error.Message);
    }

    [Fact]
    public async Task ConfiguredEvidenceFailureDoesNotFallBackToAnEmptyCatalog()
    {
        using var state = TestJsonFile.CreateCommitted("lifecycle-host-state");
        using var evidence = TestJsonFile.CreateCommitted("lifecycle-host-missing-evidence");
        using var registry = new TestJsonFile("lifecycle-host-registry");
        using var factory = new LifecycleFactory(state.Path, evidence.Path, registry.Path);
        var reader = factory.Services.GetRequiredService<IProposalEvidenceReader>();
        await Assert.ThrowsAsync<EvidenceUnavailableException>(() =>
            reader.GetAsync(new(LifecycleTestData.Definition, LifecycleTestData.Target, []), CancellationToken.None));
    }

    private static ClaimsPrincipal Principal(
        string scopes = "polari.lifecycle:review polari.lifecycle:activate",
        string scopeType = "scope") =>
        new(new ClaimsIdentity(
        [
            new Claim(ClaimTypes.NameIdentifier, "subject", ClaimValueTypes.String, "test-issuer"),
            new Claim(scopeType, scopes),
            new Claim("polari_app_id", "tetris-demo"),
            new Claim("polari_environment", "dev")
        ], "test"));

    private sealed class LifecycleFactory(string? statePath, string? evidencePath, string registryPath)
        : WebApplicationFactory<ControlPlaneAssemblyMarker>
    {
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting("Flaggo:Authentication:LocalDevelopmentBypass", "true");
            builder.UseSetting("Flaggo:Registry:LocalFilePath", registryPath);
            var configuration = JsonSerializer.Serialize(new
            {
                Flaggo = new
                {
                    State = new { LocalFilePath = statePath },
                    Lifecycle = new
                    {
                        EvidencePath = evidencePath,
                        Policies = new[]
                        {
                            LifecycleTestData.Policy with
                            {
                                AppId = "tetris-demo",
                                DecisionKey = "tetris.dropInterval"
                            }
                        }
                    }
                }
            });
            builder.ConfigureAppConfiguration((_, config) =>
                config.AddJsonStream(new MemoryStream(Encoding.UTF8.GetBytes(configuration))));
            builder.ConfigureServices(services =>
            {
                services.RemoveAll<TimeProvider>();
                services.AddSingleton<TimeProvider>(new FixedClock());
            });
        }
    }

    private sealed class FixedClock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }
}
