using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

public sealed record LocalFileAuditSinkOptions(string FilePath);

public sealed class LocalFileAuditSink(
    LocalFileAuditSinkOptions options) :
    IAuditSink,
    IExposureAuditSink,
    IAuditHealth,
    IDisposable
{
    private const string RecordSeparator = "\n";

    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
        };

    private static readonly JsonSerializerOptions StrictJsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    private readonly string _filePath = Path.GetFullPath(options.FilePath);
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly HashSet<string> _recordedExposureIds =
        new(StringComparer.Ordinal);

    public Task RecordDecisionAsync(
        DecisionAuditRecord record,
        CancellationToken cancellationToken)
    {
        ValidateDecisionRecord(record);
        return AppendAsync("decision", record, cancellationToken);
    }

    public async Task RecordExposureAsync(
        ExposureAuditRecord record,
        CancellationToken cancellationToken)
    {
        ValidateExposureRecord(record);
        await _gate.WaitAsync(cancellationToken);
        try
        {
            await ValidateExistingRecordsAsync(cancellationToken);
            if (_recordedExposureIds.Contains(record.ExposureId))
            {
                return;
            }

            await AppendCoreAsync("exposure", record, cancellationToken);
            _recordedExposureIds.Add(record.ExposureId);
        }
        finally
        {
            _gate.Release();
        }
    }

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        try
        {
            await _gate.WaitAsync(cancellationToken);
            try
            {
                EnsureDirectory();
                await ValidateExistingRecordsAsync(cancellationToken);
                await using var stream = new FileStream(
                    _filePath,
                    FileMode.OpenOrCreate,
                    FileAccess.Write,
                    FileShare.Read,
                    1,
                    FileOptions.Asynchronous);
                await stream.FlushAsync(cancellationToken);
                return true;
            }
            finally
            {
                _gate.Release();
            }
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

    public void Dispose() => _gate.Dispose();

    private async Task AppendAsync<T>(
        string kind,
        T record,
        CancellationToken cancellationToken)
    {
        await _gate.WaitAsync(cancellationToken);
        try
        {
            await ValidateExistingRecordsAsync(cancellationToken);
            await AppendCoreAsync(kind, record, cancellationToken);
        }
        finally
        {
            _gate.Release();
        }
    }

    private async Task AppendCoreAsync<T>(
        string kind,
        T record,
        CancellationToken cancellationToken)
    {
        EnsureDirectory();
        var json = JsonSerializer.Serialize(
            new AuditEnvelope<T>(kind, record),
            JsonOptions);
        var bytes = Encoding.UTF8.GetBytes($"{json}{RecordSeparator}");
        await using var stream = new FileStream(
            _filePath,
            FileMode.Append,
            FileAccess.Write,
            FileShare.Read,
            4096,
            FileOptions.Asynchronous | FileOptions.WriteThrough);
        await stream.WriteAsync(bytes, cancellationToken);
        await stream.FlushAsync(cancellationToken);
    }

    private async Task ValidateExistingRecordsAsync(
        CancellationToken cancellationToken)
    {
        _recordedExposureIds.Clear();
        if (File.Exists(_filePath))
        {
            await using var stream = new FileStream(
                _filePath,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite,
                4096,
                FileOptions.Asynchronous | FileOptions.SequentialScan);
            if (stream.Length > 0)
            {
                stream.Seek(-1, SeekOrigin.End);
                var terminalByte = new byte[1];
                if (await stream.ReadAsync(terminalByte, cancellationToken) != 1 ||
                    terminalByte[0] != (byte)'\n')
                {
                    throw new InvalidDataException(
                        "The local audit file is missing a terminating newline and may contain a torn JSON Lines record.");
                }

                stream.Seek(0, SeekOrigin.Begin);
            }

            using var reader = new StreamReader(
                stream,
                new UTF8Encoding(false, true),
                detectEncodingFromByteOrderMarks: false);
            while (await reader.ReadLineAsync(cancellationToken) is { } line)
            {
                if (string.IsNullOrWhiteSpace(line))
                {
                    throw new InvalidDataException(
                        "The local audit file contains an empty JSON Lines record.");
                }

                try
                {
                    EnsureNoDuplicateProperties(line);
                    using var document = JsonDocument.Parse(line);
                    var root = document.RootElement;
                    if (root.ValueKind != JsonValueKind.Object ||
                        !root.EnumerateObject().Select(property => property.Name)
                            .ToHashSet(StringComparer.Ordinal)
                            .SetEquals(["kind", "record"]) ||
                        !root.TryGetProperty("kind", out var kind) ||
                        kind.ValueKind != JsonValueKind.String ||
                        !root.TryGetProperty("record", out var record) ||
                        record.ValueKind != JsonValueKind.Object)
                    {
                        throw new InvalidDataException(
                            "The local audit file contains an invalid audit envelope.");
                    }

                    switch (kind.GetString())
                    {
                        case "decision":
                            ValidateDecisionRecord(
                                record.Deserialize<DecisionAuditRecord>(
                                    StrictJsonOptions));
                            break;
                        case "exposure":
                            var exposure = record.Deserialize<ExposureAuditRecord>(
                                StrictJsonOptions);
                            ValidateExposureRecord(exposure);
                            if (!_recordedExposureIds.Add(exposure!.ExposureId))
                            {
                                throw new InvalidDataException(
                                    "The local audit file contains a duplicate exposure id.");
                            }

                            break;
                        default:
                            throw new InvalidDataException(
                                "The local audit file contains an unknown record kind.");
                    }
                }
                catch (JsonException error)
                {
                    throw new InvalidDataException(
                        "The local audit file contains malformed JSON Lines data.",
                        error);
                }
            }
        }
    }

    private static void ValidateDecisionRecord(DecisionAuditRecord? record)
    {
        if (record is null ||
            string.IsNullOrWhiteSpace(record.AuditId) ||
            string.IsNullOrWhiteSpace(record.DecisionId) ||
            string.IsNullOrWhiteSpace(record.DecisionKey) ||
            string.IsNullOrWhiteSpace(record.AppId) ||
            string.IsNullOrWhiteSpace(record.Environment) ||
            record.Contract is null ||
            string.IsNullOrWhiteSpace(record.Contract.DefinitionId) ||
            string.IsNullOrWhiteSpace(record.Contract.Revision) ||
            string.IsNullOrWhiteSpace(record.Contract.ContractDigest) ||
            record.Fallback is null ||
            record.Policy is null ||
            record.RuntimeContext is null ||
            record.Inputs is null ||
            record.TargetProvenance is null ||
            record.ResolutionChain is null ||
            !ValueMatches(record.ValueType, record.Value) ||
            record.DecisionMode is not (
                "active-value" or "strategy" or "experiment" or "fallback") ||
            (record.DecisionMode is "strategy" or "experiment") &&
            (string.IsNullOrWhiteSpace(record.StrategyId) ||
             !IsValidConfidence(record.Confidence)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }
    }

    private static void ValidateExposureRecord(ExposureAuditRecord? record)
    {
        if (record is null ||
            string.IsNullOrWhiteSpace(record.ExposureId) ||
            string.IsNullOrWhiteSpace(record.DecisionId) ||
            string.IsNullOrWhiteSpace(record.AppId) ||
            string.IsNullOrWhiteSpace(record.Environment) ||
            !DateTimeOffset.TryParse(record.ConfirmedAt, out _) ||
            record.AppliedAt is not null &&
            !DateTimeOffset.TryParse(record.AppliedAt, out _))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid exposure record.");
        }
    }

    private static bool ValueMatches(string valueType, JsonElement value) =>
        valueType switch
        {
            "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "number" => value.ValueKind == JsonValueKind.Number &&
                        value.TryGetDouble(out var number) &&
                        double.IsFinite(number),
            "string" => value.ValueKind == JsonValueKind.String,
            _ => false
        };

    private static bool IsValidConfidence(ConfidenceReport? confidence) =>
        confidence is not null &&
        IsProbability(confidence.EvidenceQuality) &&
        (confidence.ModelUncertainty is null ||
         IsProbability(confidence.ModelUncertainty.Value)) &&
        (confidence.ExpectedOutcome is null ||
         IsProbability(confidence.ExpectedOutcome.Value));

    private static bool IsProbability(double value) =>
        double.IsFinite(value) && value is >= 0 and <= 1;

    private static void EnsureNoDuplicateProperties(string json)
    {
        var reader = new Utf8JsonReader(Encoding.UTF8.GetBytes(json));
        var objectProperties = new Stack<HashSet<string>>();
        while (reader.Read())
        {
            switch (reader.TokenType)
            {
                case JsonTokenType.StartObject:
                    objectProperties.Push(new HashSet<string>(StringComparer.Ordinal));
                    break;
                case JsonTokenType.EndObject:
                    objectProperties.Pop();
                    break;
                case JsonTokenType.PropertyName:
                    var propertyName = reader.GetString()!;
                    if (!objectProperties.Peek().Add(propertyName))
                    {
                        throw new JsonException(
                            $"Duplicate JSON property '{propertyName}' is not allowed.");
                    }

                    break;
            }
        }
    }

    private void EnsureDirectory()
    {
        var directory = Path.GetDirectoryName(_filePath)
            ?? throw new IOException("The local audit file has no parent directory.");
        Directory.CreateDirectory(directory);
    }

    private sealed record AuditEnvelope<T>(string Kind, T Record);
}
