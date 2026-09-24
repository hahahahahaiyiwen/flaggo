using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Microsoft.Extensions.Configuration;

namespace Flaggo.Hosting;

public static class LocalRegistryHosting
{
    private static readonly Lazy<IReadOnlyList<RegisteredDefinitionProjections>> LocalFixture = new(LoadLocalFixture);

    public static LocalFileDefinitionRegistry CreateDefinitionRegistry(
        IConfiguration configuration,
        string contentRootPath,
        TimeProvider? timeProvider = null)
    {
        var configuredPath = configuration["Flaggo:Registry:LocalFilePath"];
        var filePath = string.IsNullOrWhiteSpace(configuredPath)
            ? Path.GetFullPath(Path.Combine(contentRootPath, "..", "..", "..", "..", ".flaggo", "definition-registry-v1.json"))
            : Path.GetFullPath(configuredPath);
        return new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(filePath), DefaultDefinitions(),
            timeProvider: timeProvider, seedIntelligenceDefinitions: DefaultIntelligenceDefinitions());
    }

    public static IReadOnlyList<RuntimeDecisionDefinition> DefaultDefinitions()
    {
        var active = LocalFixture.Value.Single().Runtime;
        return
        [
            active,
            active with
            {
                Identity = new RuntimeContractIdentity(active.Identity.DefinitionId,
                    "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e",
                    "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K"),
                LifecycleStatus = "retired"
            }
        ];
    }

    public static IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot> DefaultIntelligenceDefinitions(
        IReadOnlyList<RuntimeDecisionDefinition>? runtimeDefinitions = null)
    {
        var runtime = (runtimeDefinitions ?? DefaultDefinitions()).Single(definition => definition.LifecycleStatus == "active");
        var projection = LocalFixture.Value.Single().Intelligence
            ?? throw new InvalidDataException("The local manifest has no intelligence projection.");
        return [projection with { Identity = runtime.Identity, LifecycleStatus = runtime.LifecycleStatus }];
    }

    private static IReadOnlyList<RegisteredDefinitionProjections> LoadLocalFixture()
    {
        using var source = typeof(LocalRegistryHosting).Assembly.GetManifestResourceStream("Flaggo.Hosting.ApprovedLocalManifest.json")
            ?? throw new InvalidOperationException("The approved local manifest fixture is missing.");
        using var document = JsonDocument.Parse(source);
        var fixture = document.RootElement;
        var receipt = fixture.GetProperty("expected").GetProperty("body").Deserialize<RegistrationReceipt>(
            new JsonSerializerOptions(JsonSerializerDefaults.Web))
            ?? throw new InvalidDataException("The local fixture is missing its approved receipt.");
        return InMemoryDefinitionRegistry.ProjectApprovedManifest(
            fixture.GetProperty("request").GetProperty("body"), receipt);
    }
}
