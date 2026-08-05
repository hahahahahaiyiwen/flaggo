using System.Globalization;
using System.Numerics;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Flaggo.Shared.Contracts;

public static class CanonicalJson
{
    public static string ContractDigest(JsonElement definition) =>
        Digest(NormalizeDefinition(definition, semanticIdentity: true));

    public static string BundleDigest(JsonElement bundle) =>
        Digest(NormalizeBundle(bundle));

    public static byte[] NormalizeBundleBytes(JsonElement bundle) =>
        Serialize(NormalizeBundle(bundle));

    public static JsonElement NormalizeBundleElement(JsonElement bundle) =>
        JsonSerializer.SerializeToElement(NormalizeBundle(bundle));

    public static byte[] Canonicalize(JsonElement value)
    {
        var node = JsonNode.Parse(value.GetRawText())
            ?? throw new JsonException("A JSON value is required.");
        return Serialize(node);
    }

    public static bool IsIeee754CompatibleNumber(JsonElement value)
    {
        if (value.ValueKind != JsonValueKind.Number ||
            !value.TryGetDouble(out var number) ||
            !double.IsFinite(number))
        {
            return false;
        }

        var canonical = FormatCanonicalNumber(number);
        return TryNormalizeDecimal(value.GetRawText(), out var original) &&
            TryNormalizeDecimal(canonical, out var roundTripped) &&
            original == roundTripped;
    }

    private static JsonNode NormalizeDefinition(
        JsonElement definition,
        bool semanticIdentity)
    {
        var normalized = JsonNode.Parse(definition.GetRawText())?.AsObject()
            ?? throw new JsonException("A decision definition must be an object.");
        normalized.Remove("contractDigest");
        normalized.Remove("revision");
        normalized.Remove("schemaDigest");
        if (semanticIdentity)
        {
            normalized.Remove("definitionId");
            normalized.Remove("owner");
        }

        var signalKeys = new HashSet<string>(StringComparer.Ordinal);
        if (normalized["signals"] is JsonObject signals)
        {
            foreach (var role in new[] { "evidence", "guardrails" })
            {
                if (signals[role] is JsonArray references)
                {
                    signals[role] = NormalizeSignalReferences(references, signalKeys);
                }
            }
        }

        if (normalized["inference"] is JsonObject inference &&
            inference["inputs"] is JsonArray inputs)
        {
            inference["inputs"] = NormalizeSignalReferences(inputs, signalKeys);
        }

        CollectObjectiveSignalKeys(normalized["intent"], signalKeys);
        if (normalized["signals"] is not JsonObject normalizedSignals &&
            signalKeys.Count > 0)
        {
            normalizedSignals = [];
            normalized["signals"] = normalizedSignals;
        }

        if (normalized["signals"] is JsonObject finalSignals)
        {
            if (signalKeys.Count > 0)
            {
                finalSignals["allowed"] = new JsonArray(
                    signalKeys.Order(StringComparer.Ordinal)
                        .Select(key => (JsonNode)new JsonObject { ["key"] = key })
                        .ToArray());
            }
            else
            {
                finalSignals.Remove("allowed");
            }

            if (finalSignals.Count == 0)
            {
                normalized.Remove("signals");
            }
        }

        if (normalized["policy"] is JsonObject policy &&
            string.Equals(
                policy["kind"]?.GetValue<string>(),
                "inline",
                StringComparison.Ordinal))
        {
            if (policy["clientFallback"] is JsonObject clientFallback &&
                string.Equals(
                    clientFallback["requiredEvidenceUnavailable"]?.GetValue<string>(),
                    "forbid",
                    StringComparison.Ordinal))
            {
                policy.Remove("clientFallback");
            }

            var constraints = policy["constraints"] as JsonArray ?? [];
            var kinds = new HashSet<string>(StringComparer.Ordinal);
            var ordered = constraints
                .Select(item => item?.DeepClone()
                    ?? throw new JsonException("Policy constraints cannot contain null."))
                .OrderBy(
                    item => item["kind"]?.GetValue<string>(),
                    StringComparer.Ordinal)
                .ThenBy(item => Encoding.UTF8.GetString(Serialize(item)), StringComparer.Ordinal)
                .ToArray();
            foreach (var constraint in ordered)
            {
                var kind = constraint["kind"]?.GetValue<string>()
                    ?? throw new JsonException("Policy constraint kind is required.");
                if (!kinds.Add(kind))
                {
                    throw new JsonException(
                        $"Duplicate inline policy constraint kind: {kind}.");
                }
            }

            policy["constraints"] = new JsonArray(ordered);
        }

        return normalized;
    }

    private static JsonObject NormalizeBundle(JsonElement bundle)
    {
        var normalized = JsonNode.Parse(bundle.GetRawText())?.AsObject()
            ?? throw new JsonException("A definition bundle must be an object.");

        if (normalized["signals"] is JsonArray declarations)
        {
            var signals = new SortedDictionary<string, JsonNode>(StringComparer.Ordinal);
            var digests = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var item in declarations)
            {
                var declaration = item?.DeepClone().AsObject()
                    ?? throw new JsonException("Signal declarations cannot contain null.");
                declaration.Remove("schemaDigest");
                var key = declaration["key"]?.GetValue<string>()
                    ?? throw new JsonException("Signal key is required.");
                var digest = Digest(declaration);
                if (digests.TryGetValue(key, out var previousDigest))
                {
                    if (!string.Equals(previousDigest, digest, StringComparison.Ordinal))
                    {
                        throw new JsonException($"Conflicting signal declaration: {key}.");
                    }

                    throw new JsonException($"Duplicate signal declaration: {key}.");
                }

                signals.Add(key, declaration);
                digests.Add(key, digest);
            }

            normalized["signals"] = new JsonArray(signals.Values.ToArray());
        }

        var definitionArray = normalized["definitions"] as JsonArray
            ?? throw new JsonException("Bundle definitions are required.");
        var definitions = new SortedDictionary<string, JsonNode>(StringComparer.Ordinal);
        var definitionDigests = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var item in definitionArray)
        {
            if (item is null)
            {
                throw new JsonException("Definitions cannot contain null.");
            }

            using var document = JsonDocument.Parse(item.ToJsonString());
            var candidate = NormalizeDefinition(document.RootElement, semanticIdentity: false);
            var key = candidate["key"]?.GetValue<string>()
                ?? throw new JsonException("Decision key is required.");
            using var candidateDocument = JsonDocument.Parse(candidate.ToJsonString());
            var digest = ContractDigest(candidateDocument.RootElement);
            if (definitionDigests.TryGetValue(key, out var previousDigest))
            {
                if (!string.Equals(previousDigest, digest, StringComparison.Ordinal))
                {
                    throw new JsonException($"Conflicting decision definition key: {key}.");
                }

                if (!Serialize(definitions[key]).SequenceEqual(Serialize(candidate)))
                {
                    throw new JsonException(
                        $"Metadata-conflicting duplicate decision key: {key}.");
                }

                continue;
            }

            definitions.Add(key, candidate);
            definitionDigests.Add(key, digest);
        }

        normalized["definitions"] = new JsonArray(definitions.Values.ToArray());
        return normalized;
    }

    private static JsonArray NormalizeSignalReferences(
        JsonArray references,
        ISet<string> collectedKeys)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in references)
        {
            var key = item?["key"]?.GetValue<string>()
                ?? throw new JsonException("Signal reference key is required.");
            if (!keys.Add(key))
            {
                throw new JsonException($"Duplicate signal reference: {key}.");
            }

            collectedKeys.Add(key);
        }

        return new JsonArray(
            keys.Order(StringComparer.Ordinal)
                .Select(key => (JsonNode)new JsonObject { ["key"] = key })
                .ToArray());
    }

    private static void CollectObjectiveSignalKeys(JsonNode? node, ISet<string> keys)
    {
        switch (node)
        {
            case JsonObject value:
                if (value["signal"] is JsonObject signal &&
                    signal["key"] is JsonValue keyValue &&
                    keyValue.TryGetValue<string>(out var key))
                {
                    keys.Add(key);
                }

                foreach (var child in value)
                {
                    CollectObjectiveSignalKeys(child.Value, keys);
                }

                break;
            case JsonArray values:
                foreach (var child in values)
                {
                    CollectObjectiveSignalKeys(child, keys);
                }

                break;
        }
    }

    private static string Digest(JsonNode value) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(Serialize(value))).ToLowerInvariant()}";

    private static byte[] Serialize(JsonNode value)
    {
        using var document = JsonDocument.Parse(value.ToJsonString());
        var builder = new StringBuilder();
        WriteCanonical(builder, document.RootElement);
        return Encoding.UTF8.GetBytes(builder.ToString());
    }

    private static void WriteCanonical(StringBuilder builder, JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                builder.Append('{');
                var firstProperty = true;
                foreach (var property in element.EnumerateObject().OrderBy(
                             property => property.Name,
                             StringComparer.Ordinal))
                {
                    if (!firstProperty)
                    {
                        builder.Append(',');
                    }

                    firstProperty = false;
                    WriteString(builder, property.Name);
                    builder.Append(':');
                    WriteCanonical(builder, property.Value);
                }

                builder.Append('}');
                break;
            case JsonValueKind.Array:
                builder.Append('[');
                var firstItem = true;
                foreach (var item in element.EnumerateArray())
                {
                    if (!firstItem)
                    {
                        builder.Append(',');
                    }

                    firstItem = false;
                    WriteCanonical(builder, item);
                }

                builder.Append(']');
                break;
            case JsonValueKind.Number:
                if (!IsIeee754CompatibleNumber(element))
                {
                    throw new JsonException(
                        "JSON number cannot round-trip through IEEE 754 without changing value.");
                }

                builder.Append(FormatCanonicalNumber(element.GetDouble()));
                break;
            case JsonValueKind.String:
                WriteString(builder, element.GetString()!);
                break;
            case JsonValueKind.True:
                builder.Append("true");
                break;
            case JsonValueKind.False:
                builder.Append("false");
                break;
            case JsonValueKind.Null:
                builder.Append("null");
                break;
            default:
                throw new JsonException($"Unsupported JSON kind: {element.ValueKind}.");
        }
    }

    private static void WriteString(StringBuilder builder, string value)
    {
        builder.Append('"');
        foreach (var character in value)
        {
            switch (character)
            {
                case '"':
                    builder.Append("\\\"");
                    break;
                case '\\':
                    builder.Append("\\\\");
                    break;
                case '\b':
                    builder.Append("\\b");
                    break;
                case '\t':
                    builder.Append("\\t");
                    break;
                case '\n':
                    builder.Append("\\n");
                    break;
                case '\f':
                    builder.Append("\\f");
                    break;
                case '\r':
                    builder.Append("\\r");
                    break;
                case < '\u0020':
                    builder.Append("\\u");
                    builder.Append(((int)character).ToString("x4", CultureInfo.InvariantCulture));
                    break;
                default:
                    builder.Append(character);
                    break;
            }
        }

        builder.Append('"');
    }

    private static string FormatCanonicalNumber(double value)
    {
        if (!double.IsFinite(value))
        {
            throw new JsonException("Non-finite numbers are not valid JSON.");
        }

        if (value == 0)
        {
            return "0";
        }

        var roundTrip = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentIndex = roundTrip.IndexOfAny(['E', 'e']);
        var absolute = Math.Abs(value);
        if (exponentIndex < 0)
        {
            return roundTrip;
        }

        var mantissa = roundTrip[..exponentIndex];
        var exponent = int.Parse(
            roundTrip[(exponentIndex + 1)..],
            NumberStyles.AllowLeadingSign,
            CultureInfo.InvariantCulture);
        if (absolute >= 1e-6 && absolute < 1e21)
        {
            return ExpandDecimal(mantissa, exponent);
        }

        var normalizedExponent = exponent >= 0
            ? $"+{exponent}"
            : exponent.ToString(CultureInfo.InvariantCulture);
        return $"{mantissa.ToLowerInvariant()}e{normalizedExponent}";
    }

    private static string ExpandDecimal(string mantissa, int exponent)
    {
        var negative = mantissa.StartsWith('-');
        var unsigned = negative ? mantissa[1..] : mantissa;
        var decimalIndex = unsigned.IndexOf('.');
        var digits = decimalIndex < 0 ? unsigned : unsigned.Remove(decimalIndex, 1);
        var originalDecimalPosition = decimalIndex < 0 ? unsigned.Length : decimalIndex;
        var decimalPosition = originalDecimalPosition + exponent;
        var expanded = decimalPosition switch
        {
            <= 0 => $"0.{new string('0', -decimalPosition)}{digits}",
            _ when decimalPosition >= digits.Length =>
                $"{digits}{new string('0', decimalPosition - digits.Length)}",
            _ => $"{digits[..decimalPosition]}.{digits[decimalPosition..]}"
        };
        return negative ? $"-{expanded}" : expanded;
    }

    private static bool TryNormalizeDecimal(
        string text,
        out NormalizedDecimal normalized)
    {
        normalized = default;
        var span = text.AsSpan();
        var negative = false;
        if (!span.IsEmpty && span[0] == '-')
        {
            negative = true;
            span = span[1..];
        }

        var exponent = 0;
        var exponentIndex = span.IndexOfAny('e', 'E');
        if (exponentIndex >= 0)
        {
            if (!int.TryParse(
                    span[(exponentIndex + 1)..],
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out exponent))
            {
                return false;
            }

            span = span[..exponentIndex];
        }

        var decimalIndex = span.IndexOf('.');
        var fractionalDigits = decimalIndex >= 0
            ? span.Length - decimalIndex - 1
            : 0;
        var digits = decimalIndex >= 0
            ? string.Concat(span[..decimalIndex], span[(decimalIndex + 1)..])
            : span.ToString();
        digits = digits.TrimStart('0');
        if (digits.Length == 0)
        {
            normalized = new NormalizedDecimal(BigInteger.Zero, 0);
            return true;
        }

        try
        {
            exponent = checked(exponent - fractionalDigits);
            while (digits[^1] == '0')
            {
                digits = digits[..^1];
                exponent = checked(exponent + 1);
            }
        }
        catch (OverflowException)
        {
            return false;
        }

        if (!BigInteger.TryParse(
                digits,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var coefficient))
        {
            return false;
        }

        normalized = new NormalizedDecimal(
            negative ? -coefficient : coefficient,
            exponent);
        return true;
    }

    private readonly record struct NormalizedDecimal(
        BigInteger Coefficient,
        int Exponent);
}
