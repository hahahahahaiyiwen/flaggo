using System.Globalization;
using System.Security.Cryptography;
using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Evidence;

internal sealed record TelemetryProjectionResult(InputEvidenceFrame? Frame, string? Error);

internal static class TelemetryProjection
{
    public static bool Matches(TelemetryObservation observation, RegisteredEvidenceBinding binding)
    {
        var source = binding.Source;
        return observation.Kind == source.Kind &&
            observation.ScopeName == source.ScopeName &&
            (source.ScopeVersion is null || observation.ScopeVersion == source.ScopeVersion) &&
            (source.Name is null || observation.Name == source.Name) &&
            AttributesMatch(observation.ResourceAttributes, source.ResourceAttributes) &&
            AttributesMatch(observation.Attributes, source.Attributes) &&
            (source.Kind != "log" ||
             (source.EventName is not null ? observation.EventName == source.EventName :
              observation.Body is JsonElement body && source.BodyEquals is JsonElement expected && ScalarEquals(body, expected)));
    }

    public static IEnumerable<TelemetryEvent?> SelectEvents(
        TelemetryObservation observation, RegisteredEvidenceBinding binding)
    {
        if (binding.Source.Kind != "span" || binding.Source.EventName is null)
        {
            yield return null;
            yield break;
        }
        foreach (var item in observation.Events ?? [])
        {
            if (item.Name == binding.Source.EventName && AttributesMatch(item.Attributes, binding.Source.EventAttributes))
                yield return item;
        }
    }

    public static async Task<TelemetryProjectionResult> ProjectAsync(
        ApplicationScope scope,
        EvidenceBindingProjection definition,
        RegisteredEvidenceBinding binding,
        TelemetryObservation observation,
        TelemetryEvent? spanEvent,
        DateTimeOffset materializedAt,
        IConfirmedExposureReader exposures,
        CancellationToken cancellationToken)
    {
        var timestamp = spanEvent?.TimeUnixNano ?? observation.TimeUnixNano;
        if (timestamp == 0) return new(null, "invalid-timestamp");
        var targetId = binding.TargetType == "global" ? "global" :
            StringAttribute(observation, spanEvent, binding.TargetIdAttribute!);
        if (string.IsNullOrWhiteSpace(targetId)) return new(null, "invalid-target");
        var target = new DecisionTargetRef(binding.TargetType, targetId);
        var status = "available";
        JsonElement? value = null;
        switch (binding.Source.ValueFrom)
        {
            case "value":
                value = observation.Value;
                if (observation.DataType != "gauge") status = "unsupported-metric-type";
                else if (observation.Unit != binding.Unit) status = "unit-mismatch";
                else if (observation.NoRecordedValue) status = "no-recorded-value";
                break;
            case "duration":
                if (observation.StartTimeUnixNano is not ulong start || start == 0 || start > observation.TimeUnixNano)
                    status = "invalid-duration";
                else
                    value = JsonSerializer.SerializeToElement((observation.TimeUnixNano - start) / 1_000_000d);
                break;
            case "attributes":
                value = Attribute(observation.Attributes, binding.Source.ValueKey!);
                break;
            case "eventAttributes":
                value = spanEvent is null ? null : Attribute(spanEvent.Attributes, binding.Source.ValueKey!);
                break;
            case "body":
                value = observation.Body;
                foreach (var key in binding.Source.BodyPath)
                {
                    value = value is JsonElement element && element.ValueKind == JsonValueKind.Object &&
                            element.TryGetProperty(key, out var field) ? field : null;
                }
                break;
            default:
                throw new InvalidDataException("The accepted binding contains an unsupported value selector.");
        }
        if (status == "available" && (value is null || !DecisionValues.Matches(value.Value, binding.ValueType, binding.Minimum, binding.Maximum)))
            status = "invalid-value";
        string? exposureId = null;
        if (binding.ExposureIdAttribute is not null)
        {
            var suppliedId = StringAttribute(observation, spanEvent, binding.ExposureIdAttribute);
            var exposure = string.IsNullOrWhiteSpace(suppliedId) ? null :
                await exposures.FindConfirmedAsync(suppliedId, scope, cancellationToken);
            if (exposure is null ||
                exposure.Snapshot.Contract.DefinitionId != definition.Identity.DefinitionId ||
                exposure.Snapshot.Contract.Revision != definition.Identity.Revision ||
                exposure.Snapshot.Contract.ContractDigest != definition.Identity.ContractDigest ||
                exposure.Snapshot.ResolvedTargets?.Contains(target) != true)
            {
                status = "invalid-exposure";
            }
            else
            {
                exposureId = exposure.Confirmation.ExposureId;
            }
        }
        var keyIdentity = new InputEvidenceKey(scope, definition.Identity.DefinitionId,
            definition.Identity.Revision, definition.Identity.ContractDigest, binding.Key, target);
        var stream = observation.Kind == "metric"
            ? observation.MetricStreamFingerprint
                ?? throw new InvalidDataException("The normalized metric has no stream identity.")
            : "";
        var fingerprint = spanEvent is null ? observation.Fingerprint :
            Fingerprint(JsonSerializer.SerializeToUtf8Bytes(new
            {
                observation.Fingerprint, spanEvent.Name, time = timestamp.ToString(CultureInfo.InvariantCulture),
                attributes = AttributeIdentity(spanEvent.Attributes)
            }));
        return new(new InputEvidenceFrame(
            keyIdentity, stream, observation.Kind, status == "available" ? value : null, status,
            timestamp.ToString(CultureInfo.InvariantCulture), binding.MaxAgeSeconds, materializedAt,
            fingerprint, observation.TraceId, observation.SpanId, observation.SamplingFlags, exposureId),
            status == "available" ? null : status);
    }

    public static bool ScalarEquals(JsonElement left, JsonElement right) =>
        DecisionValues.IsScalar(left) && DecisionValues.IsScalar(right) && JsonElement.DeepEquals(left, right);

    public static string Fingerprint(ReadOnlySpan<byte> bytes) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant()}";

    private static bool AttributesMatch(
        IReadOnlyDictionary<string, JsonElement> actual,
        IReadOnlyDictionary<string, JsonElement> expected) =>
        expected.All(pair => actual.TryGetValue(pair.Key, out var value) && ScalarEquals(value, pair.Value));

    private static JsonElement? Attribute(IReadOnlyDictionary<string, JsonElement> attributes, string key) =>
        attributes.TryGetValue(key, out var value) ? value : null;

    private static string? StringAttribute(
        TelemetryObservation observation, TelemetryEvent? spanEvent, TelemetryAttributeSelector selector)
    {
        var value = selector.From switch
        {
            "resourceAttributes" => Attribute(observation.ResourceAttributes, selector.Key),
            "attributes" => Attribute(observation.Attributes, selector.Key),
            "eventAttributes" => spanEvent is null ? null : Attribute(spanEvent.Attributes, selector.Key),
            _ => null
        };
        return value?.ValueKind == JsonValueKind.String ? value.Value.GetString() : null;
    }

    private static KeyValuePair<string, string>[] AttributeIdentity(IReadOnlyDictionary<string, JsonElement> attributes) =>
        attributes.OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .Select(pair => KeyValuePair.Create(pair.Key, pair.Value.GetRawText())).ToArray();
}
