using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Registry;

public sealed record RegisteredSignalInput(
    string Key,
    string ValueType,
    double? Minimum = null,
    double? Maximum = null);

public sealed record RegisteredRuntimeContextField(
    string Key,
    string ValueType);

public sealed record NumberActionSpaceContract(
    double Minimum,
    double Maximum,
    double? Step = null);

public sealed record DecisionPolicyContract(
    double? Minimum = null,
    double? Maximum = null,
    double? MaximumDelta = null,
    double? CooldownSeconds = null,
    double? MinimumEvidenceQuality = null,
    double? MaximumModelUncertainty = null,
    double? MinimumExpectedOutcome = null,
    double? MinimumSampleSize = null,
    bool Paused = false,
    string RequiredEvidenceUnavailable = "forbid")
{
    public bool RequiresEvidence =>
        MinimumEvidenceQuality is not null ||
        MaximumModelUncertainty is not null ||
        MinimumExpectedOutcome is not null ||
        MinimumSampleSize is not null;
}

public sealed record RegisteredDecisionDefinition(
    string AppId,
    string Environment,
    string DecisionKey,
    RuntimeContractIdentity Identity,
    string ValueType,
    JsonElement FallbackValue,
    string FallbackReason,
    IReadOnlyList<RegisteredSignalInput> Inputs,
    IReadOnlyList<RegisteredRuntimeContextField> RuntimeContext,
    string LifecycleStatus = "active",
    NumberActionSpaceContract? NumberActionSpace = null,
    DecisionPolicyContract? Policy = null);

public sealed record DefinitionLookup(
    bool DecisionKeyExists,
    RegisteredDecisionDefinition? Definition);

public interface IDefinitionRegistry
{
    Task<DefinitionLookup> ResolveAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken);
}

public interface IRegistryHealth
{
    Task<bool> IsAvailableAsync(CancellationToken cancellationToken);
}

public sealed partial class InMemoryDefinitionRegistry(
    IEnumerable<RegisteredDecisionDefinition> definitions,
    IDefinitionIdentityGenerator? identityGenerator = null,
    TimeProvider? timeProvider = null,
    bool available = true) : IDefinitionRegistry, IRegistryHealth
{
    private readonly object _gate = new();
    private readonly Dictionary<
            (string AppId, string Environment, string Key, string DefinitionId, string Revision),
            RegisteredDecisionDefinition>
        _definitions = definitions.ToDictionary(
            definition => (
                definition.AppId,
                definition.Environment,
                definition.DecisionKey,
                definition.Identity.DefinitionId,
                definition.Identity.Revision));
    private readonly HashSet<(string AppId, string Environment, string Key)> _decisionKeys =
        definitions.Select(definition => (
                definition.AppId,
                definition.Environment,
                definition.DecisionKey))
            .ToHashSet();
    private readonly IDefinitionIdentityGenerator _identityGenerator =
        identityGenerator ?? new GuidDefinitionIdentityGenerator();
    private readonly TimeProvider _timeProvider = timeProvider ?? TimeProvider.System;

    public Task<DefinitionLookup> ResolveAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            _definitions.TryGetValue(
                (appId, environment, decisionKey, definitionId, revision),
                out var definition);
            return Task.FromResult(
                new DefinitionLookup(
                    _decisionKeys.Contains((appId, environment, decisionKey)),
                    definition));
        }
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}
