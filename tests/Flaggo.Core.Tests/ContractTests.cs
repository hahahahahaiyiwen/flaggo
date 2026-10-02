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
            "sha256:3c93eda4a9f8ab8b602a08644db59d19ab1219406b26b32aff8ef2c52c4c2040",
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
                  {
                    "name": "z",
                    "attribute": "z",
                    "correlateBy": ["z", "a"],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.z",
                      "correlation": {
                        "z": { "location": "signal", "attribute": "demo.z" },
                        "a": { "location": "signal", "attribute": "demo.a" }
                      }
                    }
                  },
                  {
                    "name": "a",
                    "attribute": "a",
                    "correlateBy": ["a"],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.a",
                      "correlation": {
                        "a": { "location": "signal", "attribute": "demo.a" }
                      }
                    }
                  }
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.failure",
                      "correlation": {}
                    }
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
    public void LearningEvidenceSourceMustSatisfyTheWireSchema()
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
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
            issue => issue.Code == "invalid-otel-name"
                && issue.Path == "/learning/evidence/0/source/scope");
    }

    [Fact]
    public void EveryEvidenceSourceKindIsStrictlyDeserialized()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "metric_value", "schema": { "type": "number" } },
                { "name": "log_value", "schema": { "type": "number" } },
                { "name": "span_value", "schema": { "type": "number" } },
                { "name": "event_value", "schema": { "type": "number" } }
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
                    "name": "metric",
                    "attribute": "metric_value",
                    "correlateBy": [],
                    "source": {
                      "kind": "metric",
                      "scope": "demo",
                      "name": "demo.metric",
                      "metricKind": "histogram",
                      "unit": "ms",
                      "correlation": {}
                    }
                  },
                  {
                    "name": "log",
                    "attribute": "log_value",
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.log",
                      "correlation": {}
                    }
                  },
                  {
                    "name": "span",
                    "attribute": "span_value",
                    "correlateBy": [],
                    "source": {
                      "kind": "span",
                      "scope": "demo",
                      "name": "demo.span",
                      "correlation": {}
                    }
                  },
                  {
                    "name": "event",
                    "attribute": "event_value",
                    "correlateBy": [],
                    "source": {
                      "kind": "spanEvent",
                      "scope": "demo",
                      "spanName": "demo.span",
                      "name": "demo.event",
                      "correlation": {}
                    }
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "metric",
                    "direction": "maximize"
                  }
                }
              }
            }
            """);

        Assert.Collection(
            contract.Learning!.Evidence,
            evidence => Assert.IsType<MetricEvidenceSource>(evidence.Source),
            evidence => Assert.IsType<LogEvidenceSource>(evidence.Source),
            evidence => Assert.IsType<SpanEvidenceSource>(evidence.Source),
            evidence => Assert.IsType<SpanEventEvidenceSource>(evidence.Source));
        Assert.Equal("valid", ContractValidator.Validate(contract).Status);
    }

    [Fact]
    public void EvidenceRequiresOneKnownSource()
    {
        const string missingSource = """
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
            """;
        var unknownSource = missingSource.Replace(
            "\"correlateBy\": []",
            """
            "correlateBy": [],
                    "source": {
                      "kind": "unknown",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
            """,
            StringComparison.Ordinal);

        Assert.Throws<JsonException>(() => ParseContract(missingSource));
        Assert.Throws<JsonException>(() => ParseContract(unknownSource));
    }

    [Fact]
    public void EvidenceSourceCorrelationMustExactlyMatchCorrelateBy()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "outcome", "schema": { "type": "integer" } },
                { "name": "session_id", "schema": { "type": "string" } }
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
                    "correlateBy": ["session_id"],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {
                        "unexpected": {
                          "location": "parentSpan",
                          "attribute": "demo.session.id"
                        }
                      }
                    }
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
            issue => issue.Code == "missing-evidence-correlation");
        Assert.Contains(
            validation.Issues,
            issue => issue.Code == "unexpected-evidence-correlation");
        Assert.Contains(
            validation.Issues,
            issue => issue.Code == "invalid-evidence-correlation-location");
    }

    [Fact]
    public void DuplicateEvidenceSourcesAreRejected()
    {
        var contract = ParseContract("""
            {
              "name": "demo.choice",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "first", "schema": { "type": "integer" } },
                { "name": "second", "schema": { "type": "integer" } }
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
                    "name": "first",
                    "attribute": "first",
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
                  },
                  {
                    "name": "second",
                    "attribute": "second",
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "first",
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
            issue => issue.Code == "duplicate-evidence-source"
                && issue.Path == "/learning/evidence/1/source");
    }

    [Fact]
    public void EvidenceSourceSemanticsContributeToTheContractDigest()
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
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
        var evidence = Assert.Single(contract.Learning!.Evidence);
        var changed = contract with
        {
            Learning = contract.Learning with
            {
                Evidence =
                [
                    evidence with
                    {
                        Source = new SpanEvidenceSource
                        {
                            Scope = evidence.Source.Scope,
                            Name = evidence.Source.Name,
                            Correlation = evidence.Source.Correlation
                        }
                    }
                ]
            }
        };

        Assert.NotEqual(
            ContractDigests.ComputeContractDigest(contract),
            ContractDigests.ComputeContractDigest(changed));
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
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "demo",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
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
