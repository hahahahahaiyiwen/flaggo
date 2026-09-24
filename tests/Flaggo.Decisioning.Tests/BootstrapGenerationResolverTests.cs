using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Flaggo.DataPlane;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Flaggo.Decisioning.Tests;

public sealed class BootstrapGenerationResolverTests
{
    private static readonly RuntimeContractIdentity Identity = new(
        "def-phase3",
        $"sha256:{new string('a', 64)}",
        "rev-phase3");

    [Fact]
    public void AuthoritativeCohorts_IncludeDefaultsAndConfiguredMappings()
    {
        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(
                new Dictionary<string, string?>
                {
                    ["Flaggo:Targeting:AuthoritativeCohorts:worker-canary"] =
                        "adaptive-workers",
                    ["Flaggo:Targeting:AuthoritativeCohorts:adaptive-workers"] =
                        "adaptive-workers"
                })
            .Build();

        var mappings =
            LocalTargetingHosting.CreateAuthoritativeCohorts(configuration);

        Assert.Equal("new_players", mappings["whales"]);
        Assert.Equal("adaptive-workers", mappings["worker-canary"]);
        Assert.Equal("adaptive-workers", mappings["adaptive-workers"]);
    }

    [Fact]
    public void LocalDevelopmentIdentity_UsesConfiguredApplicationScope()
    {
        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(
                new Dictionary<string, string?>
                {
                    ["Flaggo:Authentication:LocalDevelopmentAppId"] =
                        "adaptive-worker-demo",
                    ["Flaggo:Authentication:LocalDevelopmentEnvironment"] =
                        "dev"
                })
            .Build();

        var identity =
            LocalDevelopmentIdentityHosting.FromConfiguration(configuration);

        Assert.Equal("adaptive-worker-demo", identity.AppId);
        Assert.Equal("dev", identity.Environment);
    }

    [Fact]
    public async Task PointerSwitch_ResolvesEachGenerationWithoutMixingFiles()
    {
        using var root = new TestGenerationDirectory();
        await root.WriteGenerationAsync("100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        await root.WriteManifestAsync("100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");

        var old = BootstrapGenerationResolver.Resolve(root.Path);
        await root.WriteGenerationAsync("200-1-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        await root.WriteManifestAsync("200-1-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        var current = BootstrapGenerationResolver.Resolve(root.Path);

        Assert.Contains(old.Generation, old.ReceiptPath, StringComparison.Ordinal);
        Assert.Contains(old.Generation, old.StatePath, StringComparison.Ordinal);
        Assert.Contains(old.Generation, old.EvidencePath, StringComparison.Ordinal);
        Assert.Contains(
            current.Generation,
            current.ReceiptPath,
            StringComparison.Ordinal);
        Assert.Contains(
            current.Generation,
            current.StatePath,
            StringComparison.Ordinal);
        Assert.Contains(
            current.Generation,
            current.EvidencePath,
            StringComparison.Ordinal);
        Assert.NotEqual(old.Generation, current.Generation);
    }

    [Fact]
    public async Task ResolvedGeneration_RemainsPinnedAfterPointerSwitch()
    {
        using var root = new TestGenerationDirectory();
        const string oldGeneration =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        const string newGeneration =
            "200-1-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        await root.WriteGenerationAsync(oldGeneration, marker: "old");
        await root.WriteManifestAsync(oldGeneration);
        var resolved = BootstrapGenerationResolver.Resolve(root.Path);
        var oldBytes = await CommittedFileSnapshot.ReadPinnedAsync(
            resolved.StateSnapshot,
            options: null,
            CancellationToken.None);

        await root.WriteGenerationAsync(newGeneration, marker: "new");
        await root.WriteManifestAsync(newGeneration);
        var pinnedBytes = await CommittedFileSnapshot.ReadPinnedAsync(
            resolved.StateSnapshot,
            options: null,
            CancellationToken.None);
        var current = BootstrapGenerationResolver.Resolve(root.Path);
        var newBytes = await CommittedFileSnapshot.ReadPinnedAsync(
            current.StateSnapshot,
            options: null,
            CancellationToken.None);

        Assert.Equal(
            "old",
            JsonDocument.Parse(oldBytes).RootElement
                .GetProperty("marker")
                .GetString());
        Assert.Equal(
            "old",
            JsonDocument.Parse(pinnedBytes).RootElement
                .GetProperty("marker")
                .GetString());
        Assert.Equal(
            "new",
            JsonDocument.Parse(newBytes).RootElement
                .GetProperty("marker")
                .GetString());
    }

    [Fact]
    public async Task RequestScope_PinsStateAndEvidenceToOneGeneration()
    {
        using var root = new TestGenerationDirectory();
        const string oldGeneration =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        const string newGeneration =
            "200-1-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        await root.WriteDecisionGenerationAsync(
            oldGeneration,
            stateValue: 800,
            evidenceGeneration: "old");
        await root.WriteManifestAsync(oldGeneration);
        await root.WriteDecisionGenerationAsync(
            newGeneration,
            stateValue: 900,
            evidenceGeneration: "new");

        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(
                new Dictionary<string, string?>
                {
                    ["Flaggo:Bootstrap:LocalGenerationPath"] = root.Path
                })
            .Build();
        var services = new ServiceCollection();
        LocalRuntimeAdapterHosting.AddDecisionSnapshotScope(
            services,
            configuration);
        LocalRuntimeAdapterHosting.AddStateAdapter(
            services,
            configuration,
            Identity);
        LocalRuntimeAdapterHosting.AddEvidenceAdapter(
            services,
            configuration);
        using var provider = services.BuildServiceProvider(
            new ServiceProviderOptions
            {
                ValidateOnBuild = true,
                ValidateScopes = true
            });
        BootstrapGenerationResolver firstResolver;

        using (var scope = provider.CreateScope())
        {
            firstResolver = scope.ServiceProvider
                .GetRequiredService<BootstrapGenerationResolver>();
            Assert.Same(
                firstResolver,
                scope.ServiceProvider
                    .GetRequiredService<IStateSnapshotProvider>());
            Assert.Same(
                firstResolver,
                scope.ServiceProvider
                    .GetRequiredService<IEvidenceSnapshotProvider>());
            var stateStore = scope.ServiceProvider
                .GetRequiredService<IStateStore>();
            var evidenceProvider = scope.ServiceProvider
                .GetRequiredService<IEvidenceProvider>();
            var state = await ReadStateAsync(stateStore);

            await root.WriteManifestAsync(newGeneration);

            var evidence = await evidenceProvider.GetEvidenceAsync(
                EvidenceRequest(state),
                CancellationToken.None);

            Assert.Equal(800, state.Value.GetInt32());
            Assert.Equal(
                "old",
                evidence!.Details!["generation"].GetString());
        }

        using (var scope = provider.CreateScope())
        {
            Assert.NotSame(
                firstResolver,
                scope.ServiceProvider
                    .GetRequiredService<BootstrapGenerationResolver>());
            var stateStore = scope.ServiceProvider
                .GetRequiredService<IStateStore>();
            var evidenceProvider = scope.ServiceProvider
                .GetRequiredService<IEvidenceProvider>();
            var state = await ReadStateAsync(stateStore);
            var evidence = await evidenceProvider.GetEvidenceAsync(
                EvidenceRequest(state),
                CancellationToken.None);

            Assert.Equal(900, state.Value.GetInt32());
            Assert.Equal(
                "new",
                evidence!.Details!["generation"].GetString());
        }
    }

    [Theory]
    [InlineData("../other.json")]
    [InlineData("..\\other.json")]
    [InlineData("sibling/evidence.json")]
    [InlineData("sibling\\evidence.json")]
    public async Task MissingOrRedirectedSibling_FailsClosed(
        string evidenceFileName)
    {
        using var root = new TestGenerationDirectory();
        const string generation =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        await root.WriteGenerationAsync(generation);
        await root.WriteManifestAsync(
            generation,
            evidenceFileName);

        Assert.Throws<InvalidDataException>(
            () => BootstrapGenerationResolver.Resolve(root.Path));
    }

    [Theory]
    [InlineData("digest")]
    [InlineData("length")]
    [InlineData("version")]
    [InlineData("missing")]
    public async Task CorruptedPinnedGeneration_FailsClosed(string corruption)
    {
        using var root = new TestGenerationDirectory();
        const string generation =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        await root.WriteGenerationAsync(generation);
        await root.WriteManifestAsync(generation, corruption: corruption);

        var error = Record.Exception(
            () => BootstrapGenerationResolver.Resolve(root.Path));
        Assert.True(
            error is InvalidDataException or FileNotFoundException,
            $"Unexpected exception: {error}");
    }

    [Fact]
    public async Task InPlaceArtifactChangeAfterManifest_FailsDigestValidation()
    {
        using var root = new TestGenerationDirectory();
        const string generation =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        await root.WriteGenerationAsync(generation);
        await root.WriteManifestAsync(generation);
        var receiptPath = System.IO.Path.Combine(
            root.Path,
            "generations",
            generation,
            "receipt.json");
        var original = await File.ReadAllTextAsync(receiptPath);
        await File.WriteAllTextAsync(
            receiptPath,
            original.Replace(
                "complete",
                "corrupt!",
                StringComparison.Ordinal));

        var error = Assert.Throws<InvalidDataException>(
            () => BootstrapGenerationResolver.Resolve(root.Path));

        Assert.Contains("sha256 digest", error.Message);
    }

    private static async Task<GovernedDecisionState> ReadStateAsync(
        IStateStore stateStore)
    {
        var state = await stateStore.GetActiveAsync(
            "tetris.dropInterval",
            Identity.DefinitionId,
            Identity.Revision,
            [null],
            CancellationToken.None);
        return Assert.IsType<GovernedDecisionState>(state);
    }

    private static DecisionEvidenceRequest EvidenceRequest(
        GovernedDecisionState state) =>
        new(
            new RuntimeDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                Identity,
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe-default",
                [],
                [],
                TargetHierarchy: ["global"],
                InferenceTarget: "global",
                FallbackOrder: []),
            state,
            new Dictionary<string, JsonElement>(),
            new Dictionary<string, JsonElement>());

    private sealed class TestGenerationDirectory : IDisposable
    {
        public TestGenerationDirectory()
        {
            Path = System.IO.Path.Combine(
                TestPaths.RepositoryRoot,
                ".flaggo",
                "test-artifacts",
                $"bootstrap-generation-{Guid.NewGuid():N}");
            Directory.CreateDirectory(Path);
        }

        public string Path { get; }

        public async Task WriteGenerationAsync(
            string generation,
            string marker = "complete")
        {
            var directory = System.IO.Path.Combine(
                Path,
                "generations",
                generation);
            Directory.CreateDirectory(directory);
            foreach (var name in new[] { "receipt", "state", "evidence" })
            {
                await File.WriteAllTextAsync(
                    System.IO.Path.Combine(directory, $"{name}.json"),
                    $"{JsonSerializer.Serialize(new { marker, name })}\n");
            }
        }

        public async Task WriteDecisionGenerationAsync(
            string generation,
            int stateValue,
            string evidenceGeneration)
        {
            var directory = System.IO.Path.Combine(
                Path,
                "generations",
                generation);
            Directory.CreateDirectory(directory);
            await File.WriteAllTextAsync(
                System.IO.Path.Combine(directory, "receipt.json"),
                $"{JsonSerializer.Serialize(new { marker = evidenceGeneration })}\n");
            await File.WriteAllTextAsync(
                System.IO.Path.Combine(directory, "state.json"),
                $$"""
                {
                  "version": 1,
                  "states": [
                    {
                      "decisionKey": "tetris.dropInterval",
                      "definitionId": "{{Identity.DefinitionId}}",
                      "revision": "{{Identity.Revision}}",
                      "contractDigest": "{{Identity.ContractDigest}}",
                      "value": {{stateValue}},
                      "mode": "strategy",
                      "strategyId": "strategy-phase3",
                      "numericRule": {
                        "inputKey": "boardPressure",
                        "threshold": 0.5,
                        "valueAtOrAbove": 900,
                        "valueBelow": 700
                      }
                    }
                  ]
                }
                """);
            await File.WriteAllTextAsync(
                System.IO.Path.Combine(directory, "evidence.json"),
                $$"""
                {
                  "version": 1,
                  "evidenceByStrategy": {
                    "strategy-phase3": {
                      "evidenceQuality": 0.8,
                      "details": {
                        "generation": "{{evidenceGeneration}}"
                      }
                    }
                  }
                }
                """);
        }

        public async Task WriteManifestAsync(
            string generation,
            string evidenceFileName = "evidence.json",
            string? corruption = null)
        {
            var generationPath = System.IO.Path.Combine(
                Path,
                "generations",
                generation);
            var files = new Dictionary<string, object>(
                StringComparer.Ordinal);
            foreach (var name in new[] { "receipt", "state", "evidence" })
            {
                var artifact = name == "evidence"
                    ? evidenceFileName
                    : $"{name}.json";
                var artifactPath = System.IO.Path.Combine(
                    generationPath,
                    $"{name}.json");
                var bytes = await File.ReadAllBytesAsync(artifactPath);
                files[name] = new
                {
                    artifact = corruption == "missing" && name == "state"
                        ? "missing.json"
                        : artifact,
                    byteLength = corruption == "length" && name == "state"
                        ? bytes.Length + 1
                        : bytes.Length,
                    sha256 = corruption == "digest" && name == "state"
                        ? new string('f', 64)
                        : Convert.ToHexString(SHA256.HashData(bytes))
                            .ToLowerInvariant()
                };
            }

            var manifestBytes = Encoding.UTF8.GetBytes(
                $"{JsonSerializer.Serialize(new
                {
                    format =
                        CommittedFileSnapshot.GenerationManifestFormat,
                    version = corruption == "version" ? 2 : 1,
                    generation,
                    files
                })}\n");
            var manifestPath = System.IO.Path.Combine(Path, "current.json");
            var stagingPath = $"{manifestPath}.{Guid.NewGuid():N}.tmp";
            await File.WriteAllBytesAsync(stagingPath, manifestBytes);
            File.Move(stagingPath, manifestPath, overwrite: true);
        }

        public void Dispose()
        {
            if (Directory.Exists(Path))
            {
                Directory.Delete(Path, recursive: true);
            }
        }
    }
}
