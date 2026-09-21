using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Evidence;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class ProposalEvidenceReaderTests
{
    [Theory]
    [InlineData("provenance")]
    [InlineData("app")]
    [InlineData("environment")]
    [InlineData("decision")]
    [InlineData("definition")]
    [InlineData("revision")]
    [InlineData("digest")]
    public async Task CatalogScopeUsesSemanticIdentityAndPreservesProvenance(string field)
    {
        var original = LifecycleTestData.Definition;
        var snapshot = Snapshot() with
        {
            Definition = original with
            {
                Contract = original.Contract with { BundleDigest = $"sha256:{new string('c', 64)}" }
            }
        };
        var requested = field switch
        {
            "provenance" => original with { Contract = original.Contract with { BuildId = "build" } },
            "app" => original with { AppId = "other" },
            "environment" => original with { Environment = "other" },
            "decision" => original with { DecisionKey = "other" },
            "definition" => original with { Contract = original.Contract with { DefinitionId = "other" } },
            "revision" => original with { Contract = original.Contract with { Revision = "other" } },
            "digest" => original with
            {
                Contract = original.Contract with { ContractDigest = $"sha256:{new string('b', 64)}" }
            },
            _ => throw new InvalidOperationException("Unknown identity field.")
        };
        var request = Request(["evidence"]) with { Definition = requested };
        var memory = new InMemoryProposalEvidenceReader([snapshot]);
        using var file = TestJsonFile.CreateCommitted("proposal-evidence-identity");
        await file.WriteAsync(LifecycleJson.Bytes(new ProposalEvidenceDocument(1, [snapshot])));
        var persisted = new LocalFileProposalEvidenceReader(file.Path);

        if (field == "provenance")
        {
            Assert.Equal(snapshot, Assert.Single(await memory.GetAsync(request, CancellationToken.None)));
            Assert.Equal(snapshot, Assert.Single(await persisted.GetAsync(request, CancellationToken.None)));
        }
        else
        {
            await Assert.ThrowsAsync<InvalidDataException>(() => memory.GetAsync(request, CancellationToken.None));
            await Assert.ThrowsAsync<EvidenceUnavailableException>(() => persisted.GetAsync(request, CancellationToken.None));
        }
    }

    [Fact]
    public void EmbeddedEvidenceCannotHideDuplicateJsonMembersOrUnsafeNumbers()
    {
        using var duplicate = JsonDocument.Parse("""{"quality":0.9,"quality":0.1}""");
        var snapshot = Snapshot();
        snapshot = snapshot with
        {
            Evidence = snapshot.Evidence with
            {
                Details = new Dictionary<string, JsonElement> { ["model"] = duplicate.RootElement.Clone() }
            }
        };
        Assert.Throws<JsonException>(() => new InMemoryProposalEvidenceReader([snapshot]));
        using var unsafeNumber = JsonDocument.Parse("9007199254740993");
        snapshot = snapshot with
        {
            Evidence = snapshot.Evidence with
            {
                Details = new Dictionary<string, JsonElement> { ["model"] = unsafeNumber.RootElement.Clone() }
            }
        };
        Assert.Throws<JsonException>(() => new InMemoryProposalEvidenceReader([snapshot]));
    }

    [Fact]
    public async Task CommittedCatalogResolvesOnlyScopedReferencesAndReloads()
    {
        using var file = TestJsonFile.CreateCommitted("proposal-evidence");
        var snapshot = Snapshot();
        await file.WriteAsync(LifecycleJson.Bytes(new ProposalEvidenceDocument(1, [snapshot])));
        var reader = new LocalFileProposalEvidenceReader(file.Path);
        var request = Request(["evidence", "missing"]);
        Assert.Equal(snapshot, Assert.Single(await reader.GetAsync(request, CancellationToken.None)));

        var changed = snapshot with { Evidence = new DecisionEvidenceSnapshot(0.8) };
        await file.WriteAsync(LifecycleJson.Bytes(new ProposalEvidenceDocument(1, [changed])));
        Assert.Equal(0.8,
            Assert.Single(await reader.GetAsync(request, CancellationToken.None)).Evidence.EvidenceQuality);
        await Assert.ThrowsAsync<EvidenceUnavailableException>(() =>
            reader.GetAsync(request with { ControlTarget = new("cohort", "other") }, CancellationToken.None));
    }

    [Theory]
    [InlineData("null")]
    [InlineData("missing-quality")]
    [InlineData("quality-out-of-range")]
    [InlineData("negative-samples")]
    [InlineData("invalid-observation")]
    [InlineData("numeric-timestamp")]
    [InlineData("duplicate-reference")]
    [InlineData("unknown-field")]
    [InlineData("unsupported-version")]
    public async Task MalformedCatalogIsAnExplicitDependencyFailure(string corruption)
    {
        using var file = TestJsonFile.CreateCommitted("proposal-evidence-invalid");
        var document = JsonNode.Parse(LifecycleJson.Bytes(new ProposalEvidenceDocument(1, [Snapshot()])))!;
        var snapshot = document["snapshots"]![0]!;
        switch (corruption)
        {
            case "null":
                document["snapshots"]!.AsArray()[0] = null;
                break;
            case "missing-quality":
                snapshot["evidence"]!.AsObject().Remove("evidenceQuality");
                break;
            case "quality-out-of-range":
                snapshot["evidence"]!["evidenceQuality"] = 1.01;
                break;
            case "negative-samples":
                snapshot["evidence"]!["sampleSize"] = -1;
                break;
            case "invalid-observation":
                snapshot["observedAt"] = "2026-09-20T20:00:00";
                break;
            case "numeric-timestamp":
                snapshot["observedAt"] = 42;
                break;
            case "duplicate-reference":
                document["snapshots"]!.AsArray().Add(snapshot.DeepClone());
                break;
            case "unknown-field":
                snapshot["trustedByProducer"] = true;
                break;
            case "unsupported-version":
                document["version"] = 2;
                break;
        }
        await file.WriteAsync(document.ToJsonString());
        await Assert.ThrowsAsync<EvidenceUnavailableException>(() =>
            new LocalFileProposalEvidenceReader(file.Path).GetAsync(Request(["evidence"]), CancellationToken.None));
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("""{"version":1,"version":1,"snapshots":[]}""")]
    public async Task InvalidCatalogEnvelopesDoNotBecomeEmptySuccess(string json)
    {
        using var file = TestJsonFile.CreateCommitted("proposal-evidence-envelope");
        await file.WriteAsync(json);
        await Assert.ThrowsAsync<EvidenceUnavailableException>(() =>
            new LocalFileProposalEvidenceReader(file.Path).GetAsync(Request([]), CancellationToken.None));
    }

    [Fact]
    public async Task MemorySnapshotsAreIsolatedAndMissingReferencesStayMissing()
    {
        ProposalEvidenceSnapshot[] source = [Snapshot()];
        var reader = new InMemoryProposalEvidenceReader(source);
        source[0] = source[0] with { Reference = "replaced" };
        var resolved = await reader.GetAsync(Request(["evidence", "missing"]), CancellationToken.None);
        Assert.Equal("evidence", Assert.Single(resolved).Reference);
        Assert.Empty(await reader.GetAsync(Request(["missing"]), CancellationToken.None));
        Assert.Throws<InvalidDataException>(() =>
            new InMemoryProposalEvidenceReader([Snapshot(), Snapshot()]));
        await Assert.ThrowsAsync<InvalidDataException>(() =>
            reader.GetAsync(Request(["evidence"]) with
            {
                Definition = LifecycleTestData.Definition with { Environment = "prod" }
            }, CancellationToken.None));
    }

    private static ProposalEvidenceRequest Request(IReadOnlyList<string> references) =>
        new(LifecycleTestData.Definition, LifecycleTestData.Target, references);

    private static ProposalEvidenceSnapshot Snapshot() =>
        new("evidence", LifecycleTestData.Definition, LifecycleTestData.Target,
            LifecycleTestData.Now, LifecycleTestData.Now.AddHours(1), new DecisionEvidenceSnapshot(0.9));
}
