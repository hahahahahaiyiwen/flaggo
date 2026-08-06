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
