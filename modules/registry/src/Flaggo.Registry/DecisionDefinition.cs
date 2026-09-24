using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Shared.Contracts;

namespace Flaggo.Registry;

public sealed record RegisteredInput(
    string Key,
    string ValueType,
    string Source,
    string Meaning,
    double? Minimum = null,
    double? Maximum = null,
    string? Unit = null,
    string? Binding = null);

public sealed record TelemetryAttributeSelector(string From, string Key);

public sealed record TelemetrySourceContract(
    string Kind,
    string ScopeName,
    string? ScopeVersion,
    string? Name,
    string? EventName,
    JsonElement? BodyEquals,
    IReadOnlyDictionary<string, JsonElement> ResourceAttributes,
    IReadOnlyDictionary<string, JsonElement> Attributes,
    IReadOnlyDictionary<string, JsonElement> EventAttributes,
    string ValueFrom,
    string? ValueKey,
    IReadOnlyList<string> BodyPath);

public sealed record RegisteredEvidenceBinding(
    string Key,
    string Meaning,
    string ValueType,
    string? Unit,
    double? Minimum,
    double? Maximum,
    TelemetrySourceContract Source,
    string TargetType,
    TelemetryAttributeSelector? TargetIdAttribute,
    long MaxAgeSeconds,
    TelemetryAttributeSelector? ExposureIdAttribute);

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
    [JsonConstructor]
    public RuntimeDecisionDefinition(
        string AppId,
        string Environment,
        string DecisionKey,
        RuntimeContractIdentity Identity,
        string ValueType,
        JsonElement FallbackValue,
        string FallbackReason,
        IReadOnlyList<RegisteredInput> Inputs,
        IReadOnlyList<RegisteredRuntimeContextField> RuntimeContext,
        string LifecycleStatus = "active",
        NumberActionSpaceContract? NumberActionSpace = null,
        DecisionPolicyContract? Policy = null,
        IReadOnlyList<string>? TargetHierarchy = null,
        string? InferenceTarget = null,
        IReadOnlyList<string>? FallbackOrder = null,
        IReadOnlyList<RegisteredEvidenceBinding>? Evidence = null)
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

        this.TargetHierarchy = TargetHierarchy?.ToArray()
            ?? throw new ArgumentException("An explicit target hierarchy is required.", nameof(TargetHierarchy));
        this.InferenceTarget = InferenceTarget
            ?? throw new ArgumentException("An explicit primary target is required.", nameof(InferenceTarget));
        this.FallbackOrder = FallbackOrder?.ToArray()
            ?? throw new ArgumentException("Explicit fallback targets are required.", nameof(FallbackOrder));
        this.Evidence = Evidence?.ToArray() ?? [];
    }

    public string AppId { get; init; }

    public string Environment { get; init; }

    public string DecisionKey { get; init; }

    public RuntimeContractIdentity Identity { get; init; }

    public string ValueType { get; init; }

    public JsonElement FallbackValue { get; init; }

    public string FallbackReason { get; init; }

    public IReadOnlyList<RegisteredInput> Inputs { get; init; }

    public IReadOnlyList<RegisteredEvidenceBinding> Evidence { get; init; }

    public IReadOnlyList<RegisteredRuntimeContextField> RuntimeContext { get; init; }

    public string LifecycleStatus { get; init; }

    public NumberActionSpaceContract? NumberActionSpace { get; init; }

    public DecisionPolicyContract? Policy { get; init; }

    public IReadOnlyList<string> TargetHierarchy { get; init; }

    public string InferenceTarget { get; init; }

    public IReadOnlyList<string> FallbackOrder { get; init; }

    public bool AllowsTargetKind(string targetType) =>
        TargetHierarchy.Contains(targetType, StringComparer.Ordinal);

}

public sealed record DecisionObjective(
    string EvidenceKey,
    string Direction,
    double? Target = null);

public sealed record DecisionObjectives(
    string? NaturalLanguage = null,
    DecisionObjective? Primary = null,
    IReadOnlyList<DecisionObjective>? Secondary = null,
    string? Rationale = null);

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
    IReadOnlyList<RegisteredEvidenceBinding> Evidence,
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

public sealed record EvidenceBindingProjection(
    ApplicationScope Scope,
    string DecisionKey,
    RuntimeContractIdentity Identity,
    IReadOnlyList<RegisteredEvidenceBinding> Bindings);

public interface IEvidenceBindingReader
{
    Task<IReadOnlyList<EvidenceBindingProjection>> ReadBindingsAsync(
        ApplicationScope scope,
        CancellationToken cancellationToken);
}

public sealed record RegisteredDefinitionProjections(
    RuntimeDecisionDefinition Runtime,
    IntelligenceLifecycleDefinitionSnapshot? Intelligence);

public sealed partial class InMemoryDefinitionRegistry :
    IRuntimeDefinitionReader,
    IIntelligenceDefinitionReader,
    IEvidenceBindingReader,
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

    public Task<IReadOnlyList<EvidenceBindingProjection>> ReadBindingsAsync(
        ApplicationScope scope,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            IReadOnlyList<EvidenceBindingProjection> bindings = _definitions.Values
                .Select(entry => entry.Runtime)
                .Where(definition => definition.AppId == scope.AppId &&
                    definition.Environment == scope.Environment &&
                    definition.LifecycleStatus == "active" && definition.Evidence.Count > 0)
                .Select(definition => new EvidenceBindingProjection(
                    scope, definition.DecisionKey, definition.Identity, definition.Evidence))
                .ToArray();
            return Task.FromResult(bindings);
        }
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
