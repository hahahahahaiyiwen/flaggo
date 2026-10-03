using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;

namespace Flaggo.Core.Tests;

public sealed class ExpressionTests
{
    private static readonly IReadOnlyDictionary<string, ValueSchema> Attributes =
        new Dictionary<string, ValueSchema>(StringComparer.Ordinal)
        {
            ["board_pressure"] = new() { Type = "number" },
            ["current_level"] = new() { Type = "integer" },
            ["label"] = new() { Type = "string" }
        };

    private readonly FlaggoExpressionCompiler _compiler = new();

    [Fact]
    public void CompilesCanonicalPredicateAndTracksReferencedAttributes()
    {
        var compiled = _compiler.Compile(
            " attributes.board_pressure>=0.8 ",
            Attributes,
            new ValueSchema { Type = "boolean" },
            ExpressionPurpose.Predicate);

        var result = compiled.Evaluate(JsonAttributes(
            ("board_pressure", 0.82),
            ("_random", 0.4)));

        Assert.Equal("attributes.board_pressure >= 0.8", compiled.CanonicalExpression);
        Assert.Equal(["board_pressure"], compiled.ReferencedAttributes);
        Assert.True(result.GetBoolean());
    }

    [Fact]
    public void EvaluatesTypedResultExpression()
    {
        var compiled = _compiler.Compile(
            "1000.0 - double(attributes.current_level) * 25.0",
            Attributes,
            new ValueSchema { Type = "number" },
            ExpressionPurpose.Result);

        var result = compiled.Evaluate(JsonAttributes(("current_level", 10L)));

        Assert.Equal(750, result.GetDouble());
    }

    [Fact]
    public void RejectsUndeclaredAttributesAndUnsupportedFeatures()
    {
        Assert.Throws<ExpressionCompilationException>(() => _compiler.Compile(
            "attributes.missing == 1",
            Attributes,
            new ValueSchema { Type = "boolean" },
            ExpressionPurpose.Predicate));
        Assert.Throws<ExpressionCompilationException>(() => _compiler.Compile(
            "[1, 2, 3].exists(x, x > 1)",
            Attributes,
            new ValueSchema { Type = "boolean" },
            ExpressionPurpose.Predicate));
    }

    [Fact]
    public void ContractValidationUsesExpressionTypeChecking()
    {
        var contract = StrictJson.Deserialize<DecisionContract>(
            """
            {
              "authority": {
                "tenant": "local",
                "application": "test",
                "environment": "test"
              },
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "label", "schema": { "type": "string" } }
              ],
              "result": { "schema": { "type": "boolean" }, "default": false },
              "authoredExecutable": {
                "rules": [
                  {
                    "name": "invalid-predicate",
                    "when": { "expression": "attributes.label" },
                    "return": { "value": true }
                  }
                ]
              }
            }
            """u8);

        var result = ContractValidator.Validate(contract, _compiler);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-expression");
    }

    [Fact]
    public void MaterializesCheckedExpressionWithoutSourceParsing()
    {
        var compiled = _compiler.Compile(
            "attributes.current_level + 2",
            Attributes,
            new ValueSchema { Type = "integer" },
            ExpressionPurpose.Result);

        var materialized = _compiler.Materialize(
            compiled.CheckedExpression,
            Attributes,
            new ValueSchema { Type = "integer" },
            ExpressionPurpose.Result);

        Assert.Equal(
            12,
            materialized.Evaluate(JsonAttributes(("current_level", 10L))).GetInt64());

        var corruptedBytes = compiled.CheckedExpression.CheckedAst.ToArray();
        corruptedBytes[^1] ^= 0xff;
        Assert.Throws<ExpressionCompilationException>(() => _compiler.Materialize(
            compiled.CheckedExpression with { CheckedAst = corruptedBytes },
            Attributes,
            new ValueSchema { Type = "integer" },
            ExpressionPurpose.Result));
    }

    private static IReadOnlyDictionary<string, JsonElement> JsonAttributes(
        params (string Name, object Value)[] attributes) =>
        attributes.ToDictionary(
            attribute => attribute.Name,
            attribute => JsonSerializer.SerializeToElement(attribute.Value),
            StringComparer.Ordinal);
}
