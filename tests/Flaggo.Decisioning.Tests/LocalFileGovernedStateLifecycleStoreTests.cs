using System.Text.Json.Nodes;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileGovernedStateLifecycleStoreTests
{
    [Fact]
    public async Task RestartPreservesReviewApprovalActivationAndRuntimeProjection()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-restart");
        var firstStore = Store(file.Path);
        var review = await LifecycleTestData.ReviewAsync(firstStore);
        var activated = await LifecycleTestData.ActivateAsync(firstStore);
        var restarted = Store(file.Path);

        var reviewReplay = await LifecycleTestData.ReviewAsync(restarted);
        var activationReplay = await LifecycleTestData.ActivateAsync(restarted);
        var runtime = await RuntimeState(file.Path);
        var audit = await restarted.ReadAsync("app", "dev", CancellationToken.None);

        Assert.Equal(LifecycleJson.Digest(review), LifecycleJson.Digest(reviewReplay));
        Assert.Equal(LifecycleJson.Digest(activated), LifecycleJson.Digest(activationReplay));
        Assert.Equal(activated.StateId, runtime!.StateId);
        Assert.Equal(review.Approval!.ApprovalId, runtime.ApprovalReference);
        Assert.Equal(4, audit.Records.Count);
    }

    [Fact]
    public async Task PublicationFailureLeavesBothPreviousAuthorityAndAuditVisible()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-publication-failure");
        var initial = Store(file.Path);
        await LifecycleTestData.ReviewAsync(initial);
        var first = await LifecycleTestData.ActivateAsync(initial);
        await LifecycleTestData.ReviewAsync(
            initial, LifecycleTestData.Proposal("second", 850, new(first.StateId, first.Generation!.Value)),
            "second");
        var before = await initial.ReadAsync("app", "dev", CancellationToken.None);
        var failing = Store(file.Path, new FailingPublisher());

        await Assert.ThrowsAsync<IOException>(() =>
            LifecycleTestData.ActivateAsync(failing, "second", "second"));

        Assert.Equal(first.StateId, (await RuntimeState(file.Path))!.StateId);
        var after = await Store(file.Path).ReadAsync("app", "dev", CancellationToken.None);
        Assert.Equal(LifecycleJson.Digest(before), LifecycleJson.Digest(after));
    }

    [Fact]
    public async Task LostPublicationResponseReplaysTheCommittedOutcomeWithoutAnotherWrite()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-lost-response");
        var initial = Store(file.Path);
        await LifecycleTestData.ReviewAsync(initial);
        await Assert.ThrowsAsync<IOException>(() =>
            LifecycleTestData.ActivateAsync(Store(file.Path, new LostResponsePublisher())));

        var committed = await RuntimeState(file.Path);
        var replay = await LifecycleTestData.ActivateAsync(Store(file.Path, new FailingPublisher()));
        Assert.Equal(LifecycleMutationStatus.Applied, replay.Status);
        Assert.Equal(committed!.StateId, replay.StateId);
        Assert.Equal(4, (await initial.ReadAsync("app", "dev", CancellationToken.None)).Records.Count);
    }

    [Fact]
    public async Task ConcurrentInstancesPublishOneAuditedReplacement()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-concurrent");
        var initial = Store(file.Path);
        await LifecycleTestData.ReviewAsync(initial);
        var first = await LifecycleTestData.ActivateAsync(initial);
        var baseline = new GovernedStateBaseline(first.StateId, first.Generation!.Value);
        await LifecycleTestData.ReviewAsync(initial, LifecycleTestData.Proposal("second", 850, baseline), "second");
        await LifecycleTestData.ReviewAsync(initial, LifecycleTestData.Proposal("third", 750, baseline), "third");

        var outcomes = await Task.WhenAll(
            LifecycleTestData.ActivateAsync(Store(file.Path), "second", "second"),
            LifecycleTestData.ActivateAsync(Store(file.Path), "third", "third"));

        var winner = Assert.Single(outcomes.Where(item => item.Status == LifecycleMutationStatus.Applied));
        var loser = Assert.Single(outcomes.Where(item => item.Status == LifecycleMutationStatus.Rejected));
        Assert.Contains("stale-baseline", loser.Reasons);
        Assert.Equal(winner.StateId, (await RuntimeState(file.Path))!.StateId);
        var journal = await Store(file.Path).ReadAsync("app", "dev", CancellationToken.None);
        Assert.Equal(2, journal.Activations.Count(item => item.Status == LifecycleMutationStatus.Applied));
    }

    [Theory]
    [InlineData("audit")]
    [InlineData("approval")]
    [InlineData("value")]
    [InlineData("duplicate-authority")]
    [InlineData("state-history")]
    [InlineData("audit-time")]
    [InlineData("audit-target")]
    [InlineData("audit-proposal")]
    [InlineData("audit-policy")]
    [InlineData("null-proposal")]
    [InlineData("null-context")]
    [InlineData("null-actor")]
    [InlineData("null-reasons")]
    public async Task RuntimeRejectsStateWithIncompleteOrInconsistentLifecycleProof(string corruption)
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-corrupt");
        var store = Store(file.Path);
        await LifecycleTestData.ReviewAsync(store);
        await LifecycleTestData.ActivateAsync(store);
        var document = JsonNode.Parse(await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path), null, CancellationToken.None))!.AsObject();
        var states = document["states"]!.AsArray();
        var state = states[0]!["state"]!;
        var journal = document["journal"]!;
        switch (corruption)
        {
            case "audit":
                journal["records"]!.AsArray().RemoveAt(3);
                break;
            case "approval":
                journal["reviews"]![0]!["receipt"]!["approval"] = null;
                break;
            case "value":
                state["value"] = 1000;
                break;
            case "duplicate-authority":
                var duplicate = states[0]!.DeepClone();
                duplicate["state"]!["stateId"] = "duplicate";
                duplicate["state"]!["generation"] = 2;
                states.Add(duplicate);
                break;
            case "state-history":
                state["lifecycleStatus"] = "Completed";
                break;
            case "audit-time":
                journal["records"]![3]!["recordedAt"] = LifecycleTestData.Now.AddSeconds(1).ToString("O");
                break;
            case "audit-target":
                journal["records"]![3]!["controlTarget"]!["id"] = "other-target";
                break;
            case "audit-proposal":
                journal["records"]![3]!["proposalId"] = "other-proposal";
                break;
            case "audit-policy":
                journal["records"]![3]!["policyRevision"] = "other-policy";
                break;
            case "null-proposal":
                journal["reviews"]![0]!["commit"]!["request"]!["proposal"] = null;
                break;
            case "null-context":
                journal["reviews"]![0]!["commit"]!["request"]!["proposal"]!["context"] = null;
                break;
            case "null-actor":
                journal["activations"]![0]!["actor"] = null;
                break;
            case "null-reasons":
                journal["activations"]![0]!["reasons"] = null;
                break;
        }
        await file.WriteAsync(document.ToJsonString());

        await Assert.ThrowsAsync<InvalidDataException>(() => RuntimeState(file.Path));
        await Assert.ThrowsAsync<InvalidDataException>(() =>
            Store(file.Path).GetBaselineAsync(
                new("app", "dev", "decision", LifecycleTestData.Target), CancellationToken.None));
        Assert.False(await new LocalFileStateStore(
            new LocalFileStateStoreOptions(file.Path)).IsAvailableAsync(CancellationToken.None));
    }

    [Theory]
    [InlineData(1)]
    [InlineData(2)]
    public async Task OldStateFormatsAreNotMigratedIntoLifecycleAuthority(int version)
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-old-format");
        await file.WriteAsync($$"""{"version":{{version}},"states":[],"activations":[],"transitions":[]}""");
        await Assert.ThrowsAsync<InvalidDataException>(() =>
            Store(file.Path).GetReviewAsync("review", CancellationToken.None));
    }

    [Fact]
    public async Task OneJournalCannotMixApplicationScopes()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-scope");
        var store = Store(file.Path);
        await LifecycleTestData.ReviewAsync(store);
        var proposal = LifecycleTestData.Proposal("other");
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                Definition = LifecycleTestData.Definition with { AppId = "other-app" }
            }
        };
        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(() =>
            LifecycleTestData.ReviewAsync(
                store, proposal, "other", actor: LifecycleTestData.Actor with { AppId = "other-app" }));
        Assert.Equal("resource-scope-mismatch", error.Code);
        Assert.Single((await store.ReadAsync("app", "dev", CancellationToken.None)).Reviews);
    }

    [Fact]
    public async Task CancellationBeforePublicationLeavesTheOriginalSnapshot()
    {
        using var file = TestJsonFile.CreateCommitted("lifecycle-cancel");
        var initial = Store(file.Path);
        await LifecycleTestData.ReviewAsync(initial);
        var review = (await initial.GetReviewAsync("review-1", CancellationToken.None))!;
        using var cancellation = new CancellationTokenSource();
        var store = Store(file.Path, new CancelBeforePublication(cancellation));
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            store.CommitActivationAsync(
                new LifecycleActivationCommit(
                    new LifecycleActivationRequest("activation-1", "review-1"),
                    LifecycleTestData.Actor, LifecycleIdentity.InputsDigest(review.Commit),
                    review.Commit.Decision),
                cancellation.Token));

        Assert.Null(await RuntimeState(file.Path));
        Assert.Empty((await initial.ReadAsync("app", "dev", CancellationToken.None)).Activations);
    }

    private static Task<GovernedDecisionState?> RuntimeState(string path) =>
        new LocalFileStateStore(new LocalFileStateStoreOptions(path)).GetActiveAsync(
            "decision", "definition", "revision", [LifecycleTestData.Target], CancellationToken.None);

    private static LocalFileGovernedStateLifecycleStore Store(
        string path,
        IGovernedStateDocumentPublisher? publisher = null) =>
        publisher is null
            ? new(new(path), new TestClock())
            : new(new(path), new TestClock(), new GuidGovernedStateIdentityGenerator(), publisher);

    private sealed class TestClock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }

    private sealed class FailingPublisher : IGovernedStateDocumentPublisher
    {
        public Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken) =>
            throw new IOException("Injected failure before state and audit publication.");
    }

    private sealed class LostResponsePublisher : IGovernedStateDocumentPublisher
    {
        public async Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken)
        {
            await CommittedFileSnapshotWriter.PublishAsync(path, bytes, cancellationToken);
            throw new IOException("Injected lost response after the complete snapshot committed.");
        }
    }

    private sealed class CancelBeforePublication(CancellationTokenSource source) : IGovernedStateDocumentPublisher
    {
        public async Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken)
        {
            source.Cancel();
            await CommittedFileSnapshotWriter.PublishAsync(path, bytes, cancellationToken);
        }
    }
}
