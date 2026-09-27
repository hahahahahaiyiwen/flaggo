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
    public void RequiredPropertiesAndGuardrailsAreUnordered()
    {
        var first = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                {
                  "name": "context",
                  "schema": {
                    "type": "object",
                    "properties": {
                      "outer_b": {
                        "type": "object",
                        "properties": {
                          "inner_b": { "type": "integer" },
                          "inner_a": { "type": "integer" }
                        },
                        "required": ["inner_b", "inner_a"],
                        "additionalProperties": false
                      },
                      "outer_a": { "type": "integer" }
                    },
                    "required": ["outer_b", "outer_a"],
                    "additionalProperties": false
                  }
                },
                { "name": "outcome", "schema": { "type": "integer" } }
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
                    "name": "outcome",
                    "attribute": "outcome",
                    "binding": "demo.outcome",
                    "correlateBy": []
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "outcome",
                    "direction": "maximize"
                  },
                  "guardrails": [
                    { "name": "z", "expression": "true" },
                    { "name": "a", "expression": "false" }
                  ]
                }
              }
            }
            """);
        var second = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                {
                  "name": "context",
                  "schema": {
                    "type": "object",
                    "properties": {
                      "outer_b": {
                        "type": "object",
                        "properties": {
                          "inner_b": { "type": "integer" },
                          "inner_a": { "type": "integer" }
                        },
                        "required": ["inner_a", "inner_b"],
                        "additionalProperties": false
                      },
                      "outer_a": { "type": "integer" }
                    },
                    "required": ["outer_a", "outer_b"],
                    "additionalProperties": false
                  }
                },
                { "name": "outcome", "schema": { "type": "integer" } }
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
                    "name": "outcome",
                    "attribute": "outcome",
                    "binding": "demo.outcome",
                    "correlateBy": []
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "outcome",
                    "direction": "maximize"
                  },
                  "guardrails": [
                    { "name": "a", "expression": "false" },
                    { "name": "z", "expression": "true" }
                  ]
                }
              }
            }
            """);

        Assert.Equal(
            ContractDigests.ComputeContractDigest(first),
            ContractDigests.ComputeContractDigest(second));
    }

    [Fact]
    public void RuleAndLiteralArrayOrderRemainSemantic()
    {
        var first = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": {
                  "type": "array",
                  "items": { "type": "integer" }
                },
                "default": [1, 2]
              },
              "authoredExecutable": {
                "rules": [
                  {
                    "name": "first",
                    "when": { "expression": "true" },
                    "return": { "value": [1] }
                  },
                  {
                    "name": "second",
                    "when": { "expression": "true" },
                    "return": { "value": [2] }
                  }
                ]
              }
            }
            """);
        var reversedRules = first with
        {
            AuthoredExecutable = first.AuthoredExecutable! with
            {
                Rules = first.AuthoredExecutable.Rules.Reverse().ToArray()
            }
        };
        var reversedDefault = first with
        {
            Result = first.Result with
            {
                Default = JsonSerializer.SerializeToElement(new[] { 2, 1 })
            }
        };
        var executable = new DecisionExecutable
        {
            ContractDigest =
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Rules =
            [
                new ExecutableRule
                {
                    Name = "first",
                    When = new ExpressionWhen("true"),
                    Return = new LiteralReturn(JsonSerializer.SerializeToElement(new[] { 1 }))
                },
                new ExecutableRule
                {
                    Name = "second",
                    When = new ExpressionWhen("true"),
                    Return = new LiteralReturn(JsonSerializer.SerializeToElement(new[] { 2 }))
                }
            ]
        };

        Assert.NotEqual(
            ContractDigests.ComputeContractDigest(first),
            ContractDigests.ComputeContractDigest(reversedRules));
        Assert.NotEqual(
            ContractDigests.ComputeContractDigest(first),
            ContractDigests.ComputeContractDigest(reversedDefault));
        Assert.NotEqual(
            ContractDigests.ComputeExecutableDigest(executable),
            ContractDigests.ComputeExecutableDigest(
                executable with { Rules = executable.Rules.Reverse().ToArray() }));
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
    public void LearningEvidenceBindingMustSatisfyTheWireSchema()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "outcome", "schema": { "type": "integer" } }
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
                    "name": "outcome",
                    "attribute": "outcome",
                    "binding": "not a valid binding",
                    "correlateBy": []
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "outcome",
                    "direction": "maximize"
                  }
                }
              }
            }
            """);

        var validation = ContractValidator.Validate(contract);

        Assert.Equal("invalid", validation.Status);
        Assert.Contains(
            validation.Issues,
            issue => issue.Code == "invalid-evidence-binding"
                && issue.Path == "/learning/evidence/0/binding");
    }

    [Fact]
    public void ValueSchemaBoundsMustSatisfyTheWireProfile()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                {
                  "name": "fractional_integer_bound",
                  "schema": { "type": "integer", "minimum": 0.5 }
                },
                {
                  "name": "long_string",
                  "schema": { "type": "string", "maxLength": 16385 }
                },
                {
                  "name": "large_array",
                  "schema": {
                    "type": "array",
                    "items": { "type": "boolean" },
                    "maxItems": 257
                  }
                },
                {
                  "name": "large_object",
                  "schema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                    "minProperties": 257
                  }
                }
              ],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              }
            }
            """);

        var validation = ContractValidator.Validate(contract);

        Assert.Equal("invalid", validation.Status);
        Assert.Contains(
            validation.Issues,
            issue => issue.Code == "non-integer-schema-keyword"
                && issue.Path == "/attributes/0/schema/minimum");
        Assert.Equal(
            3,
            validation.Issues.Count(issue => issue.Code == "invalid-schema-range"));
    }

    [Fact]
    public void LearningIntervalMustSatisfyTheWirePattern()
    {
        var invalidContract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "outcome", "schema": { "type": "integer" } }
              ],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              },
              "learning": {
                "policy": {
                  "mode": "auto-activation",
                  "evaluate": { "interval": "P1DT" }
                },
                "evidence": [
                  {
                    "name": "outcome",
                    "attribute": "outcome",
                    "binding": "demo.outcome",
                    "correlateBy": []
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "outcome",
                    "direction": "maximize"
                  }
                }
              }
            }
            """);
        var validWeekContract = invalidContract with
        {
            Learning = invalidContract.Learning! with
            {
                Policy = invalidContract.Learning.Policy with
                {
                    Evaluate = new LearningEvaluationPolicy
                    {
                        Interval = "P1W"
                    }
                }
            }
        };

        var invalidValidation = ContractValidator.Validate(invalidContract);
        var validWeekValidation = ContractValidator.Validate(validWeekContract);

        Assert.Equal("invalid", invalidValidation.Status);
        Assert.Contains(
            invalidValidation.Issues,
            issue => issue.Code == "invalid-learning-interval");
        Assert.Equal("valid", validWeekValidation.Status);
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
