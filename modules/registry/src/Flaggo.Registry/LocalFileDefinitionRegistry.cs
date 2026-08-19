using System.Diagnostics;
using System.Text.Json;

namespace Flaggo.Registry;

public sealed record LocalFileDefinitionRegistryOptions(
    string FilePath,
    TimeSpan? LockTimeout = null,
    TimeSpan? LockRetryDelay = null);

public sealed class LocalFileDefinitionRegistry :
    IRuntimeDefinitionReader,
    IIntelligenceDefinitionReader,
    IRegistryHealth,
    IDefinitionBundleManager,
    IDefinitionApprovalManager
{
    private static readonly JsonSerializerOptions FileJsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            WriteIndented = true
        };

    private readonly string _filePath;
    private readonly string _lockPath;
    private readonly TimeSpan _lockTimeout;
    private readonly TimeSpan _lockRetryDelay;
    private readonly IReadOnlyList<RuntimeDecisionDefinition> _seedDefinitions;
    private readonly IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot>
        _seedIntelligenceDefinitions;
    private readonly IDefinitionIdentityGenerator _identityGenerator;
    private readonly TimeProvider _timeProvider;

    public LocalFileDefinitionRegistry(
        LocalFileDefinitionRegistryOptions options,
        IEnumerable<RuntimeDecisionDefinition> seedDefinitions,
        IDefinitionIdentityGenerator? identityGenerator = null,
        TimeProvider? timeProvider = null,
        IEnumerable<IntelligenceLifecycleDefinitionSnapshot>?
            seedIntelligenceDefinitions = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(options.FilePath);
        _filePath = Path.GetFullPath(options.FilePath);
        _lockPath = $"{_filePath}.lock";
        _lockTimeout = options.LockTimeout ?? TimeSpan.FromSeconds(10);
        _lockRetryDelay = options.LockRetryDelay ?? TimeSpan.FromMilliseconds(25);
        if (_lockTimeout <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The registry lock timeout must be positive.");
        }

        if (_lockRetryDelay <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The registry lock retry delay must be positive.");
        }

        _seedDefinitions = seedDefinitions.Select(definition => definition with
        {
            FallbackValue = definition.FallbackValue.Clone()
        }).ToArray();
        _seedIntelligenceDefinitions =
            (seedIntelligenceDefinitions ?? []).ToArray();
        _identityGenerator = identityGenerator ?? new GuidDefinitionIdentityGenerator();
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public Task<RuntimeDefinitionLookup> ResolveRuntimeAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken) =>
        ExecuteAsync(
            false,
            registry => registry.ResolveRuntimeAsync(
                appId,
                environment,
                decisionKey,
                definitionId,
                revision,
                cancellationToken),
            cancellationToken);

    public Task<IntelligenceDefinitionLookup> ResolveIntelligenceAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken) =>
        ExecuteAsync(
            false,
            registry => registry.ResolveIntelligenceAsync(
                appId,
                environment,
                decisionKey,
                definitionId,
                revision,
                cancellationToken),
            cancellationToken);

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        try
        {
            return await ExecuteAsync(
                false,
                _ => Task.FromResult(true),
                cancellationToken);
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
        catch (JsonException)
        {
            return false;
        }
        catch (InvalidDataException)
        {
            return false;
        }
        catch (TimeoutException)
        {
            return false;
        }
    }

    public Task<DefinitionBundleValidationResult> ValidateAsync(
        JsonElement bundle,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            false,
            registry => registry.ValidateAsync(bundle, cancellationToken),
            cancellationToken);

    public Task<DefinitionBundleApplyResult> ApplyAsync(
        string idempotencyKey,
        JsonElement bundle,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            true,
            registry => registry.ApplyAsync(
                idempotencyKey,
                bundle,
                cancellationToken),
            cancellationToken);

    public Task<DefinitionBundleApprovalResult?> GetApprovalAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            true,
            registry => registry.GetApprovalAsync(
                approvalRequestId,
                cancellationToken),
            cancellationToken);

    public Task<DefinitionBundleSnapshot?> GetSnapshotAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            false,
            registry => registry.GetSnapshotAsync(
                approvalRequestId,
                cancellationToken),
            cancellationToken);

    public Task<DefinitionBundleApprovalResult> ApproveAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string? comment,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            true,
            registry => registry.ApproveAsync(
                approvalRequestId,
                expectedBundleDigest,
                actor,
                comment,
                cancellationToken),
            cancellationToken);

    public Task<DefinitionBundleApprovalResult> RejectAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string reasonCode,
        string? comment,
        CancellationToken cancellationToken = default) =>
        ExecuteAsync(
            true,
            registry => registry.RejectAsync(
                approvalRequestId,
                expectedBundleDigest,
                actor,
                reasonCode,
                comment,
                cancellationToken),
            cancellationToken);

    private async Task<TResult> ExecuteAsync<TResult>(
        bool persistMutation,
        Func<InMemoryDefinitionRegistry, Task<TResult>> operation,
        CancellationToken cancellationToken)
    {
        await using var lease = await AcquireLeaseAsync(cancellationToken);
        var registry = await LoadAsync(cancellationToken);
        var result = await operation(registry);
        if (persistMutation)
        {
            await SaveAsync(
                registry.CapturePersistenceState(),
                cancellationToken);
        }

        return result;
    }

    private async Task<FileStream> AcquireLeaseAsync(
        CancellationToken cancellationToken)
    {
        var directory = Path.GetDirectoryName(_filePath)
            ?? throw new InvalidOperationException(
                "The registry file path must include a directory.");
        Directory.CreateDirectory(directory);
        var started = Stopwatch.GetTimestamp();
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                return new FileStream(
                    _lockPath,
                    FileMode.OpenOrCreate,
                    FileAccess.ReadWrite,
                    FileShare.None,
                    1,
                    FileOptions.Asynchronous);
            }
            catch (IOException) when (
                Stopwatch.GetElapsedTime(started) < _lockTimeout)
            {
                await Task.Delay(_lockRetryDelay, cancellationToken);
            }
            catch (IOException error)
            {
                throw new TimeoutException(
                    $"Timed out acquiring the local definition registry lock '{_lockPath}'.",
                    error);
            }
        }
    }

    private async Task<InMemoryDefinitionRegistry> LoadAsync(
        CancellationToken cancellationToken)
    {
        if (!File.Exists(_filePath))
        {
            var initial = new InMemoryDefinitionRegistry(
                _seedDefinitions,
                _identityGenerator,
                _timeProvider,
                intelligenceDefinitions: _seedIntelligenceDefinitions);
            await SaveAsync(
                initial.CapturePersistenceState(),
                cancellationToken);
            return initial;
        }

        await using var stream = new FileStream(
            _filePath,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read,
            4096,
            FileOptions.Asynchronous | FileOptions.SequentialScan);
        using var document = await JsonDocument.ParseAsync(
            stream,
            cancellationToken: cancellationToken);
        return InMemoryDefinitionRegistry.RestorePersistenceState(
            document.RootElement,
            _identityGenerator,
            _timeProvider,
            _seedDefinitions,
            _seedIntelligenceDefinitions);
    }

    private async Task SaveAsync(
        JsonElement state,
        CancellationToken cancellationToken)
    {
        var temporaryPath = $"{_filePath}.{Guid.NewGuid():N}.tmp";
        try
        {
            await using (var stream = new FileStream(
                             temporaryPath,
                             FileMode.CreateNew,
                             FileAccess.Write,
                             FileShare.None,
                             4096,
                             FileOptions.Asynchronous | FileOptions.WriteThrough))
            {
                await JsonSerializer.SerializeAsync(
                    stream,
                    state,
                    FileJsonOptions,
                    cancellationToken);
                await stream.FlushAsync(cancellationToken);
                stream.Flush(true);
            }

            File.Move(temporaryPath, _filePath, true);
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }
}
