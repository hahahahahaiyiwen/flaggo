using System.Collections.Concurrent;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;

namespace Flaggo.Decision;

public interface IDecisionRuntime
{
    Task<RuntimeDecision> DecideAsync(
        string contractName,
        string contractDigest,
        RuntimeInput input,
        CancellationToken cancellationToken = default);
}

public sealed class DecisionRuntime(
    IContractVersionStore contractStore,
    IExecutableStore executableStore,
    FlaggoExecutableCompiler executableCompiler,
    DecisionEvaluator evaluator) : IDecisionRuntime
{
    private const int MaximumCompiledExecutables = 1_024;

    private readonly ConcurrentDictionary<
        CompiledExecutableCacheKey,
        Lazy<CompiledDecisionExecutable>> _compiledExecutables =
        new();
    private readonly ConcurrentQueue<
        KeyValuePair<CompiledExecutableCacheKey, Lazy<CompiledDecisionExecutable>>>
        _compiledExecutableOrder = new();

    public async Task<RuntimeDecision> DecideAsync(
        string contractName,
        string contractDigest,
        RuntimeInput input,
        CancellationToken cancellationToken = default)
    {
        ValidateIdentity(contractName, contractDigest);
        ArgumentNullException.ThrowIfNull(input);

        AcceptedContractVersion? version;
        try
        {
            version = await contractStore.GetAsync(
                contractName,
                contractDigest,
                cancellationToken);
        }
        catch (InvalidDataException exception)
        {
            throw new RuntimeIntegrityException(
                "The stored contract version failed integrity validation.",
                exception);
        }

        if (version is null)
        {
            throw new DecisionContractVersionNotFoundException(
                contractName,
                contractDigest);
        }

        var inputIssues = new List<ValidationIssue>();
        ContractValidator.ValidateRuntimeInput(version.Contract, input, inputIssues);
        if (inputIssues.Count > 0)
        {
            throw new RuntimeInputValidationException(inputIssues);
        }

        StoredExecutable? active;
        try
        {
            active = await executableStore.GetActiveAsync(
                contractDigest,
                cancellationToken);
        }
        catch (InvalidDataException exception)
        {
            throw new RuntimeIntegrityException(
                "The active executable failed integrity validation.",
                exception);
        }

        if (active is null)
        {
            throw new ActiveExecutableNotFoundException(contractDigest);
        }

        if (!string.Equals(
                active.Executable.ContractDigest,
                contractDigest,
                StringComparison.Ordinal))
        {
            throw new RuntimeIntegrityException(
                "The active executable is bound to a different contract digest.");
        }

        var compiled = GetOrMaterialize(version, active);
        return evaluator.Evaluate(
            version.Contract,
            compiled,
            input,
            cancellationToken);
    }

    private CompiledDecisionExecutable GetOrMaterialize(
        AcceptedContractVersion version,
        StoredExecutable active)
    {
        var cacheKey = new CompiledExecutableCacheKey(
            active.ExecutableDigest,
            string.Join(
                "|",
                active.CheckedExecutable.Rules.Select(rule =>
                    $"{rule.Name}:{rule.Predicate.CheckedAstDigest}:"
                    + $"{rule.Result?.CheckedAstDigest ?? "-"}")));
        var candidate = new Lazy<CompiledDecisionExecutable>(
            () => executableCompiler.Materialize(
                version.Contract,
                version.ContractDigest,
                active.Executable,
                active.ExecutableDigest,
                active.CheckedExecutable),
            LazyThreadSafetyMode.ExecutionAndPublication);
        var lazy = _compiledExecutables.GetOrAdd(cacheKey, candidate);
        try
        {
            var compiled = lazy.Value;
            if (ReferenceEquals(lazy, candidate))
            {
                _compiledExecutableOrder.Enqueue(
                    new KeyValuePair<
                        CompiledExecutableCacheKey,
                        Lazy<CompiledDecisionExecutable>>(cacheKey, lazy));
                TrimCompiledExecutableCache();
            }

            return compiled;
        }
        catch (ExecutableMaterializationException exception)
        {
            _compiledExecutables.TryRemove(
                new KeyValuePair<CompiledExecutableCacheKey, Lazy<CompiledDecisionExecutable>>(
                    cacheKey,
                    lazy));
            throw new RuntimeIntegrityException(
                "The active executable could not be materialized.",
                exception);
        }
    }

    private void TrimCompiledExecutableCache()
    {
        while (_compiledExecutables.Count > MaximumCompiledExecutables
            && _compiledExecutableOrder.TryDequeue(out var oldest))
        {
            _compiledExecutables.TryRemove(oldest);
        }
    }

    private readonly record struct CompiledExecutableCacheKey(
        string ExecutableDigest,
        string CheckedRepresentation);

    private static void ValidateIdentity(
        string contractName,
        string contractDigest)
    {
        if (!ContractValidator.IsDecisionName(contractName))
        {
            throw new ArgumentException(
                "contractName is not a valid decision name.",
                nameof(contractName));
        }

        if (!ContractDigests.IsSha256Digest(contractDigest))
        {
            throw new ArgumentException(
                "contractDigest must be a lowercase sha256 digest.",
                nameof(contractDigest));
        }
    }
}

public sealed class DecisionContractVersionNotFoundException(
    string contractName,
    string contractDigest)
    : Exception(
        $"Contract version '{contractName}' at '{contractDigest}' was not found.");

public sealed class RuntimeInputValidationException(
    IReadOnlyList<ValidationIssue> issues)
    : Exception("Runtime input does not satisfy the decision contract.")
{
    public IReadOnlyList<ValidationIssue> Issues { get; } = issues;
}

public sealed class ActiveExecutableNotFoundException(
    string contractDigest)
    : Exception($"No executable is active for contract '{contractDigest}'.");

public sealed class RuntimeIntegrityException : Exception
{
    public RuntimeIntegrityException(string message)
        : base(message)
    {
    }

    public RuntimeIntegrityException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}
