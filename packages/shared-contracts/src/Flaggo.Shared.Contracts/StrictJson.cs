using System.Text.Json;

namespace Flaggo.Shared.Contracts;

public static class StrictJson
{
    public static void Validate(ReadOnlySpan<byte> json)
    {
        var reader = new Utf8JsonReader(json);
        if (!reader.Read())
        {
            throw new JsonException("JSON data must contain one complete value.");
        }

        var objectProperties = new Stack<HashSet<string>>();
        var rootComplete = false;
        do
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

            rootComplete =
                reader.CurrentDepth == 0 &&
                reader.TokenType is
                    JsonTokenType.EndObject or
                    JsonTokenType.EndArray or
                    JsonTokenType.String or
                    JsonTokenType.Number or
                    JsonTokenType.True or
                    JsonTokenType.False or
                    JsonTokenType.Null;
            if (rootComplete)
            {
                break;
            }
        }
        while (reader.Read());

        if (!rootComplete)
        {
            throw new JsonException("JSON data must contain one complete value.");
        }

        var trailing = json[(int)reader.BytesConsumed..];
        foreach (var value in trailing)
        {
            if (value is not ((byte)' ' or (byte)'\t' or (byte)'\r' or (byte)'\n'))
            {
                throw new JsonException(
                    "JSON data must contain exactly one complete value.");
            }
        }
    }
}
