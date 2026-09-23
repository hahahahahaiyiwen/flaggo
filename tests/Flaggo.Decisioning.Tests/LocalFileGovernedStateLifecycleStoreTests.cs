using System.Text.Json;
using System.Text.Json.Nodes;
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
    public async Task RestartPreservesDerivedNumericRuleStrategyIdentity()
    {
        using var file = TestJsonFile.CreateCommitted("state-numeric-rule");
        var firstStore = Store(file.Path, "state-1");
        var request = NumericRuleRequest(
            "activation-1",
            "proposal-1",
            new(null, 0));
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

        Assert.StartsWith("strategy_", activated.StrategyId);
        Assert.Equal(activated.StrategyId, replay.StrategyId);
        Assert.Equal(activated.StrategyId, projected!.StrategyId);
        Assert.Equal("numeric-rule", projected.Mode);
        Assert.Equal(
            activated.NumericRule!.Threshold,
            projected.NumericRule!.Threshold);
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
    public async Task CancellationLeavesPreviousAuthorityVisible()
    {
        using var file = TestJsonFile.CreateCommitted("state-cancellation");
        var initial = Store(file.Path, "state-1");
        var first = await initial.ActivateAsync(
            Request("activation-1", "proposal-1", new(null, 0), 800),
            CancellationToken.None);
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => Store(file.Path, "state-2").ActivateAsync(
                Request(
                    "activation-2",
                    "proposal-2",
                    new GovernedStateBaseline(
                        first.StateId,
                        first.Generation),
                    850),
                cancellation.Token));

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
    public async Task RuntimeRejectsMultipleActiveRevisionsAtOneAuthorityAddress()
    {
        using var file =
            TestJsonFile.CreateCommitted("state-duplicate-active-address");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        var states = document["states"]!.AsArray();
        var duplicate = states[0]!.DeepClone().AsObject();
        duplicate["stateId"] = "state-2";
        duplicate["proposalId"] = "proposal-2";
        duplicate["revision"] = "revision-2";
        duplicate["generation"] = 2;
        duplicate["predecessorStateId"] = "state-1";
        states.Add(duplicate);
        await file.WriteAsync(document.ToJsonString());
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => runtime.GetActiveAsync(
                "decision",
                "definition",
                "revision",
                [null],
                CancellationToken.None));

        Assert.Contains(
            "Only one governed state may be active",
            error.Message);
    }

    [Fact]
    public async Task RemovedTransitionReplayFieldIsRejected()
    {
        using var file = TestJsonFile.CreateCommitted("state-old-transitions");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        document["transitions"] = new JsonArray();
        await file.WriteAsync(document.ToJsonString());
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => runtime.GetActiveAsync(
                "decision",
                "definition",
                "revision",
                [null],
                CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => Store(file.Path, "state-2").ActivateAsync(
                Request(
                    "activation-2",
                    "proposal-2",
                    new(null, 0),
                    850),
                CancellationToken.None));
    }

    [Theory]
    [InlineData("missing-collection")]
    [InlineData("null-collection")]
    [InlineData("invalid-fingerprint")]
    [InlineData("duplicate-activation")]
    [InlineData("duplicate-proposal")]
    [InlineData("missing-state")]
    public async Task RuntimeRejectsInvalidActivationReplay(
        string corruption)
    {
        using var file = TestJsonFile.CreateCommitted(
            $"state-invalid-activation-{corruption}");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        var activations = document["activations"]!.AsArray();
        switch (corruption)
        {
            case "missing-collection":
                document.Remove("activations");
                break;
            case "null-collection":
                document["activations"] = null;
                break;
            case "invalid-fingerprint":
                activations[0]!["fingerprint"] = "invalid";
                break;
            case "duplicate-activation":
                activations.Add(activations[0]!.DeepClone());
                break;
            case "duplicate-proposal":
                var duplicate = activations[0]!.DeepClone().AsObject();
                duplicate["activationId"] = "activation-2";
                activations.Add(duplicate);
                break;
            case "missing-state":
                activations[0]!["stateId"] = "state-missing";
                break;
            default:
                throw new InvalidOperationException(
                    $"Unknown corruption case '{corruption}'.");
        }

        await file.WriteAsync(document.ToJsonString());
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => runtime.GetActiveAsync(
                "decision",
                "definition",
                "revision",
                [null],
                CancellationToken.None));
        Assert.False(await runtime.IsAvailableAsync(
            CancellationToken.None));
    }

    [Theory]
    [InlineData("missing-replay")]
    [InlineData("proposal-mismatch")]
    [InlineData("duplicate-state-reference")]
    public async Task ReadersRejectActivationReplayThatDoesNotBindToState(
        string corruption)
    {
        using var file = TestJsonFile.CreateCommitted(
            $"state-invalid-replay-binding-{corruption}");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        var activations = document["activations"]!.AsArray();
        switch (corruption)
        {
            case "missing-replay":
                activations.Clear();
                break;
            case "proposal-mismatch":
                activations[0]!["proposalId"] = "proposal-other";
                break;
            case "duplicate-state-reference":
                var duplicate = activations[0]!.DeepClone().AsObject();
                duplicate["activationId"] = "activation-2";
                duplicate["proposalId"] = "proposal-2";
                activations.Add(duplicate);
                break;
            default:
                throw new InvalidOperationException(
                    $"Unknown corruption case '{corruption}'.");
        }

        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Fact]
    public async Task ReadersRejectStrategyIdentityThatDoesNotMatchActivation()
    {
        using var file = TestJsonFile.CreateCommitted(
            "state-invalid-strategy-identity");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            NumericRuleRequest(
                "activation-1",
                "proposal-1",
                new(null, 0)),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        document["states"]![0]!["strategyId"] = "strategy_tampered";
        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Theory]
    [InlineData("first-predecessor")]
    [InlineData("dangling-predecessor")]
    [InlineData("generation-gap")]
    public async Task ReadersRejectInvalidPredecessorLineage(
        string corruption)
    {
        using var file = TestJsonFile.CreateCommitted(
            $"state-invalid-lineage-{corruption}");
        var lifecycle = Store(file.Path, "state-1", "state-2");
        var first = await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        await lifecycle.ActivateAsync(
            Request(
                "activation-2",
                "proposal-2",
                new(first.StateId, first.Generation),
                850),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        var states = document["states"]!.AsArray();
        switch (corruption)
        {
            case "first-predecessor":
                states[0]!["predecessorStateId"] = "state-unexpected";
                break;
            case "dangling-predecessor":
                states[1]!["predecessorStateId"] = "state-missing";
                break;
            case "generation-gap":
                states[1]!["generation"] = 3;
                break;
            default:
                throw new InvalidOperationException(
                    $"Unknown corruption case '{corruption}'.");
        }

        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Fact]
    public async Task ReadersRejectNoncanonicalNumericRuleNumber()
    {
        using var file = TestJsonFile.CreateCommitted(
            "state-noncanonical-rule-number");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            NumericRuleRequest(
                "activation-1",
                "proposal-1",
                new(null, 0)),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        document["states"]![0]!["numericRule"]!["threshold"] =
            9_007_199_254_740_993L;
        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Theory]
    [InlineData("empty-inputs")]
    [InlineData("threshold-below-range")]
    [InlineData("threshold-above-range")]
    public async Task ReadersRejectInvalidWeightedRuleShape(
        string invalidRule)
    {
        using var file = TestJsonFile.CreateCommitted(
            $"state-invalid-weighted-rule-{invalidRule}");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            NumericRuleRequest(
                "activation-1",
                "proposal-1",
                new(null, 0)),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        var rule = document["states"]![0]!["numericRule"]!;
        rule["weightedInputs"] = invalidRule == "empty-inputs"
            ? new JsonArray()
            : JsonSerializer.SerializeToNode(
                new[]
                {
                    new
                    {
                        signalKey = "pressure",
                        minimum = 0,
                        maximum = 1,
                        weight = 1
                    }
                });
        if (invalidRule == "threshold-below-range")
        {
            rule["threshold"] = -0.1;
        }
        else if (invalidRule == "threshold-above-range")
        {
            rule["threshold"] = 1.1;
        }

        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Fact]
    public async Task ReadersRejectUnequalLifecycleTimestamps()
    {
        using var file = TestJsonFile.CreateCommitted(
            "state-unequal-lifecycle-timestamps");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        document["states"]![0]!["lastChangedAt"] =
            Now.AddMinutes(-5).ToString("O");
        await file.WriteAsync(document.ToJsonString());

        await AssertInvalidLifecycleDocument(file.Path);
    }

    [Theory]
    [InlineData("pending")]
    [InlineData("completed")]
    [InlineData("expired")]
    [InlineData("rolled-back")]
    public async Task RemovedLifecycleStatusIsRejected(string status)
    {
        using var file = TestJsonFile.CreateCommitted(
            $"state-removed-status-{status}");
        var lifecycle = Store(file.Path, "state-1");
        await lifecycle.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                new(null, 0),
                800),
            CancellationToken.None);
        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            options: null,
            CancellationToken.None);
        var document = JsonNode.Parse(bytes)!.AsObject();
        document["states"]![0]!["lifecycleStatus"] = status;
        await file.WriteAsync(document.ToJsonString());

        await Assert.ThrowsAsync<InvalidDataException>(
            () => Store(file.Path, "state-2").ActivateAsync(
                Request(
                    "activation-2",
                    "proposal-2",
                    new(null, 0),
                    850),
                CancellationToken.None));
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

    private static async Task AssertInvalidLifecycleDocument(string path)
    {
        var runtime = new LocalFileStateStore(
            new LocalFileStateStoreOptions(path));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => runtime.GetActiveAsync(
                "decision",
                "definition",
                "revision",
                [null],
                CancellationToken.None));
        Assert.False(await runtime.IsAvailableAsync(
            CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => Store(path, "state-unexpected").GetBaselineAsync(
                new GovernedStateAddress(
                    "app",
                    "dev",
                    "decision",
                    null),
                CancellationToken.None));
    }

    private static GovernedStateActivationRequest Request(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline,
        int value,
        string appId = "app",
        string environment = "dev") =>
        new(
            activationId,
            proposalId,
            $"approval-{activationId}",
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
            new ActiveValueActivationCandidate(
                "Improve the governed value.",
                JsonSerializer.SerializeToElement(value)));

    private static GovernedStateActivationRequest NumericRuleRequest(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline) =>
        new(
            activationId,
            proposalId,
            $"approval-{activationId}",
            new GovernedDefinitionIdentity(
                "app",
                "dev",
                "decision",
                new RuntimeContractIdentity(
                    "definition",
                    $"sha256:{new string('a', 64)}",
                    "revision")),
            null,
            baseline,
            new NumericRuleActivationCandidate(
                "Adapt the governed value.",
                JsonSerializer.SerializeToElement(800),
                new NumericRuleStrategy(
                    "pressure",
                    0.5,
                    750,
                    850)));

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
