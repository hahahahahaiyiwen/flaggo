using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Evidence;
using Flaggo.Lifecycle;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging.Abstractions;

namespace Flaggo.Decisioning.Tests;

public sealed class RegisteredProposalGovernanceTests
{
    [Theory]
    [InlineData("registered")]
    [InlineData("semantic-only")]
    [InlineData("build")]
    [InlineData("deployment")]
    [InlineData("artifact")]
    [InlineData("all")]
    public async Task RegisteredBundleCanGovernStrategiesWithIndependentProvenance(string provenance)
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var bundle = Bundle();
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("registration", bundle)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId, pending.BundleDigest, new ApprovalActor("registrar"), null);
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        var runtime = (await registry.ResolveRuntimeAsync("tetris-demo", "dev", "tetris.dropInterval",
            accepted.DefinitionId, accepted.Revision, CancellationToken.None)).Definition!;
        var intelligence = (await registry.ResolveIntelligenceAsync("tetris-demo", "dev", "tetris.dropInterval",
            accepted.DefinitionId, accepted.Revision, CancellationToken.None)).Definition!;
        Assert.Equal(runtime.Inputs.Select(input => input.Key).Order(StringComparer.Ordinal),
            intelligence.WorkflowPermissions.LiveInputs.Order(StringComparer.Ordinal));

        var contract = provenance switch
        {
            "semantic-only" => runtime.Identity with { BundleDigest = null },
            "build" => runtime.Identity with { BuildId = "producer-build" },
            "deployment" => runtime.Identity with { DeploymentId = "producer-deployment" },
            "artifact" => runtime.Identity with { ArtifactDigest = $"sha256:{new string('c', 64)}" },
            "all" => runtime.Identity with
            {
                BundleDigest = $"sha256:{new string('b', 64)}",
                BuildId = "producer-build",
                DeploymentId = "producer-deployment",
                ArtifactDigest = $"sha256:{new string('c', 64)}"
            },
            _ => runtime.Identity
        };
        var definition = new GovernedDefinitionIdentity("tetris-demo", "dev", "tetris.dropInterval", contract);
        var target = new DecisionTargetRef("cohort", "new_players");
        var evidence = new InMemoryProposalEvidenceReader(
        [
            new("evidence", definition with { Contract = runtime.Identity }, target,
                LifecycleTestData.Now, LifecycleTestData.Now.AddHours(1), new(0.9, 0.1, 0.8, 40))
        ]);
        var state = new InMemoryGovernedStateLifecycleStore(new Clock());
        var service = new ProposalGovernance(registry, registry,
            new ConfiguredLifecyclePolicyContextProvider(
            [
                LifecycleTestData.Policy with { AppId = "tetris-demo", DecisionKey = "tetris.dropInterval" }
            ]), evidence, new Actors(), new DefaultLifecyclePolicyEvaluator(), state, new Clock(),
            NullLogger<ProposalGovernance>.Instance);
        var proposal = new NumericStrategyDecisionProposal(
            new("proposal", new(DecisionProposalSourceKind.Scripted, "test"), definition, target,
                new(null, 0), "Govern a registered strategy.", ["evidence"], [],
                LifecycleTestData.Now, LifecycleTestData.Now.AddHours(1)),
            JsonSerializer.SerializeToElement(800), "strategy",
            new("tetris.boardPressure", 0.7, 850, 750));

        var review = await service.ReviewAsync(new("review", proposal), CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Approved, review.Disposition);
        var activation = await service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        Assert.Equal(LifecycleMutationStatus.Applied, activation.Status);
        var stored = (await state.GetReviewAsync("review", CancellationToken.None))!;
        Assert.Equal(contract, stored.Commit.Request.Proposal.Context.Definition.Contract);
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public async Task OnlyReferencedAmbiguousInputNamesFailRegistration(bool referenced)
    {
        var node = JsonNode.Parse(Bundle().GetRawText())!;
        var signals = node["signals"]!.AsArray();
        var duplicate = signals.Single(signal => signal!["key"]!.GetValue<string>() == "tetris.boardPressure")!.DeepClone();
        duplicate["key"] = "other.boardPressure";
        duplicate.AsObject().Remove("schemaDigest");
        signals.Add(duplicate);
        node["definitions"]![0]!["inference"]!["inputs"]!.AsArray()
            .Add(new JsonObject { ["key"] = "other.boardPressure" });
        if (!referenced)
        {
            var liveInputs = node["definitions"]![0]!["onlineStrategy"]!["liveInputs"]!.AsArray();
            liveInputs.Remove(liveInputs.Single(input => input!.GetValue<string>() == "boardPressure"));
        }
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await new InMemoryDefinitionRegistry([]).ValidateAsync(document.RootElement);
        Assert.Equal(referenced ? "invalid" : "valid", result.Status);
        Assert.Equal(referenced, result.Issues.Any(issue => issue.Code == "invalid-strategy"));
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(false, true)]
    [InlineData(true, false)]
    [InlineData(true, true)]
    public async Task MissingOrEmptyLiveInputsDoNotGrantPermissions(bool empty, bool removeInference)
    {
        var node = JsonNode.Parse(Bundle().GetRawText())!;
        var definition = node["definitions"]![0]!.AsObject();
        if (empty)
        {
            definition["onlineStrategy"]!["liveInputs"] = new JsonArray();
        }
        else
        {
            definition["onlineStrategy"]!.AsObject().Remove("liveInputs");
        }
        if (removeInference)
        {
            definition.Remove("inference");
        }
        using var document = JsonDocument.Parse(node.ToJsonString());
        var registry = new InMemoryDefinitionRegistry([]);
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("registration", document.RootElement)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId, pending.BundleDigest, new ApprovalActor("registrar"), null);
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        var intelligence = (await registry.ResolveIntelligenceAsync("tetris-demo", "dev", "tetris.dropInterval",
            accepted.DefinitionId, accepted.Revision, CancellationToken.None)).Definition!;

        Assert.Empty(intelligence.WorkflowPermissions.LiveInputs);
    }

    private static JsonElement Bundle()
    {
        var path = Path.Combine(TestPaths.RepositoryRoot, "examples", "tetris-integration",
            "tetris-definition-bundle.json");
        var node = JsonNode.Parse(File.ReadAllBytes(path))!;
        foreach (var definition in node["definitions"]!.AsArray())
        {
            definition!.AsObject().Remove("definitionId");
        }
        using var document = JsonDocument.Parse(node.ToJsonString());
        return document.RootElement.Clone();
    }

    private sealed class Clock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }

    private sealed class Actors : ILifecycleActorProvider
    {
        public Task<LifecycleActor> GetAsync(string appId, string environment, CancellationToken cancellationToken) =>
            Task.FromResult(LifecycleTestData.Actor with { AppId = appId, Environment = environment });
    }
}
