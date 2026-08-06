using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

internal sealed class SegmentedAuditStore
{
    private const int ManifestFormatVersion = 1;
    private const int SegmentFormatVersion = 1;
    private const int MarkerFormatVersion = 1;
    private const string ManifestFileName = "manifest.json";
    private const string RecordSeparator = "\n";

    private static readonly Regex SegmentFileNamePattern = new(
        "\\Asegment-(?<sequence>\\d{20})-(?<id>[0-9a-f]{32})\\.jsonl\\z",
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

    private readonly string _legacyFilePath;
    private readonly string _auditDirectoryPath;
    private readonly string _segmentsDirectoryPath;
    private readonly string _markersDirectoryPath;
    private readonly string _manifestPath;
    private readonly int _maximumSegmentRecords;
    private readonly long _maximumSegmentBytes;
    private readonly Func<byte[], string?> _validateRecord;

    public SegmentedAuditStore(
        string legacyFilePath,
        string auditDirectoryPath,
        string segmentsDirectoryPath,
        string markersDirectoryPath,
        int maximumSegmentRecords,
        long maximumSegmentBytes,
        Func<byte[], string?> validateRecord)
    {
        _legacyFilePath = legacyFilePath;
        _auditDirectoryPath = auditDirectoryPath;
        _segmentsDirectoryPath = segmentsDirectoryPath;
        _markersDirectoryPath = markersDirectoryPath;
        _manifestPath = Path.Combine(auditDirectoryPath, ManifestFileName);
        _maximumSegmentRecords = maximumSegmentRecords;
        _maximumSegmentBytes = maximumSegmentBytes;
        _validateRecord = validateRecord;
    }

    public int LastAppendValidatedRecordCount { get; private set; }

    public Action? AfterNewSegmentDurablyFlushed { get; set; }

    public Action? AfterRotationManifestDurablyFlushed { get; set; }

    public async Task EnsureLayoutAsync(CancellationToken cancellationToken)
    {
        EnsureParentDirectory();
        DeleteAbandonedInitializationDirectories();

        if (Directory.Exists(_auditDirectoryPath))
        {
            if (File.Exists(_legacyFilePath))
            {
                throw new InvalidDataException(
                    "The local audit store contains both legacy and segmented layouts.");
            }
            RequireExistingLayout();
            CleanupTemporaryManifestFiles();
            return;
        }

        if (File.Exists(_legacyFilePath))
        {
            throw new InvalidDataException(
                "Legacy local audit files are not migrated automatically.");
        }

        await InitializeFreshStoreAsync(cancellationToken);
    }

    public async Task AppendAsync(
        byte[] recordBytes,
        string? exposureId,
        Action? afterRecordCommitted,
        CancellationToken cancellationToken)
    {
        var manifest = await ReadManifestAsync(cancellationToken);
        var currentCatalog = manifest.Segments.Single(segment => segment.Current);
        var current = await ValidateSegmentAsync(
            currentCatalog,
            cancellationToken);
        LastAppendValidatedRecordCount = current.Records.Count;

        if (exposureId is not null)
        {
            var existing = manifest.Exposures.SingleOrDefault(item =>
                string.Equals(item.ExposureId, exposureId, StringComparison.Ordinal));
            if (existing is not null)
            {
                await VerifyExposureReferenceAsync(
                    manifest,
                    existing,
                    cancellationToken);
                await EnsureMarkerAsync(
                    MarkerFor(existing),
                    allowMissing: true,
                    cancellationToken);
                return;
            }

            if (File.Exists(GetMarkerPath(exposureId)))
            {
                throw new InvalidDataException(
                    "The local audit exposure marker is not cataloged.");
            }
        }

        if (recordBytes.LongLength + HeaderBytes(current.Header).LongLength >
            _maximumSegmentBytes)
        {
            throw new InvalidDataException(
                "The audit record exceeds the configured segment byte limit.");
        }

        if (current.Records.Count >= _maximumSegmentRecords ||
            current.Bytes.LongLength + recordBytes.LongLength > _maximumSegmentBytes)
        {
            (manifest, currentCatalog, current) = await RotateAsync(
                manifest,
                currentCatalog,
                cancellationToken);
        }

        var recordIndex = current.Records.Count;
        await AppendDurablyAsync(
            Path.Combine(_segmentsDirectoryPath, currentCatalog.FileName),
            current.Bytes.LongLength,
            currentCatalog.ContentHash,
            recordBytes,
            cancellationToken);

        var committedBytes = new byte[current.Bytes.Length + recordBytes.Length];
        current.Bytes.CopyTo(committedBytes, 0);
        recordBytes.CopyTo(committedBytes, current.Bytes.Length);
        var updatedCatalog = currentCatalog with
        {
            Length = committedBytes.LongLength,
            RecordCount = checked(currentCatalog.RecordCount + 1),
            ContentHash = Hash(committedBytes)
        };
        var updatedSegments = manifest.Segments
            .Select(item => item.Sequence == updatedCatalog.Sequence
                ? updatedCatalog
                : item)
            .ToArray();
        var updatedExposures = manifest.Exposures.ToList();
        ExposureCatalogEntry? exposure = null;
        if (exposureId is not null)
        {
            exposure = new ExposureCatalogEntry(
                exposureId,
                updatedCatalog.Sequence,
                updatedCatalog.SegmentId,
                recordIndex,
                Hash(recordBytes));
            updatedExposures.Add(exposure);
        }
        manifest = manifest with
        {
            Generation = checked(manifest.Generation + 1),
            Segments = updatedSegments,
            Exposures = updatedExposures
        };
        await WriteManifestAtomicAsync(manifest, cancellationToken);
        afterRecordCommitted?.Invoke();

        if (exposure is not null)
        {
            await EnsureMarkerAsync(
                MarkerFor(exposure),
                allowMissing: true,
                cancellationToken);
        }
    }

    public async Task ValidateReadinessAsync(CancellationToken cancellationToken)
    {
        CleanupTemporaryMarkerFiles();
        var manifest = await ReadManifestAsync(cancellationToken);
        var expectedExposures = new Dictionary<string, ExposureCatalogEntry>(
            StringComparer.Ordinal);
        var validatedSegments = new Dictionary<long, SegmentState>();

        foreach (var catalog in manifest.Segments)
        {
            var segment = await ValidateSegmentAsync(catalog, cancellationToken);
            validatedSegments.Add(catalog.Sequence, segment);
            foreach (var record in segment.Records)
            {
                if (record.ExposureId is null)
                {
                    continue;
                }
                var exposure = new ExposureCatalogEntry(
                    record.ExposureId,
                    catalog.Sequence,
                    catalog.SegmentId,
                    record.Index,
                    record.Hash);
                if (!expectedExposures.TryAdd(record.ExposureId, exposure))
                {
                    throw new InvalidDataException(
                        "The local audit segments contain a duplicate exposure id.");
                }
            }
        }

        if (manifest.Exposures.Count != expectedExposures.Count)
        {
            throw new InvalidDataException(
                "The local audit manifest exposure catalog is incomplete.");
        }
        foreach (var exposure in manifest.Exposures)
        {
            if (!expectedExposures.TryGetValue(exposure.ExposureId, out var expected) ||
                exposure != expected)
            {
                throw new InvalidDataException(
                    "The local audit manifest exposure catalog is corrupt.");
            }
        }

        foreach (var exposure in manifest.Exposures)
        {
            var marker = MarkerFor(exposure);
            var path = GetMarkerPath(exposure.ExposureId);
            if (!File.Exists(path))
            {
                await WriteMarkerAtomicAsync(marker, cancellationToken);
                continue;
            }

            var persisted = await ReadMarkerAsync(path, cancellationToken);
            if (persisted != marker)
            {
                throw new InvalidDataException(
                    "The local audit exposure marker does not reference its committed record.");
            }
            VerifyMarkerAgainstValidatedSegments(
                persisted,
                validatedSegments);
        }

        var expectedMarkerPaths = manifest.Exposures
            .Select(item => GetMarkerPath(item.ExposureId))
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var path in Directory.EnumerateFiles(
                     _markersDirectoryPath,
                     "*",
                     SearchOption.TopDirectoryOnly))
        {
            if (!expectedMarkerPaths.Contains(path))
            {
                throw new InvalidDataException(
                    "The local audit exposure marker directory contains an unreferenced file.");
            }
        }
    }

    private async Task InitializeFreshStoreAsync(
        CancellationToken cancellationToken)
    {
        var stagingPath = $"{_auditDirectoryPath}.tmp-{Guid.NewGuid():N}";
        try
        {
            var stagingSegments = Path.Combine(stagingPath, "segments");
            var stagingMarkers = Path.Combine(stagingPath, "exposures");
            Directory.CreateDirectory(stagingSegments);
            Directory.CreateDirectory(stagingMarkers);

            var header = NewHeader(1);
            var bytes = HeaderBytes(header);
            var fileName = SegmentFileName(header);
            await WriteDurableFileAsync(
                Path.Combine(stagingSegments, fileName),
                bytes,
                FileMode.CreateNew,
                cancellationToken);
            var manifest = new AuditManifest(
                ManifestFormatVersion,
                Guid.NewGuid().ToString("N"),
                1,
                [
                    new SegmentCatalogEntry(
                        header.Sequence,
                        header.SegmentId,
                        fileName,
                        true,
                        bytes.LongLength,
                        0,
                        Hash(bytes))
                ],
                []);
            await WriteManifestFileAsync(
                Path.Combine(stagingPath, ManifestFileName),
                manifest,
                FileMode.CreateNew,
                cancellationToken);
            FlushDirectory(stagingSegments);
            FlushDirectory(stagingMarkers);
            FlushDirectory(stagingPath);
            Directory.Move(stagingPath, _auditDirectoryPath);
            FlushDirectory(Path.GetDirectoryName(_auditDirectoryPath)!);
        }
        finally
        {
            if (Directory.Exists(stagingPath))
            {
                Directory.Delete(stagingPath, recursive: true);
            }
        }
    }

    private async Task<(AuditManifest Manifest, SegmentCatalogEntry Catalog, SegmentState State)>
        RotateAsync(
            AuditManifest manifest,
            SegmentCatalogEntry currentCatalog,
            CancellationToken cancellationToken)
    {
        var nextHeader = NewHeader(checked(currentCatalog.Sequence + 1));
        var nextBytes = HeaderBytes(nextHeader);
        var nextCatalog = new SegmentCatalogEntry(
            nextHeader.Sequence,
            nextHeader.SegmentId,
            SegmentFileName(nextHeader),
            true,
            nextBytes.LongLength,
            0,
            Hash(nextBytes));
        await WriteDurableFileAsync(
            Path.Combine(_segmentsDirectoryPath, nextCatalog.FileName),
            nextBytes,
            FileMode.CreateNew,
            cancellationToken);
        FlushDirectory(_segmentsDirectoryPath);
        AfterNewSegmentDurablyFlushed?.Invoke();

        var updatedSegments = manifest.Segments
            .Select(item => item.Sequence == currentCatalog.Sequence
                ? item with { Current = false }
                : item)
            .Append(nextCatalog)
            .ToArray();
        var updatedManifest = manifest with
        {
            Generation = checked(manifest.Generation + 1),
            Segments = updatedSegments
        };
        await WriteManifestAtomicAsync(updatedManifest, cancellationToken);
        AfterRotationManifestDurablyFlushed?.Invoke();
        return (
            updatedManifest,
            nextCatalog,
            new SegmentState(
                nextHeader,
                nextBytes,
                []));
    }

    private async Task<AuditManifest> ReadManifestAsync(
        CancellationToken cancellationToken)
    {
        RequireExistingLayout();
        try
        {
            var bytes = await File.ReadAllBytesAsync(
                _manifestPath,
                cancellationToken);
            StrictJson.Validate(bytes);
            var manifest = JsonSerializer.Deserialize<AuditManifest>(
                bytes,
                StrictJsonOptions);
            ValidateManifest(manifest);
            ValidateSegmentDirectory(manifest!);
            return manifest!;
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The local audit manifest is corrupt and is not rebuilt automatically.",
                error);
        }
    }

    private static void ValidateManifest(AuditManifest? manifest)
    {
        if (manifest is null ||
            manifest.Version != ManifestFormatVersion ||
            !Guid.TryParseExact(manifest.StoreId, "N", out _) ||
            manifest.Generation <= 0 ||
            manifest.Segments is not { Count: > 0 } ||
            manifest.Exposures is null)
        {
            throw new InvalidDataException(
                "The local audit manifest is corrupt and is not rebuilt automatically.");
        }

        var currentCount = 0;
        var segmentIds = new HashSet<string>(StringComparer.Ordinal);
        var fileNames = new HashSet<string>(StringComparer.Ordinal);
        for (var index = 0; index < manifest.Segments.Count; index++)
        {
            var segment = manifest.Segments[index];
            var expectedSequence = index + 1L;
            if (segment is null ||
                segment.Sequence != expectedSequence ||
                !Guid.TryParseExact(segment.SegmentId, "N", out _) ||
                !string.Equals(
                    segment.FileName,
                    $"segment-{segment.Sequence:D20}-{segment.SegmentId}.jsonl",
                    StringComparison.Ordinal) ||
                !SegmentFileNamePattern.IsMatch(segment.FileName) ||
                !segmentIds.Add(segment.SegmentId) ||
                !fileNames.Add(segment.FileName) ||
                segment.Length <= 0 ||
                segment.RecordCount < 0 ||
                !Sha256DigestPattern.IsMatch(segment.ContentHash))
            {
                throw new InvalidDataException(
                    "The local audit manifest contains invalid segment metadata.");
            }
            if (segment.Current)
            {
                currentCount++;
                if (index != manifest.Segments.Count - 1)
                {
                    throw new InvalidDataException(
                        "The local audit manifest current segment is not the final generation.");
                }
            }
        }
        if (currentCount != 1)
        {
            throw new InvalidDataException(
                "The local audit manifest must identify exactly one current segment.");
        }

        var exposureIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var exposure in manifest.Exposures)
        {
            var segment = exposure is null
                ? null
                : manifest.Segments.SingleOrDefault(item =>
                    item.Sequence == exposure.SegmentSequence);
            if (exposure is null ||
                string.IsNullOrWhiteSpace(exposure.ExposureId) ||
                !exposureIds.Add(exposure.ExposureId) ||
                segment is null ||
                !string.Equals(
                    segment.SegmentId,
                    exposure.SegmentId,
                    StringComparison.Ordinal) ||
                exposure.RecordIndex < 0 ||
                exposure.RecordIndex >= segment.RecordCount ||
                !Sha256DigestPattern.IsMatch(exposure.RecordHash))
            {
                throw new InvalidDataException(
                    "The local audit manifest contains invalid exposure metadata.");
            }
        }
    }

    private void ValidateSegmentDirectory(AuditManifest manifest)
    {
        var expected = manifest.Segments
            .Select(item => item.FileName)
            .ToHashSet(StringComparer.Ordinal);
        var actual = Directory.EnumerateFiles(
                _segmentsDirectoryPath,
                "*",
                SearchOption.TopDirectoryOnly)
        .Select(path => Path.GetFileName(path)!)
            .ToHashSet(StringComparer.Ordinal);
        if (!expected.SetEquals(actual))
        {
            throw new InvalidDataException(
                "The local audit segment directory contains missing or unlisted segments.");
        }
    }

    private async Task<SegmentState> ValidateSegmentAsync(
        SegmentCatalogEntry catalog,
        CancellationToken cancellationToken)
    {
        var path = Path.Combine(_segmentsDirectoryPath, catalog.FileName);
        var info = new FileInfo(path);
        if (!info.Exists ||
            info.Length != catalog.Length ||
            info.Length > _maximumSegmentBytes)
        {
            throw new InvalidDataException(
                "A listed local audit segment is missing or has an invalid length.");
        }
        var bytes = await File.ReadAllBytesAsync(path, cancellationToken);
        if (!string.Equals(Hash(bytes), catalog.ContentHash, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "A listed local audit segment does not match its manifest hash.");
        }
        var lines = SplitLines(bytes, "audit segment");
        if (lines.Count == 0)
        {
            throw new InvalidDataException("The local audit segment is empty.");
        }
        var header = ParseHeader(lines[0]);
        if (header.Sequence != catalog.Sequence ||
            !string.Equals(header.SegmentId, catalog.SegmentId, StringComparison.Ordinal) ||
            lines.Count - 1 != catalog.RecordCount ||
            catalog.RecordCount > _maximumSegmentRecords)
        {
            throw new InvalidDataException(
                "The local audit segment does not match its manifest metadata.");
        }

        var records = new List<SegmentRecord>(catalog.RecordCount);
        for (var index = 1; index < lines.Count; index++)
        {
            records.Add(
                new SegmentRecord(
                    index - 1,
                    _validateRecord(lines[index]),
                    Hash(lines[index])));
        }
        return new SegmentState(header, bytes, records);
    }

    private async Task VerifyExposureReferenceAsync(
        AuditManifest manifest,
        ExposureCatalogEntry exposure,
        CancellationToken cancellationToken)
    {
        var catalog = manifest.Segments.Single(segment =>
            segment.Sequence == exposure.SegmentSequence);
        var segment = await ValidateSegmentAsync(catalog, cancellationToken);
        VerifyExposureRecord(exposure, segment);
    }

    private static void VerifyExposureRecord(
        ExposureCatalogEntry exposure,
        SegmentState segment)
    {
        if (exposure.RecordIndex >= segment.Records.Count)
        {
            throw new InvalidDataException(
                "The local audit exposure reference points outside its segment.");
        }
        var record = segment.Records[exposure.RecordIndex];
        if (!string.Equals(
                record.ExposureId,
                exposure.ExposureId,
                StringComparison.Ordinal) ||
            !string.Equals(record.Hash, exposure.RecordHash, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The local audit exposure reference does not match its audit record.");
        }
    }

    private static void VerifyMarkerAgainstValidatedSegments(
        ExposureMarker marker,
        IReadOnlyDictionary<long, SegmentState> segments)
    {
        if (!segments.TryGetValue(marker.SegmentSequence, out var segment) ||
            !string.Equals(
                segment.Header.SegmentId,
                marker.SegmentId,
                StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The local audit exposure marker references a missing segment.");
        }
        VerifyExposureRecord(
            new ExposureCatalogEntry(
                marker.ExposureId,
                marker.SegmentSequence,
                marker.SegmentId,
                marker.RecordIndex,
                marker.RecordHash),
            segment);
    }

    private async Task EnsureMarkerAsync(
        ExposureMarker expected,
        bool allowMissing,
        CancellationToken cancellationToken)
    {
        var path = GetMarkerPath(expected.ExposureId);
        if (!File.Exists(path))
        {
            if (!allowMissing)
            {
                throw new InvalidDataException(
                    "The local audit exposure marker is missing.");
            }
            await WriteMarkerAtomicAsync(expected, cancellationToken);
            return;
        }

        var marker = await ReadMarkerAsync(path, cancellationToken);
        if (marker != expected)
        {
            throw new InvalidDataException(
                "The local audit exposure marker does not reference its committed record.");
        }
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

    private async Task WriteMarkerAtomicAsync(
        ExposureMarker marker,
        CancellationToken cancellationToken)
    {
        var finalPath = GetMarkerPath(marker.ExposureId);
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
            File.Move(stagingPath, finalPath, overwrite: false);
            FlushDirectory(_markersDirectoryPath);
        }
        finally
        {
            File.Delete(stagingPath);
        }
    }

    private async Task WriteManifestAtomicAsync(
        AuditManifest manifest,
        CancellationToken cancellationToken)
    {
        var stagingPath = $"{_manifestPath}.tmp-{Guid.NewGuid():N}";
        try
        {
            await WriteManifestFileAsync(
                stagingPath,
                manifest,
                FileMode.CreateNew,
                cancellationToken);
            File.Move(stagingPath, _manifestPath, overwrite: true);
            FlushDirectory(_auditDirectoryPath);
        }
        finally
        {
            File.Delete(stagingPath);
        }
    }

    private static async Task WriteManifestFileAsync(
        string path,
        AuditManifest manifest,
        FileMode mode,
        CancellationToken cancellationToken)
    {
        var bytes = Encoding.UTF8.GetBytes(
            $"{JsonSerializer.Serialize(manifest, JsonOptions)}{RecordSeparator}");
        StrictJson.Validate(bytes);
        await WriteDurableFileAsync(path, bytes, mode, cancellationToken);
    }

    private static SegmentHeader ParseHeader(byte[] rawLine)
    {
        try
        {
            var line = TrimLineEnding(rawLine);
            StrictJson.Validate(line);
            var envelope = JsonSerializer.Deserialize<SegmentHeaderEnvelope>(
                line,
                StrictJsonOptions);
            if (envelope is null ||
                !string.Equals(envelope.Kind, "segment", StringComparison.Ordinal) ||
                envelope.Record.Version != SegmentFormatVersion ||
                envelope.Record.Sequence <= 0 ||
                !Guid.TryParseExact(envelope.Record.SegmentId, "N", out _))
            {
                throw new InvalidDataException(
                    "The local audit segment contains an invalid header.");
            }
            return envelope.Record;
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The local audit segment contains an invalid header.",
                error);
        }
    }

    private void RequireExistingLayout()
    {
        if (!Directory.Exists(_auditDirectoryPath) ||
            !Directory.Exists(_segmentsDirectoryPath) ||
            !Directory.Exists(_markersDirectoryPath) ||
            !File.Exists(_manifestPath))
        {
            throw new InvalidDataException(
                "The local audit segmented layout is incomplete.");
        }
    }

    private void EnsureParentDirectory()
    {
        var parent = Path.GetDirectoryName(_legacyFilePath)
            ?? throw new IOException("The local audit path has no parent directory.");
        Directory.CreateDirectory(parent);
    }

    private void DeleteAbandonedInitializationDirectories()
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

    private void CleanupTemporaryManifestFiles()
    {
        foreach (var path in Directory.EnumerateFiles(
                     _auditDirectoryPath,
                     $"{ManifestFileName}.tmp-*",
                     SearchOption.TopDirectoryOnly))
        {
            File.Delete(path);
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
        await stream.ReadExactlyAsync(currentBytes, cancellationToken);
        if (!string.Equals(Hash(currentBytes), expectedHash, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                "The current audit segment changed while it was validated.");
        }
        stream.Seek(0, SeekOrigin.End);
        await stream.WriteAsync(bytes, cancellationToken);
        await stream.FlushAsync(cancellationToken);
        stream.Flush(flushToDisk: true);
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
            var line = new byte[index - start + 1];
            Array.Copy(bytes, start, line, 0, line.Length);
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

    private static SegmentHeader NewHeader(long sequence) =>
        new(SegmentFormatVersion, sequence, Guid.NewGuid().ToString("N"));

    private static byte[] HeaderBytes(SegmentHeader header) =>
        Encoding.UTF8.GetBytes(
            $"{JsonSerializer.Serialize(new SegmentHeaderEnvelope("segment", header), JsonOptions)}{RecordSeparator}");

    private static string SegmentFileName(SegmentHeader header) =>
        $"segment-{header.Sequence:D20}-{header.SegmentId}.jsonl";

    private static ExposureMarker MarkerFor(ExposureCatalogEntry exposure) =>
        new(
            MarkerFormatVersion,
            exposure.ExposureId,
            exposure.SegmentId,
            exposure.SegmentSequence,
            exposure.RecordIndex,
            exposure.RecordHash);

    private string GetMarkerPath(string exposureId) =>
        Path.Combine(_markersDirectoryPath, MarkerFileName(exposureId));

    private static string MarkerFileName(string exposureId) =>
        $"{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(exposureId))).ToLowerInvariant()}.json";

    private static string Hash(byte[] value) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(value)).ToLowerInvariant()}";

    private static void FlushDirectory(string path)
    {
        if (OperatingSystem.IsWindows())
        {
            return;
        }
        using var handle = File.OpenHandle(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete);
        RandomAccess.FlushToDisk(handle);
    }

    private sealed record AuditManifest(
        int Version,
        string StoreId,
        long Generation,
        IReadOnlyList<SegmentCatalogEntry> Segments,
        IReadOnlyList<ExposureCatalogEntry> Exposures);

    private sealed record SegmentCatalogEntry(
        long Sequence,
        string SegmentId,
        string FileName,
        bool Current,
        long Length,
        int RecordCount,
        string ContentHash);

    private sealed record ExposureCatalogEntry(
        string ExposureId,
        long SegmentSequence,
        string SegmentId,
        int RecordIndex,
        string RecordHash);

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
        byte[] Bytes,
        IReadOnlyList<SegmentRecord> Records);

    private sealed record SegmentRecord(int Index, string? ExposureId, string Hash);
}
