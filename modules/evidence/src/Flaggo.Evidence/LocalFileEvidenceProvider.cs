using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Shared.Contracts;

namespace Flaggo.Evidence;

public sealed record LocalFileEvidenceProviderOptions(string FilePath);

public sealed class LocalFileEvidenceProvider(
    LocalFileEvidenceProviderOptions options) :
    IEvidenceProvider,
    IEvidenceHealth
{
    private const int FormatVersion = 1;

    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    private readonly string _filePath = Path.GetFullPath(options.FilePath);

    public async Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
        DecisionEvidenceRequest request,
        CancellationToken cancellationToken)
    {
        if (request.State.StrategyId is null)
        {
            return null;
        }

        var evidence = await LoadAsync(cancellationToken);
        return evidence.GetValueOrDefault(request.State.StrategyId);
    }

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        try
        {
            await LoadAsync(cancellationToken);
            return true;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch
        {
            return false;
        }
    }

    private async Task<IReadOnlyDictionary<string, DecisionEvidenceSnapshot>>
        LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            await using var stream = OpenSnapshotRead(_filePath);
            using var memory = new MemoryStream();
            await stream.CopyToAsync(memory, cancellationToken);
            var json = memory.ToArray();
            StrictJson.Validate(json);
            var document = JsonSerializer.Deserialize<PersistedEvidenceDocument>(
                json,
                JsonOptions);
            if (document is null ||
                document.Version != FormatVersion ||
                document.EvidenceByStrategy is null)
            {
                throw new InvalidDataException(
                    $"The local evidence file must use version {FormatVersion}.");
            }

            foreach (var (strategyId, evidence) in document.EvidenceByStrategy)
            {
                if (string.IsNullOrWhiteSpace(strategyId) ||
                    evidence is null ||
                    evidence.EvidenceQuality is not double evidenceQuality ||
                    !IsProbability(evidenceQuality) ||
                    evidence.ModelUncertainty is double uncertainty &&
                    !IsProbability(uncertainty) ||
                    evidence.ExpectedOutcome is double outcome &&
                    !IsProbability(outcome) ||
                    evidence.SampleSize is double sampleSize &&
                    (!double.IsFinite(sampleSize) || sampleSize < 0))
                {
                    throw new InvalidDataException(
                        "The local evidence file contains an invalid strategy snapshot.");
                }
            }

            return document.EvidenceByStrategy.ToDictionary(
                pair => pair.Key,
                pair =>
                {
                    var evidence = pair.Value!;
                    return new DecisionEvidenceSnapshot(
                        evidence.EvidenceQuality!.Value,
                        evidence.ModelUncertainty,
                        evidence.ExpectedOutcome,
                        evidence.SampleSize,
                        evidence.Details);
                },
                StringComparer.Ordinal);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (JsonException error)
        {
            throw new EvidenceUnavailableException(
                "The local evidence file is not valid strict JSON.",
                error);
        }
        catch (Exception error) when (
            error is IOException or
            UnauthorizedAccessException or
            InvalidDataException)
        {
            throw new EvidenceUnavailableException(
                "The local evidence file is unavailable.",
                error);
        }
    }

    internal static FileStream OpenSnapshotRead(string filePath) =>
        new(
            filePath,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read | FileShare.Delete,
            4096,
            FileOptions.Asynchronous | FileOptions.SequentialScan);

    private static bool IsProbability(double value) =>
        double.IsFinite(value) && value is >= 0 and <= 1;

    private sealed record PersistedEvidenceDocument(
        int? Version,
        IReadOnlyDictionary<string, PersistedEvidenceSnapshot?>? EvidenceByStrategy);

    private sealed record PersistedEvidenceSnapshot(
        double? EvidenceQuality,
        double? ModelUncertainty = null,
        double? ExpectedOutcome = null,
        double? SampleSize = null,
        IReadOnlyDictionary<string, JsonElement>? Details = null);
}
