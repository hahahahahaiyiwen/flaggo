using System.Text;
using System.Text.Json;
using Flaggo.DataPlane;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class RuntimeStateTests
{
    [Fact]
    public void Fingerprint_RejectsNumberThatWouldCollideAfterRounding()
    {
        var body = Encoding.UTF8.GetBytes(
            """{"runtimeContext":{"score":9007199254740993}}""");

        Assert.Throws<JsonException>(
            () => RuntimeHttp.Fingerprint(body, "decision.test"));
    }

    [Fact]
    public async Task IdempotencyStore_ReplaysOriginalResult()
    {
        var calls = 0;
        var store = new InMemoryDecideIdempotencyStore(new FixedTimeProvider());

        var first = await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "fingerprint",
            _ => Task.FromResult(CreateOutcome($"decision-{++calls}")),
            CancellationToken.None);
        var replay = await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "fingerprint",
            _ => Task.FromResult(CreateOutcome($"decision-{++calls}")),
            CancellationToken.None);

        Assert.Equal(first.Outcome.Result!.DecisionId, replay.Outcome.Result!.DecisionId);
        Assert.Equal(1, calls);
    }

    [Fact]
    public async Task IdempotencyStore_RejectsDifferentFingerprint()
    {
        var store = new InMemoryDecideIdempotencyStore(new FixedTimeProvider());
        await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "first",
            _ => Task.FromResult(CreateOutcome("decision-1")),
            CancellationToken.None);

        await Assert.ThrowsAsync<IdempotencyConflictException>(
            () => store.ExecuteAsync(
                "tenant/app/dev",
                "key",
                "second",
                _ => Task.FromResult(CreateOutcome("decision-2")),
                CancellationToken.None));
    }

    [Fact]
    public async Task IdempotencyStore_AllowsNewRequestAfterExpiry()
    {
        var calls = 0;
        var time = new MutableTimeProvider();
        var store = new InMemoryDecideIdempotencyStore(time);

        await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "first",
            _ => Task.FromResult(CreateOutcome($"decision-{++calls}")),
            CancellationToken.None);
        time.Advance(TimeSpan.FromHours(24));
        var replacement = await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "second",
            _ => Task.FromResult(CreateOutcome($"decision-{++calls}")),
            CancellationToken.None);

        Assert.Equal("decision-2", replacement.Outcome.Result!.DecisionId);
    }

    [Fact]
    public async Task IdempotencyStore_RetainsDeterministicFailure()
    {
        var calls = 0;
        var store = new InMemoryDecideIdempotencyStore(new FixedTimeProvider());
        var failure = new DecisionFailure(409, "contract-conflict", "conflict");

        var first = await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "fingerprint",
            _ =>
            {
                calls++;
                return Task.FromResult(DecideTerminalOutcome.Rejected(failure));
            },
            CancellationToken.None);
        var replay = await store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "fingerprint",
            _ =>
            {
                calls++;
                return Task.FromResult(DecideTerminalOutcome.Rejected(failure));
            },
            CancellationToken.None);

        Assert.Equal("contract-conflict", first.Outcome.Failure!.Code);
        Assert.Equal("contract-conflict", replay.Outcome.Failure!.Code);
        Assert.Equal(1, calls);
    }

    [Fact]
    public async Task IdempotencyStore_ReturnsInProgressAfterFollowerBudget()
    {
        var ownerCompletion = new TaskCompletionSource<DecideTerminalOutcome>();
        var store = new InMemoryDecideIdempotencyStore(
            new FixedTimeProvider(),
            TimeSpan.FromMilliseconds(10));
        var owner = store.ExecuteAsync(
            "tenant/app/dev",
            "key",
            "fingerprint",
            _ => ownerCompletion.Task,
            CancellationToken.None);

        await Assert.ThrowsAsync<IdempotencyInProgressException>(
            () => store.ExecuteAsync(
                "tenant/app/dev",
                "key",
                "fingerprint",
                _ => Task.FromResult(CreateOutcome("unexpected")),
                CancellationToken.None));
        ownerCompletion.SetResult(CreateOutcome("decision-1"));
        await owner;
    }

    [Fact]
    public async Task ExposureStore_ConfirmsIdempotentlyAndRejectsConflict()
    {
        var store = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-1");
        await store.CreatePendingAsync(
            "decision-1",
            "confirm-1",
            CreateSnapshot(),
            CancellationToken.None);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-07-31T18:00:00Z");

        var first = await store.ConfirmAsync(
            "decision-1",
            request,
            new HashSet<string> { "app" },
            new HashSet<string> { "dev" },
            CancellationToken.None);
        var replay = await store.ConfirmAsync(
            "decision-1",
            request,
            new HashSet<string> { "app" },
            new HashSet<string> { "dev" },
            CancellationToken.None);

        Assert.Equal(first, replay);
        await Assert.ThrowsAsync<ExposureConfirmationConflictException>(
            () => store.ConfirmAsync(
                "decision-1",
                request with { AppliedAt = "2026-07-31T18:01:00Z" },
                new HashSet<string> { "app" },
                new HashSet<string> { "dev" },
                CancellationToken.None));
    }

    [Fact]
    public async Task ExposureStore_HidesExposureFromDifferentApplicationScope()
    {
        var store = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-1");
        await store.CreatePendingAsync(
            "decision-1",
            "confirm-1",
            CreateSnapshot(),
            CancellationToken.None);

        await Assert.ThrowsAsync<ExposureNotFoundException>(
            () => store.ConfirmAsync(
                "decision-1",
                new ExposureConfirmationRequest("confirm-1"),
                new HashSet<string> { "other-app" },
                new HashSet<string> { "dev" },
                CancellationToken.None));
    }

    [Fact]
    public async Task ExposureStore_AcceptsMatchingNonFirstAuthorizedScope()
    {
        var store = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-1");
        await store.CreatePendingAsync(
            "decision-1",
            "confirm-1",
            CreateSnapshot(),
            CancellationToken.None);

        var result = await store.ConfirmAsync(
            "decision-1",
            new ExposureConfirmationRequest("confirm-1"),
            new HashSet<string> { "other-app", "app" },
            new HashSet<string> { "prod", "dev" },
            CancellationToken.None);

        Assert.Equal("confirmed", result.Status);
    }

    [Fact]
    public async Task ExposureStore_ReplaysAcceptedConfirmationBeforeClockValidation()
    {
        var store = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-1");
        await store.CreatePendingAsync(
            "decision-1",
            "confirm-1",
            CreateSnapshot(),
            CancellationToken.None);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-07-31T18:00:00Z");
        var confirmed = await store.ConfirmAsync(
            "decision-1",
            request,
            new HashSet<string> { "app" },
            new HashSet<string> { "dev" },
            CancellationToken.None);

        var replay = await store.FindReplayAsync(
            "decision-1",
            request,
            new HashSet<string> { "app" },
            new HashSet<string> { "dev" },
            CancellationToken.None);

        Assert.Equal(confirmed, replay);
    }

    private static DecideTerminalOutcome CreateOutcome(string decisionId) =>
        DecideTerminalOutcome.Success(CreateResult(decisionId));

    private static ServerDecisionResult CreateResult(string decisionId) => new(
        "key",
        new DecisionDefinitionRef("app", "dev", "key", "definition", "revision"),
        decisionId,
        JsonSerializer.SerializeToElement(1),
        "number",
        "active-value",
        null,
        [],
        ["global"],
        new ServerFallbackInfo("server", false, false, null),
        new PolicyEvaluationResult("approved", [], []),
        new ContractRuntimeStatus(
            "definition",
            "revision",
            $"sha256:{new string('a', 64)}",
            "verified"),
        new ExposureDirective(true, "confirm"),
        "reason",
        "audit");

    private static DecisionSnapshot CreateSnapshot() => new(
        "app",
        "dev",
        new RuntimeContractIdentity(
            "definition",
            $"sha256:{new string('a', 64)}",
            "revision"),
        JsonSerializer.SerializeToElement(1),
        "number",
        new ServerFallbackInfo("server", false, false, null),
        new Dictionary<string, JsonElement>(),
        [],
        null,
        new DecisionTargetRef("global", "global"),
        [],
        ["global"],
        new PolicyEvaluationResult("approved", [], []));

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);
    }

    private sealed class MutableTimeProvider : TimeProvider
    {
        private DateTimeOffset _utcNow = new(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);

        public override DateTimeOffset GetUtcNow() => _utcNow;

        public void Advance(TimeSpan duration) => _utcNow += duration;
    }
}
