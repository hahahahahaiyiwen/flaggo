using System.Text.Json;
using System.Text;
using System.Text.Json.Nodes;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class DefinitionLifecycleTests
{
    private const string DefinitionId = "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A";
    private const string ActiveRevision = "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3";
    private const string ActiveDigest =
        "sha256:6eadd7bd76b36ae06e89376d57107da83fdcabf07ff58c528ae97fddb7f08ee9";
    private const string PreviousRevision = "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K";
    private const string PreviousDigest =
        "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e";

    [Fact]
    public void CanonicalDigests_MatchFrozenFixture()
    {
        var bundle = FixtureBody("definition-bundle", "04-apply-approved-receipt.json");

        Assert.Equal(
            "sha256:b906aceba616dda027d6001cb8cd94a72cd96bc10a37b983a9a7c0fefd8e9a6f",
            CanonicalJson.BundleDigest(bundle));
        Assert.Equal(
            ActiveDigest,
            CanonicalJson.ContractDigest(bundle.GetProperty("definitions")[0]));
    }

    [Fact]
    public void Canonicalization_MatchesEveryFrozenVector()
    {
        using var vectors = JsonDocument.Parse(
            File.ReadAllBytes(ContractPath(
                "conformance",
                "canonicalization-vectors-v1.json")));
        foreach (var vector in vectors.RootElement.GetProperty("vectors").EnumerateArray())
        {
            Assert.Equal(
                vector.GetProperty("expected").GetString(),
                Encoding.UTF8.GetString(
                    CanonicalJson.Canonicalize(vector.GetProperty("input"))));
        }
    }

    [Fact]
    public void Canonicalization_RejectsNumberThatWouldLosePrecision()
    {
        using var document = JsonDocument.Parse("9007199254740993");

        var error = Assert.Throws<JsonException>(
            () => CanonicalJson.Canonicalize(document.RootElement));

        Assert.Contains("IEEE 754", error.Message);
    }

    [Fact]
    public void SemanticDigests_MatchEveryFrozenVector()
    {
        using var vectors = JsonDocument.Parse(
            File.ReadAllBytes(ContractPath(
                "conformance",
                "semantic-digest-vectors-v1.json")));
        foreach (var testCase in vectors.RootElement
                     .GetProperty("definitionCases")
                     .EnumerateArray())
        {
            var expected = testCase.GetProperty("expectedContractDigest").GetString();
            foreach (var variant in testCase.GetProperty("variants").EnumerateArray())
            {
                Assert.Equal(
                    expected,
                    CanonicalJson.ContractDigest(variant.GetProperty("definition")));
            }
        }

        foreach (var testCase in vectors.RootElement
                     .GetProperty("bundleCases")
                     .EnumerateArray())
        {
            var expected = testCase.GetProperty("expectedBundleDigest").GetString();
            foreach (var variant in testCase.GetProperty("variants").EnumerateArray())
            {
                Assert.Equal(
                    expected,
                    CanonicalJson.BundleDigest(variant.GetProperty("bundle")));
            }
        }
    }

    [Fact]
    public async Task ValidateAsync_ReturnsFrozenIdenticalResult()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var bundle = FixtureBody("definition-bundle", "01-validate-identical.json");

        var result = await registry.ValidateAsync(bundle);

        Assert.Equal("valid", result.Status);
        Assert.Equal("identical", result.Compatibility);
        Assert.Empty(result.Issues);
        var accepted = Assert.Single(result.ValidatedDefinitions!);
        Assert.Equal(ActiveRevision, accepted.Value.Revision);
        Assert.Equal(ActiveDigest, accepted.Value.ContractDigest);
    }

    [Fact]
    public async Task ValidateAsync_ReportsUnknownSignalWithoutMutation()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var bundle = FixtureBody("definition-bundle", "02-validate-invalid.json");

        var result = await registry.ValidateAsync(bundle);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(
            result.Issues,
            issue => issue.Code == "unknown-signal" &&
                     issue.Path == "/definitions/0/inference/inputs/0");
        var lookup = await registry.ResolveAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            DefinitionId,
            ActiveRevision,
            CancellationToken.None);
        Assert.NotNull(lookup.Definition);
    }

    [Fact]
    public async Task ValidateAsync_ReturnsEveryStableSemanticIssueCategory()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var bundle = FixtureBody(
            "definition-bundle",
            "13-validate-stable-issue-codes.json");

        var result = await registry.ValidateAsync(bundle);

        Assert.Equal("invalid", result.Status);
        Assert.Equal(
            [
                "invalid-definition",
                "invalid-objective",
                "invalid-policy",
                "invalid-signal-schema",
                "invalid-strategy"
            ],
            result.Issues.Select(issue => issue.Code).Order());
    }

    [Fact]
    public async Task ApplyAsync_ReplaysSameBodyAndRejectsConflictingBody()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var bundle = FixtureBody("definition-bundle", "04-apply-approved-receipt.json");

        var first = await registry.ApplyAsync("apply-key-1", bundle);
        var replay = await registry.ApplyAsync("apply-key-1", bundle);

        Assert.Equal(200, first.StatusCode);
        Assert.Same(first.Body, replay.Body);

        var conflictBundle = FixtureBody(
            "definition-bundle",
            "05-apply-idempotency-conflict.json");
        var error = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApplyAsync("apply-key-1", conflictBundle));
        Assert.Equal(409, error.Status);
        Assert.Equal("bundle-idempotency-conflict", error.Code);
    }

    [Fact]
    public async Task SemanticApply_ReplaysApprovedReceiptAndRejectsConflictingBody()
    {
        var registry = CreateRegistry(PreviousRevision, PreviousDigest);
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");

        var apply = await registry.ApplyAsync("semantic-key", bundle);

        Assert.Equal(202, apply.StatusCode);
        var pending = Assert.IsType<RequiresApprovalResult>(apply.Body);
        var before = await registry.ResolveAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            DefinitionId,
            PreviousRevision,
            CancellationToken.None);
        Assert.NotNull(before.Definition);

        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("user:operator-1", "Release Operator"),
            "Reviewed.",
            CancellationToken.None);
        var replay = await registry.ApplyAsync("semantic-key", bundle);

        Assert.Equal("approved", approved.Status);
        Assert.Equal(200, replay.StatusCode);
        Assert.Same(approved.Receipt, replay.Body);
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        var after = await registry.ResolveAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None);
        Assert.Equal(ActiveDigest, after.Definition!.Identity.ContractDigest);

        var conflictBundle = FixtureBody(
            "definition-bundle",
            "05-apply-idempotency-conflict.json");
        var conflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApplyAsync("semantic-key", conflictBundle));
        Assert.Equal("bundle-idempotency-conflict", conflict.Code);
    }

    [Fact]
    public async Task RejectAsync_IsTerminalAndDoesNotMutateRegistry()
    {
        var registry = CreateRegistry(PreviousRevision, PreviousDigest);
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("semantic-key", bundle)).Body);

        var rejected = await registry.RejectAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("user:operator-1"),
            "operator-rejected",
            "Not ready.",
            CancellationToken.None);

        Assert.Equal("rejected", rejected.Status);
        var conflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApproveAsync(
                pending.ApprovalRequestId,
                pending.BundleDigest,
                new ApprovalActor("user:operator-1"),
                null,
                CancellationToken.None));
        Assert.Equal("approval-terminal-conflict", conflict.Code);
        var lookup = await registry.ResolveAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            DefinitionId,
            PreviousRevision,
            CancellationToken.None);
        Assert.NotNull(lookup.Definition);
    }

    [Fact]
    public async Task Approval_ValidatesDigestAndReplaysTerminalOutcome()
    {
        var registry = CreateRegistry(PreviousRevision, PreviousDigest);
        var bundle = FixtureBody("definition-bundle", "06-apply-requires-approval.json");
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("semantic-key", bundle)).Body);

        var digestConflict = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApproveAsync(
                pending.ApprovalRequestId,
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                new ApprovalActor("user:operator-1"),
                null));
        Assert.Equal("approval-bundle-conflict", digestConflict.Code);

        var first = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("user:operator-1"),
            "Reviewed.");
        var replay = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("user:different-operator"),
            "A replay cannot replace the original decision.");

        Assert.Same(first, replay);
        Assert.Equal("user:operator-1", replay.Approval!.Actor.Subject);
        Assert.Equal("Reviewed.", replay.Approval.Comment);
    }

    [Fact]
    public async Task Approval_RejectsSnapshotWhenActiveBaselineChanged()
    {
        var registry = CreateRegistry(PreviousRevision, PreviousDigest);
        var original = FixtureBody(
            "definition-bundle",
            "06-apply-requires-approval.json");
        var competingNode = JsonNode.Parse(original.GetRawText())!.AsObject();
        competingNode["definitions"]![0]!["fallback"]!["value"] = 850;
        competingNode["definitions"]![0]!["actionSpace"]!["default"] = 850;
        using var competingDocument = JsonDocument.Parse(competingNode.ToJsonString());
        var competing = competingDocument.RootElement.Clone();
        var originalPending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("original-key", original)).Body);
        var competingPending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("competing-key", competing)).Body);

        var competingApproved = await registry.ApproveAsync(
            competingPending.ApprovalRequestId,
            competingPending.BundleDigest,
            new ApprovalActor("user:operator-1"),
            null);
        var stale = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApproveAsync(
                originalPending.ApprovalRequestId,
                originalPending.BundleDigest,
                new ApprovalActor("user:operator-2"),
                null));

        Assert.Equal(409, stale.Status);
        Assert.Equal("approval-stale-conflict", stale.Code);
        var accepted = Assert.Single(
            competingApproved.Receipt!.AcceptedDefinitions).Value;
        var active = await registry.ResolveAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None);
        Assert.Equal(
            accepted.ContractDigest,
            active.Definition!.Identity.ContractDigest);
    }

    [Fact]
    public async Task Approval_RejectsSnapshotAfterMetadataChangesBundleIdentity()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var pendingNode = JsonNode.Parse(
            FixtureBody(
                "definition-bundle",
                "06-apply-requires-approval.json").GetRawText())!.AsObject();
        pendingNode["definitions"]![0]!["fallback"]!["value"] = 850;
        pendingNode["definitions"]![0]!["actionSpace"]!["default"] = 850;
        using var pendingDocument = JsonDocument.Parse(pendingNode.ToJsonString());
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync(
                "semantic-key",
                pendingDocument.RootElement)).Body);
        var metadataNode = JsonNode.Parse(
            FixtureBody(
                "definition-bundle",
                "08-apply-metadata-only.json").GetRawText())!.AsObject();
        metadataNode["build"]!["version"] = "0.1.1";
        using var metadataDocument = JsonDocument.Parse(metadataNode.ToJsonString());

        var metadataApply = await registry.ApplyAsync(
            "metadata-key",
            metadataDocument.RootElement);
        var stale = await Assert.ThrowsAsync<DefinitionLifecycleException>(
            () => registry.ApproveAsync(
                pending.ApprovalRequestId,
                pending.BundleDigest,
                new ApprovalActor("user:operator-1"),
                null));

        Assert.Equal(200, metadataApply.StatusCode);
        Assert.Equal("approval-stale-conflict", stale.Code);
    }

    [Fact]
    public async Task ValidateAsync_RejectsUnresolvedPolicyReference()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var node = JsonNode.Parse(
            FixtureBody(
                "definition-bundle",
                "01-validate-identical.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["policy"] = new JsonObject
        {
            ["kind"] = "reference",
            ["policyId"] = "policy-unresolved"
        };
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-policy");
    }

    [Fact]
    public async Task ValidateAsync_RejectsDefinitionWithoutPolicy()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var node = JsonNode.Parse(
            FixtureBody(
                "definition-bundle",
                "01-validate-identical.json").GetRawText())!.AsObject();
        node["definitions"]![0]!.AsObject().Remove("policy");
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-policy");
    }

    [Fact]
    public async Task ValidateAsync_RejectsInvalidClientFallbackPolicy()
    {
        var registry = CreateRegistry(ActiveRevision, ActiveDigest);
        var node = JsonNode.Parse(
            FixtureBody(
                "definition-bundle",
                "01-validate-identical.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["policy"]!["clientFallback"] = new JsonObject
        {
            ["requiredEvidenceUnavailable"] = "sometimes"
        };
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-policy");
    }

    private static InMemoryDefinitionRegistry CreateRegistry(
        string revision,
        string digest) =>
        new(
        [
            new RegisteredDecisionDefinition(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                new RuntimeContractIdentity(
                    DefinitionId,
                    digest,
                    revision,
                    digest == ActiveDigest
                        ? "sha256:b906aceba616dda027d6001cb8cd94a72cd96bc10a37b983a9a7c0fefd8e9a6f"
                        : "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                "number",
                JsonSerializer.SerializeToElement(800),
                "safe_default_drop_interval",
                [],
                [])
        ],
        new SequenceDefinitionIdentityGenerator());

    private static JsonElement FixtureBody(string group, string file)
    {
        var path = ContractPath("fixtures", "management", group, file);
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.GetProperty("request").GetProperty("body").Clone();
    }

    private static string ContractPath(params string[] segments) =>
        Path.GetFullPath(
            Path.Combine(
                [
                    AppContext.BaseDirectory,
                    "..",
                    "..",
                    "..",
                    "..",
                    "..",
                    "contracts",
                    .. segments
                ]));
}
