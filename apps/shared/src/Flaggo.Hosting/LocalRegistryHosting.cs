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
        return new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(filePath),
            DefaultDefinitions(),
            timeProvider: timeProvider);
    }

    public static IReadOnlyList<RegisteredDecisionDefinition> DefaultDefinitions()
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
            new("userId", "string"),
            new("sessionId", "string"),
            new("cohort", "string"),
            new("deviceType", "string")
        ];
        return
        [
            new RegisteredDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                contractIdentity,
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe_default_drop_interval",
                registeredInputs,
                registeredRuntimeContext),
            new RegisteredDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                retiredIdentity,
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe_default_drop_interval",
                registeredInputs,
                registeredRuntimeContext,
                "retired")
        ];
    }
}
