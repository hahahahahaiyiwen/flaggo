using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Microsoft.Extensions.Configuration;

namespace Flaggo.Hosting;

public static class LocalRegistryHosting
{
    public static LocalFileDefinitionRegistry CreateDefinitionRegistry(
        IConfiguration configuration,
        string contentRootPath,
        TimeProvider? timeProvider = null)
    {
        var configuredPath = configuration["Flaggo:Registry:LocalFilePath"];
        var filePath = string.IsNullOrWhiteSpace(configuredPath)
            ? Path.GetFullPath(
                Path.Combine(
                    contentRootPath,
                    "..",
                    "..",
                    "..",
                    "..",
                    ".flaggo",
                    "definition-registry-v1.json"))
            : Path.GetFullPath(configuredPath);
        var definitions = DefaultDefinitions();
        return new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(filePath),
            definitions,
            timeProvider: timeProvider,
            seedIntelligenceDefinitions:
                DefaultIntelligenceDefinitions(definitions));
    }

    public static IReadOnlyList<RuntimeDecisionDefinition> DefaultDefinitions()
    {
        var contractIdentity = new RuntimeContractIdentity(
            "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
            "sha256:6eadd7bd76b36ae06e89376d57107da83fdcabf07ff58c528ae97fddb7f08ee9",
            "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
            "sha256:5dc39235981925be2bc052cca32a09b6b336bcd5eda2357e2911a00f393d41a1");
        var retiredIdentity = new RuntimeContractIdentity(
            contractIdentity.DefinitionId,
            "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e",
            "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K");
        RegisteredSignalInput[] registeredInputs =
        [
            new("tetris.boardPressure", "number", 0, 1),
            new("tetris.currentLevel", "number"),
            new("tetris.recentPlacementTimeMs", "number"),
            new("tetris.recoveryFailures", "number")
        ];
        RegisteredRuntimeContextField[] registeredRuntimeContext =
        [
            new("userId", "string", TargetType: "user"),
            new("sessionId", "string", TargetType: "session"),
            new("cohort", "string", TargetType: "cohort"),
            new("deviceType", "string")
        ];
        var actionSpace = new NumberActionSpaceContract(200, 1500, 50);
        var policy = DefaultTetrisPolicy();
        return
        [
            new RuntimeDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                contractIdentity,
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe_default_drop_interval",
                registeredInputs,
                registeredRuntimeContext,
                NumberActionSpace: actionSpace,
                Policy: policy,
                TargetHierarchy: ["session", "user", "cohort", "global"],
                InferenceTarget: "session",
                FallbackOrder: ["cohort", "global"]),
            new RuntimeDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                retiredIdentity,
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe_default_drop_interval",
                registeredInputs,
                registeredRuntimeContext,
                "retired",
                NumberActionSpace: actionSpace,
                Policy: policy,
                TargetHierarchy: ["session", "user", "cohort", "global"],
                InferenceTarget: "session",
                FallbackOrder: ["cohort", "global"])
        ];
    }

    public static IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot>
        DefaultIntelligenceDefinitions(
            IReadOnlyList<RuntimeDecisionDefinition>? runtimeDefinitions = null)
    {
        var runtime = (runtimeDefinitions ?? DefaultDefinitions())
            .Single(definition => definition.LifecycleStatus == "active");
        return
        [
            new IntelligenceLifecycleDefinitionSnapshot(
                runtime.AppId,
                runtime.Environment,
                runtime.DecisionKey,
                runtime.Identity,
                runtime.LifecycleStatus,
                new DecisionObjectives(
                    Primary: new DecisionObjective(
                        "tetris.earlyLossRate24h",
                        "minimize"),
                    Secondary:
                    [
                        new DecisionObjective(
                            "tetris.hardDropRate24h",
                            "target",
                            0.45),
                        new DecisionObjective(
                            "tetris.recentPlacementTimeMs",
                            "minimize")
                    ],
                    Rationale:
                        "Keep gameplay challenging but playable while reducing early frustration."),
                new RegisteredDecisionSignalRoles(
                [
                    "tetris.boardPressure",
                    "tetris.currentLevel",
                    "tetris.earlyLossRate24h",
                    "tetris.hardDropRate24h",
                    "tetris.recentPlacementTimeMs",
                    "tetris.recoveryFailures"
                ],
                [],
                []),
                new DecisionWorkflowPermissions(
                    "approved-strategy",
                    [
                        "tetris.currentLevel",
                        "tetris.boardPressure",
                        "tetris.recentPlacementTimeMs",
                        "tetris.recoveryFailures"
                    ]),
                new DecisionActionSpaceContract(
                    runtime.ValueType,
                    runtime.FallbackValue,
                    runtime.NumberActionSpace?.Minimum,
                    runtime.NumberActionSpace?.Maximum,
                    runtime.NumberActionSpace?.Step,
                    []),
                runtime.Policy ?? new DecisionPolicyContract())
        ];
    }

    private static DecisionPolicyContract DefaultTetrisPolicy() =>
        new(
            Minimum: 200,
            Maximum: 1500,
            MaximumDelta: 50,
            CooldownSeconds: 20,
            MinimumEvidenceQuality: 0.7,
            MaximumModelUncertainty: 0.35,
            MinimumSampleSize: 30);
}
