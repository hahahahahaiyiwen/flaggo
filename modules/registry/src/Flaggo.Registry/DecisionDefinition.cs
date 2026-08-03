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
    string LifecycleStatus = "active");

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

public sealed class InMemoryDefinitionRegistry(
    IEnumerable<RegisteredDecisionDefinition> definitions,
    bool available = true) : IDefinitionRegistry, IRegistryHealth
{
    private readonly IReadOnlyDictionary<
            (string AppId, string Environment, string Key, string DefinitionId, string Revision),
            RegisteredDecisionDefinition>
        _definitions = definitions.ToDictionary(
            definition => (
                definition.AppId,
                definition.Environment,
                definition.DecisionKey,
                definition.Identity.DefinitionId,
                definition.Identity.Revision));
    private readonly IReadOnlySet<(string AppId, string Environment, string Key)> _decisionKeys =
        definitions.Select(definition => (
                definition.AppId,
                definition.Environment,
                definition.DecisionKey))
            .ToHashSet();

    public Task<DefinitionLookup> ResolveAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        _definitions.TryGetValue(
            (appId, environment, decisionKey, definitionId, revision),
            out var definition);
        return Task.FromResult(
            new DefinitionLookup(
                _decisionKeys.Contains((appId, environment, decisionKey)),
                definition));
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}
