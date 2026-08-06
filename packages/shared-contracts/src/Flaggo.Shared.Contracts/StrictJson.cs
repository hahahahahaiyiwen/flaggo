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
        }
        while (reader.Read());
    }
}
