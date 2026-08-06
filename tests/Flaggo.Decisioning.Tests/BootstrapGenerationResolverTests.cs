using System.Text.Json;
using Flaggo.DataPlane;

namespace Flaggo.Decisioning.Tests;

public sealed class BootstrapGenerationResolverTests
{
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
    public async Task MissingOrRedirectedSibling_FailsClosed()
    {
        using var root = new TestGenerationDirectory();
        const string generation =
            "100-1-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        await root.WriteGenerationAsync(generation);
        await root.WriteManifestAsync(
            generation,
            evidenceFileName: "..\\other.json");

        Assert.Throws<InvalidDataException>(
            () => BootstrapGenerationResolver.Resolve(root.Path));
    }

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

        public async Task WriteGenerationAsync(string generation)
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
                    "{}\n");
            }
        }

        public Task WriteManifestAsync(
            string generation,
            string evidenceFileName = "evidence.json") =>
            File.WriteAllTextAsync(
                System.IO.Path.Combine(Path, "current.json"),
                $"{JsonSerializer.Serialize(new
                {
                    version = 1,
                    generation,
                    files = new Dictionary<string, string>
                    {
                        ["receipt"] = "receipt.json",
                        ["state"] = "state.json",
                        ["evidence"] = evidenceFileName
                    }
                })}\n");

        public void Dispose()
        {
            if (Directory.Exists(Path))
            {
                Directory.Delete(Path, recursive: true);
            }
        }
    }
}
