using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class DecisionServiceTests
{
    private static readonly RuntimeContractIdentity Identity = new(
        "def_test",
        $"sha256:{new string('a', 64)}",
        "rev_test",
        $"sha256:{new string('b', 64)}");

    [Fact]
    public async Task DecideAsync_ReturnsActiveFixedValueAndRecordsAudit()
    {
        var audit = new InMemoryAuditSink();
        var service = CreateService(
            audit,
            new GovernedDecisionState(
                Identity.DefinitionId,
                Identity.Revision,
                Identity.ContractDigest,
                JsonSerializer.SerializeToElement(700)));

        var result = await service.DecideAsync("tetris.dropInterval", CreateRequest());

        Assert.Equal("active-value", result.DecisionMode);
        Assert.Equal(700, result.Value.GetInt32());
        Assert.False(result.Fallback.DecisionFallbackUsed);
        Assert.Equal("approved", result.Policy.Result);
        Assert.Equal("verified", result.DefinitionStatus.Integrity);
        Assert.Single(audit.Records);
        Assert.Equal(result.DecisionId, audit.Records[0].DecisionId);
        Assert.Equal("session", audit.Records[0].RuntimeTarget!.Type);
        Assert.Equal("tetris-demo", audit.Records[0].AppId);
        Assert.Equal("dev", audit.Records[0].Environment);
        Assert.Equal(700, audit.Records[0].Value.GetInt32());
        Assert.Equal("number", audit.Records[0].ValueType);
        Assert.False(audit.Records[0].Fallback.DecisionFallbackUsed);
    }

    [Fact]
    public async Task DecideAsync_ReturnsGovernedFallbackWhenStateIsAbsent()
    {
        var audit = new InMemoryAuditSink();
        var service = CreateService(audit);

        var result = await service.DecideAsync("tetris.dropInterval", CreateRequest());

        Assert.Equal("fallback", result.DecisionMode);
        Assert.Equal(800, result.Value.GetInt32());
        Assert.True(result.Fallback.DecisionFallbackUsed);
        Assert.Equal("fallback", result.Policy.Result);
        Assert.Null(result.Confidence);
        Assert.False(result.Exposure.ConfirmationRequired);
        Assert.Single(audit.Records);
    }

    [Fact]
    public async Task DecideAsync_RejectsConflictingContractWithoutAudit()
    {
        var audit = new InMemoryAuditSink();
        var service = CreateService(audit);
        var request = CreateRequest() with
        {
            ExpectedContract = Identity with { ContractDigest = $"sha256:{new string('c', 64)}" }
        };

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", request));

        Assert.Equal(409, error.Status);
        Assert.Equal("contract-conflict", error.Code);
        Assert.Empty(audit.Records);
    }

    [Fact]
    public async Task DecideAsync_SurfacesAuditFailure()
    {
        var service = CreateService(new FailingAuditSink());

        var error = await Assert.ThrowsAsync<IOException>(
            () => service.DecideAsync("tetris.dropInterval", CreateRequest()));

        Assert.Equal("audit unavailable", error.Message);
    }

    [Fact]
    public async Task DecideAsync_RejectsDuplicateInferenceInputs()
    {
        var service = CreateService(new InMemoryAuditSink());
        var input = new SignalInput(
            new SignalRef("tetris.boardPressure"),
            JsonSerializer.SerializeToElement(0.5));
        var request = CreateRequest() with { Inputs = [input, input] };

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", request));

        Assert.Equal("duplicate-signal-input", error.Code);
    }

    [Fact]
    public async Task DecideAsync_RejectsInvalidInferenceInputType()
    {
        var service = CreateService(new InMemoryAuditSink());
        var request = CreateRequest() with
        {
            Inputs =
            [
                new SignalInput(
                    new SignalRef("tetris.boardPressure"),
                    JsonSerializer.SerializeToElement("high"))
            ]
        };

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", request));

        Assert.Equal("invalid-inference-input", error.Code);
        Assert.Equal("signal-type-mismatch", Assert.Single(error.Issues!).Code);
    }

    [Fact]
    public async Task DecideAsync_RejectsInvalidRuntimeContextType()
    {
        var service = CreateService(new InMemoryAuditSink());
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["userId"] = JsonSerializer.SerializeToElement(42)
            }
        };

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", request));

        Assert.Equal("invalid-runtime-context", error.Code);
    }

    [Fact]
    public async Task DecideAsync_PersistsExposureAttributionSnapshot()
    {
        var exposure = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-test");
        var service = CreateService(
            new InMemoryAuditSink(),
            new GovernedDecisionState(
                Identity.DefinitionId,
                Identity.Revision,
                Identity.ContractDigest,
                JsonSerializer.SerializeToElement(700),
                new DecisionTargetRef("cohort", "new_players")),
            exposure);
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["userId"] = JsonSerializer.SerializeToElement("user-1"),
                ["cohort"] = JsonSerializer.SerializeToElement("new_players")
            }
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);
        var pending = exposure.Find(result.DecisionId);

        Assert.Equal("cohort", pending!.Snapshot.ControlTarget!.Type);
        Assert.Equal("tetris-demo", pending.Snapshot.AppId);
        Assert.Equal("dev", pending.Snapshot.Environment);
        Assert.Equal(700, pending.Snapshot.Value.GetInt32());
        Assert.Equal("number", pending.Snapshot.ValueType);
        Assert.False(pending.Snapshot.Fallback.DecisionFallbackUsed);
        Assert.Equal("client-claimed", Assert.Single(pending.Snapshot.TargetProvenance).Source);
        Assert.Contains("user:user-1", pending.Snapshot.ResolutionChain);
    }

    [Fact]
    public async Task DecideAsync_TargetlessActiveValueDoesNotCreateExposure()
    {
        var exposure = new InMemoryExposureStore(new FixedTimeProvider(), () => "exposure-test");
        var service = CreateService(
            new InMemoryAuditSink(),
            new GovernedDecisionState(
                Identity.DefinitionId,
                Identity.Revision,
                Identity.ContractDigest,
                JsonSerializer.SerializeToElement(700)),
            exposure);
        var request = CreateRequest() with
        {
            RuntimeTarget = null,
            RuntimeContext = new Dictionary<string, JsonElement>()
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);

        Assert.Null(result.ControlTarget);
        Assert.False(result.Exposure.ConfirmationRequired);
        Assert.Null(exposure.Find(result.DecisionId));
    }

    [Fact]
    public async Task DecideAsync_DoesNotAttributeStateToUnmatchedCohort()
    {
        var service = CreateService(
            new InMemoryAuditSink(),
            new GovernedDecisionState(
                Identity.DefinitionId,
                Identity.Revision,
                Identity.ContractDigest,
                JsonSerializer.SerializeToElement(700),
                new DecisionTargetRef("cohort", "new_players")));
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["cohort"] = JsonSerializer.SerializeToElement("other_players")
            }
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);

        Assert.Equal("fallback", result.DecisionMode);
        Assert.Null(result.ControlTarget);
    }

    [Fact]
    public async Task DecideAsync_AttributesBroaderTargetResolutionFallback()
    {
        var service = CreateService(
            new InMemoryAuditSink(),
            new GovernedDecisionState(
                Identity.DefinitionId,
                Identity.Revision,
                Identity.ContractDigest,
                JsonSerializer.SerializeToElement(700),
                new DecisionTargetRef("global", "global")));
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["cohort"] = JsonSerializer.SerializeToElement("new_players")
            }
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);

        Assert.True(result.Fallback.ResolutionFallbackUsed);
        Assert.Equal("resolution_fallback_broader_target", result.Fallback.Reason);
        Assert.Equal("global", result.ControlTarget!.Type);
        Assert.Equal("server-derived", Assert.Single(result.TargetProvenance).Source);
    }

    private static DecisionService CreateService(
        IAuditSink auditSink,
        GovernedDecisionState? state = null,
        IExposureStore? exposureStore = null)
    {
        var definition = new RegisteredDecisionDefinition(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            Identity,
            "number",
            JsonSerializer.SerializeToElement(800),
            "safe-default",
            [new RegisteredSignalInput("tetris.boardPressure", "number", 0, 1)],
            [
                new RegisteredRuntimeContextField("userId", "string"),
                new RegisteredRuntimeContextField("sessionId", "string"),
                new RegisteredRuntimeContextField("cohort", "string"),
                new RegisteredRuntimeContextField("deviceType", "string")
            ]);

        return new DecisionService(
            new InMemoryDefinitionRegistry([definition]),
            new InMemoryStateStore(
                state is null
                    ? []
                    : [("tetris.dropInterval", state)]),
            exposureStore ?? new InMemoryExposureStore(
                new FixedTimeProvider(),
                () => "exposure-test"),
            auditSink,
            new TestIdGenerator(),
            new FixedTimeProvider());
    }

    private static DecideRequest CreateRequest() => new(
        Identity,
        new Dictionary<string, JsonElement>(),
        new DecisionClient("tetris-demo", "dev"),
        new DecisionTargetRef("session", "game-123"));

    private sealed class TestIdGenerator : IRuntimeIdGenerator
    {
        public string CreateDecisionId() => "decision-test";

        public string CreateAuditId() => "audit-test";

        public string CreateConfirmToken() => "confirm-test";

        public string CreateExposureId() => "exposure-test";
    }

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);
    }

    private sealed class FailingAuditSink : IAuditSink
    {
        public Task RecordDecisionAsync(
            DecisionAuditRecord record,
            CancellationToken cancellationToken) =>
            Task.FromException(new IOException("audit unavailable"));
    }
}
