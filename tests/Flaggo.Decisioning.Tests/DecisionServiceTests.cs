using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Evidence;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging.Abstractions;

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
    public async Task DecideAsync_RejectsMissingRequiredRuntimeContext()
    {
        var service = CreateService(
            new InMemoryAuditSink(),
            runtimeContext:
            [
                new RegisteredRuntimeContextField(
                    "sessionId",
                    "string",
                    Required: true,
                    TargetType: "session")
            ]);

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", CreateRequest()));

        Assert.Equal(422, error.Status);
        Assert.Equal("invalid-runtime-context", error.Code);
        Assert.Contains("sessionId", error.Message);
    }

    [Fact]
    public async Task DecideAsync_RejectsRuntimeTargetOutsideDefinitionHierarchy()
    {
        var service = CreateService(
            new InMemoryAuditSink(),
            targetHierarchy: ["cohort", "global"],
            inferenceTarget: "cohort",
            fallbackOrder: ["global"]);

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", CreateRequest()));

        Assert.Equal(422, error.Status);
        Assert.Equal("invalid-runtime-target", error.Code);
    }

    [Fact]
    public async Task DecideAsync_RejectsConflictingTargetBearingContext()
    {
        var service = CreateService(
            new InMemoryAuditSink(),
            runtimeContext:
            [
                new RegisteredRuntimeContextField(
                    "sessionId",
                    "string",
                    TargetType: "session")
            ]);
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["sessionId"] = JsonSerializer.SerializeToElement("other-game")
            }
        };

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", request));

        Assert.Equal("invalid-runtime-context", error.Code);
    }

    [Fact]
    public async Task DecideAsync_RejectsStoreTargetOutsideDefinitionHierarchy()
    {
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(700),
            new DecisionTargetRef("cohort", "new_players"));
        var service = CreateService(
            new InMemoryAuditSink(),
            stateStore: new IgnoringStateStore(state),
            targetHierarchy: ["session", "global"],
            inferenceTarget: "session",
            fallbackOrder: ["global"]);

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync("tetris.dropInterval", CreateRequest()));

        Assert.Equal(409, error.Status);
        Assert.Equal("contract-conflict", error.Code);
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
        Assert.Equal("client-verified", Assert.Single(pending.Snapshot.TargetProvenance).Source);
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

    [Fact]
    public async Task DecideAsync_ExecutesNumericRuleWithEvidenceAndPolicy()
    {
        var audit = new InMemoryAuditSink();
        var evidence = new DecisionEvidenceSnapshot(
            0.82,
            0.31,
            0.72,
            50,
            new Dictionary<string, JsonElement>
            {
                ["window"] = JsonSerializer.SerializeToElement("24h")
            });
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(750),
            new DecisionTargetRef("cohort", "new_players"),
            "numeric-rule",
            "strategy-test",
            new NumericRuleStrategy(
                "tetris.boardPressure",
                0.75,
                700,
                800),
            new DateTimeOffset(2026, 7, 31, 17, 59, 0, TimeSpan.Zero));
        var service = CreateService(
            audit,
            state,
            evidenceProvider: new InMemoryEvidenceProvider(
                new Dictionary<string, DecisionEvidenceSnapshot>
                {
                    ["strategy-test"] = evidence
                }),
            numberActionSpace: new NumberActionSpaceContract(200, 1500, 50),
            policy: new DecisionPolicyContract(
                Minimum: 200,
                Maximum: 1500,
                MaximumDelta: 50,
                CooldownSeconds: 20,
                MinimumEvidenceQuality: 0.7,
                MaximumModelUncertainty: 0.35,
                MinimumSampleSize: 30));
        var request = CreateRequest() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["cohort"] = JsonSerializer.SerializeToElement("new_players")
            },
            Inputs =
            [
                new SignalInput(
                    new SignalRef("tetris.boardPressure"),
                    JsonSerializer.SerializeToElement(0.82))
            ]
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);

        Assert.Equal("strategy", result.DecisionMode);
        Assert.Equal(700, result.Value.GetInt32());
        Assert.Equal("strategy-test", result.StrategyId);
        Assert.Equal(0.82, result.Confidence!.EvidenceQuality);
        Assert.Equal("approved", result.Policy.Result);
        Assert.Equal(evidence, Assert.Single(audit.Records).Evidence);
    }

    [Fact]
    public async Task DecideAsync_PolicyBlockReturnsGovernedFallback()
    {
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(750),
            Mode: "strategy",
            StrategyId: "strategy-test",
            NumericRule: new NumericRuleStrategy(
                "tetris.boardPressure",
                0.75,
                700,
                800));
        var service = CreateService(
            new InMemoryAuditSink(),
            state,
            evidenceProvider: new InMemoryEvidenceProvider(
                new Dictionary<string, DecisionEvidenceSnapshot>
                {
                    ["strategy-test"] = new(0.5, 0.31, 0.72, 50)
                }),
            numberActionSpace: new NumberActionSpaceContract(200, 1500, 50),
            policy: new DecisionPolicyContract(MinimumEvidenceQuality: 0.7));
        var request = CreateRequest() with
        {
            Inputs =
            [
                new SignalInput(
                    new SignalRef("tetris.boardPressure"),
                    JsonSerializer.SerializeToElement(0.82))
            ]
        };

        var result = await service.DecideAsync("tetris.dropInterval", request);

        Assert.Equal("fallback", result.DecisionMode);
        Assert.Equal(800, result.Value.GetInt32());
        Assert.Null(result.Confidence);
        Assert.Null(result.StrategyId);
        Assert.Equal("blocked", result.Policy.Result);
        Assert.Contains("insufficient_evidence_quality", result.Policy.Reasons);
        Assert.True(result.Fallback.DecisionFallbackUsed);
        Assert.False(result.Exposure.ConfirmationRequired);
    }

    [Theory]
    [InlineData("forbid", false)]
    [InlineData("allow", true)]
    public async Task DecideAsync_FailsWhenRequiredEvidenceIsUnavailable(
        string requiredEvidenceUnavailable,
        bool clientFallbackEligible)
    {
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(750),
            Mode: "strategy",
            StrategyId: "strategy-test");
        var service = CreateService(
            new InMemoryAuditSink(),
            state,
            evidenceProvider: new InMemoryEvidenceProvider(),
            policy: new DecisionPolicyContract(
                MinimumEvidenceQuality: 0.7,
                RequiredEvidenceUnavailable: requiredEvidenceUnavailable));

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync(
                "tetris.dropInterval",
                CreateRequest()));

        Assert.Equal(503, error.Status);
        Assert.Equal("required-evidence-unavailable", error.Code);
        Assert.Equal(clientFallbackEligible, error.ClientFallback!.Eligible);
    }

    [Theory]
    [InlineData("forbid", false)]
    [InlineData("allow", true)]
    public async Task DecideAsync_MapsEvidenceProviderFailureThroughDefinitionPolicy(
        string requiredEvidenceUnavailable,
        bool clientFallbackEligible)
    {
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(750),
            Mode: "strategy",
            StrategyId: "strategy-test",
            NumericRule: new NumericRuleStrategy(
                "tetris.boardPressure",
                0.5,
                700,
                800));
        var service = CreateService(
            new InMemoryAuditSink(),
            state,
            evidenceProvider: new ThrowingEvidenceProvider(),
            policy: new DecisionPolicyContract(
                MinimumEvidenceQuality: 0.7,
                RequiredEvidenceUnavailable: requiredEvidenceUnavailable));

        var error = await Assert.ThrowsAsync<DecisionContractException>(
            () => service.DecideAsync(
                "tetris.dropInterval",
                CreateRequest()));

        Assert.Equal(503, error.Status);
        Assert.Equal("required-evidence-unavailable", error.Code);
        Assert.Equal(clientFallbackEligible, error.ClientFallback!.Eligible);
    }

    [Fact]
    public async Task DecideAsync_FallsBackWhenStrategyExecutorOmitsRequiredConfidence()
    {
        var state = new GovernedDecisionState(
            Identity.DefinitionId,
            Identity.Revision,
            Identity.ContractDigest,
            JsonSerializer.SerializeToElement(750),
            Mode: "strategy",
            StrategyId: "strategy-test",
            NumericRule: new NumericRuleStrategy(
                "tetris.boardPressure",
                0.5,
                700,
                800));
        var service = CreateService(
            new InMemoryAuditSink(),
            state,
            strategyExecutor: new InvalidConfidenceStrategyExecutor());

        var result = await service.DecideAsync(
            "tetris.dropInterval",
            CreateRequest());

        Assert.Equal("fallback", result.DecisionMode);
        Assert.Null(result.Confidence);
        Assert.Null(result.StrategyId);
        Assert.Contains("strategy_confidence_unavailable", result.Policy.Reasons);
    }

    private static DecisionService CreateService(
        IAuditSink auditSink,
        GovernedDecisionState? state = null,
        IExposureStore? exposureStore = null,
        IEvidenceProvider? evidenceProvider = null,
        NumberActionSpaceContract? numberActionSpace = null,
        DecisionPolicyContract? policy = null,
        IStrategyExecutor? strategyExecutor = null,
        IReadOnlyList<RegisteredRuntimeContextField>? runtimeContext = null,
        IReadOnlyList<string>? targetHierarchy = null,
        string inferenceTarget = "session",
        IReadOnlyList<string>? fallbackOrder = null,
        IStateStore? stateStore = null)
    {
        var definition = new RuntimeDecisionDefinition(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            Identity,
            "number",
            JsonSerializer.SerializeToElement(800),
            "safe-default",
            [new RegisteredSignalInput("tetris.boardPressure", "number", 0, 1)],
            runtimeContext ??
            [
                new RegisteredRuntimeContextField(
                    "userId",
                    "string",
                    TargetType: "user"),
                new RegisteredRuntimeContextField(
                    "sessionId",
                    "string",
                    TargetType: "session"),
                new RegisteredRuntimeContextField(
                    "cohort",
                    "string",
                    TargetType: "cohort"),
                new RegisteredRuntimeContextField("deviceType", "string")
            ],
            NumberActionSpace: numberActionSpace,
            Policy: policy,
            TargetHierarchy:
                targetHierarchy ?? ["session", "user", "cohort", "global"],
            InferenceTarget: inferenceTarget,
            FallbackOrder: fallbackOrder ?? ["user", "cohort", "global"]);

        return new DecisionService(
            new InMemoryDefinitionRegistry([definition]),
            stateStore ??
                new InMemoryStateStore(
                    state is null
                        ? []
                        : [("tetris.dropInterval", state)]),
            exposureStore ?? new InMemoryExposureStore(
                new FixedTimeProvider(),
                () => "exposure-test"),
            auditSink,
            new TestIdGenerator(),
            new FixedTimeProvider(),
            new DefaultTargetResolver(
                new Dictionary<string, string>
                {
                    ["new_players"] = "new_players"
                }),
            evidenceProvider ?? new InMemoryEvidenceProvider(),
            strategyExecutor ?? new DeterministicStrategyExecutor(),
            new DefaultPolicyEvaluator(new FixedTimeProvider()),
            NullLogger<DecisionService>.Instance);
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

    private sealed class IgnoringStateStore(GovernedDecisionState state) : IStateStore
    {
        public Task<GovernedDecisionState?> GetActiveAsync(
            string decisionKey,
            string definitionId,
            string revision,
            IReadOnlyList<DecisionTargetRef?> resolutionTargets,
            CancellationToken cancellationToken) =>
            Task.FromResult<GovernedDecisionState?>(state);
    }

    private sealed class ThrowingEvidenceProvider : IEvidenceProvider
    {
        public Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
            DecisionEvidenceRequest request,
            CancellationToken cancellationToken) =>
            Task.FromException<DecisionEvidenceSnapshot?>(
                new EvidenceUnavailableException(
                    "Evidence unavailable.",
                    new IOException("evidence file unavailable")));
    }

    private sealed class InvalidConfidenceStrategyExecutor : IStrategyExecutor
    {
        public Task<StrategyExecutionResult> ExecuteAsync(
            StrategyExecutionRequest request,
            CancellationToken cancellationToken) =>
            Task.FromResult(
                new StrategyExecutionResult(
                    JsonSerializer.SerializeToElement(700),
                    "strategy",
                    "strategy-test",
                    null,
                    "Invalid test strategy result."));
    }
}
