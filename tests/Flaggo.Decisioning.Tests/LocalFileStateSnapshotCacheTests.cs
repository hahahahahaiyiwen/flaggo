using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileStateSnapshotCacheTests
{
    [Fact]
    public async Task IdenticalConcurrentReadsShareValidationAndOnlyOneProjectionIsRetained()
    {
        var cache = new LocalFileStateSnapshotCache();
        var first = Artifact('a');
        var calls = 0;
        IReadOnlyDictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState> Validate()
        {
            Interlocked.Increment(ref calls);
            return new Dictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState>();
        }
        var reads = await Task.WhenAll(Enumerable.Range(0, 8).Select(_ => Task.Run(
            () => cache.GetOrAdd(first, Validate, CancellationToken.None))));

        Assert.Equal(1, calls);
        Assert.All(reads, projection => Assert.Same(reads[0], projection));
        Assert.Same(reads[0], cache.GetOrAdd(first with { ArtifactPath = "another-verified-path" },
            Validate, CancellationToken.None));
        cache.GetOrAdd(Artifact('b'), Validate, CancellationToken.None);
        Assert.Equal(2, calls);
        Assert.NotSame(reads[0], cache.GetOrAdd(first, Validate, CancellationToken.None));
        Assert.Equal(3, calls);
        cache.GetOrAdd(first with { ByteLength = first.ByteLength + 1 }, Validate, CancellationToken.None);
        Assert.Equal(4, calls);
    }

    [Fact]
    public void FailedValidationIsNotCachedAndCannotReturnThePreviousProjection()
    {
        var cache = new LocalFileStateSnapshotCache();
        var previous = cache.GetOrAdd(Artifact('a'),
            () => new Dictionary<LocalFileStateStore.StateIdentity, GovernedDecisionState>(), CancellationToken.None);
        for (var attempt = 0; attempt < 2; attempt++)
        {
            Assert.Throws<InvalidDataException>(() => cache.GetOrAdd(Artifact('b'),
                () => throw new InvalidDataException("Invalid lifecycle proof."), CancellationToken.None));
        }
        Assert.Same(previous, cache.GetOrAdd(Artifact('a'),
            () => throw new InvalidOperationException("The previous verified content is still cached."),
            CancellationToken.None));
        using var cancelled = new CancellationTokenSource();
        cancelled.Cancel();
        Assert.Throws<OperationCanceledException>(() => cache.GetOrAdd(Artifact('a'),
            () => throw new InvalidOperationException("A cancelled read cannot validate."), cancelled.Token));
    }

    private static CommittedArtifactReference Artifact(char digest) =>
        new("verified-path", 100, $"sha256:{new string(digest, 64)}");
}
