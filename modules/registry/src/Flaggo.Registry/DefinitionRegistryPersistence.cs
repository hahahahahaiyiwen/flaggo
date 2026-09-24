using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Registry;

public sealed partial class InMemoryDefinitionRegistry
{
    private const int PersistenceFormatVersion = 2;

    private static readonly JsonSerializerOptions PersistenceJsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
            RespectRequiredConstructorParameters = true,
            RespectNullableAnnotations = true
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
        TimeProvider timeProvider)
    {
        if (!persistedState.TryGetProperty("version", out var version) ||
            !version.TryGetInt32(out var format) || format != PersistenceFormatVersion)
        {
            throw new InvalidDataException("The local definition registry requires format version 2.");
        }
        var state = persistedState.Deserialize<PersistedRegistryState>(
                PersistenceJsonOptions)
            ?? throw new InvalidDataException(
                "The local definition registry state is empty.");
        if (state.Version != PersistenceFormatVersion)
        {
            throw new InvalidDataException(
                $"Unsupported local definition registry format version '{state.Version}'.");
        }

        var registry = new InMemoryDefinitionRegistry(
            state.Definitions,
            identityGenerator,
            timeProvider,
            intelligenceDefinitions: state.IntelligenceDefinitions);
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

    private sealed record PersistedRegistryState(
        int Version,
        IReadOnlyList<RuntimeDecisionDefinition> Definitions,
        IReadOnlyList<PersistedApplyEntry> ApplyEntries,
        IReadOnlyList<PersistedApprovalEntry> Approvals,
        IReadOnlyList<IntelligenceLifecycleDefinitionSnapshot> IntelligenceDefinitions);

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
