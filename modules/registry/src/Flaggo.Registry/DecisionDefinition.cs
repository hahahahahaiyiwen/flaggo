using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Shared.Contracts;

namespace Flaggo.Registry;

public sealed record RegisteredSignalInput(
    string Key,
    string ValueType,
    double? Minimum = null,
    double? Maximum = null);

public sealed record RegisteredRuntimeContextField(
    string Key,
    string ValueType,
    bool Required = false,
    string? TargetType = null);

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

public sealed record RuntimeDecisionDefinition
{
    private static readonly string[] LegacyTargetHierarchy =
        ["session", "user", "cohort", "global"];

    [JsonConstructor]
    public RuntimeDecisionDefinition(
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
        DecisionPolicyContract? Policy = null,
        IReadOnlyList<string>? TargetHierarchy = null,
        string? InferenceTarget = null,
        IReadOnlyList<string>? FallbackOrder = null)
    {
        this.AppId = AppId;
        this.Environment = Environment;
        this.DecisionKey = DecisionKey;
        this.Identity = Identity;
        this.ValueType = ValueType;
        this.FallbackValue = FallbackValue.Clone();
        this.FallbackReason = FallbackReason;
        this.Inputs = Inputs.ToArray();
        this.RuntimeContext = RuntimeContext.ToArray();
        this.LifecycleStatus = LifecycleStatus;
        this.NumberActionSpace = NumberActionSpace;
        this.Policy = Policy;

        var explicitHierarchy = TargetHierarchy?
            .Where(target => !string.IsNullOrWhiteSpace(target))
            .ToArray();
        var hierarchy = explicitHierarchy is { Length: > 0 }
            ? explicitHierarchy
            : BuildDerivedHierarchy(
                InferenceTarget,
                this.RuntimeContext,
                FallbackOrder);
        this.TargetHierarchy = hierarchy;
        this.InferenceTarget = string.IsNullOrWhiteSpace(InferenceTarget)
            ? hierarchy[0]
            : InferenceTarget;
        this.FallbackOrder = FallbackOrder?.ToArray() ?? [];
    }

    public string AppId { get; init; }

    public string Environment { get; init; }

    public string DecisionKey { get; init; }

    public RuntimeContractIdentity Identity { get; init; }

    public string ValueType { get; init; }

    public JsonElement FallbackValue { get; init; }

    public string FallbackReason { get; init; }

    public IReadOnlyList<RegisteredSignalInput> Inputs { get; init; }

    public IReadOnlyList<RegisteredRuntimeContextField> RuntimeContext { get; init; }

    public string LifecycleStatus { get; init; }

    public NumberActionSpaceContract? NumberActionSpace { get; init; }

    public DecisionPolicyContract? Policy { get; init; }

    public IReadOnlyList<string> TargetHierarchy { get; init; }

    public string InferenceTarget { get; init; }

    public IReadOnlyList<string> FallbackOrder { get; init; }

    public bool AllowsTargetKind(string targetType) =>
        TargetHierarchy.Contains(targetType, StringComparer.Ordinal);

    private static IReadOnlyList<string> BuildDerivedHierarchy(
        string? inferenceTarget,
        IReadOnlyList<RegisteredRuntimeContextField> runtimeContext,
        IReadOnlyList<string>? fallbackOrder)
    {
        if (string.IsNullOrWhiteSpace(inferenceTarget) &&
            runtimeContext.All(field => string.IsNullOrWhiteSpace(field.TargetType)) &&
            fallbackOrder is null)
        {
            return LegacyTargetHierarchy.ToArray();
        }

        var hierarchy = new List<string>();
        AddTarget(hierarchy, inferenceTarget);
        foreach (var target in runtimeContext
                     .Select(field => field.TargetType)
                     .Where(target => !string.IsNullOrWhiteSpace(target))
                     .Distinct(StringComparer.Ordinal)
                     .OrderBy(TargetPrecedence)
                     .ThenBy(target => target, StringComparer.Ordinal))
        {
            AddTarget(hierarchy, target);
        }

        if (fallbackOrder is not null)
        {
            foreach (var target in fallbackOrder)
            {
                AddTarget(hierarchy, target);
            }
        }

        if (hierarchy.Count == 0)
        {
            hierarchy.AddRange(LegacyTargetHierarchy);
        }

        return hierarchy;
    }

    private static int TargetPrecedence(string? target) =>
        Array.FindIndex(
            LegacyTargetHierarchy,
            candidate => string.Equals(
                candidate,
                target,
                StringComparison.Ordinal)) is var index && index >= 0
            ? index
            : LegacyTargetHierarchy.Length;

    private static void AddTarget(ICollection<string> targets, string? target)
    {
        if (!string.IsNullOrWhiteSpace(target) &&
            !targets.Contains(target, StringComparer.Ordinal))
        {
            targets.Add(target);
        }
    }
}

public sealed record DecisionObjective(
    string SignalKey,
    string Direction,
    double? Target = null);

public sealed record DecisionObjectives(
    string? NaturalLanguage = null,
    DecisionObjective? Primary = null,
    IReadOnlyList<DecisionObjective>? Secondary = null,
    string? Rationale = null);

public sealed record RegisteredDecisionSignalRoles(
    IReadOnlyList<string> Allowed,
    IReadOnlyList<string> Evidence,
    IReadOnlyList<string> Guardrails);

public sealed record DecisionWorkflowPermissions(
    string Mode,
    IReadOnlyList<string> LiveInputs);

public sealed record DecisionActionSpaceContract(
    string ValueType,
    JsonElement DefaultValue,
    double? Minimum = null,
    double? Maximum = null,
    double? Step = null,
    IReadOnlyList<string>? AllowedValues = null);

public sealed record IntelligenceLifecycleDefinitionSnapshot(
    string AppId,
    string Environment,
    string DecisionKey,
    RuntimeContractIdentity Identity,
    string LifecycleStatus,
    DecisionObjectives Objectives,
    RegisteredDecisionSignalRoles SignalRoles,
    DecisionWorkflowPermissions WorkflowPermissions,
    DecisionActionSpaceContract ActionSpace,
    DecisionPolicyContract SafetyEnvelope);

public sealed record RuntimeDefinitionLookup(
    bool DecisionKeyExists,
    RuntimeDecisionDefinition? Definition);

public sealed record IntelligenceDefinitionLookup(
    bool DecisionKeyExists,
    IntelligenceLifecycleDefinitionSnapshot? Definition);

public interface IRuntimeDefinitionReader
{
    Task<RuntimeDefinitionLookup> ResolveRuntimeAsync(
        string appId,
        string environment,
        string decisionKey,
        string definitionId,
        string revision,
        CancellationToken cancellationToken);
}

public interface IIntelligenceDefinitionReader
{
    Task<IntelligenceDefinitionLookup> ResolveIntelligenceAsync(
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

internal sealed record RegisteredDefinitionProjections(
    RuntimeDecisionDefinition Runtime,
    IntelligenceLifecycleDefinitionSnapshot? Intelligence);

public sealed partial class InMemoryDefinitionRegistry :
    IRuntimeDefinitionReader,
    IIntelligenceDefinitionReader,
    IRegistryHealth
{
    private readonly object _gate = new();
    private readonly Dictionary<
        (string AppId, string Environment, string Key, string DefinitionId, string Revision),
        RegisteredDefinitionProjections> _definitions;
    private readonly HashSet<(string AppId, string Environment, string Key)> _decisionKeys;
    private readonly IDefinitionIdentityGenerator _identityGenerator;
    private readonly TimeProvider _timeProvider;
    private readonly bool _available;

    public InMemoryDefinitionRegistry(
        IEnumerable<RuntimeDecisionDefinition> definitions,
        IDefinitionIdentityGenerator? identityGenerator = null,
        TimeProvider? timeProvider = null,
        bool available = true,
        IEnumerable<IntelligenceLifecycleDefinitionSnapshot>?
            intelligenceDefinitions = null)
    {
        var runtimeDefinitions = definitions.ToArray();
        var intelligenceByIdentity = (intelligenceDefinitions ?? [])
            .ToDictionary(
                definition => (
                    definition.AppId,
                    definition.Environment,
                    definition.DecisionKey,
                    definition.Identity.DefinitionId,
                    definition.Identity.Revision));
        _definitions = runtimeDefinitions.ToDictionary(
            DefinitionKey,
            definition =>
            {
                var key = DefinitionKey(definition);
                return new RegisteredDefinitionProjections(
                    definition,
                    intelligenceByIdentity.TryGetValue(key, out var intelligence) &&
                    intelligence.Identity == definition.Identity &&
                    string.Equals(
                        intelligence.LifecycleStatus,
                        definition.LifecycleStatus,
                        StringComparison.Ordinal)
                        ? intelligence
                        : null);
            });
        _decisionKeys = runtimeDefinitions.Select(definition => (
                definition.AppId,
                definition.Environment,
                definition.DecisionKey))
            .ToHashSet();
        _identityGenerator = identityGenerator ?? new GuidDefinitionIdentityGenerator();
        _timeProvider = timeProvider ?? TimeProvider.System;
        _available = available;
    }

    public Task<RuntimeDefinitionLookup> ResolveRuntimeAsync(
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
                new RuntimeDefinitionLookup(
                    _decisionKeys.Contains((appId, environment, decisionKey)),
                    definition?.Runtime));
        }
    }

    public Task<IntelligenceDefinitionLookup> ResolveIntelligenceAsync(
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
                new IntelligenceDefinitionLookup(
                    _decisionKeys.Contains((appId, environment, decisionKey)),
                    definition?.Intelligence));
        }
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(_available);
    }

    private static (
        string AppId,
        string Environment,
        string Key,
        string DefinitionId,
        string Revision) DefinitionKey(RuntimeDecisionDefinition definition) =>
        (
            definition.AppId,
            definition.Environment,
            definition.DecisionKey,
            definition.Identity.DefinitionId,
            definition.Identity.Revision);

}
