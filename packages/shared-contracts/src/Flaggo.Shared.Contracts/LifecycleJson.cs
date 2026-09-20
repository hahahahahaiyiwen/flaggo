using System.Globalization;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace Flaggo.Shared.Contracts;

public static partial class LifecycleJson
{
    public static JsonSerializerOptions Options { get; } = CreateOptions();

    public static string Digest<T>(T value) =>
        $"sha256:{Convert.ToHexString(SHA256.HashData(Bytes(value))).ToLowerInvariant()}";

    public static byte[] Bytes<T>(T value)
    {
        var serialized = JsonSerializer.SerializeToUtf8Bytes(value, Options);
        StrictJson.Validate(serialized);
        using var document = JsonDocument.Parse(serialized);
        var element = document.RootElement;
        ValidateNumbers(element);
        return element.ValueKind == JsonValueKind.Null
            ? "null"u8.ToArray()
            : CanonicalJson.Canonicalize(element);
    }

    public static T Read<T>(ReadOnlySpan<byte> bytes)
    {
        StrictJson.Validate(bytes);
        using var document = JsonDocument.Parse(bytes.ToArray());
        ValidateNumbers(document.RootElement);
        return JsonSerializer.Deserialize<T>(bytes, Options)
            ?? throw new JsonException("A lifecycle value cannot be null.");
    }

    public static T Copy<T>(T value) =>
        JsonSerializer.Deserialize<T>(Bytes(value), Options)
        ?? throw new JsonException("A lifecycle value cannot be null.");

    public static bool IsDigest(string? value) =>
        value is { Length: 71 } &&
        value.StartsWith("sha256:", StringComparison.Ordinal) &&
        value.AsSpan(7).IndexOfAnyExcept("0123456789abcdef") < 0;

    private static JsonSerializerOptions CreateOptions()
    {
        var options = new JsonSerializerOptions(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
            RespectRequiredConstructorParameters = true,
            RespectNullableAnnotations = true,
            // Canonical property ordering can put "kind" after ordinary fields.
            AllowOutOfOrderMetadataProperties = true
        };
        options.Converters.Add(
            new JsonStringEnumConverter(
                JsonNamingPolicy.KebabCaseLower,
                allowIntegerValues: false));
        options.Converters.Add(new LifecycleTimestampConverter());
        options.MakeReadOnly(populateMissingResolver: true);
        return options;
    }

    private static void ValidateNumbers(JsonElement value)
    {
        switch (value.ValueKind)
        {
            case JsonValueKind.Number when !CanonicalJson.IsIeee754CompatibleNumber(value):
                throw new JsonException("Lifecycle numbers must preserve their canonical IEEE-754 value.");
            case JsonValueKind.Object:
                foreach (var property in value.EnumerateObject())
                {
                    ValidateNumbers(property.Value);
                }
                break;
            case JsonValueKind.Array:
                foreach (var item in value.EnumerateArray())
                {
                    ValidateNumbers(item);
                }
                break;
        }
    }

    private sealed class LifecycleTimestampConverter : JsonConverter<DateTimeOffset>
    {
        public override DateTimeOffset Read(
            ref Utf8JsonReader reader,
            Type typeToConvert,
            JsonSerializerOptions options)
        {
            if (reader.TokenType != JsonTokenType.String)
            {
                throw new JsonException("Lifecycle timestamps must be strings.");
            }
            var text = reader.GetString();
            if (text is null || !TimestampPattern().IsMatch(text) ||
                !DateTimeOffset.TryParse(
                    text,
                    CultureInfo.InvariantCulture,
                    DateTimeStyles.None,
                    out var timestamp))
            {
                throw new JsonException("Lifecycle timestamps require RFC 3339 with an explicit offset.");
            }

            return timestamp.ToUniversalTime();
        }

        public override void Write(
            Utf8JsonWriter writer,
            DateTimeOffset value,
            JsonSerializerOptions options) =>
            writer.WriteStringValue(value.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture));
    }

    [GeneratedRegex(
        "\\A\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d{1,7})?(?:Z|[+-]\\d{2}:\\d{2})\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex TimestampPattern();
}
