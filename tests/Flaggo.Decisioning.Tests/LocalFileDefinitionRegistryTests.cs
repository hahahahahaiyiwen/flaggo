using System.Text.Json;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileDefinitionRegistryTests
{
    [Fact]
    public async Task ApprovalCommit_IsVisibleToAnAlreadyCreatedReader()
    {
        using var file = new TestRegistryFile();
        var time = new FixedTimeProvider();
        var seed = PreviousDefinition();
        var writer = new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(file.Path),
            [seed],
            new SequenceDefinitionIdentityGenerator(),
            time);
        var reader = new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(file.Path),
            [seed],
            new SequenceDefinitionIdentityGenerator(),
            time);
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");

        var pending = Assert.IsType<RequiresApprovalResult>(
            (await writer.ApplyAsync("shared-file-apply", bundle)).Body);
        var before = await reader.ResolveRuntimeAsync(
            seed.AppId,
            seed.Environment,
            seed.DecisionKey,
            seed.Identity.DefinitionId,
            seed.Identity.Revision,
            CancellationToken.None);
        Assert.NotNull(before.Definition);

        var approved = await writer.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("test-operator"),
            "Approved in another adapter instance.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        var after = await reader.ResolveRuntimeAsync(
            seed.AppId,
            seed.Environment,
            seed.DecisionKey,
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None);

        Assert.NotNull(after.Definition);
        Assert.Equal(accepted.ContractDigest, after.Definition.Identity.ContractDigest);
        Assert.Equal("active", after.Definition.LifecycleStatus);
    }

    [Fact]
    public async Task ApprovedApply_ReplaysReceiptImmediately()
    {
        using var file = new TestRegistryFile();
        var seed = PreviousDefinition();
        var registry = new LocalFileDefinitionRegistry(
            new LocalFileDefinitionRegistryOptions(file.Path),
            [seed],
            new SequenceDefinitionIdentityGenerator(),
            new FixedTimeProvider());
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("immediate-replay-apply", bundle)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("test-operator"),
            "Approved.");

        var replayed = await registry.ApplyAsync("immediate-replay-apply", bundle);

        Assert.Equal(200, replayed.StatusCode);
        var receipt = Assert.IsType<RegistrationReceipt>(replayed.Body);
        Assert.Equal(approved.Receipt!.BundleDigest, receipt.BundleDigest);
        Assert.Equal(
            approved.Receipt.AcceptedDefinitions,
            receipt.AcceptedDefinitions);
    }

    [Fact]
    public async Task ApprovedApplyAndTerminalApprovalState_SurviveAdapterRestart()
    {
        using var file = new TestRegistryFile();
        var seed = PreviousDefinition();
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(
            options,
            [seed],
            new SequenceDefinitionIdentityGenerator(),
            new FixedTimeProvider());
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await first.ApplyAsync("restart-apply", bundle)).Body);
        var approved = await first.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("original-operator"),
            "Original decision.");

        var restarted = new LocalFileDefinitionRegistry(
            options,
            [seed],
            new SequenceDefinitionIdentityGenerator(),
            new FixedTimeProvider());
        var replayedApply = await restarted.ApplyAsync("restart-apply", bundle);
        var replayedApproval = await restarted.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("different-operator"),
            "Must not replace the original.");

        Assert.Equal(200, replayedApply.StatusCode);
        var replayedReceipt = Assert.IsType<RegistrationReceipt>(replayedApply.Body);
        Assert.Equal(approved.Receipt!.BundleDigest, replayedReceipt.BundleDigest);
        Assert.Equal(
            approved.Receipt.AcceptedDefinitions,
            replayedReceipt.AcceptedDefinitions);
        Assert.Equal(approved.Status, replayedApproval.Status);
        Assert.Equal(approved.DecidedAt, replayedApproval.DecidedAt);
        Assert.Equal(
            approved.Receipt!.AcceptedDefinitions,
            replayedApproval.Receipt!.AcceptedDefinitions);
        Assert.Equal("original-operator", replayedApproval.Approval!.Actor.Subject);

        var conflicting = FixtureBody(
            "definition-bundle",
            "05-apply-idempotency-conflict.json");
        var conflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => restarted.ApplyAsync("restart-apply", conflicting));
        Assert.Equal("bundle-idempotency-conflict", conflict.Code);
    }

    [Fact]
    public async Task ConcurrentWriters_PreserveBothAtomicApplyEntries()
    {
        using var file = new TestRegistryFile();
        var seed = LocalRegistryHosting.DefaultDefinitions();
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(options, seed);
        var second = new LocalFileDefinitionRegistry(options, seed);
        var bundle = FixtureBody("definition-bundle", "04-apply-approved-receipt.json");

        await Task.WhenAll(
            first.ApplyAsync("concurrent-apply-1", bundle),
            second.ApplyAsync("concurrent-apply-2", bundle));

        var restarted = new LocalFileDefinitionRegistry(options, seed);
        var conflicting = FixtureBody(
            "definition-bundle",
            "05-apply-idempotency-conflict.json");
        var firstConflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => restarted.ApplyAsync("concurrent-apply-1", conflicting));
        var secondConflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => restarted.ApplyAsync("concurrent-apply-2", conflicting));
        Assert.Equal("bundle-idempotency-conflict", firstConflict.Code);
        Assert.Equal("bundle-idempotency-conflict", secondConflict.Code);
    }

    private static RuntimeDecisionDefinition PreviousDefinition() =>
        new(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            new RuntimeContractIdentity(
                "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
                "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e",
                "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            "number",
            JsonSerializer.SerializeToElement(800),
            "safe_default_drop_interval",
            [],
            []);

    private static JsonElement FixtureBody(string group, string file)
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            group,
            file);
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.GetProperty("request").GetProperty("body").Clone();
    }

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);
    }
}

internal static class TestPaths
{
    public static readonly string RepositoryRoot = Path.GetFullPath(
        Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", ".."));
}

internal sealed class TestRegistryFile : IDisposable
{
    private readonly string _directory = System.IO.Path.Combine(
        TestPaths.RepositoryRoot,
        "TestResults",
        "local-registry",
        Guid.NewGuid().ToString("N"));

    public TestRegistryFile()
    {
        Directory.CreateDirectory(_directory);
        Path = System.IO.Path.Combine(_directory, "definition-registry-v1.json");
    }

    public string Path { get; }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, true);
        }
    }
}
