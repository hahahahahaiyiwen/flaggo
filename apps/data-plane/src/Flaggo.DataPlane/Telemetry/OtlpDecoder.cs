using System.Collections.Frozen;
using System.Security.Cryptography;
using System.Text.Json;
using Flaggo.Evidence;
using Google.Protobuf;
using OpenTelemetry.Proto.Collector.Logs.V1;
using OpenTelemetry.Proto.Collector.Metrics.V1;
using OpenTelemetry.Proto.Collector.Trace.V1;
using OpenTelemetry.Proto.Common.V1;
using OpenTelemetry.Proto.Metrics.V1;
using OpenTelemetry.Proto.Resource.V1;

namespace Flaggo.DataPlane.Telemetry;

internal sealed class OtlpRequestTooLargeException : Exception;

internal static class OtlpDecoder
{
    public static IReadOnlyList<TelemetryObservation> Decode(string signal, byte[] bytes, int maximumRecords)
    {
        var observations = new List<TelemetryObservation>();
        long work = 0;
        void Add(TelemetryObservation observation)
        {
            work += 1L + (observation.Events?.Count ?? 0);
            if (work > maximumRecords) throw new OtlpRequestTooLargeException();
            observations.Add(observation);
        }
        switch (signal)
        {
            case "metrics":
                foreach (var resource in ExportMetricsServiceRequest.Parser.ParseFrom(bytes).ResourceMetrics)
                foreach (var scope in resource.ScopeMetrics)
                {
                    var template = Template("metric", resource.Resource, scope.Scope, resource.SchemaUrl, scope.SchemaUrl);
                    foreach (var metric in scope.Metrics)
                    {
                        var metricTemplate = template with { Name = metric.Name, Unit = metric.Unit };
                        switch (metric.DataCase)
                        {
                            case Metric.DataOneofCase.Gauge:
                                foreach (var point in metric.Gauge.DataPoints) Add(NumberPoint(metricTemplate, point, "gauge"));
                                break;
                            case Metric.DataOneofCase.Sum:
                                foreach (var point in metric.Sum.DataPoints) Add(NumberPoint(metricTemplate, point, "sum"));
                                break;
                            case Metric.DataOneofCase.Histogram:
                                foreach (var point in metric.Histogram.DataPoints)
                                    Add(UnsupportedPoint(metricTemplate, point, point.Attributes, point.TimeUnixNano, "histogram"));
                                break;
                            case Metric.DataOneofCase.ExponentialHistogram:
                                foreach (var point in metric.ExponentialHistogram.DataPoints)
                                    Add(UnsupportedPoint(metricTemplate, point, point.Attributes, point.TimeUnixNano, "exponential-histogram"));
                                break;
                            case Metric.DataOneofCase.Summary:
                                foreach (var point in metric.Summary.DataPoints)
                                    Add(UnsupportedPoint(metricTemplate, point, point.Attributes, point.TimeUnixNano, "summary"));
                                break;
                        }
                    }
                }
                break;
            case "traces":
                foreach (var resource in ExportTraceServiceRequest.Parser.ParseFrom(bytes).ResourceSpans)
                foreach (var scope in resource.ScopeSpans)
                {
                    var template = Template("span", resource.Resource, scope.Scope, resource.SchemaUrl, scope.SchemaUrl);
                    foreach (var span in scope.Spans)
                    {
                        if (work + 1L + span.Events.Count > maximumRecords) throw new OtlpRequestTooLargeException();
                        Add(template with
                        {
                            Name = span.Name,
                            Attributes = Attributes(span.Attributes),
                            TimeUnixNano = span.EndTimeUnixNano,
                            StartTimeUnixNano = span.StartTimeUnixNano,
                            TraceId = Identifier(span.TraceId, 16),
                            SpanId = Identifier(span.SpanId, 8),
                            SamplingFlags = span.Flags,
                            Fingerprint = RecordFingerprint(template, span),
                            Events = span.Events.Select(item => new TelemetryEvent(
                                item.Name, item.TimeUnixNano, Attributes(item.Attributes))).ToArray()
                        });
                    }
                }
                break;
            case "logs":
                foreach (var resource in ExportLogsServiceRequest.Parser.ParseFrom(bytes).ResourceLogs)
                foreach (var scope in resource.ScopeLogs)
                {
                    var template = Template("log", resource.Resource, scope.Scope, resource.SchemaUrl, scope.SchemaUrl);
                    foreach (var log in scope.LogRecords)
                    {
                        Add(template with
                        {
                            Attributes = Attributes(log.Attributes),
                            TimeUnixNano = log.TimeUnixNano,
                            EventName = log.EventName,
                            Body = Value(log.Body),
                            TraceId = Identifier(log.TraceId, 16),
                            SpanId = Identifier(log.SpanId, 8),
                            SamplingFlags = log.Flags,
                            Fingerprint = RecordFingerprint(template, log)
                        });
                    }
                }
                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(signal));
        }
        return observations;
    }

    private static TelemetryObservation Template(
        string kind, Resource? resource, InstrumentationScope? scope, string resourceSchema, string scopeSchema) =>
        new(kind, null, scope?.Name ?? "", scope?.Version ?? "",
            Attributes(resource?.Attributes ?? []),
            FrozenDictionary<string, JsonElement>.Empty, 0,
            Hash(JsonSerializer.SerializeToUtf8Bytes(new
            {
                resourceSchema, scopeSchema, scope?.Name, scope?.Version,
                resource = AttributeIdentity(resource?.Attributes ?? []),
                scope = AttributeIdentity(scope?.Attributes ?? [])
            })));

    private static TelemetryObservation NumberPoint(TelemetryObservation template, NumberDataPoint point, string dataType) =>
        template with
        {
            Attributes = Attributes(point.Attributes),
            DataType = dataType,
            TimeUnixNano = point.TimeUnixNano,
            NoRecordedValue = (point.Flags & 1) != 0,
            MetricStreamFingerprint = StreamFingerprint(template, point.Attributes, dataType),
            Value = point.ValueCase switch
            {
                NumberDataPoint.ValueOneofCase.AsDouble when double.IsFinite(point.AsDouble) =>
                    JsonSerializer.SerializeToElement(point.AsDouble),
                NumberDataPoint.ValueOneofCase.AsInt => JsonSerializer.SerializeToElement(point.AsInt),
                _ => null
            },
            Fingerprint = RecordFingerprint(template, point)
        };

    private static string StreamFingerprint(
        TelemetryObservation template, IEnumerable<KeyValue> attributes, string dataType) =>
        Hash(JsonSerializer.SerializeToUtf8Bytes(new
        {
            template.Fingerprint, template.Name, template.Unit, dataType,
            attributes = AttributeIdentity(attributes)
        }));

    private static KeyValuePair<string, JsonElement>[] AttributeIdentity(IEnumerable<KeyValue> attributes) =>
        OrderedAttributes(attributes).Select(entry =>
        {
            using var document = JsonDocument.Parse(JsonFormatter.Default.Format(OrderedValue(entry.Value)));
            return KeyValuePair.Create(entry.Key, document.RootElement.Clone());
        }).ToArray();

    private static KeyValue[] OrderedAttributes(IEnumerable<KeyValue> attributes)
    {
        var ordered = attributes.OrderBy(entry => entry.Key, StringComparer.Ordinal).ToArray();
        for (var index = 1; index < ordered.Length; index++)
        {
            if (ordered[index - 1].Key == ordered[index].Key)
                throw new InvalidDataException("OTLP attribute keys must be unique.");
        }
        return ordered;
    }

    private static AnyValue OrderedValue(AnyValue? value) => value?.ValueCase switch
    {
        AnyValue.ValueOneofCase.KvlistValue => new()
        {
            KvlistValue = new()
            {
                Values = { OrderedAttributes(value.KvlistValue.Values)
                    .Select(entry => new KeyValue { Key = entry.Key, Value = OrderedValue(entry.Value) }) }
            }
        },
        AnyValue.ValueOneofCase.ArrayValue => new()
        {
            ArrayValue = new() { Values = { value.ArrayValue.Values.Select(OrderedValue) } }
        },
        _ => value ?? new AnyValue()
    };

    private static TelemetryObservation UnsupportedPoint(
        TelemetryObservation template, IMessage point, IEnumerable<KeyValue> attributes, ulong timestamp, string dataType) =>
        template with
        {
            Attributes = Attributes(attributes), TimeUnixNano = timestamp, DataType = dataType,
            Fingerprint = RecordFingerprint(template, point),
            MetricStreamFingerprint = StreamFingerprint(template, attributes, dataType)
        };

    private static FrozenDictionary<string, JsonElement> Attributes(IEnumerable<KeyValue> attributes)
    {
        var values = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var entry in attributes)
        {
            if (!values.TryAdd(entry.Key, Value(entry.Value)))
                throw new InvalidDataException("OTLP attribute keys must be unique.");
        }
        return values.ToFrozenDictionary(StringComparer.Ordinal);
    }

    private static JsonElement Value(AnyValue? value) =>
        value?.ValueCase switch
        {
            AnyValue.ValueOneofCase.StringValue => JsonSerializer.SerializeToElement(value.StringValue),
            AnyValue.ValueOneofCase.BoolValue => JsonSerializer.SerializeToElement(value.BoolValue),
            AnyValue.ValueOneofCase.IntValue => JsonSerializer.SerializeToElement(value.IntValue),
            AnyValue.ValueOneofCase.DoubleValue when double.IsFinite(value.DoubleValue) =>
                JsonSerializer.SerializeToElement(value.DoubleValue),
            AnyValue.ValueOneofCase.ArrayValue =>
                JsonSerializer.SerializeToElement(value.ArrayValue.Values.Select(Value).ToArray()),
            AnyValue.ValueOneofCase.KvlistValue => JsonSerializer.SerializeToElement(Attributes(value.KvlistValue.Values)),
            _ => JsonSerializer.SerializeToElement<object?>(null)
        };

    private static string RecordFingerprint(TelemetryObservation template, IMessage record) =>
        Hash(JsonSerializer.SerializeToUtf8Bytes(new[]
        {
            template.Kind, template.Name, template.Unit, template.Fingerprint, Hash(record.ToByteArray())
        }));

    private static string Hash(ReadOnlySpan<byte> bytes) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant()}";

    private static string? Identifier(ByteString bytes, int length) =>
        bytes.Length == length && bytes.Span.IndexOfAnyExcept((byte)0) >= 0
            ? Convert.ToHexString(bytes.Span).ToLowerInvariant() : null;
}
