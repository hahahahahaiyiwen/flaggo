using System.Text;
using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class StrictJsonTests
{
    [Fact]
    public void SequentialObjects_AreRejected()
    {
        Assert.Throws<JsonException>(() => Validate("{}{}"));
    }

    [Fact]
    public void SequentialScalars_AreRejected()
    {
        Assert.Throws<JsonException>(() => Validate("1 true"));
    }

    [Fact]
    public void TrailingJsonWhitespace_IsAccepted()
    {
        Validate("{} \t\r\n");
    }

    [Fact]
    public void MalformedTrailingToken_IsRejected()
    {
        Assert.Throws<JsonException>(() => Validate("{} ]"));
    }

    [Fact]
    public void MalformedRoot_RemainsAJsonError()
    {
        Assert.ThrowsAny<JsonException>(() => Validate("""{"value":"""));
    }

    [Fact]
    public void NestedDuplicateProperty_IsRejected()
    {
        var error = Assert.Throws<JsonException>(
            () => Validate("""{"outer":{"value":1,"value":2}}"""));

        Assert.Contains("Duplicate JSON property 'value'", error.Message);
    }

    private static void Validate(string json) =>
        StrictJson.Validate(Encoding.UTF8.GetBytes(json));
}
