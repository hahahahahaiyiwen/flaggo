using System.Security.Cryptography;
using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

if (!OperatingSystem.IsWindows())
{
    Console.Error.WriteLine("Flaggo.DirectoryFlush is supported only on Windows.");
    return 2;
}

if (args is ["--server"])
{
    var jsonOptions = new JsonSerializerOptions(JsonSerializerDefaults.Web);
    while (await Console.In.ReadLineAsync() is { } line)
    {
        HelperResponse response;
        string? id = null;
        try
        {
            var request = JsonSerializer.Deserialize<HelperRequest>(
                    line,
                    jsonOptions)
                ?? throw new HelperException(
                    "invalid_request",
                    "A helper request is required.");
            id = request.Id;
            response = new HelperResponse(
                request.Id,
                true,
                HelperOperations.Execute(request),
                null);
        }
        catch (HelperException error)
        {
            response = new HelperResponse(
                id,
                false,
                null,
                new HelperError(error.Code, error.Message));
        }
        catch (Exception error)
        {
            response = new HelperResponse(
                id,
                false,
                null,
                new HelperError("io_error", error.Message));
        }

        await Console.Out.WriteLineAsync(
            JsonSerializer.Serialize(response, jsonOptions));
        await Console.Out.FlushAsync();
    }

    return 0;
}

if (args is [var directory])
{
    using var fileSystem = WindowsSecureFileSystem.OpenRoot(
        RequireAbsolutePath(directory, "directory"),
        create: false);
    fileSystem.FlushDirectory(string.Empty);
    return 0;
}

Console.Error.WriteLine(
    "Usage: Flaggo.DirectoryFlush <directory> | --server");
return 2;

static string RequireAbsolutePath(string? path, string name)
{
    if (string.IsNullOrWhiteSpace(path) || !Path.IsPathFullyQualified(path))
    {
        throw new HelperException(
            "invalid_request",
            $"Request field '{name}' must be an absolute path.");
    }
    return Path.GetFullPath(path);
}

internal sealed record HelperRequest
{
    public string? Id { get; init; }

    public string? Operation { get; init; }

    public string? Path { get; init; }

    public string? Root { get; init; }

    public string? ContentBase64 { get; init; }

    public IReadOnlyDictionary<string, string>? Files { get; init; }

    public IReadOnlyList<string>? RequiredNames { get; init; }

    public string? ArtifactStem { get; init; }

    public HelperTestHook? TestHook { get; init; }
}

internal sealed record HelperTestHook
{
    public string? AfterOpenPath { get; init; }

    public string? SignalPath { get; init; }

    public string? ReleasePath { get; init; }

    public string? FailAt { get; init; }
}

internal sealed record HelperResponse(
    string? Id,
    bool Ok,
    object? Result,
    HelperError? Error);

internal sealed record HelperError(string Code, string Message);

internal static partial class HelperOperations
{
    private const string ArtifactDescriptorFormat =
        "flaggo.committed-artifact";
    private const string GenerationManifestFormat =
        "flaggo.committed-generation";
    private const int MaximumCommitBytes = 64 * 1024;
    private const int MaximumArtifactBytes = 16 * 1024 * 1024;

    private static readonly JsonSerializerOptions IndentedJson =
        new(JsonSerializerDefaults.Web)
        {
            WriteIndented = true
        };

    public static object Execute(HelperRequest request) =>
        request.Operation switch
        {
            "flush" => Flush(request),
            "validate" => Validate(request),
            "ensure-directory" => EnsureDirectory(request),
            "write-atomic" => WriteAtomic(request),
            "publish-artifact" => PublishArtifact(request),
            "publish-generation" => PublishGeneration(request),
            "resolve-generation" => ResolveGeneration(request),
            "test-handle-count" => TestHandleCount(request),
            _ => throw new HelperException(
                "invalid_request",
                $"Unsupported helper operation '{request.Operation}'.")
        };

    private static object Flush(HelperRequest request)
    {
        var path = RequireAbsolute(request.Path, "path");
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            path,
            create: false,
            CreateTestHook(request));
        fileSystem.FlushDirectory(string.Empty);
        return new { resolvedPath = fileSystem.RootFinalPath };
    }

    private static object Validate(HelperRequest request)
    {
        var path = RequireAbsolute(request.Path, "path");
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            path,
            create: false,
            CreateTestHook(request));
        fileSystem.ValidatePinnedPaths();
        return new { resolvedPath = fileSystem.RootFinalPath };
    }

    private static object EnsureDirectory(HelperRequest request)
    {
        var root = RequireAbsolute(request.Root, "root");
        var path = RequireAbsolute(request.Path, "path");
        WindowsSecureFileSystem.EnsureContained(root, path);
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            root,
            create: true,
            CreateTestHook(request));
        var relative = Path.GetRelativePath(root, path);
        fileSystem.EnsureDirectory(relative);
        var parent = Path.GetDirectoryName(relative) ?? string.Empty;
        fileSystem.FlushDirectory(parent);
        if (!string.Equals(
                parent,
                relative,
                StringComparison.OrdinalIgnoreCase))
        {
            fileSystem.FlushDirectory(relative);
        }
        return new
        {
            rootPath = fileSystem.RootFinalPath,
            path = fileSystem.AbsolutePath(relative)
        };
    }

    private static object WriteAtomic(HelperRequest request)
    {
        var root = RequireAbsolute(request.Root, "root");
        var path = RequireAbsolute(request.Path, "path");
        WindowsSecureFileSystem.EnsureContained(root, path);
        var content = DecodeContent(request.ContentBase64);
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            root,
            create: true,
            CreateTestHook(request));
        var relative = Path.GetRelativePath(root, path);
        var parent = Path.GetDirectoryName(relative) ?? string.Empty;
        fileSystem.EnsureDirectory(parent);
        var stagingRelative =
            $"{relative}.{Environment.ProcessId}.{Guid.NewGuid():N}.tmp";
        WindowsSecureFileSystem.OpenedPath? staging = null;
        var published = false;
        try
        {
            staging = fileSystem.CreateNewFile(stagingRelative, content);
            ThrowAtTestHook(request, "before-native-rename");
            fileSystem.Rename(
                staging,
                relative,
                replace: true,
                onCommitted: () => published = true);
            ThrowAtTestHook(request, "after-native-rename");
            fileSystem.ValidatePinnedPaths();
            ThrowAtTestHook(request, "after-rename-validation");
            fileSystem.FlushDirectory(parent);
            return new { path = fileSystem.AbsolutePath(relative) };
        }
        finally
        {
            if (!published && staging is not null)
            {
                fileSystem.TryDelete(staging);
            }
        }
    }

    private static object PublishArtifact(HelperRequest request)
    {
        var root = RequireAbsolute(request.Root, "root");
        var descriptorPath = RequireAbsolute(request.Path, "path");
        WindowsSecureFileSystem.EnsureContained(root, descriptorPath);
        var descriptorRelative = Path.GetRelativePath(root, descriptorPath);
        var descriptorDirectory = Path.GetDirectoryName(descriptorRelative)
            ?? string.Empty;
        var descriptorName = Path.GetFileName(descriptorRelative);
        var stem = request.ArtifactStem ??
            Path.GetFileNameWithoutExtension(descriptorName)
                .Replace(".commit", string.Empty, StringComparison.Ordinal);
        var artifactName = $"{stem}-{Guid.NewGuid():N}.json";
        if (!SafeArtifactName().IsMatch(artifactName))
        {
            throw new HelperException(
                "invalid_request",
                "Commit descriptor generates an unsafe artifact filename.");
        }

        var content = DecodeContent(request.ContentBase64);
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            root,
            create: true,
            CreateTestHook(request));
        fileSystem.EnsureDirectory(descriptorDirectory);
        var artifactRelative = CombineRelative(
            descriptorDirectory,
            artifactName);
        var stagingRelative = CombineRelative(
            descriptorDirectory,
            $"{descriptorName}.{Environment.ProcessId}.{Guid.NewGuid():N}.tmp");
        WindowsSecureFileSystem.OpenedPath? artifact = null;
        WindowsSecureFileSystem.OpenedPath? staging = null;
        var descriptorPublished = false;
        try
        {
            artifact = fileSystem.CreateNewFile(artifactRelative, content);
            fileSystem.FlushDirectory(descriptorDirectory);
            var sha256 = Convert.ToHexString(SHA256.HashData(content))
                .ToLowerInvariant();
            var descriptorBytes = JsonBytes(new ArtifactDescriptor(
                ArtifactDescriptorFormat,
                1,
                artifactName,
                content.Length,
                sha256));
            staging = fileSystem.CreateNewFile(
                stagingRelative,
                descriptorBytes);
            ThrowAtTestHook(request, "before-native-rename");
            fileSystem.Rename(
                staging,
                descriptorRelative,
                replace: true,
                onCommitted: () => descriptorPublished = true);
            ThrowAtTestHook(request, "after-native-rename");
            fileSystem.ValidatePinnedPaths();
            ThrowAtTestHook(request, "after-rename-validation");
            fileSystem.FlushDirectory(descriptorDirectory);
            return new
            {
                commitDescriptorPath = descriptorPath,
                artifactPath = fileSystem.AbsolutePath(artifactRelative),
                artifact = artifactName,
                byteLength = content.Length,
                sha256
            };
        }
        finally
        {
            if (!descriptorPublished)
            {
                if (staging is not null)
                {
                    fileSystem.TryDelete(staging);
                }
                if (artifact is not null)
                {
                    fileSystem.TryDelete(artifact);
                }
            }
        }
    }

    private static object PublishGeneration(HelperRequest request)
    {
        var root = RequireAbsolute(request.Root, "root");
        var files = request.Files ??
            throw new HelperException(
                "invalid_request",
                "Generation files are required.");
        if (files.Count == 0 ||
            files.Keys.Any(name => !LogicalName().IsMatch(name)))
        {
            throw new HelperException(
                "invalid_request",
                "Generation files require safe non-empty logical names.");
        }

        var decoded = files
            .OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .ToDictionary(
                pair => pair.Key,
                pair => DecodeContent(pair.Value),
                StringComparer.Ordinal);
        var generation =
            $"{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()}-" +
            $"{Environment.ProcessId}-{Guid.NewGuid():D}";
        var generationRelative = Path.Combine("generations", generation);
        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            root,
            create: true,
            CreateTestHook(request));
        var pointerPublished = false;
        WindowsSecureFileSystem.OpenedPath? pointerStaging = null;
        try
        {
            fileSystem.EnsureDirectory(generationRelative);
            var entries =
                new Dictionary<string, ArtifactDescriptorEntry>(
                    StringComparer.Ordinal);
            var paths = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var (name, bytes) in decoded)
            {
                var artifactName = $"{name}.json";
                var relative = Path.Combine(
                    generationRelative,
                    artifactName);
                fileSystem.CreateNewFile(relative, bytes);
                entries.Add(
                    name,
                    new ArtifactDescriptorEntry(
                        artifactName,
                        bytes.Length,
                        Convert.ToHexString(SHA256.HashData(bytes))
                            .ToLowerInvariant()));
                paths.Add(name, fileSystem.AbsolutePath(relative));
            }

            fileSystem.FlushDirectory(generationRelative);
            fileSystem.FlushDirectory("generations");
            fileSystem.FlushDirectory(string.Empty);
            var manifest = new GenerationManifest(
                GenerationManifestFormat,
                1,
                generation,
                entries);
            var stagingRelative =
                $"current.json.{Environment.ProcessId}.{Guid.NewGuid():N}.tmp";
            pointerStaging = fileSystem.CreateNewFile(
                stagingRelative,
                JsonBytes(manifest));
            ThrowAtTestHook(request, "before-native-rename");
            fileSystem.Rename(
                pointerStaging,
                "current.json",
                replace: true,
                onCommitted: () => pointerPublished = true);
            ThrowAtTestHook(request, "after-native-rename");
            fileSystem.ValidatePinnedPaths();
            ThrowAtTestHook(request, "after-rename-validation");
            fileSystem.FlushDirectory(string.Empty);
            return new
            {
                rootPath = root,
                generation,
                generationPath = fileSystem.AbsolutePath(generationRelative),
                manifestPath = fileSystem.AbsolutePath("current.json"),
                paths
            };
        }
        finally
        {
            if (!pointerPublished)
            {
                if (pointerStaging is not null)
                {
                    fileSystem.TryDelete(pointerStaging);
                }
                fileSystem.DeleteSubtree(generationRelative);
            }
        }
    }

    private static object ResolveGeneration(HelperRequest request)
    {
        var root = RequireAbsolute(request.Root, "root");
        var requiredNames = request.RequiredNames ??
            throw new HelperException(
                "invalid_request",
                "Required generation names are missing.");
        if (requiredNames.Count == 0 ||
            requiredNames.Any(name => !LogicalName().IsMatch(name)) ||
            requiredNames.Distinct(StringComparer.Ordinal).Count() !=
                requiredNames.Count)
        {
            throw new HelperException(
                "invalid_request",
                "Required generation names are invalid.");
        }

        using var fileSystem = WindowsSecureFileSystem.OpenRoot(
            root,
            create: false,
            CreateTestHook(request));
        var manifestBytes = fileSystem.ReadFile(
            "current.json",
            MaximumCommitBytes);
        using var manifestDocument = ParseJson(
            manifestBytes,
            "generation manifest");
        var manifest = manifestDocument.RootElement;
        if (!TryGetExactString(
                manifest,
                "format",
                GenerationManifestFormat) ||
            !TryGetExactInt(manifest, "version", 1) ||
            !manifest.TryGetProperty("generation", out var generationElement) ||
            generationElement.ValueKind != JsonValueKind.String ||
            generationElement.GetString() is not { } generation ||
            !GenerationName().IsMatch(generation) ||
            !manifest.TryGetProperty("files", out var filesElement) ||
            filesElement.ValueKind != JsonValueKind.Object)
        {
            throw new HelperException(
                "invalid_manifest",
                "Bootstrap generation manifest is invalid.");
        }

        var generationRelative = Path.Combine("generations", generation);
        fileSystem.OpenDirectory("generations");
        fileSystem.OpenDirectory(generationRelative);
        var paths = new Dictionary<string, string>(StringComparer.Ordinal);
        var artifacts =
            new Dictionary<string, ArtifactDescriptorEntry>(
                StringComparer.Ordinal);
        var contents = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var name in requiredNames)
        {
            if (!filesElement.TryGetProperty(name, out var entry) ||
                entry.ValueKind != JsonValueKind.Object ||
                !entry.TryGetProperty("artifact", out var artifactElement) ||
                artifactElement.ValueKind != JsonValueKind.String ||
                artifactElement.GetString() is not { } artifact ||
                artifact != $"{name}.json" ||
                !SafeArtifactName().IsMatch(artifact) ||
                !entry.TryGetProperty("byteLength", out var lengthElement) ||
                !lengthElement.TryGetInt32(out var byteLength) ||
                byteLength < 0 ||
                !entry.TryGetProperty("sha256", out var digestElement) ||
                digestElement.ValueKind != JsonValueKind.String ||
                digestElement.GetString() is not { } sha256 ||
                !Sha256().IsMatch(sha256))
            {
                throw new HelperException(
                    "invalid_manifest",
                    $"Bootstrap generation manifest is missing '{name}'.");
            }

            var relative = Path.Combine(generationRelative, artifact);
            var bytes = fileSystem.ReadFile(relative, MaximumArtifactBytes);
            var actualDigest = Convert.ToHexString(SHA256.HashData(bytes))
                .ToLowerInvariant();
            if (bytes.Length != byteLength ||
                !CryptographicOperations.FixedTimeEquals(
                    Convert.FromHexString(actualDigest),
                    Convert.FromHexString(sha256)))
            {
                throw new HelperException(
                    "integrity_mismatch",
                    $"Bootstrap generation artifact '{name}' does not match " +
                    "its manifest.");
            }

            paths.Add(name, fileSystem.AbsolutePath(relative));
            artifacts.Add(
                name,
                new ArtifactDescriptorEntry(
                    artifact,
                    byteLength,
                    sha256));
            contents.Add(name, Convert.ToBase64String(bytes));
        }

        fileSystem.ValidatePinnedPaths();
        return new
        {
            rootPath = root,
            generation,
            generationPath = fileSystem.AbsolutePath(generationRelative),
            manifestPath = fileSystem.AbsolutePath("current.json"),
            paths,
            artifacts,
            contents
        };
    }

    private static JsonDocument ParseJson(
        byte[] bytes,
        string description)
    {
        try
        {
            return JsonDocument.Parse(bytes);
        }
        catch (JsonException error)
        {
            throw new HelperException(
                "invalid_json",
                $"The {description} is invalid JSON.",
                error);
        }
    }

    private static bool TryGetExactString(
        JsonElement element,
        string property,
        string expected) =>
        element.ValueKind == JsonValueKind.Object &&
        element.TryGetProperty(property, out var value) &&
        value.ValueKind == JsonValueKind.String &&
        string.Equals(
            value.GetString(),
            expected,
            StringComparison.Ordinal);

    private static bool TryGetExactInt(
        JsonElement element,
        string property,
        int expected) =>
        element.ValueKind == JsonValueKind.Object &&
        element.TryGetProperty(property, out var value) &&
        value.TryGetInt32(out var actual) &&
        actual == expected;

    private static byte[] DecodeContent(string? content)
    {
        if (string.IsNullOrWhiteSpace(content))
        {
            throw new HelperException(
                "invalid_request",
                "Base64 file content is required.");
        }
        try
        {
            return Convert.FromBase64String(content);
        }
        catch (FormatException error)
        {
            throw new HelperException(
                "invalid_request",
                "File content is not valid base64.",
                error);
        }
    }

    private static string RequireAbsolute(string? path, string name)
    {
        if (string.IsNullOrWhiteSpace(path) ||
            !Path.IsPathFullyQualified(path))
        {
            throw new HelperException(
                "invalid_request",
                $"Request field '{name}' must be an absolute path.");
        }
        return Path.GetFullPath(path);
    }

    private static Action<string>? CreateTestHook(HelperRequest request)
    {
        if (request.TestHook is null)
        {
            return null;
        }
        RequireTestHooksEnabled();

        if (request.TestHook.AfterOpenPath is null)
        {
            return null;
        }

        var afterOpenPath = RequireAbsolute(
            request.TestHook.AfterOpenPath,
            "testHook.afterOpenPath");
        if (string.Equals(
                request.TestHook.FailAt,
                "after-open",
                StringComparison.Ordinal))
        {
            return openedPath =>
            {
                if (string.Equals(
                        Path.GetFullPath(openedPath),
                        afterOpenPath,
                        StringComparison.OrdinalIgnoreCase))
                {
                    throw new HelperException(
                        "test_hook_failure",
                        $"Injected failure after opening '{afterOpenPath}'.");
                }
            };
        }

        var signalPath = RequireAbsolute(
            request.TestHook.SignalPath,
            "testHook.signalPath");
        var releasePath = RequireAbsolute(
            request.TestHook.ReleasePath,
            "testHook.releasePath");
        var fired = false;
        return openedPath =>
        {
            if (fired || !string.Equals(
                    Path.GetFullPath(openedPath),
                    afterOpenPath,
                    StringComparison.OrdinalIgnoreCase))
            {
                return;
            }

            fired = true;
            File.WriteAllText(signalPath, "opened");
            var timeout = Stopwatch.StartNew();
            while (!File.Exists(releasePath))
            {
                if (timeout.Elapsed > TimeSpan.FromSeconds(10))
                {
                    throw new HelperException(
                        "test_hook_timeout",
                        "Timed out waiting for helper test release.");
                }
                Thread.Sleep(10);
            }
        };
    }

    private static object TestHandleCount(HelperRequest request)
    {
        _ = request;
        RequireTestHooksEnabled();
        using var process = Process.GetCurrentProcess();
        return new { handleCount = process.HandleCount };
    }

    private static void ThrowAtTestHook(
        HelperRequest request,
        string failurePoint)
    {
        if (!string.Equals(
                request.TestHook?.FailAt,
                failurePoint,
                StringComparison.Ordinal))
        {
            return;
        }

        RequireTestHooksEnabled();
        throw new HelperException(
            "test_hook_failure",
            $"Injected failure at '{failurePoint}'.");
    }

    private static void RequireTestHooksEnabled()
    {
        if (!string.Equals(
                Environment.GetEnvironmentVariable(
                    "FLAGGO_DIRECTORY_HELPER_TEST_HOOKS"),
                "1",
                StringComparison.Ordinal))
        {
            throw new HelperException(
                "invalid_request",
                "Helper test hooks are disabled.");
        }
    }

    private static byte[] JsonBytes<T>(T value)
    {
        var serialized = JsonSerializer.SerializeToUtf8Bytes(
            value,
            IndentedJson);
        var bytes = GC.AllocateUninitializedArray<byte>(
            serialized.Length + 1);
        serialized.CopyTo(bytes, 0);
        bytes[^1] = (byte)'\n';
        return bytes;
    }

    private static string CombineRelative(string directory, string name) =>
        directory.Length == 0 ? name : Path.Combine(directory, name);

    [GeneratedRegex(
        "\\A[A-Za-z0-9][A-Za-z0-9._-]{0,127}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex SafeArtifactName();

    [GeneratedRegex(
        "\\A[a-z][a-z0-9-]*\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex LogicalName();

    [GeneratedRegex(
        "\\A\\d+-\\d+-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex GenerationName();

    [GeneratedRegex(
        "\\A[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex Sha256();

    private sealed record ArtifactDescriptor(
        string Format,
        int Version,
        string Artifact,
        int ByteLength,
        string Sha256);

    private sealed record ArtifactDescriptorEntry(
        string Artifact,
        int ByteLength,
        string Sha256);

    private sealed record GenerationManifest(
        string Format,
        int Version,
        string Generation,
        IReadOnlyDictionary<string, ArtifactDescriptorEntry> Files);
}
