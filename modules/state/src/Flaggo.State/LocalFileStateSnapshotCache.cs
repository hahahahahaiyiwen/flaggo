using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed class LocalFileStateSnapshotCache
{
    private readonly object _gate = new();
    private Entry? _entry;

    internal IReadOnlyDictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState> GetOrAdd(
        CommittedArtifactReference verifiedArtifact,
        Func<IReadOnlyDictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState>> validate,
        CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (_entry is { } entry &&
                entry.Sha256 == verifiedArtifact.Sha256 &&
                entry.ByteLength == verifiedArtifact.ByteLength)
            {
                return entry.States;
            }

            var states = validate();
            cancellationToken.ThrowIfCancellationRequested();
            _entry = new Entry(verifiedArtifact.Sha256, verifiedArtifact.ByteLength, states);
            return states;
        }
    }

    private sealed record Entry(
        string Sha256,
        long ByteLength,
        IReadOnlyDictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState> States);
}
