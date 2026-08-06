using System.Text.Json;
using Flaggo.Evidence;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileEvidenceProviderTests
{
    [Fact]
    public async Task GetEvidenceAsync_LoadsStrategySnapshot()
    {
        using var file = new TestJsonFile("evidence");
        await file.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy-tetris-balanced-v1": {
                  "evidenceQuality": 0.82,
                  "modelUncertainty": 0.2,
                  "expectedOutcome": 0.74,
                  "sampleSize": 50,
                  "details": { "source": "phase3-local-fixture" }
                }
              }
            }
            """);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var evidence = await provider.GetEvidenceAsync(
            Request("strategy-tetris-balanced-v1"),
            CancellationToken.None);

        Assert.Equal(0.82, evidence!.EvidenceQuality);
        Assert.Equal(50, evidence.SampleSize);
        Assert.True(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task GetEvidenceAsync_PreservesLegitimateZeroScalars()
    {
        using var file = new TestJsonFile("evidence-zero");
        await file.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy": {
                  "evidenceQuality": 0,
                  "modelUncertainty": 0,
                  "expectedOutcome": 0,
                  "sampleSize": 0
                }
              }
            }
            """);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var evidence = await provider.GetEvidenceAsync(
            Request("strategy"),
            CancellationToken.None);

        Assert.Equal(0, evidence!.EvidenceQuality);
        Assert.Equal(0, evidence.ModelUncertainty);
        Assert.Equal(0, evidence.ExpectedOutcome);
        Assert.Equal(0, evidence.SampleSize);
    }

    [Theory]
    [InlineData(
        """
        {
          "evidenceByStrategy": {
            "strategy": { "evidenceQuality": 0.8 }
          }
        }
        """,
        "The local evidence file must use version 1.")]
    [InlineData(
        """
        {
          "version": 1,
          "evidenceByStrategy": {
            "strategy": { "sampleSize": 10 }
          }
        }
        """,
        "The local evidence file contains an invalid strategy snapshot.")]
    public async Task OmittedRequiredEvidenceScalar_FailsLookupAndHealth(
        string json,
        string expectedReadError)
    {
        using var file = new TestJsonFile("evidence-required");
        await file.WriteAsync(json);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var error = await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));

        var readError = Assert.IsType<InvalidDataException>(error.InnerException);
        Assert.Equal(expectedReadError, readError.Message);
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task MalformedEvidenceFile_FailsLookupAndHealth()
    {
        using var file = new TestJsonFile("evidence-malformed");
        await file.WriteAsync(
            """{"version":1,"evidenceByStrategy":""");
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string, string> DuplicatePropertyDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
            "root",
            """
            {
              "version": 1,
              "version": 1,
              "evidenceByStrategy": {}
            }
            """);
        cases.Add(
            "evidence map",
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy": { "evidenceQuality": 0.8 },
                "strategy": { "evidenceQuality": 0.9 }
              }
            }
            """);
        cases.Add(
            "evidence entry",
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy": {
                  "evidenceQuality": 0.8,
                  "evidenceQuality": 0.9
                }
              }
            }
            """);
        cases.Add(
            "escaped equivalent evidence entry",
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy": {
                  "evidenceQuality": 0.8,
                  "\u0065videnceQuality": 0.9
                }
              }
            }
            """);
        return cases;
    }

    [Theory]
    [MemberData(nameof(DuplicatePropertyDocuments))]
    public async Task DuplicateJsonProperty_FailsBeforeEvidenceDeserialization(
        string location,
        string document)
    {
        _ = location;
        using var file = new TestJsonFile("evidence-duplicate-property");
        await file.WriteAsync(document);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var error = await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));

        var jsonError = Assert.IsType<System.Text.Json.JsonException>(
            error.InnerException);
        Assert.Contains("Duplicate JSON property", jsonError.Message);
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task TrailingJsonData_FailsBeforeEvidenceDeserialization()
    {
        using var file = new TestJsonFile("evidence-trailing-data");
        await file.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {}
            }
            {}
            """);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var error = await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));

        Assert.IsAssignableFrom<System.Text.Json.JsonException>(
            error.InnerException);
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task SamePropertyNamesInSiblingEvidenceEntries_AreAccepted()
    {
        using var file = new TestJsonFile("evidence-sibling-properties");
        await file.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy-a": {
                  "evidenceQuality": 0.8,
                  "details": { "source": "fixture-a" }
                },
                "strategy-b": {
                  "evidenceQuality": 0.9,
                  "details": { "source": "fixture-b" }
                }
              }
            }
            """);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var evidence = await provider.GetEvidenceAsync(
            Request("strategy-b"),
            CancellationToken.None);

        Assert.Equal(0.9, evidence!.EvidenceQuality);
        Assert.True(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task PropertyNamesThatDifferOnlyByCase_AreDistinct()
    {
        using var file = new TestJsonFile("evidence-property-case");
        await file.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy": {
                  "evidenceQuality": 0.8,
                  "details": {
                    "source": "lower",
                    "Source": "upper"
                  }
                }
              }
            }
            """);
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var evidence = await provider.GetEvidenceAsync(
            Request("strategy"),
            CancellationToken.None);

        Assert.Equal(2, evidence!.Details!.Count);
        Assert.Equal("lower", evidence.Details["source"].GetString());
        Assert.Equal("upper", evidence.Details["Source"].GetString());
    }

    [Fact]
    public async Task MissingEvidenceFile_FailsThroughEvidenceBoundary()
    {
        using var file = new TestJsonFile("evidence-missing");
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var error = await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));

        Assert.IsType<FileNotFoundException>(error.InnerException);
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task UnreadableEvidencePath_FailsThroughEvidenceBoundary()
    {
        using var file = new TestJsonFile("evidence-unreadable");
        Directory.CreateDirectory(file.Path);
        try
        {
            var provider = new LocalFileEvidenceProvider(
                new LocalFileEvidenceProviderOptions(file.Path));

            await Assert.ThrowsAsync<EvidenceUnavailableException>(
                () => provider.GetEvidenceAsync(
                    Request("strategy"),
                    CancellationToken.None));
            Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
        }
        finally
        {
            Directory.Delete(file.Path);
        }
    }

    [Fact]
    public async Task SnapshotLease_BlocksInPlaceRewriteAndAllowsAtomicReplacement()
    {
        using var file = new TestJsonFile("evidence-snapshot");
        using var replacement = new TestJsonFile("evidence-snapshot-replacement");
        await file.WriteAsync(EvidenceDocument(0.8));
        await replacement.WriteAsync(EvidenceDocument(0.9));
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        await using var snapshot =
            LocalFileEvidenceProvider.OpenSnapshotRead(file.Path);
        Assert.Throws<IOException>(
            () =>
            {
                using var writer = new FileStream(
                    file.Path,
                    FileMode.Truncate,
                    FileAccess.Write,
                    FileShare.ReadWrite | FileShare.Delete);
            });

        File.Replace(replacement.Path, file.Path, destinationBackupFileName: null);
        using var memory = new MemoryStream();
        await snapshot.CopyToAsync(memory);
        using var original = JsonDocument.Parse(memory.ToArray());
        Assert.Equal(
            0.8,
            original.RootElement
                .GetProperty("evidenceByStrategy")
                .GetProperty("strategy")
                .GetProperty("evidenceQuality")
                .GetDouble());

        var current = await provider.GetEvidenceAsync(
            Request("strategy"),
            CancellationToken.None);
        Assert.Equal(0.9, current!.EvidenceQuality);
    }

    private static string EvidenceDocument(double evidenceQuality) =>
        JsonSerializer.Serialize(
            new
            {
                version = 1,
                evidenceByStrategy = new Dictionary<string, object>
                {
                    ["strategy"] = new { evidenceQuality }
                }
            });

    private static DecisionEvidenceRequest Request(string strategyId) => new(
        new RegisteredDecisionDefinition(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            new RuntimeContractIdentity(
                "definition",
                $"sha256:{new string('a', 64)}",
                "revision"),
            "number",
            System.Text.Json.JsonSerializer.SerializeToElement(800),
            "fallback",
            [],
            []),
        new GovernedDecisionState(
            "definition",
            "revision",
            $"sha256:{new string('a', 64)}",
            System.Text.Json.JsonSerializer.SerializeToElement(800),
            StrategyId: strategyId),
        new Dictionary<string, System.Text.Json.JsonElement>(),
        []);
}
