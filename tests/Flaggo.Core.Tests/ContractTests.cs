using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;

namespace Flaggo.Core.Tests;

public sealed class ContractTests
{
    [Fact]
    public void ValidFixtureProducesExpectedDigest()
    {
        var fixture = JsonDocument.Parse(
            File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "Fixtures", "validate-contract-valid.json")));
        var body = fixture.RootElement.GetProperty("request").GetProperty("body");
        var contract = StrictJson.Deserialize<DecisionContract>(Encoding.UTF8.GetBytes(body.GetRawText()));

        var validation = ContractValidator.Validate(contract, new FlaggoExpressionCompiler());

        Assert.Equal("valid", validation.Status);
        Assert.Empty(validation.Issues);
        Assert.Equal(
            "sha256:76837952bcc85c474be3f721cb6236a7e610ecdc38672e0149fc4c347816a7ae",
            validation.ContractDigest);
    }

    [Fact]
    public void AttributeAndEvidenceOrderAndDescriptionsAreNonSemantic()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "z", "schema": { "type": "integer" } },
                { "name": "a", "schema": { "type": "string" } }
              ],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              },
              "learning": {
                "policy": {
                  "mode": "auto-activation",
                  "evaluate": { "interval": "PT1M" }
                },
                "evidence": [
                  { "name": "z", "attribute": "z", "binding": "demo.z", "correlateBy": ["z", "a"] },
                  { "name": "a", "attribute": "a", "binding": "demo.a", "correlateBy": ["a"] }
                ],
                "objective": {
                  "primary": { "evidence": "z", "direction": "minimize" }
                }
              }
            }
            """);

        var reordered = contract with
        {
            Attributes = contract.Attributes
                .Reverse()
                .Select(attribute => attribute with
                {
                    Schema = attribute.Schema with
                    {
                        Description = "display only"
                    }
                })
                .ToArray(),
            Learning = contract.Learning! with
            {
                Evidence = contract.Learning.Evidence
                    .Reverse()
                    .Select(evidence => evidence with
                    {
                        Description = "display only",
                        CorrelateBy = evidence.CorrelateBy.Reverse().ToArray()
                    })
                    .ToArray()
            }
        };

        Assert.Equal(
            ContractDigests.ComputeContractDigest(contract),
            ContractDigests.ComputeContractDigest(reordered));
    }

    [Fact]
    public void ResultPropertiesNamedDescriptionRemainSemantic()
    {
        var first = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "description": { "type": "string", "maxLength": 10 }
                  },
                  "required": ["description"],
                  "additionalProperties": false
                },
                "default": { "description": "allow" }
              }
            }
            """);
        var second = first with
        {
            Result = first.Result with
            {
                Default = JsonSerializer.SerializeToElement(
                    new { description = "deny" })
            }
        };

        Assert.NotEqual(
            ContractDigests.ComputeContractDigest(first),
            ContractDigests.ComputeContractDigest(second));
    }

    [Fact]
    public void LiteralPropertiesNamedDescriptionRemainExecutableSemantic()
    {
        var contractDigest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        var first = new DecisionExecutable
        {
            ContractDigest = contractDigest,
            Rules =
            [
                new ExecutableRule
                {
                    Name = "choice",
                    When = new ExpressionWhen("attributes._random < 1.0"),
                    Return = new LiteralReturn(
                        JsonSerializer.SerializeToElement(
                            new { description = "allow" }))
                }
            ]
        };
        var second = first with
        {
            Rules =
            [
                first.Rules[0] with
                {
                    Return = new LiteralReturn(
                        JsonSerializer.SerializeToElement(
                            new { description = "deny" }))
                }
            ]
        };

        Assert.NotEqual(
            ContractDigests.ComputeExecutableDigest(first),
            ContractDigests.ComputeExecutableDigest(second));
    }

    [Fact]
    public void AuthoredExecutableCannotExceedExecutableRuleLimit()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              }
            }
            """) with
        {
            AuthoredExecutable = new AuthoredExecutable
            {
                Rules = Enumerable.Range(0, ContractValidator.MaximumCollectionSize + 1)
                    .Select(index => new AuthoredRule
                    {
                        Name = $"rule-{index}",
                        When = new ExpressionWhen("true"),
                        Return = new LiteralReturn(
                            JsonSerializer.SerializeToElement(true))
                    })
                    .ToArray()
            }
        };

        var validation = ContractValidator.Validate(contract);

        Assert.Equal("invalid", validation.Status);
        Assert.Contains(validation.Issues, issue => issue.Code == "too-many-rules");
    }

    [Fact]
    public void MalformedGuardrailIsRejectedBeforeDigestCalculation()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "failures", "schema": { "type": "integer" } }
              ],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              },
              "learning": {
                "policy": {
                  "mode": "auto-activation",
                  "evaluate": { "interval": "PT1M" }
                },
                "evidence": [
                  {
                    "name": "failure",
                    "attribute": "failures",
                    "binding": "demo.failure",
                    "correlateBy": []
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "failure",
                    "direction": "minimize"
                  },
                  "guardrails": [
                    { "name": "invalid", "expression": "sum(" }
                  ]
                }
              }
            }
            """);

        var validation = ContractValidator.Validate(
            contract,
            new FlaggoExpressionCompiler());

        Assert.Equal("invalid", validation.Status);
        Assert.Contains(
            validation.Issues,
            issue => issue.Code == "invalid-guardrail-expression");
    }

    [Fact]
    public void StrictJsonRejectsDuplicateAndUnknownMembers()
    {
        var duplicate = """
            {
              "name": "demo.choice",
              "name": "demo.other",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": { "schema": { "type": "boolean" }, "default": false }
            }
            """;
        var unknown = """
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": { "type": "string", "pattern": "^x" },
                "default": "x"
              }
            }
            """;

        Assert.Throws<JsonException>(() => ParseContract(duplicate));
        Assert.Throws<JsonException>(() => ParseContract(unknown));
    }

    [Fact]
    public void StrictJsonRejectsNullForRequiredReferenceMembers()
    {
        const string nullAttributes = """
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": null,
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              }
            }
            """;

        Assert.Throws<JsonException>(() => ParseContract(nullAttributes));
    }

    [Fact]
    public void InvalidDefaultIsReported()
    {
        var contract = ParseContract("""
            {
              "name": "demo.delay",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": { "type": "number", "minimum": 100, "maximum": 1000 },
                "default": 1200
              }
            }
            """);

        var validation = ContractValidator.Validate(contract);

        Assert.Equal("invalid", validation.Status);
        Assert.Contains(validation.Issues, issue => issue.Code == "default-out-of-schema");
    }

    [Fact]
    public void RuntimeInputAllowsMissingDeclaredAttributesButRejectsUndeclaredValues()
    {
        var contract = ParseContract("""
            {
              "name": "demo.delay",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "level", "schema": { "type": "integer", "minimum": 0 } }
              ],
              "result": {
                "schema": { "type": "number" },
                "default": 500
              }
            }
            """);
        var missing = StrictJson.Deserialize<RuntimeInput>(
            """{"attributes":{"_random":0.5}}"""u8);
        var undeclared = StrictJson.Deserialize<RuntimeInput>(
            """{"attributes":{"_random":0.5,"other":1}}"""u8);
        var missingIssues = new List<ValidationIssue>();
        var undeclaredIssues = new List<ValidationIssue>();

        ContractValidator.ValidateRuntimeInput(contract, missing, missingIssues);
        ContractValidator.ValidateRuntimeInput(contract, undeclared, undeclaredIssues);

        Assert.Empty(missingIssues);
        Assert.Contains(undeclaredIssues, issue => issue.Code == "undeclared-attribute");
    }

    private static DecisionContract ParseContract(string json) =>
        StrictJson.Deserialize<DecisionContract>(Encoding.UTF8.GetBytes(json));
}
