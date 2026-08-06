using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.DataPlane;

public sealed record BootstrapGenerationPaths(
    string RootPath,
    string Generation,
    string ReceiptPath,
    string StatePath,
    string EvidencePath);

public static partial class BootstrapGenerationResolver
{
    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    public static BootstrapGenerationPaths? ResolveOptional(
        IConfiguration configuration)
    {
        var rootPath = configuration["Flaggo:Bootstrap:LocalGenerationPath"];
        return string.IsNullOrWhiteSpace(rootPath) ? null : Resolve(rootPath);
    }

    public static BootstrapGenerationPaths Resolve(string rootPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(rootPath);
        var root = Path.GetFullPath(rootPath);
        var manifestPath = Path.Combine(root, "current.json");
        try
        {
            var bytes = File.ReadAllBytes(manifestPath);
            StrictJson.Validate(bytes);
            var manifest = JsonSerializer.Deserialize<BootstrapManifest>(
                bytes,
                JsonOptions);
            if (manifest?.Version != 1 ||
                string.IsNullOrWhiteSpace(manifest.Generation) ||
                !GenerationPattern().IsMatch(manifest.Generation) ||
                manifest.Files is null)
            {
                throw new InvalidDataException(
                    "The bootstrap generation manifest is invalid.");
            }

            var generationPath = Path.Combine(
                root,
                "generations",
                manifest.Generation);
            var receipt = ResolveFile(manifest.Files, generationPath, "receipt");
            var state = ResolveFile(manifest.Files, generationPath, "state");
            var evidence = ResolveFile(manifest.Files, generationPath, "evidence");
            return new BootstrapGenerationPaths(
                root,
                manifest.Generation,
                receipt,
                state,
                evidence);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The bootstrap generation manifest is invalid.",
                error);
        }
    }

    private static string ResolveFile(
        IReadOnlyDictionary<string, string> files,
        string generationPath,
        string logicalName)
    {
        var expectedName = $"{logicalName}.json";
        if (!files.TryGetValue(logicalName, out var fileName) ||
            !string.Equals(fileName, expectedName, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                $"The bootstrap generation manifest is missing '{logicalName}'.");
        }

        var path = Path.GetFullPath(Path.Combine(generationPath, fileName));
        var expected = Path.GetFullPath(Path.Combine(generationPath, expectedName));
        if (!string.Equals(path, expected, StringComparison.Ordinal) ||
            !File.Exists(path))
        {
            throw new InvalidDataException(
                $"The bootstrap generation file '{logicalName}' is missing.");
        }
        return path;
    }

    [GeneratedRegex(
        "\\A\\d+-\\d+-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex GenerationPattern();

    private sealed record BootstrapManifest(
        int? Version,
        string? Generation,
        IReadOnlyDictionary<string, string>? Files);
}
