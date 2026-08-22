using System.Text.Json;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileGovernedStateLifecycleStoreTests
{
    private static readonly DateTimeOffset Now =
        new(2026, 8, 21, 20, 0, 0, TimeSpan.Zero);

    [Fact]
    public async Task RestartPreservesActivationReplayAndRuntimeProjection()
    {
        using var file = TestJsonFile.CreateCommitted("state-lifecycle");
        var firstStore = Store(file.Path, "state-1");
        var request = Request("activation-1", "proposal-1", new(null, 0), 800);
        var activated = await firstStore.ActivateAsync(
            request,
            CancellationToken.None);

        var restarted = Store(file.Path, "state-unexpected");
        var replay = await restarted.ActivateAsync(
            request,
            CancellationToken.None);
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        var projected = await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None);

        Assert.Equal("state-1", activated.StateId);
        Assert.Equal(activated.StateId, replay.StateId);
        Assert.Equal(activated.ProposalId, replay.ProposalId);
        Assert.Equal(activated.Generation, replay.Generation);
        Assert.Equal(activated.Value.GetRawText(), replay.Value.GetRawText());
        Assert.Equal(activated.StateId, projected!.StateId);
        Assert.Equal(activated.ProposalId, projected.ProposalId);
        Assert.Equal(activated.Generation, projected.Generation);
        Assert.Equal(activated.Value.GetRawText(), projected.Value.GetRawText());
    }

    [Fact]
    public async Task PublicationFailureLeavesPreviousAuthorityVisible()
    {
        using var file = TestJsonFile.CreateCommitted("state-publication-failure");
        var initial = Store(file.Path, "state-1");
        var first = await initial.ActivateAsync(
            Request("activation-1", "proposal-1", new(null, 0), 800),
            CancellationToken.None);
        var failing = new LocalFileGovernedStateLifecycleStore(
            new LocalFileGovernedStateLifecycleStoreOptions(file.Path),
            new FixedTimeProvider(),
            new SequenceStateIdentityGenerator(["state-2"]),
            new FailingPublisher());

        await Assert.ThrowsAsync<IOException>(
            () => failing.ActivateAsync(
                Request(
                    "activation-2",
                    "proposal-2",
                    new GovernedStateBaseline(
                        first.StateId,
                        first.Generation),
                    850),
                CancellationToken.None));

        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        var projected = await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None);
        Assert.Equal("state-1", projected!.StateId);
        Assert.Equal(800, projected.Value.GetInt32());
    }

    [Fact]
    public async Task ConcurrentInstancesRejectStaleReplacement()
    {
        using var file = TestJsonFile.CreateCommitted("state-concurrent-local");
        var initial = Store(file.Path, "state-1");
        var first = await initial.ActivateAsync(
            Request("activation-1", "proposal-1", new(null, 0), 800),
            CancellationToken.None);
        var baseline = new GovernedStateBaseline(
            first.StateId,
            first.Generation);
        var firstWriter = Store(file.Path, "state-2");
        var secondWriter = Store(file.Path, "state-3");

        var outcomes = await Task.WhenAll(
            new[]
            {
                (Store: firstWriter, Request: Request(
                    "activation-2",
                    "proposal-2",
                    baseline,
                    850)),
                (Store: secondWriter, Request: Request(
                    "activation-3",
                    "proposal-3",
                    baseline,
                    750))
            }.Select(async item =>
            {
                try
                {
                    return (State: await item.Store.ActivateAsync(
                        item.Request,
                        CancellationToken.None), Error: (Exception?)null);
                }
                catch (Exception error)
                {
                    return (State: (GovernedDecisionState?)null, Error: error);
                }
            }));

        Assert.Single(outcomes.Where(outcome => outcome.State is not null));
        var conflict = Assert.IsType<GovernedStateConflictException>(
            Assert.Single(outcomes.Where(outcome => outcome.Error is not null)).Error);
        Assert.Equal("stale-baseline", conflict.Code);
    }

    [Fact]
    public async Task LocalDocumentRejectsMixedApplicationScopes()
    {
        using var file = TestJsonFile.CreateCommitted("state-mixed-scope");
        var firstStore = Store(file.Path, "state-1");
        await firstStore.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var secondStore = Store(file.Path, "state-2");

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => secondStore.ActivateAsync(
                Request(
                    "activation-2",
                    "proposal-2",
                    new(null, 0),
                    850,
                    appId: "other-app"),
                CancellationToken.None));

        Assert.Contains(
            "exactly one application and environment scope",
            error.Message);
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        var projected = await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None);
        Assert.Equal("state-1", projected!.StateId);
    }

    [Fact]
    public async Task LegacyRuntimeDocumentIsReadableButNotLifecycleMutable()
    {
        using var file = TestJsonFile.CreateCommitted("state-legacy-lifecycle");
        await file.WriteAsync(
            JsonSerializer.Serialize(
                new
                {
                    version = 1,
                    states = new[]
                    {
                        new
                        {
                            decisionKey = "decision",
                            definitionId = "definition",
                            revision = "revision",
                            contractDigest =
                                $"sha256:{new string('a', 64)}",
                            value = 800,
                            controlTarget = (object?)null,
                            mode = "active-value",
                            strategyId = (string?)null,
                            numericRule = (object?)null,
                            lastChangedAt = Now.ToString("O")
                        }
                    }
                }));
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        Assert.NotNull(await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None));
        var lifecycle = Store(file.Path, "state-1");

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => lifecycle.ActivateAsync(
                Request(
                    "activation-1",
                    "proposal-1",
                    new(null, 0),
                    850),
                CancellationToken.None));

        Assert.Contains("must use version 2", error.Message);
    }

    private static LocalFileGovernedStateLifecycleStore Store(
        string path,
        params string[] stateIds) =>
        new(
            new LocalFileGovernedStateLifecycleStoreOptions(path),
            new FixedTimeProvider(),
            new SequenceStateIdentityGenerator(stateIds));

    private static GovernedStateActivationRequest Request(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline,
        int value,
        string appId = "app",
        string environment = "dev") =>
        new(
            activationId,
            $"approval-{activationId}",
            new FixedValueDecisionProposal(
                new DecisionProposalContext(
                    proposalId,
                    new DecisionProposalSource(
                        DecisionProposalSourceKind.Scripted,
                        "fixture"),
                    new GovernedDefinitionIdentity(
                        appId,
                        environment,
                        "decision",
                        new RuntimeContractIdentity(
                            "definition",
                            $"sha256:{new string('a', 64)}",
                            "revision")),
                    null,
                    baseline,
                    "Improve the governed value.",
                    ["evidence-1"],
                    ["confidence-1"],
                    Now.AddMinutes(-1),
                    Now.AddHours(1)),
                JsonSerializer.SerializeToElement(value)));

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => Now;
    }

    private sealed class SequenceStateIdentityGenerator(
        IEnumerable<string> stateIds) : IGovernedStateIdentityGenerator
    {
        private readonly Queue<string> _stateIds = new(stateIds);

        public string CreateStateId() => _stateIds.Dequeue();
    }

    private sealed class FailingPublisher : IGovernedStateDocumentPublisher
    {
        public Task PublishAsync(
            string commitDescriptorPath,
            ReadOnlyMemory<byte> bytes,
            CancellationToken cancellationToken) =>
            throw new IOException("Injected publication failure.");
    }
}
