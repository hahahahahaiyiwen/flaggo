using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileStateStoreTests
{
    [Fact]
    public async Task GetActiveAsync_LoadsWeightedRuleAndReloadsReplacedFile()
    {
        using var file = new TestJsonFile("state");
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
    public async Task GetActiveAsync_FollowsTargetResolutionOrder()
    {
        using var file = new TestJsonFile("target-order");
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
        using var file = new TestJsonFile("malformed");
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

    [Fact]
    public async Task OmittedFormatVersion_FailsWithStableReadErrorAndHealth()
    {
        using var file = new TestJsonFile("state-version");
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
            "The local governed-state file must use version 1.",
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
        using var file = new TestJsonFile("invalid-state");
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

    private static string InvalidDocument(Action<JsonObject> mutate)
    {
        var document = JsonNode.Parse(StateDocument(800, null))!.AsObject();
        var state = document["states"]![0]!.AsObject();
        mutate(state);
        return document.ToJsonString();
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
}
