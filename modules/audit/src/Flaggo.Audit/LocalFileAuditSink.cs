using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

public sealed record LocalFileAuditSinkOptions(
    string FilePath,
    TimeSpan? LockTimeout = null,
    TimeSpan? LockRetryDelay = null,
    int MaximumSegmentRecords = 256,
    long MaximumSegmentBytes = 4 * 1024 * 1024);

public sealed class LocalFileAuditSink :
    IAuditSink,
    IExposureAuditSink,
    IAuditHealth,
    IDisposable
{
    private const int SegmentFormatVersion = 1;
    private const int MarkerFormatVersion = 1;
    private const string CurrentSegmentFileName = "current.jsonl";
    private const string RecordSeparator = "\n";
    private static readonly Regex ClosedSegmentNamePattern = new(
        "\\Asegment-(?<sequence>\\d{20})-(?<hash>[0-9a-f]{64})\\.jsonl\\z",
        RegexOptions.CultureInvariant);
    private static readonly Regex PersistedTimestampPattern = new(
        "\\A\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d{1,16})?(?:Z|[+-]\\d{2}:\\d{2})\\z",
        RegexOptions.CultureInvariant);
    private static readonly Regex Sha256DigestPattern = new(
        "\\Asha256:[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant);
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

    private readonly string _filePath;
    private readonly string _lockPath;
    private readonly string _auditDirectoryPath;
    private readonly string _segmentsDirectoryPath;
    private readonly string _markersDirectoryPath;
    private readonly TimeSpan _lockTimeout;
    private readonly TimeSpan _lockRetryDelay;
    private readonly int _maximumSegmentRecords;
    private readonly long _maximumSegmentBytes;
    private readonly IDurableDirectoryOperations _directoryOperations;
    private readonly SegmentedAuditStore _segmentedStore;
    private readonly SemaphoreSlim _gate = new(1, 1);

    public LocalFileAuditSink(LocalFileAuditSinkOptions options) :
        this(options, FileSystemAuditDirectoryOperations.Instance)
    {
    }

    internal LocalFileAuditSink(
        LocalFileAuditSinkOptions options,
        IDurableDirectoryOperations directoryOperations)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(directoryOperations);
        ArgumentException.ThrowIfNullOrWhiteSpace(options.FilePath);
        _filePath = Path.GetFullPath(options.FilePath);
        _lockPath = $"{_filePath}.lock";
        _auditDirectoryPath = $"{_filePath}.d";
        _segmentsDirectoryPath = Path.Combine(_auditDirectoryPath, "segments");
        _markersDirectoryPath = Path.Combine(_auditDirectoryPath, "exposures");
        _lockTimeout = options.LockTimeout ?? TimeSpan.FromSeconds(10);
        _lockRetryDelay = options.LockRetryDelay ?? TimeSpan.FromMilliseconds(25);
        _maximumSegmentRecords = options.MaximumSegmentRecords;
        _maximumSegmentBytes = options.MaximumSegmentBytes;
        _directoryOperations = directoryOperations;
        if (_lockTimeout <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The audit file lock timeout must be positive.");
        }
        if (_lockRetryDelay <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The audit file lock retry delay must be positive.");
        }
        if (_maximumSegmentRecords <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The audit segment record limit must be positive.");
        }
        if (_maximumSegmentBytes < 1024)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The audit segment byte limit must be at least 1024 bytes.");
        }
        _segmentedStore = new SegmentedAuditStore(
            _filePath,
            _auditDirectoryPath,
            _segmentsDirectoryPath,
            _markersDirectoryPath,
            _maximumSegmentRecords,
            _maximumSegmentBytes,
            ValidateRecord,
            directoryOperations);
    }

    internal int FullValidationCount { get; private set; }

    internal int LastAppendValidatedRecordCount { get; private set; }

    internal string AuditDirectoryPath => _auditDirectoryPath;

    internal Action? AfterRecordDurablyFlushed { get; set; }

    internal Action? AfterNewSegmentDurablyFlushed
    {
        get => _segmentedStore.AfterNewSegmentDurablyFlushed;
        set => _segmentedStore.AfterNewSegmentDurablyFlushed = value;
    }

    internal Action? AfterRotationManifestDurablyFlushed
    {
        get => _segmentedStore.AfterRotationManifestDurablyFlushed;
        set => _segmentedStore.AfterRotationManifestDurablyFlushed = value;
    }

    public Task RecordDecisionAsync(
        DecisionAuditRecord record,
        CancellationToken cancellationToken)
    {
        ValidateDecisionRecord(record);
        return AppendAsync("decision", record, exposureId: null, cancellationToken);
    }

    public Task RecordExposureAsync(
        ExposureAuditRecord record,
        CancellationToken cancellationToken)
    {
        ValidateExposureRecord(record);
        return AppendAsync("exposure", record, record.ExposureId, cancellationToken);
    }

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        try
        {
            await _gate.WaitAsync(cancellationToken);
            try
            {
                await using var lease = await AcquireLeaseAsync(cancellationToken);
                await _segmentedStore.EnsureLayoutAsync(cancellationToken);
                FullValidationCount++;
                await _segmentedStore.ValidateReadinessAsync(cancellationToken);
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
        string? exposureId,
        CancellationToken cancellationToken)
    {
        var bytes = SerializeRecord(kind, record);
        var validated = ValidateRecord(bytes);
        await _gate.WaitAsync(cancellationToken);
        try
        {
            await using var lease = await AcquireLeaseAsync(cancellationToken);
            await _segmentedStore.EnsureLayoutAsync(cancellationToken);
            await _segmentedStore.AppendAsync(
                bytes,
                exposureId,
                validated.ExposureHash,
                AfterRecordDurablyFlushed,
                cancellationToken);
            LastAppendValidatedRecordCount =
                _segmentedStore.LastAppendValidatedRecordCount;
        }
        finally
        {
            _gate.Release();
        }
    }

    private async Task EnsureLayoutAsync(CancellationToken cancellationToken)
    {
        EnsureParentDirectory();
        DeleteAbandonedStagingDirectories();
        if (Directory.Exists(_auditDirectoryPath))
        {
            DurableDirectory.Create(
                _segmentsDirectoryPath,
                _directoryOperations);
            DurableDirectory.Create(
                _markersDirectoryPath,
                _directoryOperations);
            if (File.Exists(_filePath))
            {
                File.Delete(_filePath);
                _directoryOperations.Flush(Path.GetDirectoryName(_filePath)!);
            }
            return;
        }

        var legacyRecords = File.Exists(_filePath)
            ? await ReadLegacyRecordsAsync(cancellationToken)
            : [];
        var stagingPath = $"{_auditDirectoryPath}.tmp-{Guid.NewGuid():N}";
        try
        {
            var stagingSegments = Path.Combine(stagingPath, "segments");
            var stagingMarkers = Path.Combine(stagingPath, "exposures");
            DurableDirectory.Create(stagingSegments, _directoryOperations);
            DurableDirectory.Create(stagingMarkers, _directoryOperations);
            await WriteMigratedSegmentsAsync(
                stagingSegments,
                stagingMarkers,
                legacyRecords,
                cancellationToken);
            _directoryOperations.Flush(stagingSegments);
            _directoryOperations.Flush(stagingMarkers);
            _directoryOperations.Flush(stagingPath);
            Directory.Move(stagingPath, _auditDirectoryPath);
            _directoryOperations.Flush(
                Path.GetDirectoryName(_auditDirectoryPath)!);
            if (File.Exists(_filePath))
            {
                File.Delete(_filePath);
                _directoryOperations.Flush(Path.GetDirectoryName(_filePath)!);
            }
        }
        finally
        {
            if (Directory.Exists(stagingPath))
            {
                Directory.Delete(stagingPath, recursive: true);
            }
        }
    }

    private async Task<IReadOnlyList<LegacyRecord>> ReadLegacyRecordsAsync(
        CancellationToken cancellationToken)
    {
        var bytes = await File.ReadAllBytesAsync(_filePath, cancellationToken);
        if (bytes.Length == 0)
        {
            return [];
        }
        var records = SplitLines(bytes, "legacy audit file");
        var exposureIds = new HashSet<string>(StringComparer.Ordinal);
        var result = new List<LegacyRecord>(records.Count);
        foreach (var rawRecord in records)
        {
            var validated = ValidateRecord(rawRecord);
            if (validated.ExposureId is not null &&
                !exposureIds.Add(validated.ExposureId))
            {
                throw new InvalidDataException(
                    "The local audit file contains a duplicate exposure id.");
            }
            result.Add(new LegacyRecord(rawRecord, validated.ExposureId));
        }
        return result;
    }

    private async Task WriteMigratedSegmentsAsync(
        string segmentsPath,
        string markersPath,
        IReadOnlyList<LegacyRecord> records,
        CancellationToken cancellationToken)
    {
        var chunks = new List<List<LegacyRecord>>();
        var currentChunk = new List<LegacyRecord>();
        var sequence = 1L;
        var header = NewHeader(sequence);
        var currentLength = HeaderBytes(header).LongLength;
        foreach (var record in records)
        {
            if (record.Bytes.LongLength + HeaderBytes(header).LongLength >
                _maximumSegmentBytes)
            {
                throw new InvalidDataException(
                    "A legacy audit record exceeds the configured segment byte limit.");
            }
            if (currentChunk.Count >= _maximumSegmentRecords ||
                currentLength + record.Bytes.LongLength > _maximumSegmentBytes)
            {
                chunks.Add(currentChunk);
                currentChunk = [];
                sequence++;
                header = NewHeader(sequence);
                currentLength = HeaderBytes(header).LongLength;
            }
            currentChunk.Add(record);
            currentLength += record.Bytes.LongLength;
        }
        chunks.Add(currentChunk);

        for (var chunkIndex = 0; chunkIndex < chunks.Count; chunkIndex++)
        {
            sequence = chunkIndex + 1;
            header = NewHeader(sequence);
            var chunk = chunks[chunkIndex];
            var isCurrent = chunkIndex == chunks.Count - 1;
            var content = SegmentBytes(
                header,
                chunk.Select(item => item.Bytes).ToArray());
            var fileName = isCurrent
                ? CurrentSegmentFileName
                : ClosedSegmentFileName(sequence, content);
            await WriteDurableFileAsync(
                Path.Combine(segmentsPath, fileName),
                content,
                FileMode.CreateNew,
                cancellationToken);
            for (var recordIndex = 0; recordIndex < chunk.Count; recordIndex++)
            {
                var record = chunk[recordIndex];
                if (record.ExposureId is null)
                {
                    continue;
                }
                await WriteMarkerAtomicAsync(
                    markersPath,
                    new ExposureMarker(
                        MarkerFormatVersion,
                        record.ExposureId,
                        header.SegmentId,
                        header.Sequence,
                        recordIndex,
                        Hash(record.Bytes)),
                    overwrite: false,
                    cancellationToken);
            }
        }
    }

    private async Task<SegmentState> ReadCurrentSegmentAsync(
        CancellationToken cancellationToken)
    {
        var currentPath = Path.Combine(
            _segmentsDirectoryPath,
            CurrentSegmentFileName);
        if (!File.Exists(currentPath))
        {
            var sequence = NextSequenceFromClosedSegments();
            await WriteSegmentAsync(
                currentPath,
                NewHeader(sequence),
                [],
                cancellationToken);
        }
        return await ValidateSegmentAsync(
            currentPath,
            expectedSequence: null,
            cancellationToken);
    }

    private async Task<SegmentState> RotateAsync(
        SegmentState current,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var currentPath = Path.Combine(
            _segmentsDirectoryPath,
            CurrentSegmentFileName);
        var content = await File.ReadAllBytesAsync(currentPath, cancellationToken);
        if (!string.Equals(
                Hash(content),
                current.ContentHash,
                StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The current audit segment changed while it was validated.");
        }
        var closedPath = Path.Combine(
            _segmentsDirectoryPath,
            ClosedSegmentFileName(current.Header.Sequence, content));
        File.Move(currentPath, closedPath);
        _directoryOperations.Flush(_segmentsDirectoryPath);
        var nextHeader = NewHeader(checked(current.Header.Sequence + 1));
        await WriteSegmentAsync(currentPath, nextHeader, [], cancellationToken);
        return new SegmentState(
            nextHeader,
            HeaderBytes(nextHeader).LongLength,
            Hash(HeaderBytes(nextHeader)),
            []);
    }

    private async Task ValidateAllAndRepairMarkersAsync(
        CancellationToken cancellationToken)
    {
        FullValidationCount++;
        CleanupTemporaryMarkerFiles();
        var paths = await OrderedSegmentPathsAsync(cancellationToken);
        var expectedSequence = 1L;
        var expectedMarkers = new Dictionary<string, ExposureMarker>(
            StringComparer.Ordinal);
        foreach (var path in paths)
        {
            var segment = await ValidateSegmentAsync(
                path,
                expectedSequence,
                cancellationToken);
            expectedSequence++;
            foreach (var record in segment.Records)
            {
                if (record.ExposureId is null)
                {
                    continue;
                }
                if (!expectedMarkers.TryAdd(
                        record.ExposureId,
                        MarkerFor(segment.Header, record)))
                {
                    throw new InvalidDataException(
                        "The local audit segments contain a duplicate exposure id.");
                }
            }
        }

        foreach (var marker in expectedMarkers.Values)
        {
            await EnsureMarkerAsync(
                marker,
                repairInvalid: true,
                cancellationToken);
        }

        var expectedPaths = expectedMarkers.Keys
            .Select(GetMarkerPath)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var markerPath in Directory.EnumerateFiles(
                     _markersDirectoryPath,
                     "*.json",
                     SearchOption.TopDirectoryOnly))
        {
            if (!expectedPaths.Contains(markerPath))
            {
                throw new InvalidDataException(
                    "The local audit exposure markers contain an unreferenced marker.");
            }
            var marker = await ReadMarkerAsync(markerPath, cancellationToken);
            if (!expectedMarkers.TryGetValue(marker.ExposureId, out var expected) ||
                marker != expected)
            {
                throw new InvalidDataException(
                    "The local audit exposure marker does not reference its committed record.");
            }
        }
    }

    private async Task RepairCurrentSegmentMarkersAsync(
        SegmentState current,
        CancellationToken cancellationToken)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var record in current.Records)
        {
            if (record.ExposureId is null)
            {
                continue;
            }
            if (!seen.Add(record.ExposureId))
            {
                throw new InvalidDataException(
                    "The current audit segment contains a duplicate exposure id.");
            }
            await EnsureMarkerAsync(
                MarkerFor(current.Header, record),
                repairInvalid: true,
                cancellationToken);
        }
    }

    private async Task<IReadOnlyList<string>> OrderedSegmentPathsAsync(
        CancellationToken cancellationToken)
    {
        var closed = new SortedDictionary<long, string>();
        foreach (var path in Directory.EnumerateFiles(
                     _segmentsDirectoryPath,
                     "*.jsonl",
                     SearchOption.TopDirectoryOnly))
        {
            var name = Path.GetFileName(path);
            if (string.Equals(
                    name,
                    CurrentSegmentFileName,
                    StringComparison.Ordinal))
            {
                continue;
            }
            var match = ClosedSegmentNamePattern.Match(name);
            if (!match.Success ||
                !long.TryParse(
                    match.Groups["sequence"].Value,
                    CultureInfo.InvariantCulture,
                    out var sequence) ||
                !closed.TryAdd(sequence, path))
            {
                throw new InvalidDataException(
                    "The local audit segment directory contains an invalid segment name.");
            }
        }

        var result = closed.Values.ToList();
        var currentPath = Path.Combine(
            _segmentsDirectoryPath,
            CurrentSegmentFileName);
        if (!File.Exists(currentPath))
        {
            var sequence = closed.Count == 0 ? 1 : checked(closed.Keys.Max() + 1);
            await WriteSegmentAsync(
                currentPath,
                NewHeader(sequence),
                [],
                cancellationToken);
        }
        result.Add(currentPath);
        return result;
    }

    private async Task<SegmentState> ValidateSegmentAsync(
        string path,
        long? expectedSequence,
        CancellationToken cancellationToken)
    {
        var info = new FileInfo(path);
        if (info.Length > _maximumSegmentBytes)
        {
            throw new InvalidDataException(
                "A local audit segment exceeds the configured byte limit.");
        }
        var bytes = await File.ReadAllBytesAsync(path, cancellationToken);
        var lines = SplitLines(bytes, "audit segment");
        if (lines.Count == 0)
        {
            throw new InvalidDataException("The local audit segment is empty.");
        }
        var header = ParseHeader(lines[0]);
        if (expectedSequence is not null &&
            header.Sequence != expectedSequence.Value)
        {
            throw new InvalidDataException(
                "The local audit segment sequence is not contiguous.");
        }
        var fileName = Path.GetFileName(path);
        if (!string.Equals(
                fileName,
                CurrentSegmentFileName,
                StringComparison.Ordinal))
        {
            var match = ClosedSegmentNamePattern.Match(fileName);
            var actualHash = Convert.ToHexString(SHA256.HashData(bytes))
                .ToLowerInvariant();
            if (!match.Success ||
                !string.Equals(
                    match.Groups["sequence"].Value,
                    header.Sequence.ToString("D20", CultureInfo.InvariantCulture),
                    StringComparison.Ordinal) ||
                !string.Equals(
                    match.Groups["hash"].Value,
                    actualHash,
                    StringComparison.Ordinal))
            {
                throw new InvalidDataException(
                    "The local audit segment name does not match its durable content.");
            }
        }
        if (lines.Count - 1 > _maximumSegmentRecords)
        {
            throw new InvalidDataException(
                "A local audit segment exceeds the configured record limit.");
        }

        var records = new List<SegmentRecord>(lines.Count - 1);
        for (var index = 1; index < lines.Count; index++)
        {
            var validated = ValidateRecord(lines[index]);
            records.Add(
                new SegmentRecord(
                    index - 1,
                    validated.ExposureId,
                    Hash(lines[index])));
        }
        return new SegmentState(header, bytes.LongLength, Hash(bytes), records);
    }

    private static SegmentHeader ParseHeader(byte[] rawLine)
    {
        var line = TrimLineEnding(rawLine);
        try
        {
            StrictJson.Validate(line);
            var header = JsonSerializer.Deserialize<SegmentHeaderEnvelope>(
                line,
                StrictJsonOptions);
            if (header is null ||
                !string.Equals(
                    header.Kind,
                    "segment",
                    StringComparison.Ordinal) ||
                header.Record.Version != SegmentFormatVersion ||
                header.Record.Sequence <= 0 ||
                !Guid.TryParseExact(
                    header.Record.SegmentId,
                    "N",
                    out _))
            {
                throw new InvalidDataException(
                    "The local audit segment contains an invalid header.");
            }
            return header.Record;
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The local audit segment contains an invalid header.",
                error);
        }
    }

    private async Task<ExposureMarker?> TryReadMarkerAsync(
        string exposureId,
        CancellationToken cancellationToken)
    {
        var path = GetMarkerPath(exposureId);
        if (!File.Exists(path))
        {
            return null;
        }
        var marker = await ReadMarkerAsync(path, cancellationToken);
        if (!string.Equals(
                marker.ExposureId,
                exposureId,
                StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The local audit exposure marker is corrupt.");
        }
        return marker;
    }

    private async Task<ExposureMarker> ReadMarkerAsync(
        string path,
        CancellationToken cancellationToken)
    {
        try
        {
            var bytes = await File.ReadAllBytesAsync(path, cancellationToken);
            StrictJson.Validate(bytes);
            var marker = JsonSerializer.Deserialize<ExposureMarker>(
                bytes,
                StrictJsonOptions);
            if (marker is null ||
                marker.Version != MarkerFormatVersion ||
                string.IsNullOrWhiteSpace(marker.ExposureId) ||
                !Guid.TryParseExact(marker.SegmentId, "N", out _) ||
                marker.SegmentSequence <= 0 ||
                marker.RecordIndex < 0 ||
                !Sha256DigestPattern.IsMatch(marker.RecordHash) ||
                !string.Equals(
                    path,
                    GetMarkerPath(marker.ExposureId),
                    StringComparison.OrdinalIgnoreCase))
            {
                throw new InvalidDataException(
                    "The local audit exposure marker is corrupt.");
            }
            return marker;
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The local audit exposure marker is corrupt.",
                error);
        }
    }

    private async Task EnsureMarkerAsync(
        ExposureMarker marker,
        bool repairInvalid,
        CancellationToken cancellationToken)
    {
        var path = GetMarkerPath(marker.ExposureId);
        if (File.Exists(path))
        {
            try
            {
                if (await ReadMarkerAsync(path, cancellationToken) == marker)
                {
                    return;
                }
            }
            catch (InvalidDataException) when (repairInvalid)
            {
            }
            if (!repairInvalid)
            {
                throw new InvalidDataException(
                    "The local audit exposure marker conflicts with the committed record.");
            }
        }
        await WriteMarkerAtomicAsync(
            _markersDirectoryPath,
            marker,
            overwrite: File.Exists(path),
            cancellationToken);
    }

    private async Task WriteMarkerAtomicAsync(
        string directory,
        ExposureMarker marker,
        bool overwrite,
        CancellationToken cancellationToken)
    {
        DurableDirectory.Create(directory, _directoryOperations);
        var finalPath = Path.Combine(directory, MarkerFileName(marker.ExposureId));
        var stagingPath = $"{finalPath}.tmp-{Guid.NewGuid():N}";
        var bytes = Encoding.UTF8.GetBytes(
            $"{JsonSerializer.Serialize(marker, JsonOptions)}{RecordSeparator}");
        try
        {
            await WriteDurableFileAsync(
                stagingPath,
                bytes,
                FileMode.CreateNew,
                cancellationToken);
            File.Move(stagingPath, finalPath, overwrite);
            _directoryOperations.Flush(directory);
        }
        finally
        {
            File.Delete(stagingPath);
        }
    }

    private async Task<FileStream> AcquireLeaseAsync(
        CancellationToken cancellationToken)
    {
        EnsureParentDirectory();
        var started = Stopwatch.GetTimestamp();
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                return new FileStream(
                    _lockPath,
                    FileMode.OpenOrCreate,
                    FileAccess.ReadWrite,
                    FileShare.None,
                    1,
                    FileOptions.Asynchronous);
            }
            catch (IOException error)
            {
                var elapsed = Stopwatch.GetElapsedTime(started);
                if (elapsed >= _lockTimeout)
                {
                    throw new TimeoutException(
                        $"Timed out acquiring the local audit file lock '{_lockPath}'.",
                        error);
                }
                var remaining = _lockTimeout - elapsed;
                await Task.Delay(
                    remaining < _lockRetryDelay ? remaining : _lockRetryDelay,
                    cancellationToken);
            }
        }
    }

    private static async Task AppendDurablyAsync(
        string path,
        long expectedLength,
        string expectedHash,
        byte[] bytes,
        CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.ReadWrite,
            FileShare.Read,
            4096,
            FileOptions.Asynchronous |
            FileOptions.SequentialScan |
            FileOptions.WriteThrough);
        if (stream.Length != expectedLength)
        {
            throw new InvalidDataException(
                "The current audit segment changed while it was validated.");
        }
        var currentBytes = new byte[checked((int)expectedLength)];
        var offset = 0;
        while (offset < currentBytes.Length)
        {
            var read = await stream.ReadAsync(
                currentBytes.AsMemory(offset),
                cancellationToken);
            if (read == 0)
            {
                throw new InvalidDataException(
                    "The current audit segment changed while it was validated.");
            }
            offset += read;
        }
        if (!string.Equals(
                Hash(currentBytes),
                expectedHash,
                StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The current audit segment changed while it was validated.");
        }
        stream.Seek(0, SeekOrigin.End);
        await stream.WriteAsync(bytes, cancellationToken);
        await stream.FlushAsync(cancellationToken);
        stream.Flush(flushToDisk: true);
    }

    private static Task WriteSegmentAsync(
        string path,
        SegmentHeader header,
        IReadOnlyList<byte[]> records,
        CancellationToken cancellationToken) =>
        WriteDurableFileAsync(
            path,
            SegmentBytes(header, records),
            FileMode.CreateNew,
            cancellationToken);

    private static byte[] SegmentBytes(
        SegmentHeader header,
        IReadOnlyList<byte[]> records)
    {
        using var stream = new MemoryStream();
        stream.Write(HeaderBytes(header));
        foreach (var record in records)
        {
            stream.Write(record);
        }
        return stream.ToArray();
    }

    private static async Task WriteDurableFileAsync(
        string path,
        byte[] bytes,
        FileMode mode,
        CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(
            path,
            mode,
            FileAccess.Write,
            FileShare.None,
            4096,
            FileOptions.Asynchronous | FileOptions.WriteThrough);
        await stream.WriteAsync(bytes, cancellationToken);
        await stream.FlushAsync(cancellationToken);
        stream.Flush(flushToDisk: true);
    }

    private static IReadOnlyList<byte[]> SplitLines(byte[] bytes, string source)
    {
        if (bytes.Length == 0)
        {
            return [];
        }
        if (bytes[^1] != (byte)'\n')
        {
            throw new InvalidDataException(
                $"The local {source} is missing a terminating newline and may contain a torn JSON Lines record.");
        }
        var result = new List<byte[]>();
        var start = 0;
        for (var index = 0; index < bytes.Length; index++)
        {
            if (bytes[index] != (byte)'\n')
            {
                continue;
            }
            var length = index - start + 1;
            var line = new byte[length];
            Array.Copy(bytes, start, line, 0, length);
            result.Add(line);
            start = index + 1;
        }
        return result;
    }

    private static ReadOnlySpan<byte> TrimLineEnding(byte[] rawLine)
    {
        var length = rawLine.Length;
        if (length > 0 && rawLine[length - 1] == (byte)'\n')
        {
            length--;
        }
        if (length > 0 && rawLine[length - 1] == (byte)'\r')
        {
            length--;
        }
        return rawLine.AsSpan(0, length);
    }

    private static byte[] SerializeRecord<T>(string kind, T record) =>
        Encoding.UTF8.GetBytes(
            $"{JsonSerializer.Serialize(new AuditEnvelope<T>(kind, record), JsonOptions)}{RecordSeparator}");

    private static byte[] HeaderBytes(SegmentHeader header) =>
        Encoding.UTF8.GetBytes(
            $"{JsonSerializer.Serialize(new SegmentHeaderEnvelope("segment", header), JsonOptions)}{RecordSeparator}");

    private static SegmentHeader NewHeader(long sequence) =>
        new(SegmentFormatVersion, sequence, Guid.NewGuid().ToString("N"));

    private static string ClosedSegmentFileName(long sequence, byte[] content) =>
        $"segment-{sequence:D20}-" +
        $"{Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant()}.jsonl";

    private long NextSequenceFromClosedSegments()
    {
        var maximum = 0L;
        foreach (var path in Directory.EnumerateFiles(
                     _segmentsDirectoryPath,
                     "segment-*.jsonl",
                     SearchOption.TopDirectoryOnly))
        {
            var match = ClosedSegmentNamePattern.Match(Path.GetFileName(path));
            if (!match.Success ||
                !long.TryParse(
                    match.Groups["sequence"].Value,
                    CultureInfo.InvariantCulture,
                    out var sequence))
            {
                throw new InvalidDataException(
                    "The local audit segment directory contains an invalid segment name.");
            }
            maximum = Math.Max(maximum, sequence);
        }
        return checked(maximum + 1);
    }

    private static ExposureMarker MarkerFor(
        SegmentHeader header,
        SegmentRecord record) =>
        new(
            MarkerFormatVersion,
            record.ExposureId!,
            header.SegmentId,
            header.Sequence,
            record.Index,
            record.Hash);

    private string GetMarkerPath(string exposureId) =>
        Path.Combine(_markersDirectoryPath, MarkerFileName(exposureId));

    private static string MarkerFileName(string exposureId) =>
        $"{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(exposureId))).ToLowerInvariant()}.json";

    private static string Hash(byte[] value) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(value)).ToLowerInvariant()}";

    private void EnsureParentDirectory()
    {
        var directory = Path.GetDirectoryName(_filePath)
            ?? throw new IOException("The local audit path has no parent directory.");
        DurableDirectory.Create(directory, _directoryOperations);
    }

    private void DeleteAbandonedStagingDirectories()
    {
        var parent = Path.GetDirectoryName(_auditDirectoryPath)!;
        var name = Path.GetFileName(_auditDirectoryPath);
        foreach (var path in Directory.EnumerateDirectories(
                     parent,
                     $"{name}.tmp-*",
                     SearchOption.TopDirectoryOnly))
        {
            Directory.Delete(path, recursive: true);
        }
    }

    private void CleanupTemporaryMarkerFiles()
    {
        foreach (var path in Directory.EnumerateFiles(
                     _markersDirectoryPath,
                     "*.tmp-*",
                     SearchOption.TopDirectoryOnly))
        {
            File.Delete(path);
        }
    }

    private static ValidatedAuditRecord ValidateRecord(byte[] rawRecord)
    {
        var line = TrimLineEnding(rawRecord);
        if (line.IsEmpty ||
            line.IndexOfAnyExcept((byte)' ', (byte)'\t', (byte)'\r') < 0)
        {
            throw new InvalidDataException(
                "The local audit file contains an empty JSON Lines record.");
        }
        try
        {
            StrictJson.Validate(line);
            using var document = JsonDocument.Parse(line.ToArray());
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
                    ValidatePersistedDecisionShape(record);
                    ValidateDecisionRecord(
                        record.Deserialize<DecisionAuditRecord>(
                            StrictJsonOptions));
                    return new ValidatedAuditRecord(null, null);
                case "exposure":
                    var exposure = record.Deserialize<ExposureAuditRecord>(
                        StrictJsonOptions);
                    ValidateExposureRecord(exposure);
                    return new ValidatedAuditRecord(
                        exposure!.ExposureId,
                        ExposureAuditIdentity.Hash(exposure));
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

    private static void ValidateDecisionRecord(DecisionAuditRecord? record)
    {
        if (record is null ||
            string.IsNullOrWhiteSpace(record.AuditId) ||
            string.IsNullOrWhiteSpace(record.DecisionId) ||
            string.IsNullOrWhiteSpace(record.DecisionKey) ||
            string.IsNullOrWhiteSpace(record.TenantId) ||
            string.IsNullOrWhiteSpace(record.AppId) ||
            string.IsNullOrWhiteSpace(record.Environment) ||
            record.Contract is null ||
            !IsValidContract(record.Contract) ||
            record.Fallback is null ||
            record.Policy is null ||
            record.RuntimeContext is null ||
            record.Inputs is null ||
            record.RequestInputs is null ||
            record.InputProvenance is null ||
            record.TargetProvenance is null ||
            record.ResolutionChain is null ||
            record.RecordedAt == default ||
            !IsValidFallback(record.Fallback) ||
            !IsValidPolicy(record.Policy) ||
            !IsValidRuntimeContext(record.RuntimeContext) ||
            record.Inputs.Any(input => !IsValidInput(input)) ||
            !IsValidInputProvenance(record) ||
            record.TargetProvenance.Any(item => !IsValidTargetProvenance(item)) ||
            record.ResolutionChain.Any(string.IsNullOrWhiteSpace) ||
            !IsValidTarget(record.RuntimeTarget) ||
            !IsValidTarget(record.ControlTarget) ||
            record.Evidence is not null &&
            !IsValidEvidence(record.Evidence) ||
            !ValueMatches(record.ValueType, record.Value) ||
            !IsValidDecisionSemantics(record))
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
            !IsValidPersistedTimestamp(record.ConfirmedAt) ||
            record.AppliedAt is not null &&
            !IsValidPersistedTimestamp(record.AppliedAt))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid exposure record.");
        }
    }

    private static void ValidatePersistedDecisionShape(JsonElement record)
    {
        RequireProperties(
            record,
            "auditId",
            "decisionId",
            "decisionKey",
            "appId",
            "environment",
            "contract",
            "value",
            "valueType",
            "decisionMode",
            "fallback",
            "runtimeContext",
            "inputs",
            "requestInputs",
            "inputProvenance",
            "targetProvenance",
            "resolutionChain",
            "policy",
            "recordedAt");
        if (!record.TryGetProperty("recordedAt", out var recordedAt) ||
            recordedAt.ValueKind != JsonValueKind.String ||
            !IsValidPersistedTimestamp(recordedAt.GetString()))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }

        RequireNestedProperties(
            record,
            "fallback",
            "source",
            "resolutionFallbackUsed",
            "decisionFallbackUsed",
            "reason");
        RequireNestedProperties(
            record,
            "policy",
            "result",
            "reasons",
            "appliedConstraints");
        RequireNestedProperties(
            record,
            "contract",
            "definitionId",
            "contractDigest",
            "revision");
        if (record.GetProperty("contract").TryGetProperty(
                "contractDigest",
                out var contractDigest) &&
            (contractDigest.ValueKind != JsonValueKind.String ||
             !Sha256DigestPattern.IsMatch(contractDigest.GetString() ?? string.Empty)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }

        if (record.GetProperty("contract").TryGetProperty(
                "bundleDigest",
                out var bundleDigest) &&
            (bundleDigest.ValueKind != JsonValueKind.String ||
             !Sha256DigestPattern.IsMatch(bundleDigest.GetString() ?? string.Empty)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }

        var runtimeContext = record.GetProperty("runtimeContext");
        if (runtimeContext.ValueKind != JsonValueKind.Object ||
            runtimeContext.EnumerateObject().Any(
                property => string.IsNullOrWhiteSpace(property.Name) ||
                            !IsRuntimePrimitive(property.Value)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }

        var inputs = record.GetProperty("inputs");
        if (inputs.ValueKind != JsonValueKind.Object ||
            inputs.EnumerateObject().Any(
                input => string.IsNullOrWhiteSpace(input.Name) || !IsRuntimePrimitive(input.Value)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }

        if (record.TryGetProperty("confidence", out var confidence) &&
            confidence.ValueKind == JsonValueKind.Object)
        {
            RequireProperties(confidence, "evidenceQuality");
            ValidateProbabilityNumbers(
                confidence,
                "evidenceQuality",
                "modelUncertainty",
                "expectedOutcome");
        }
        if (record.TryGetProperty("evidence", out var evidence) &&
            evidence.ValueKind == JsonValueKind.Object)
        {
            RequireProperties(evidence, "evidenceQuality");
            ValidateProbabilityNumbers(
                evidence,
                "evidenceQuality",
                "modelUncertainty",
                "expectedOutcome");
            if (evidence.TryGetProperty("sampleSize", out var sampleSize) &&
                (!CanonicalJson.IsIeee754CompatibleNumber(sampleSize) ||
                 sampleSize.GetDouble() < 0))
            {
                throw new InvalidDataException(
                    "The local audit file contains an invalid decision record.");
            }
        }
    }

    private static void ValidateProbabilityNumbers(
        JsonElement value,
        params string[] propertyNames)
    {
        foreach (var propertyName in propertyNames)
        {
            if (value.TryGetProperty(propertyName, out var number) &&
                (!CanonicalJson.IsIeee754CompatibleNumber(number) ||
                 number.GetDouble() is < 0 or > 1))
            {
                throw new InvalidDataException(
                    "The local audit file contains an invalid decision record.");
            }
        }
    }

    private static void RequireNestedProperties(
        JsonElement parent,
        string propertyName,
        params string[] requiredProperties)
    {
        if (!parent.TryGetProperty(propertyName, out var nested) ||
            nested.ValueKind != JsonValueKind.Object)
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }
        RequireProperties(nested, requiredProperties);
    }

    private static void RequireProperties(
        JsonElement value,
        params string[] requiredProperties)
    {
        if (value.ValueKind != JsonValueKind.Object ||
            requiredProperties.Any(property => !value.TryGetProperty(property, out _)))
        {
            throw new InvalidDataException(
                "The local audit file contains an invalid decision record.");
        }
    }

    private static bool IsValidPersistedTimestamp(string? value) =>
        value is not null &&
        PersistedTimestampPattern.IsMatch(value) &&
        DateTimeOffset.TryParse(
            value,
            CultureInfo.InvariantCulture,
            DateTimeStyles.None,
            out var timestamp) &&
        timestamp != default;

    private static bool IsValidContract(RuntimeContractIdentity contract) =>
        !string.IsNullOrWhiteSpace(contract.DefinitionId) &&
        !string.IsNullOrWhiteSpace(contract.Revision) &&
        Sha256DigestPattern.IsMatch(contract.ContractDigest ?? string.Empty) &&
        (contract.BundleDigest is null ||
         Sha256DigestPattern.IsMatch(contract.BundleDigest));

    private static bool IsValidFallback(ServerFallbackInfo fallback) =>
        string.Equals(fallback.Source, "server", StringComparison.Ordinal);

    private static bool IsValidPolicy(PolicyEvaluationResult policy) =>
        (policy.Result is "approved" or "blocked" or "fallback") &&
        policy.Reasons is not null &&
        policy.AppliedConstraints is not null &&
        policy.Reasons.All(reason => !string.IsNullOrWhiteSpace(reason)) &&
        policy.AppliedConstraints.All(
            constraint => !string.IsNullOrWhiteSpace(constraint));

    private static bool IsValidRuntimeContext(
        IReadOnlyDictionary<string, JsonElement> runtimeContext) =>
        runtimeContext.All(
            item => !string.IsNullOrWhiteSpace(item.Key) &&
                    IsRuntimePrimitive(item.Value));

    private static bool IsValidInput(KeyValuePair<string, JsonElement> input) =>
        !string.IsNullOrWhiteSpace(input.Key) &&
        IsRuntimePrimitive(input.Value);

    private static bool IsRuntimePrimitive(JsonElement value) => DecisionValues.IsScalar(value);

    private static bool IsValidInputProvenance(DecisionAuditRecord record)
    {
        var provenance = record.InputProvenance!;
        var requested = record.RequestInputs!;
        if (provenance.Count != record.Inputs.Count ||
            requested.Any(input => !record.Inputs.TryGetValue(input.Key, out var resolved) ||
                !JsonElement.DeepEquals(input.Value, resolved)))
            return false;
        foreach (var key in record.Inputs.Keys)
        {
            if (!provenance.TryGetValue(key, out var source) || source is null) return false;
            if (source.Source == "request")
            {
                if (!requested.ContainsKey(key) || source != new InputProvenance("request")) return false;
            }
            else if (source.Source != "evidence" || requested.ContainsKey(key) ||
                string.IsNullOrWhiteSpace(source.Binding) || string.IsNullOrWhiteSpace(source.Generation) ||
                !ulong.TryParse(source.ObservedTimeUnixNano, NumberStyles.None, CultureInfo.InvariantCulture, out var timestamp) ||
                timestamp == 0 || source.MaterializedAt is null || source.Coverage != "observed" ||
                !Sha256DigestPattern.IsMatch(source.SourceFingerprint ?? "") ||
                !IsValidTargetProvenance(source.TargetResolution) ||
                source.ExposureId is not null && string.IsNullOrWhiteSpace(source.ExposureId))
            {
                return false;
            }
        }
        return true;
    }

    private static bool IsValidTarget(DecisionTargetRef? target) =>
        target is null ||
        !string.IsNullOrWhiteSpace(target.Type) &&
        !string.IsNullOrWhiteSpace(target.Id);

    private static bool IsValidTargetProvenance(
        TargetResolutionProvenance? provenance) =>
        provenance is not null &&
        !string.IsNullOrWhiteSpace(provenance.TargetType) &&
        !string.IsNullOrWhiteSpace(provenance.ResolvedId) &&
        provenance.Source is
            "client-claimed" or
            "client-verified" or
            "server-derived" or
            "server-replaced";

    private static bool IsValidEvidence(DecisionEvidenceSnapshot evidence) =>
        IsProbability(evidence.EvidenceQuality) &&
        (evidence.ModelUncertainty is null ||
         IsProbability(evidence.ModelUncertainty.Value)) &&
        (evidence.ExpectedOutcome is null ||
         IsProbability(evidence.ExpectedOutcome.Value)) &&
        (evidence.SampleSize is null ||
         double.IsFinite(evidence.SampleSize.Value) &&
         evidence.SampleSize.Value >= 0);

    private static bool ValueMatches(string valueType, JsonElement value) =>
        valueType switch
        {
            "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "number" => value.ValueKind == JsonValueKind.Number &&
                        CanonicalJson.IsIeee754CompatibleNumber(value),
            "string" => value.ValueKind == JsonValueKind.String,
            _ => false
        };

    private static bool IsValidDecisionSemantics(DecisionAuditRecord record) =>
        record.DecisionMode switch
        {
            "active-value" =>
                record.StrategyId is null &&
                record.Evidence is null &&
                record.Confidence is null &&
                !record.Fallback.DecisionFallbackUsed &&
                record.Policy.Result == "approved",
            "strategy" or "experiment" =>
                !string.IsNullOrWhiteSpace(record.StrategyId) &&
                (record.Confidence is null ||
                 record.Evidence is not null &&
                 IsValidConfidence(record.Confidence) &&
                 IsConfidenceConsistent(record.Evidence, record.Confidence)) &&
                !record.Fallback.DecisionFallbackUsed &&
                record.Policy.Result == "approved",
            "fallback" =>
                (record.StrategyId is null ||
                 !string.IsNullOrWhiteSpace(record.StrategyId)) &&
                (record.StrategyId is not null ||
                 record.Evidence is null) &&
                record.Confidence is null &&
                record.Fallback.DecisionFallbackUsed &&
                record.Policy.Result is "blocked" or "fallback",
            _ => false
        };

    private static bool IsValidConfidence(ConfidenceReport? confidence) =>
        confidence is not null &&
        IsProbability(confidence.EvidenceQuality) &&
        (confidence.ModelUncertainty is null ||
         IsProbability(confidence.ModelUncertainty.Value)) &&
        (confidence.ExpectedOutcome is null ||
         IsProbability(confidence.ExpectedOutcome.Value));

    private static bool IsConfidenceConsistent(
        DecisionEvidenceSnapshot evidence,
        ConfidenceReport confidence) =>
        evidence.EvidenceQuality.Equals(confidence.EvidenceQuality) &&
        Nullable.Equals(
            evidence.ModelUncertainty,
            confidence.ModelUncertainty) &&
        Nullable.Equals(
            evidence.ExpectedOutcome,
            confidence.ExpectedOutcome);

    private static bool IsProbability(double value) =>
        double.IsFinite(value) && value is >= 0 and <= 1;

    private sealed record AuditEnvelope<T>(string Kind, T Record);

    private sealed record SegmentHeaderEnvelope(string Kind, SegmentHeader Record);

    private sealed record SegmentHeader(int Version, long Sequence, string SegmentId);

    private sealed record ExposureMarker(
        int Version,
        string ExposureId,
        string SegmentId,
        long SegmentSequence,
        int RecordIndex,
        string RecordHash);

    private sealed record SegmentState(
        SegmentHeader Header,
        long Length,
        string ContentHash,
        IReadOnlyList<SegmentRecord> Records);

    private sealed record SegmentRecord(int Index, string? ExposureId, string Hash);

    private sealed record LegacyRecord(byte[] Bytes, string? ExposureId);

}
