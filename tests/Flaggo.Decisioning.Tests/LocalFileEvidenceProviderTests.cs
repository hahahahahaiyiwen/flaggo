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
        using var file = TestJsonFile.CreateCommitted("evidence");
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
        using var file = TestJsonFile.CreateCommitted("evidence-zero");
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
        using var file = TestJsonFile.CreateCommitted("evidence-required");
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
        using var file = TestJsonFile.CreateCommitted("evidence-malformed");
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
        using var file =
            TestJsonFile.CreateCommitted("evidence-duplicate-property");
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
        using var file = TestJsonFile.CreateCommitted("evidence-trailing-data");
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
        using var file =
            TestJsonFile.CreateCommitted("evidence-sibling-properties");
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
        using var file = TestJsonFile.CreateCommitted("evidence-property-case");
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
        using var file = TestJsonFile.CreateCommitted("evidence-missing");
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var error = await Assert.ThrowsAsync<EvidenceUnavailableException>(
            () => provider.GetEvidenceAsync(
                Request("strategy"),
                CancellationToken.None));

        Assert.IsAssignableFrom<IOException>(error.InnerException);
        Assert.False(await provider.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task UnreadableEvidencePath_FailsThroughEvidenceBoundary()
    {
        using var file = TestJsonFile.CreateCommitted("evidence-unreadable");
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
    public async Task PublishedReplacement_IsObservedOnNextRead()
    {
        using var file = TestJsonFile.CreateCommitted("evidence-replacement");
        await file.WriteAsync(EvidenceDocument(0.8));
        var provider = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(file.Path));

        var oldEvidence = await provider.GetEvidenceAsync(
            Request("strategy"),
            CancellationToken.None);
        await file.WriteAsync(EvidenceDocument(0.9));
        var newEvidence = await provider.GetEvidenceAsync(
            Request("strategy"),
            CancellationToken.None);

        Assert.Equal(0.8, oldEvidence!.EvidenceQuality);
        Assert.Equal(0.9, newEvidence!.EvidenceQuality);
    }

    [Fact]
    public async Task DirectRawFileWithoutCommitDescriptor_FailsClosed()
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"evidence-raw-{Guid.NewGuid():N}.json");
        try
        {
            await File.WriteAllTextAsync(path, EvidenceDocument(0.8));
            var provider = new LocalFileEvidenceProvider(
                new LocalFileEvidenceProviderOptions(path));

            await Assert.ThrowsAsync<EvidenceUnavailableException>(
                () => provider.GetEvidenceAsync(
                    Request("strategy"),
                    CancellationToken.None));
            Assert.False(
                await provider.IsAvailableAsync(CancellationToken.None));
        }
        finally
        {
            File.Delete(path);
        }
    }

    private static string EvidenceDocument(
        double evidenceQuality,
        string? generation = null) =>
        JsonSerializer.Serialize(
            new
            {
                version = 1,
                evidenceByStrategy = new Dictionary<string, object>
                {
                    ["strategy"] = new
                    {
                        evidenceQuality,
                        details = generation is null
                            ? null
                            : new { generation }
                    }
                }
            });

    private static DecisionEvidenceRequest Request(string strategyId) => new(
        new RuntimeDecisionDefinition(
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
