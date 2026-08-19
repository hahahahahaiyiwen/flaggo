using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Registry;

public sealed partial class InMemoryDefinitionRegistry
{
    private const int PersistenceFormatVersion = 1;

    private static readonly JsonSerializerOptions PersistenceJsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    internal JsonElement CapturePersistenceState()
    {
        lock (_gate)
        {
            var applyEntries = _applyEntries.Select(pair =>
            {
                var (bodyKind, body) = pair.Value.Outcome.Body switch
                {
                    RegistrationReceipt receipt => (
                        "registration-receipt",
                        JsonSerializer.SerializeToElement(
                            receipt,
                            PersistenceJsonOptions)),
                    RequiresApprovalResult pending => (
                        "requires-approval",
                        JsonSerializer.SerializeToElement(
                            pending,
                            PersistenceJsonOptions)),
                    _ => throw new InvalidOperationException(
                        $"Unsupported persisted apply outcome '{pair.Value.Outcome.Body.GetType().Name}'.")
                };
                return new PersistedApplyEntry(
                    pair.Key,
                    pair.Value.BundleDigest,
                    pair.Value.Outcome.StatusCode,
                    bodyKind,
                    body);
            }).ToArray();
            var approvals = _approvals.Select(pair =>
                new PersistedApprovalEntry(
                    pair.Key,
                    pair.Value.Bundle.Clone(),
                    pair.Value.CanonicalBytes.ToArray(),
                    pair.Value.Result,
                    pair.Value.ExpiresAt,
                    pair.Value.Baselines)).ToArray();
            return JsonSerializer.SerializeToElement(
                new PersistedRegistryState(
                    PersistenceFormatVersion,
                    _definitions.Values
                        .Select(definition => definition.Runtime)
                        .ToArray(),
                    applyEntries,
                    approvals,
                    _definitions.Values
                        .Select(definition => definition.Intelligence)
                        .Where(definition => definition is not null)
                        .Select(definition => definition!)
                        .ToArray()),
                PersistenceJsonOptions);
        }
    }

    internal static InMemoryDefinitionRegistry RestorePersistenceState(
        JsonElement persistedState,
        IDefinitionIdentityGenerator identityGenerator,
        TimeProvider timeProvider,
        IEnumerable<RuntimeDecisionDefinition>?
            legacyRuntimeDefinitions = null,
        IEnumerable<IntelligenceLifecycleDefinitionSnapshot>?
            legacyIntelligenceDefinitions = null)
    {
        var state = persistedState.Deserialize<PersistedRegistryState>(
                PersistenceJsonOptions)
            ?? throw new InvalidDataException(
                "The local definition registry state is empty.");
        if (state.Version != PersistenceFormatVersion)
        {
            throw new InvalidDataException(
                $"Unsupported local definition registry format version '{state.Version}'.");
        }

        var runtimeDefinitions = MigrateLegacyRuntimeDefinitions(
            persistedState,
            state.Definitions,
            legacyRuntimeDefinitions ?? []);
        var intelligenceDefinitions = state.IntelligenceDefinitions ??
            MatchLegacyIntelligenceDefinitions(
                runtimeDefinitions,
                legacyIntelligenceDefinitions ?? []);
        var registry = new InMemoryDefinitionRegistry(
            runtimeDefinitions,
            identityGenerator,
            timeProvider,
            intelligenceDefinitions: intelligenceDefinitions);
        foreach (var entry in state.ApplyEntries)
        {
            object body = entry.BodyKind switch
            {
                "registration-receipt" =>
                    entry.Body.Deserialize<RegistrationReceipt>(
                        PersistenceJsonOptions)
                    ?? throw new InvalidDataException(
                        "A persisted registration receipt is empty."),
                "requires-approval" =>
                    entry.Body.Deserialize<RequiresApprovalResult>(
                        PersistenceJsonOptions)
                    ?? throw new InvalidDataException(
                        "A persisted pending approval result is empty."),
                _ => throw new InvalidDataException(
                    $"Unsupported persisted apply outcome kind '{entry.BodyKind}'.")
            };
            registry._applyEntries.Add(
                entry.IdempotencyKey,
                new ApplyEntry(
                    entry.BundleDigest,
                    new DefinitionBundleApplyResult(entry.StatusCode, body)));
        }

        foreach (var entry in state.Approvals)
        {
            registry._approvals.Add(
                entry.ApprovalRequestId,
                new ApprovalEntry(
                    entry.Bundle.Clone(),
                    entry.CanonicalBytes.ToArray(),
                    entry.Result,
                    entry.ExpiresAt,
                    entry.Baselines));
        }

        return registry;
    }

    private static IReadOnlyList<RuntimeDecisionDefinition>
        MigrateLegacyRuntimeDefinitions(
            JsonElement persistedState,
            IReadOnlyList<RuntimeDecisionDefinition> definitions,
            IEnumerable<RuntimeDecisionDefinition> legacyRuntimeDefinitions)
    {
        if (!persistedState.TryGetProperty("definitions", out var persisted) ||
            persisted.ValueKind != JsonValueKind.Array ||
            persisted.GetArrayLength() != definitions.Count)
        {
            return definitions;
        }

        var persistedDefinitions = persisted.EnumerateArray().ToArray();
        var legacyDocument =
            !persistedState.TryGetProperty("intelligenceDefinitions", out _);
        var seedDefinitions = legacyRuntimeDefinitions.ToArray();
        return definitions.Select((definition, index) =>
        {
            var serialized = persistedDefinitions[index];
            var hasExplicitTargeting =
                serialized.TryGetProperty("targetHierarchy", out _) ||
                serialized.TryGetProperty("inferenceTarget", out _) ||
                serialized.TryGetProperty("fallbackOrder", out _);
            var migrated = hasExplicitTargeting
                ? definition
                : definition with
                {
                    RuntimeContext = definition.RuntimeContext
                        .Select(field => field with
                        {
                            TargetType = field.TargetType ??
                                LegacyTargetType(field.Key)
                        })
                        .ToArray(),
                    FallbackOrder = definition.TargetHierarchy
                        .SkipWhile(target => !string.Equals(
                            target,
                            definition.InferenceTarget,
                            StringComparison.Ordinal))
                        .Skip(1)
                        .ToArray()
                };
            if (!legacyDocument)
            {
                return migrated;
            }

            var seed = seedDefinitions.SingleOrDefault(candidate =>
                SameDefinitionIdentity(candidate, migrated) &&
                string.Equals(
                    candidate.LifecycleStatus,
                    migrated.LifecycleStatus,
                    StringComparison.Ordinal));
            return seed is null
                ? migrated
                : migrated with
            {
                NumberActionSpace =
                    migrated.NumberActionSpace ?? seed.NumberActionSpace,
                Policy = migrated.Policy ?? seed.Policy
            };
        }).ToArray();
    }

    private static IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot>
        MatchLegacyIntelligenceDefinitions(
            IReadOnlyList<RuntimeDecisionDefinition> runtimeDefinitions,
            IEnumerable<IntelligenceLifecycleDefinitionSnapshot>
                legacyIntelligenceDefinitions)
    {
        return legacyIntelligenceDefinitions
            .Where(intelligence => runtimeDefinitions.Any(runtime =>
                SameDefinitionIdentity(runtime, intelligence) &&
                string.Equals(
                    runtime.LifecycleStatus,
                    intelligence.LifecycleStatus,
                    StringComparison.Ordinal)))
            .ToArray();
    }

    private static bool SameDefinitionIdentity(
        RuntimeDecisionDefinition left,
        RuntimeDecisionDefinition right) =>
        string.Equals(left.AppId, right.AppId, StringComparison.Ordinal) &&
        string.Equals(
            left.Environment,
            right.Environment,
            StringComparison.Ordinal) &&
        string.Equals(
            left.DecisionKey,
            right.DecisionKey,
            StringComparison.Ordinal) &&
        left.Identity == right.Identity;

    private static bool SameDefinitionIdentity(
        RuntimeDecisionDefinition runtime,
        IntelligenceLifecycleDefinitionSnapshot intelligence) =>
        string.Equals(
            runtime.AppId,
            intelligence.AppId,
            StringComparison.Ordinal) &&
        string.Equals(
            runtime.Environment,
            intelligence.Environment,
            StringComparison.Ordinal) &&
        string.Equals(
            runtime.DecisionKey,
            intelligence.DecisionKey,
            StringComparison.Ordinal) &&
        runtime.Identity == intelligence.Identity;

    private static string? LegacyTargetType(string key) =>
        key switch
        {
            "sessionId" => "session",
            "userId" => "user",
            "cohort" => "cohort",
            _ => null
        };

    private sealed record PersistedRegistryState(
        int Version,
        IReadOnlyList<RuntimeDecisionDefinition> Definitions,
        IReadOnlyList<PersistedApplyEntry> ApplyEntries,
        IReadOnlyList<PersistedApprovalEntry> Approvals,
        IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot>?
            IntelligenceDefinitions = null);

    private sealed record PersistedApplyEntry(
        string IdempotencyKey,
        string BundleDigest,
        int StatusCode,
        string BodyKind,
        JsonElement Body);

    private sealed record PersistedApprovalEntry(
        string ApprovalRequestId,
        JsonElement Bundle,
        byte[] CanonicalBytes,
        DefinitionBundleApprovalResult Result,
        DateTimeOffset ExpiresAt,
        IReadOnlyDictionary<string, ApprovalBaseline?> Baselines);
}
