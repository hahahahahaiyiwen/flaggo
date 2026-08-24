using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Audit;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileStateStoreTests
{
    [Fact]
    public async Task GetActiveAsync_LoadsWeightedRuleAndReloadsReplacedFile()
    {
        using var file = TestJsonFile.CreateCommitted("state");
        await file.WriteAsync(StateDocument(850, "2026-08-05T23:00:00Z"));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var first = await store.GetActiveAsync(
            "tetris.dropInterval",
            "def-phase3",
            "rev-phase3",
            [new DecisionTargetRef("cohort", "new_players")],
            CancellationToken.None);
        await file.WriteAsync(StateDocument(800, "2026-08-06T00:00:00Z"));
        var second = await store.GetActiveAsync(
            "tetris.dropInterval",
            "def-phase3",
            "rev-phase3",
            [new DecisionTargetRef("cohort", "new_players")],
            CancellationToken.None);

        Assert.Equal(850, first!.Value.GetInt32());
        Assert.Equal(800, second!.Value.GetInt32());
        Assert.Equal(4, first.NumericRule!.WeightedInputs!.Count);
        Assert.Equal(
            new DateTimeOffset(2026, 8, 6, 0, 0, 0, TimeSpan.Zero),
            second.LastChangedAt);
    }

    [Fact]
    public async Task DirectRawFileWithoutCommitDescriptor_FailsClosed()
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"state-raw-{Guid.NewGuid():N}.json");
        try
        {
            await File.WriteAllTextAsync(path, StateDocument(800, null));
            var store = new LocalFileStateStore(
                new LocalFileStateStoreOptions(path));

            await Assert.ThrowsAsync<InvalidDataException>(
                () => LoadStateAsync(store));
            Assert.False(await store.IsAvailableAsync(CancellationToken.None));
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public async Task LastChangedAt_ZAndOffsetNormalizeToEquivalentUtc()
    {
        using var zFile = TestJsonFile.CreateCommitted("state-timestamp-z");
        using var offsetFile =
            TestJsonFile.CreateCommitted("state-timestamp-offset");
        await zFile.WriteAsync(StateDocument(800, "2026-08-05T22:00:00Z"));
        await offsetFile.WriteAsync(
            StateDocument(800, "2026-08-06T00:00:00+02:00"));
        var zStore = new LocalFileStateStore(
            new LocalFileStateStoreOptions(zFile.Path));
        var offsetStore = new LocalFileStateStore(
            new LocalFileStateStoreOptions(offsetFile.Path));

        var zState = await LoadStateAsync(zStore);
        var offsetState = await LoadStateAsync(offsetStore);

        Assert.Equal(zState.LastChangedAt, offsetState.LastChangedAt);
        Assert.Equal(TimeSpan.Zero, offsetState.LastChangedAt!.Value.Offset);
        Assert.Equal(
            new DateTimeOffset(2026, 8, 5, 22, 0, 0, TimeSpan.Zero),
            offsetState.LastChangedAt);
    }

    [Theory]
    [InlineData("2026-08-06T00:00:00")]
    [InlineData("2026-08-06 00:00:00Z")]
    [InlineData("2026-08-06T00:00:00+24:00")]
    [InlineData("0001-01-01T00:00:00+00:00")]
    [InlineData("not-a-time")]
    public async Task InvalidOrOffsetlessLastChangedAt_FailsLookupAndHealth(
        string timestamp)
    {
        using var file =
            TestJsonFile.CreateCommitted("state-timestamp-invalid");
        await file.WriteAsync(StateDocument(800, timestamp));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => LoadStateAsync(store));
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task LastChangedAt_AcceptsValidExtremeTimestampWithoutCooldownBound()
    {
        using var file =
            TestJsonFile.CreateCommitted("state-timestamp-extreme");
        await file.WriteAsync(
            StateDocument(800, DateTimeOffset.MaxValue.ToString("O")));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        var state = await LoadStateAsync(store);

        Assert.Equal(DateTimeOffset.MaxValue, state.LastChangedAt);
        var evaluator = new DefaultPolicyEvaluator(
            new FixedTimeProvider(DateTimeOffset.MaxValue));
        var decision = await evaluator.EvaluateAsync(
            new PolicyEvaluationRequest(
                JsonSerializer.SerializeToElement(850),
                state.Value,
                null,
                new DecisionPolicyContract(
                    CooldownSeconds: double.MaxValue),
                null,
                state.LastChangedAt,
                null),
            CancellationToken.None);

        Assert.False(decision.Approved);
        Assert.Contains("cooldown_active", decision.Result.Reasons);
        Assert.True(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task OffsetNormalizedTimestamp_PreservesCooldownBehavior()
    {
        using var file =
            TestJsonFile.CreateCommitted("state-timestamp-cooldown");
        await file.WriteAsync(
            StateDocument(800, "2026-08-06T00:00:00+02:00"));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));
        var state = await LoadStateAsync(store);
        var evaluator = new DefaultPolicyEvaluator(
            new FixedTimeProvider(
                new DateTimeOffset(2026, 8, 5, 22, 0, 10, TimeSpan.Zero)));

        var decision = await evaluator.EvaluateAsync(
            new PolicyEvaluationRequest(
                JsonSerializer.SerializeToElement(850),
                state.Value,
                null,
                new DecisionPolicyContract(CooldownSeconds: 20),
                null,
                state.LastChangedAt,
                null),
            CancellationToken.None);

        Assert.False(decision.Approved);
        Assert.Contains("cooldown_active", decision.Result.Reasons);
    }

    [Fact]
    public async Task GetActiveAsync_FollowsTargetResolutionOrder()
    {
        using var file = TestJsonFile.CreateCommitted("target-order");
        await file.WriteAsync(StateDocument(800, null));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var state = await store.GetActiveAsync(
            "tetris.dropInterval",
            "def-phase3",
            "rev-phase3",
            [
                new DecisionTargetRef("session", "game-1"),
                new DecisionTargetRef("cohort", "new_players"),
                null
            ],
            CancellationToken.None);

        Assert.Equal("cohort", state!.ControlTarget!.Type);
    }

    [Fact]
    public async Task MalformedStateFile_FailsLookupAndHealth()
    {
        using var file = TestJsonFile.CreateCommitted("malformed");
        await file.WriteAsync("""{"version":1,"states":[{"decisionKey":""}]}""");
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => store.GetActiveAsync(
                "tetris.dropInterval",
                "def-phase3",
                "rev-phase3",
                [null],
                CancellationToken.None));
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string, string> DuplicatePropertyDocuments()
    {
        var state = StateDocument(800, null);
        var cases = new TheoryData<string, string>();
        cases.Add(
            "root",
            state.Replace(
                "\"version\":1",
                "\"version\":1,\"version\":1",
                StringComparison.Ordinal));
        cases.Add(
            "state entry",
            state.Replace(
                "\"mode\":\"strategy\"",
                "\"mode\":\"strategy\",\"mode\":\"strategy\"",
                StringComparison.Ordinal));
        cases.Add(
            "numeric rule",
            state.Replace(
                "\"threshold\":0.55",
                "\"threshold\":0.55,\"threshold\":0.55",
                StringComparison.Ordinal));
        return cases;
    }

    [Theory]
    [MemberData(nameof(DuplicatePropertyDocuments))]
    public async Task DuplicateJsonProperty_FailsBeforeStateDeserialization(
        string location,
        string document)
    {
        _ = location;
        using var file =
            TestJsonFile.CreateCommitted("state-duplicate-property");
        await file.WriteAsync(document);
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => store.GetActiveAsync(
                "tetris.dropInterval",
                "def-phase3",
                "rev-phase3",
                [new DecisionTargetRef("cohort", "new_players")],
                CancellationToken.None));

        var jsonError = Assert.IsType<JsonException>(error.InnerException);
        Assert.Contains("Duplicate JSON property", jsonError.Message);
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task TrailingJsonData_FailsBeforeStateDeserialization()
    {
        using var file = TestJsonFile.CreateCommitted("state-trailing-data");
        await file.WriteAsync($"{StateDocument(800, null)}{{}}");
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => store.GetActiveAsync(
                "tetris.dropInterval",
                "def-phase3",
                "rev-phase3",
                [null],
                CancellationToken.None));

        Assert.IsAssignableFrom<JsonException>(error.InnerException);
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task SamePropertyNamesInSiblingStates_AreAccepted()
    {
        var document = JsonNode.Parse(StateDocument(800, null))!.AsObject();
        var states = document["states"]!.AsArray();
        var sibling = states[0]!.DeepClone().AsObject();
        sibling["definitionId"] = "def-sibling";
        sibling["revision"] = "rev-sibling";
        states.Add(sibling);
        using var file =
            TestJsonFile.CreateCommitted("state-sibling-properties");
        await file.WriteAsync(document.ToJsonString());
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var state = await store.GetActiveAsync(
            "tetris.dropInterval",
            "def-sibling",
            "rev-sibling",
            [new DecisionTargetRef("cohort", "new_players")],
            CancellationToken.None);

        Assert.Equal(800, state!.Value.GetInt32());
        Assert.True(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task StructuralIdentity_KeepsColonAndNewlineComponentsDistinct()
    {
        var document = JsonNode.Parse(StateDocument(800, null))!.AsObject();
        var template = document["states"]![0]!.AsObject();
        var states = new JsonArray
        {
            StateEntry(
                template,
                "colon-decision",
                "colon-definition",
                "colon-revision",
                701,
                new JsonObject { ["type"] = "cohort:blue", ["id"] = "one" }),
            StateEntry(
                template,
                "colon-decision",
                "colon-definition",
                "colon-revision",
                702,
                new JsonObject { ["type"] = "cohort", ["id"] = "blue:one" }),
            StateEntry(
                template,
                "decision\npart",
                "definition",
                "newline-revision",
                703,
                null),
            StateEntry(
                template,
                "decision",
                "part\ndefinition",
                "newline-revision",
                704,
                null)
        };
        document["states"] = states;
        using var file =
            TestJsonFile.CreateCommitted("state-structural-identity");
        await file.WriteAsync(document.ToJsonString());
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var colonInType = await store.GetActiveAsync(
            "colon-decision",
            "colon-definition",
            "colon-revision",
            [new DecisionTargetRef("cohort:blue", "one")],
            CancellationToken.None);
        var colonInId = await store.GetActiveAsync(
            "colon-decision",
            "colon-definition",
            "colon-revision",
            [new DecisionTargetRef("cohort", "blue:one")],
            CancellationToken.None);
        var newlineInDecision = await store.GetActiveAsync(
            "decision\npart",
            "definition",
            "newline-revision",
            [null],
            CancellationToken.None);
        var newlineInDefinition = await store.GetActiveAsync(
            "decision",
            "part\ndefinition",
            "newline-revision",
            [null],
            CancellationToken.None);

        Assert.Equal(701, colonInType!.Value.GetInt32());
        Assert.Equal(702, colonInId!.Value.GetInt32());
        Assert.Equal(703, newlineInDecision!.Value.GetInt32());
        Assert.Equal(704, newlineInDefinition!.Value.GetInt32());
    }

    [Fact]
    public async Task OmittedFormatVersion_FailsWithStableReadErrorAndHealth()
    {
        using var file = TestJsonFile.CreateCommitted("state-version");
        var document = JsonNode.Parse(StateDocument(800, null))!.AsObject();
        document.Remove("version");
        await file.WriteAsync(document.ToJsonString());
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => store.GetActiveAsync(
                "tetris.dropInterval",
                "def-phase3",
                "rev-phase3",
                [null],
                CancellationToken.None));

        Assert.Equal(
            "The local governed-state file must use version 1 or 2.",
            error.Message);
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string, string> InvalidStateDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add("unknown mode", InvalidDocument(state => state["mode"] = "unknown"));
        cases.Add("unsupported experiment", InvalidDocument(state => state["mode"] = "experiment"));
        cases.Add("unsupported fallback", InvalidDocument(state => state["mode"] = "fallback"));
        cases.Add(
            "active value with strategy id",
            InvalidDocument(state =>
            {
                state["mode"] = "active-value";
                state["numericRule"] = null;
            }));
        cases.Add(
            "active value with numeric rule",
            InvalidDocument(state =>
            {
                state["mode"] = "active-value";
                state["strategyId"] = null;
            }));
        cases.Add(
            "strategy without strategy id",
            InvalidDocument(state => state.Remove("strategyId")));
        cases.Add(
            "strategy without numeric rule",
            InvalidDocument(state => state.Remove("numericRule")));
        cases.Add(
            "strategy with nonnumeric current value",
            InvalidDocument(state => state["value"] = "800"));
        cases.Add(
            "strategy with nonfinite threshold",
            InvalidDocument(state =>
                state["numericRule"]!["threshold"] = JsonNode.Parse("1e400")));
        cases.Add(
            "numeric rule without input signal key",
            InvalidDocument(state =>
                state["numericRule"]!.AsObject().Remove("inputSignalKey")));
        cases.Add(
            "numeric rule without threshold",
            InvalidDocument(state =>
                state["numericRule"]!.AsObject().Remove("threshold")));
        cases.Add(
            "numeric rule without value at or above",
            InvalidDocument(state =>
                state["numericRule"]!.AsObject().Remove("valueAtOrAbove")));
        cases.Add(
            "numeric rule without value below",
            InvalidDocument(state =>
                state["numericRule"]!.AsObject().Remove("valueBelow")));
        cases.Add(
            "target without type",
            InvalidDocument(state =>
                state["controlTarget"]!.AsObject().Remove("type")));
        cases.Add(
            "target without id",
            InvalidDocument(state =>
                state["controlTarget"]!.AsObject().Remove("id")));
        cases.Add(
            "weighted input without signal key",
            InvalidDocument(state =>
                state["numericRule"]!["weightedInputs"]![0]!
                    .AsObject().Remove("signalKey")));
        cases.Add(
            "weighted input without minimum",
            InvalidDocument(state =>
                state["numericRule"]!["weightedInputs"]![0]!
                    .AsObject().Remove("minimum")));
        cases.Add(
            "weighted input without maximum",
            InvalidDocument(state =>
                state["numericRule"]!["weightedInputs"]![0]!
                    .AsObject().Remove("maximum")));
        cases.Add(
            "weighted input without weight",
            InvalidDocument(state =>
                state["numericRule"]!["weightedInputs"]![0]!
                    .AsObject().Remove("weight")));
        cases.Add(
            "strategy with null weighted input",
            InvalidDocument(state =>
                state["numericRule"]!["weightedInputs"]![0] = null));
        return cases;
    }

    [Theory]
    [MemberData(nameof(InvalidStateDocuments))]
    public async Task InvalidStateCombination_FailsLookupAndHealth(
        string name,
        string document)
    {
        _ = name;
        using var file = TestJsonFile.CreateCommitted("invalid-state");
        await file.WriteAsync(document);
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => store.GetActiveAsync(
                "tetris.dropInterval",
                "def-phase3",
                "rev-phase3",
                [new DecisionTargetRef("cohort", "new_players")],
                CancellationToken.None));
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Theory]
    [InlineData("\"value\":800", "\"value\":9007199254740993")]
    [InlineData("\"value\":800", "\"value\":0.10000000000000001")]
    [InlineData("\"threshold\":0.55", "\"threshold\":9007199254740993")]
    [InlineData("\"valueAtOrAbove\":850", "\"valueAtOrAbove\":0.10000000000000001")]
    [InlineData("\"minimum\":0", "\"minimum\":9007199254740993")]
    [InlineData("\"weight\":0.45", "\"weight\":0.10000000000000001")]
    public async Task ReplayRejectsNumericValuesThatCollideThroughIeee754(
        string original,
        string replacement)
    {
        using var file = TestJsonFile.CreateCommitted("state-ieee-collision");
        await file.WriteAsync(
            StateDocument(800, null).Replace(
                original,
                replacement,
                StringComparison.Ordinal));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => LoadStateAsync(store));
        Assert.False(await store.IsAvailableAsync(CancellationToken.None));
    }

    [Theory]
    [InlineData("9007199254740992")]
    [InlineData("9007199254740994")]
    [InlineData("-0")]
    [InlineData("0.10000000000000002")]
    public async Task EveryAcceptedStateNumber_IsAcceptedByAudit(string rawValue)
    {
        using var stateFile =
            TestJsonFile.CreateCommitted("state-audit-parity");
        await stateFile.WriteAsync(
            StateDocument(800, null).Replace(
                "\"value\":800",
                $"\"value\":{rawValue}",
                StringComparison.Ordinal));
        var store = new LocalFileStateStore(
            new LocalFileStateStoreOptions(stateFile.Path));
        var state = await LoadStateAsync(store);

        using var auditFile =
            TestJsonFile.CreateCommitted("state-audit-parity-log");
        using var audit = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(auditFile.Path));
        await audit.RecordDecisionAsync(
            AuditRecord(state.Value),
            CancellationToken.None);

        Assert.True(await store.IsAvailableAsync(CancellationToken.None));
        Assert.True(await audit.IsAvailableAsync(CancellationToken.None));
    }

    private static string InvalidDocument(Action<JsonObject> mutate)
    {
        var document = JsonNode.Parse(StateDocument(800, null))!.AsObject();
        var state = document["states"]![0]!.AsObject();
        mutate(state);
        return document.ToJsonString();
    }

    private static DecisionAuditRecord AuditRecord(JsonElement value) =>
        new(
            "audit-state-parity",
            "decision-state-parity",
            "tetris.dropInterval",
            "tetris-demo",
            "dev",
            new RuntimeContractIdentity(
                "def-phase3",
                $"sha256:{new string('a', 64)}",
                "rev-phase3"),
            value,
            "number",
            "active-value",
            new ServerFallbackInfo("server", false, false, null),
            new Dictionary<string, JsonElement>(),
            [],
            null,
            null,
            [],
            ["global"],
            new PolicyEvaluationResult("approved", [], []),
            new DateTimeOffset(2026, 8, 6, 0, 0, 0, TimeSpan.Zero));

    private static async Task<GovernedDecisionState> LoadStateAsync(
        LocalFileStateStore store) =>
        (await store.GetActiveAsync(
            "tetris.dropInterval",
            "def-phase3",
            "rev-phase3",
            [new DecisionTargetRef("cohort", "new_players")],
            CancellationToken.None))!;

    private static JsonObject StateEntry(
        JsonObject template,
        string decisionKey,
        string definitionId,
        string revision,
        int value,
        JsonObject? controlTarget)
    {
        var state = template.DeepClone().AsObject();
        state["decisionKey"] = decisionKey;
        state["definitionId"] = definitionId;
        state["revision"] = revision;
        state["value"] = value;
        state["controlTarget"] = controlTarget;
        return state;
    }

    private static string StateDocument(int value, string? lastChangedAt) =>
        JsonSerializer.Serialize(
            new
            {
                version = 1,
                states = new[]
                {
                    new
                    {
                        decisionKey = "tetris.dropInterval",
                        definitionId = "def-phase3",
                        revision = "rev-phase3",
                        contractDigest = $"sha256:{new string('a', 64)}",
                        value,
                        controlTarget = new { type = "cohort", id = "new_players" },
                        mode = "strategy",
                        strategyId = "strategy-tetris-balanced-v1",
                        numericRule = new
                        {
                            inputSignalKey = "tetris.boardPressure",
                            threshold = 0.55,
                            valueAtOrAbove = 850,
                            valueBelow = 750,
                            weightedInputs = new[]
                            {
                                new
                                {
                                    signalKey = "tetris.boardPressure",
                                    minimum = 0,
                                    maximum = 1,
                                    weight = 0.45
                                },
                                new
                                {
                                    signalKey = "tetris.recentPlacementTimeMs",
                                    minimum = 0,
                                    maximum = 2000,
                                    weight = 0.25
                                },
                                new
                                {
                                    signalKey = "tetris.recoveryFailures",
                                    minimum = 0,
                                    maximum = 5,
                                    weight = 0.20
                                },
                                new
                                {
                                    signalKey = "tetris.currentLevel",
                                    minimum = 0,
                                    maximum = 20,
                                    weight = 0.10
                                }
                            }
                        },
                        lastChangedAt
                    }
                }
            });

    private sealed class FixedTimeProvider(DateTimeOffset utcNow) : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => utcNow;
    }
}
