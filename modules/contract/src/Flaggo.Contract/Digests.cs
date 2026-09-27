using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Nodes;
using Jcs.Net;

namespace Flaggo.Contract;

public interface IExpressionCanonicalizer
{
    string Canonicalize(string expression);
}

public static class ContractDigests
{
    public static DecisionContract CanonicalizeExpressions(
        DecisionContract contract,
        IExpressionCanonicalizer expressionCanonicalizer)
    {
        ArgumentNullException.ThrowIfNull(contract);
        ArgumentNullException.ThrowIfNull(expressionCanonicalizer);

        var authoredExecutable = contract.AuthoredExecutable is null
            ? null
            : contract.AuthoredExecutable with
            {
                Rules = contract.AuthoredExecutable.Rules
                    .Select(rule => rule with
                    {
                        When = rule.When is ExpressionWhen predicate
                            ? new ExpressionWhen(
                                expressionCanonicalizer.Canonicalize(predicate.Expression))
                            : rule.When,
                        Return = rule.Return is ExpressionReturn result
                            ? new ExpressionReturn(
                                expressionCanonicalizer.Canonicalize(result.Expression))
                            : rule.Return
                    })
                    .ToArray()
            };
        var learning = contract.Learning is null
            ? null
            : contract.Learning with
            {
                Objective = contract.Learning.Objective with
                {
                    Guardrails = contract.Learning.Objective.Guardrails?
                        .Select(guardrail => guardrail with
                        {
                            Expression = expressionCanonicalizer.Canonicalize(
                                guardrail.Expression)
                        })
                        .ToArray()
                }
            };
        return contract with
        {
            AuthoredExecutable = authoredExecutable,
            Learning = learning
        };
    }

    public static string ComputeContractDigest(
        DecisionContract contract,
        IExpressionCanonicalizer? expressionCanonicalizer = null)
    {
        var semanticContract = WithoutDescriptions(contract);
        var node = JsonSerializer.SerializeToNode(semanticContract, StrictJson.Options)
            ?? throw new InvalidOperationException("Contract serialization returned null.");
        NormalizeContract(node.AsObject(), expressionCanonicalizer);
        return Digest(node);
    }

    public static string ComputeExecutableDigest(
        DecisionExecutable executable,
        IExpressionCanonicalizer? expressionCanonicalizer = null)
    {
        var node = JsonSerializer.SerializeToNode(executable, StrictJson.Options)
            ?? throw new InvalidOperationException("Executable serialization returned null.");
        CanonicalizeExecutableExpressions(node.AsObject(), expressionCanonicalizer);
        return Digest(node);
    }

    public static bool IsSha256Digest(string? value) =>
        value is { Length: 71 }
        && value.StartsWith("sha256:", StringComparison.Ordinal)
        && value.AsSpan(7).IndexOfAnyExcept("0123456789abcdef") < 0;

    private static string Digest(JsonNode node)
    {
        using var document = JsonDocument.Parse(node.ToJsonString(StrictJson.Options));
        var canonical = JsonCanonicalizer.CanonicalizeToUtf8(document.RootElement);
        return $"sha256:{Convert.ToHexStringLower(SHA256.HashData(canonical))}";
    }

    private static void NormalizeContract(
        JsonObject contract,
        IExpressionCanonicalizer? expressionCanonicalizer)
    {
        if (contract["attributes"] is JsonArray attributes)
        {
            foreach (var attribute in attributes.OfType<JsonObject>())
            {
                NormalizeValueSchema(attribute["schema"]);
            }

            SortNamedArray(attributes);
        }

        if (contract["result"] is JsonObject result)
        {
            NormalizeValueSchema(result["schema"]);
        }

        if (contract["learning"] is JsonObject learning)
        {
            SortNamedArray(learning["evidence"]);
            if (learning["evidence"] is JsonArray evidence)
            {
                foreach (var item in evidence.OfType<JsonObject>())
                {
                    SortStringArray(item["correlateBy"]);
                }
            }

            if (learning["objective"] is JsonObject objective
                && objective["guardrails"] is JsonArray guardrails)
            {
                SortNamedArray(guardrails);
                foreach (var guardrail in guardrails.OfType<JsonObject>())
                {
                    CanonicalizeExpressionProperty(guardrail, "expression", expressionCanonicalizer);
                }
            }
        }

        if (contract["authoredExecutable"] is JsonObject authored
            && authored["rules"] is JsonArray rules)
        {
            CanonicalizeRuleExpressions(rules, expressionCanonicalizer);
        }
    }

    private static void NormalizeValueSchema(JsonNode? node)
    {
        if (node is not JsonObject schema)
        {
            return;
        }

        SortStringArray(schema["required"]);
        NormalizeValueSchema(schema["items"]);
        if (schema["properties"] is JsonObject properties)
        {
            foreach (var property in properties)
            {
                NormalizeValueSchema(property.Value);
            }
        }
    }

    private static void CanonicalizeExecutableExpressions(
        JsonObject executable,
        IExpressionCanonicalizer? expressionCanonicalizer)
    {
        if (executable["rules"] is JsonArray rules)
        {
            CanonicalizeRuleExpressions(rules, expressionCanonicalizer);
        }
    }

    private static void CanonicalizeRuleExpressions(
        JsonArray rules,
        IExpressionCanonicalizer? expressionCanonicalizer)
    {
        foreach (var rule in rules.OfType<JsonObject>())
        {
            if (rule["when"] is JsonObject when)
            {
                CanonicalizeExpressionProperty(when, "expression", expressionCanonicalizer);
            }

            if (rule["return"] is JsonObject result)
            {
                CanonicalizeExpressionProperty(result, "expression", expressionCanonicalizer);
            }
        }
    }

    private static void CanonicalizeExpressionProperty(
        JsonObject owner,
        string property,
        IExpressionCanonicalizer? expressionCanonicalizer)
    {
        if (owner[property] is not JsonValue value
            || !value.TryGetValue<string>(out var expression)
            || expressionCanonicalizer is null)
        {
            return;
        }

        owner[property] = expressionCanonicalizer.Canonicalize(expression);
    }

    private static DecisionContract WithoutDescriptions(DecisionContract contract) =>
        contract with
        {
            Attributes = contract.Attributes
                .Select(attribute => attribute with
                {
                    Schema = WithoutDescriptions(attribute.Schema)
                })
                .ToArray(),
            Result = contract.Result with
            {
                Schema = WithoutDescriptions(contract.Result.Schema)
            },
            AuthoredExecutable = contract.AuthoredExecutable is null
                ? null
                : contract.AuthoredExecutable with
                {
                    Rules = contract.AuthoredExecutable.Rules
                        .Select(rule => rule with { Description = null })
                        .ToArray()
                },
            Learning = contract.Learning is null
                ? null
                : contract.Learning with
                {
                    Evidence = contract.Learning.Evidence
                        .Select(evidence => evidence with { Description = null })
                        .ToArray(),
                    Objective = contract.Learning.Objective with
                    {
                        Guardrails = contract.Learning.Objective.Guardrails?
                            .Select(guardrail => guardrail with
                            {
                                Description = null
                            })
                            .ToArray()
                    }
                }
        };

    private static ValueSchema WithoutDescriptions(ValueSchema schema) =>
        schema with
        {
            Description = null,
            Items = schema.Items is null
                ? null
                : WithoutDescriptions(schema.Items),
            Properties = schema.Properties?.ToDictionary(
                property => property.Key,
                property => WithoutDescriptions(property.Value),
                StringComparer.Ordinal)
        };

    private static void SortNamedArray(JsonNode? node)
    {
        if (node is not JsonArray array)
        {
            return;
        }

        var sorted = array
            .Select(item => item?.DeepClone())
            .OrderBy(
                item => item?["name"]?.GetValue<string>(),
                StringComparer.Ordinal)
            .ToArray();
        array.Clear();
        foreach (var item in sorted)
        {
            array.Add(item);
        }
    }

    private static void SortStringArray(JsonNode? node)
    {
        if (node is not JsonArray array)
        {
            return;
        }

        var sorted = array
            .Select(item => item?.GetValue<string>())
            .OrderBy(value => value, StringComparer.Ordinal)
            .ToArray();
        array.Clear();
        foreach (var value in sorted)
        {
            array.Add(value);
        }
    }
}
