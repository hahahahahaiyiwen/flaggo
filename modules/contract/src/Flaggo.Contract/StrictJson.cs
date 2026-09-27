using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Contract;

public static class StrictJson
{
    public const int MaximumDocumentBytes = 262_144;
    public const int MaximumDepth = 16;
    public const int MaximumStringBytes = 16_384;

    public static JsonSerializerOptions Options { get; } = CreateOptions();

    public static T Deserialize<T>(ReadOnlySpan<byte> utf8Json)
    {
        if (utf8Json.Length > MaximumDocumentBytes)
        {
            throw new JsonException($"JSON exceeds the {MaximumDocumentBytes}-byte limit.");
        }

        ValidateTokens(utf8Json);
        return JsonSerializer.Deserialize<T>(utf8Json, Options)
            ?? throw new JsonException("JSON body must not be null.");
    }

    public static byte[] SerializeToUtf8Bytes<T>(T value) =>
        JsonSerializer.SerializeToUtf8Bytes(value, Options);

    private static JsonSerializerOptions CreateOptions()
    {
        var options = new JsonSerializerOptions(JsonSerializerDefaults.Web)
        {
            AllowTrailingCommas = false,
            DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
            MaxDepth = MaximumDepth,
            NumberHandling = JsonNumberHandling.Strict,
            PropertyNameCaseInsensitive = false,
            ReadCommentHandling = JsonCommentHandling.Disallow,
            RespectNullableAnnotations = true,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };
        return options;
    }

    private static void ValidateTokens(ReadOnlySpan<byte> utf8Json)
    {
        var reader = new Utf8JsonReader(
            utf8Json,
            new JsonReaderOptions
            {
                AllowTrailingCommas = false,
                CommentHandling = JsonCommentHandling.Disallow,
                MaxDepth = MaximumDepth
            });
        var containers = new Stack<HashSet<string>?>();

        while (reader.Read())
        {
            switch (reader.TokenType)
            {
                case JsonTokenType.StartObject:
                    containers.Push(new HashSet<string>(StringComparer.Ordinal));
                    break;
                case JsonTokenType.StartArray:
                    containers.Push(null);
                    break;
                case JsonTokenType.EndObject:
                case JsonTokenType.EndArray:
                    containers.Pop();
                    break;
                case JsonTokenType.PropertyName:
                    var name = reader.GetString()
                        ?? throw new JsonException("JSON property name must not be null.");
                    if (Encoding.UTF8.GetByteCount(name) > MaximumStringBytes)
                    {
                        throw new JsonException("JSON property name exceeds the string limit.");
                    }

                    var objectMembers = containers.Peek()
                        ?? throw new JsonException("JSON property appeared outside an object.");
                    if (!objectMembers.Add(name))
                    {
                        throw new JsonException($"Duplicate JSON property '{name}'.");
                    }

                    break;
                case JsonTokenType.String:
                    var value = reader.GetString()!;
                    if (Encoding.UTF8.GetByteCount(value) > MaximumStringBytes)
                    {
                        throw new JsonException("JSON string exceeds the string limit.");
                    }

                    break;
            }
        }
    }
}
