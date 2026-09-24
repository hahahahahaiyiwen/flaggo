using System.Collections.Frozen;
using System.Globalization;
using System.Numerics;
using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Evidence;

public sealed record InputEvidenceLimits(int MaximumRecords = 1_000, int MaximumFrames = 10_000, int MaximumBindings = 1_000);

public sealed class InputEvidenceMaterializer : IInputTelemetrySink, IInputEvidenceReader, IAsyncDisposable
{
    private readonly IEvidenceBindingReader _bindings;
    private readonly IConfirmedExposureReader _exposures;
    private readonly IInputEvidenceSnapshotStore _store;
    private readonly TimeProvider _clock;
    private readonly InputEvidenceLimits _limits;
    private readonly SemaphoreSlim _writer = new(1, 1);
    private SnapshotView? _view;

    public InputEvidenceMaterializer(
        IEvidenceBindingReader bindings,
        IConfirmedExposureReader exposures,
        IInputEvidenceSnapshotStore store,
        TimeProvider clock,
        InputEvidenceLimits? limits = null)
    {
        _bindings = bindings;
        _exposures = exposures;
        _store = store;
        _clock = clock;
        _limits = limits ?? new();
        if (_limits.MaximumRecords <= 0 || _limits.MaximumFrames <= 0 || _limits.MaximumBindings <= 0)
            throw new ArgumentOutOfRangeException(nameof(limits), "Materialization limits must be positive.");
    }

    public bool IsInitialized => Volatile.Read(ref _view) is not null;

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await _writer.WaitAsync(cancellationToken);
        try { await LoadAsync(cancellationToken); }
        finally { _writer.Release(); }
    }

    public async Task<TelemetryIngestResult> IngestAsync(
        ApplicationScope scope,
        IReadOnlyList<TelemetryObservation> observations,
        CancellationToken cancellationToken)
    {
        if (observations.Sum(observation => 1L + (observation.Events?.Count ?? 0)) > _limits.MaximumRecords)
            throw new InputEvidenceCapacityException("The telemetry record limit was exceeded.");
        IReadOnlyList<EvidenceBindingProjection> projections;
        try
        {
            projections = await _bindings.ReadBindingsAsync(scope, cancellationToken);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or InvalidDataException or JsonException)
        {
            throw new InputEvidenceUnavailableException("Approved evidence bindings are unavailable.", error);
        }
        if (projections.Any(projection => projection.Scope != scope))
            throw new InputEvidenceUnavailableException("The registry returned a projection outside the authorized scope.");
        if (projections.Sum(projection => (long)projection.Bindings.Count) > _limits.MaximumBindings)
            throw new InputEvidenceCapacityException("The active binding limit was exceeded.");
        await _writer.WaitAsync(cancellationToken);
        try
        {
            if (_view is null) await LoadAsync(cancellationToken);
            var before = _view!;
            var now = _clock.GetUtcNow();
            var frames = before.Snapshot.Frames
                .Where(frame => Freshness(frame, now, frame.MaxAgeSeconds) != "stale")
                .ToDictionary(frame => (frame.Key, frame.Stream));
            var changed = frames.Count != before.Snapshot.Frames.Count;
            var accepted = 0;
            var rejected = 0;
            var unmatched = 0;
            var diagnostics = new HashSet<string>(StringComparer.Ordinal);
            foreach (var observation in observations)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var matchedRecord = false;
                var acceptedRecord = false;
                foreach (var definition in projections)
                foreach (var binding in definition.Bindings)
                {
                    if (!TelemetryProjection.Matches(observation, binding)) continue;
                    foreach (var spanEvent in TelemetryProjection.SelectEvents(observation, binding))
                    {
                        cancellationToken.ThrowIfCancellationRequested();
                        matchedRecord = true;
                        var projection = await TelemetryProjection.ProjectAsync(
                            scope, definition, binding, observation, spanEvent, now, frames, _exposures, cancellationToken);
                        acceptedRecord |= projection.Error is null;
                        if (projection.Error is not null && diagnostics.Count < 16)
                            diagnostics.Add($"{binding.Key}: {projection.Error}");
                        if (projection.Frame is not InputEvidenceFrame candidate) continue;
                        var identity = (candidate.Key, candidate.Stream);
                        if (!frames.TryGetValue(identity, out var previous))
                        {
                            if (frames.Count >= _limits.MaximumFrames)
                                throw new InputEvidenceCapacityException("The active binding/target frame limit was exceeded.");
                            frames.Add(identity, candidate);
                            changed = true;
                            continue;
                        }
                        var comparison = Timestamp(candidate).CompareTo(Timestamp(previous));
                        if (comparison < 0) continue;
                        if (comparison == 0)
                        {
                            if (previous.Status == "ambiguous") continue;
                            var equivalent = previous.Status == candidate.Status &&
                                NullableValueEquals(previous.Value, candidate.Value) && previous.ExposureId == candidate.ExposureId;
                            if (equivalent)
                            {
                                if (string.CompareOrdinal(candidate.Fingerprint, previous.Fingerprint) >= 0) continue;
                                candidate = candidate with { MaterializedAt = previous.MaterializedAt };
                            }
                            else
                            {
                                candidate = candidate with { Status = "ambiguous", Value = null };
                            }
                        }
                        frames[identity] = candidate;
                        changed = true;
                    }
                }
                if (!matchedRecord) unmatched++;
                else if (acceptedRecord) accepted++;
                else rejected++;
            }
            if (changed)
            {
                var snapshot = new InputEvidenceSnapshot(1, Guid.NewGuid().ToString("N"), frames.Values.ToArray());
                try
                {
                    await _store.PublishAsync(snapshot, cancellationToken);
                    Volatile.Write(ref _view, new SnapshotView(snapshot));
                }
                catch (InputEvidenceCapacityException)
                {
                    throw;
                }
                catch
                {
                    Volatile.Write(ref _view, null);
                    throw;
                }
            }
            return new(accepted, rejected, unmatched, diagnostics.Order(StringComparer.Ordinal).ToArray());
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or InvalidDataException or JsonException)
        {
            throw new InputEvidenceUnavailableException("Input evidence could not be durably materialized.", error);
        }
        finally { _writer.Release(); }
    }

    public Task<InputEvidenceResult> ReadInputsAsync(InputEvidenceRequest request, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var view = Volatile.Read(ref _view)
            ?? throw new InputEvidenceUnavailableException("A verified materialized input generation is unavailable.");
        var values = new Dictionary<string, InputEvidenceValue>(StringComparer.Ordinal);
        foreach (var input in request.Inputs)
        {
            var key = new InputEvidenceKey(request.Scope, request.Definition.DefinitionId,
                request.Definition.Revision, request.Definition.ContractDigest, input.Binding.Key, input.Target);
            if (!view.Frames.TryGetValue(key, out var candidates))
            {
                values.Add(input.InputKey, new(null, "missing", new InputProvenance(
                    "evidence", input.Binding.Key, view.Snapshot.Generation, Coverage: "observed")));
                continue;
            }
            var fresh = candidates.Where(frame =>
                Freshness(frame, request.EvaluatedAt, input.Binding.MaxAgeSeconds) == "fresh").ToArray();
            var future = candidates.Where(frame =>
                Freshness(frame, request.EvaluatedAt, input.Binding.MaxAgeSeconds) == "future").ToArray();
            var frame = future.Length > 0 ? future.MaxBy(Timestamp)! :
                fresh.Length == 1 ? fresh[0] : candidates.MaxBy(Timestamp)!;
            var freshness = Freshness(frame, request.EvaluatedAt, input.Binding.MaxAgeSeconds);
            var status = future.Length > 0 ? "future" : fresh.Length > 1 ? "ambiguous" :
                freshness != "fresh" ? freshness : frame.Status;
            var provenance = new InputProvenance(
                "evidence", input.Binding.Key, view.Snapshot.Generation, frame.TimeUnixNano,
                frame.MaterializedAt, "observed", frame.Fingerprint, frame.TraceId, frame.SpanId,
                frame.SamplingFlags, frame.ExposureId);
            values.Add(input.InputKey, new(status == "available" ? frame.Value : null, status, provenance));
        }
        return Task.FromResult(new InputEvidenceResult(view.Snapshot.Generation, values.ToFrozenDictionary(StringComparer.Ordinal)));
    }

    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            var snapshot = await _store.LoadAsync(cancellationToken);
            if (snapshot.Version != 1 || snapshot.Frames.Count > _limits.MaximumFrames ||
                string.IsNullOrWhiteSpace(snapshot.Generation))
                throw new InvalidDataException("The materialized snapshot exceeds its format or capacity contract.");
            Volatile.Write(ref _view, new SnapshotView(snapshot));
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or InvalidDataException or JsonException)
        {
            Volatile.Write(ref _view, null);
            throw new InputEvidenceUnavailableException("The committed input evidence snapshot is unavailable.", error);
        }
    }

    private static bool NullableValueEquals(JsonElement? left, JsonElement? right) =>
        left is null ? right is null : right is JsonElement value && JsonElement.DeepEquals(left.Value, value);

    private static ulong Timestamp(InputEvidenceFrame frame) =>
        ulong.Parse(frame.TimeUnixNano, NumberStyles.None, CultureInfo.InvariantCulture);

    private static string Freshness(InputEvidenceFrame frame, DateTimeOffset now, long maxAgeSeconds)
    {
        var nowNanoseconds = (BigInteger)(now.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks) * 100;
        var age = nowNanoseconds - Timestamp(frame);
        return age < 0 ? "future" : age > (BigInteger)maxAgeSeconds * 1_000_000_000 ? "stale" : "fresh";
    }

    public async ValueTask DisposeAsync()
    {
        await _writer.WaitAsync();
        try
        {
            Volatile.Write(ref _view, null);
            await _store.DisposeAsync();
        }
        finally { _writer.Release(); }
    }

    private sealed class SnapshotView(InputEvidenceSnapshot snapshot)
    {
        public InputEvidenceSnapshot Snapshot { get; } = snapshot;
        public FrozenDictionary<InputEvidenceKey, InputEvidenceFrame[]> Frames { get; } =
            snapshot.Frames.GroupBy(frame => frame.Key).ToFrozenDictionary(group => group.Key, group => group.ToArray());
    }
}
