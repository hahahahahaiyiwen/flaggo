using System.Text.Json;
using System.Text.Json.Serialization;
using System.Globalization;

namespace Flaggo.Contract;

internal sealed class UtcDateTimeOffsetJsonConverter : JsonConverter<DateTimeOffset>
{
    public override DateTimeOffset Read(
        ref Utf8JsonReader reader,
        Type typeToConvert,
        JsonSerializerOptions options)
    {
        var value = reader.GetString();
        if (value is null
            || !value.EndsWith('Z')
            || !DateTimeOffset.TryParse(
                value,
                CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal,
                out var parsed))
        {
            throw new JsonException("Timestamp must be an RFC 3339 UTC value with a Z suffix.");
        }

        return parsed;
    }

    public override void Write(
        Utf8JsonWriter writer,
        DateTimeOffset value,
        JsonSerializerOptions options) =>
        writer.WriteStringValue(value.UtcDateTime.ToString(
            "yyyy-MM-dd'T'HH:mm:ss.FFFFFFF'Z'",
            CultureInfo.InvariantCulture));
}

internal sealed class RuleWhenJsonConverter : JsonConverter<RuleWhen>
{
    public override RuleWhen Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        var root = document.RootElement;
        EnsureObjectWithOneProperty(root, "condition", "expression");

        if (root.TryGetProperty("condition", out var condition))
        {
            return new NaturalLanguageWhen(ReadNonEmptyString(condition, "condition"));
        }

        return new ExpressionWhen(ReadNonEmptyString(root.GetProperty("expression"), "expression"));
    }

    public override void Write(Utf8JsonWriter writer, RuleWhen value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        switch (value)
        {
            case NaturalLanguageWhen natural:
                writer.WriteString("condition", natural.Condition);
                break;
            case ExpressionWhen expression:
                writer.WriteString("expression", expression.Expression);
                break;
            default:
                throw new JsonException($"Unsupported rule condition type {value.GetType().Name}.");
        }

        writer.WriteEndObject();
    }

    private static void EnsureObjectWithOneProperty(JsonElement element, params string[] allowed)
    {
        if (element.ValueKind != JsonValueKind.Object)
        {
            throw new JsonException("Rule condition must be an object.");
        }

        var properties = element.EnumerateObject().ToArray();
        if (properties.Length != 1 || !allowed.Contains(properties[0].Name, StringComparer.Ordinal))
        {
            throw new JsonException("Rule condition must contain exactly one supported member.");
        }
    }

    private static string ReadNonEmptyString(JsonElement element, string property)
    {
        if (element.ValueKind != JsonValueKind.String || string.IsNullOrEmpty(element.GetString()))
        {
            throw new JsonException($"{property} must be a non-empty string.");
        }

        return element.GetString()!;
    }
}

internal sealed class RuleReturnJsonConverter : JsonConverter<RuleReturn>
{
    public override RuleReturn Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        var root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
        {
            throw new JsonException("Rule return must be an object.");
        }

        var properties = root.EnumerateObject().ToArray();
        if (properties.Length != 1)
        {
            throw new JsonException("Rule return must contain exactly one member.");
        }

        return properties[0].Name switch
        {
            "value" => new LiteralReturn(properties[0].Value.Clone()),
            "expression" when properties[0].Value.ValueKind == JsonValueKind.String
                && !string.IsNullOrEmpty(properties[0].Value.GetString())
                => new ExpressionReturn(properties[0].Value.GetString()!),
            _ => throw new JsonException("Rule return must contain value or a non-empty expression.")
        };
    }

    public override void Write(Utf8JsonWriter writer, RuleReturn value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        switch (value)
        {
            case LiteralReturn literal:
                writer.WritePropertyName("value");
                literal.Value.WriteTo(writer);
                break;
            case ExpressionReturn expression:
                writer.WriteString("expression", expression.Expression);
                break;
            default:
                throw new JsonException($"Unsupported rule return type {value.GetType().Name}.");
        }

        writer.WriteEndObject();
    }
}

internal sealed class EvaluationProvenanceJsonConverter : JsonConverter<EvaluationProvenance>
{
    public override EvaluationProvenance Read(
        ref Utf8JsonReader reader,
        Type typeToConvert,
        JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        var root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object
            || !root.TryGetProperty("source", out var source)
            || source.ValueKind != JsonValueKind.String)
        {
            throw new JsonException("Evaluation provenance requires a source.");
        }

        var properties = root.EnumerateObject().ToArray();
        return source.GetString() switch
        {
            "default" when properties.Length == 1 => new DefaultEvaluation(),
            "rule" when properties.Length == 2
                && root.TryGetProperty("rule", out var rule)
                && rule.ValueKind == JsonValueKind.String
                && !string.IsNullOrEmpty(rule.GetString())
                => new RuleEvaluation(rule.GetString()!),
            _ => throw new JsonException("Invalid evaluation provenance.")
        };
    }

    public override void Write(
        Utf8JsonWriter writer,
        EvaluationProvenance value,
        JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        switch (value)
        {
            case DefaultEvaluation:
                writer.WriteString("source", "default");
                break;
            case RuleEvaluation rule:
                writer.WriteString("source", "rule");
                writer.WriteString("rule", rule.Rule);
                break;
            default:
                throw new JsonException($"Unsupported evaluation type {value.GetType().Name}.");
        }

        writer.WriteEndObject();
    }
}
