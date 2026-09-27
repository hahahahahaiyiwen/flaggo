using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;

namespace Flaggo.Decision;

public sealed class DecisionEvaluator
{
    public RuntimeDecision Evaluate(
        DecisionContract contract,
        CompiledDecisionExecutable executable,
        RuntimeInput input,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(contract);
        ArgumentNullException.ThrowIfNull(executable);
        ArgumentNullException.ThrowIfNull(input);

        foreach (var rule in executable.Rules)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (rule.ReferencedAttributes.Any(
                    attribute => !input.Attributes.ContainsKey(attribute)))
            {
                continue;
            }

            JsonElement predicate;
            try
            {
                predicate = rule.Predicate.Evaluate(input.Attributes, cancellationToken);
            }
            catch (ExpressionEvaluationException exception)
            {
                throw new DecisionEvaluationException(
                    rule.Name,
                    "predicate",
                    exception);
            }

            if (predicate.ValueKind is not JsonValueKind.True
                and not JsonValueKind.False)
            {
                throw new DecisionEvaluationException(
                    rule.Name,
                    "predicate",
                    "Predicate did not produce a boolean value.");
            }

            if (!predicate.GetBoolean())
            {
                continue;
            }

            JsonElement result;
            try
            {
                result = rule.Return switch
                {
                    LiteralReturn literal => literal.Value.Clone(),
                    ExpressionReturn when rule.ResultExpression is not null =>
                        rule.ResultExpression.Evaluate(input.Attributes, cancellationToken),
                    _ => throw new InvalidOperationException(
                        $"Compiled rule '{rule.Name}' has no executable return.")
                };
            }
            catch (ExpressionEvaluationException exception)
            {
                throw new DecisionEvaluationException(
                    rule.Name,
                    "return",
                    exception);
            }

            ValidateResult(contract, result, rule.Name);
            return new RuntimeDecision
            {
                ContractDigest = executable.Executable.ContractDigest,
                ExecutableDigest = executable.ExecutableDigest,
                Result = result,
                Evaluation = new RuleEvaluation(rule.Name)
            };
        }

        var defaultResult = contract.Result.Default.Clone();
        ValidateResult(contract, defaultResult, ruleName: null);
        return new RuntimeDecision
        {
            ContractDigest = executable.Executable.ContractDigest,
            ExecutableDigest = executable.ExecutableDigest,
            Result = defaultResult,
            Evaluation = new DefaultEvaluation()
        };
    }

    private static void ValidateResult(
        DecisionContract contract,
        JsonElement result,
        string? ruleName)
    {
        var issues = new List<ValidationIssue>();
        ValueSchemaValidator.ValidateValue(
            contract.Result.Schema,
            result,
            "/result",
            issues);
        if (issues.Count > 0)
        {
            throw new DecisionResultValidationException(ruleName, issues);
        }
    }
}

public sealed class DecisionEvaluationException : Exception
{
    public DecisionEvaluationException(
        string ruleName,
        string expressionRole,
        Exception innerException)
        : base(
            $"Rule '{ruleName}' {expressionRole} evaluation failed.",
            innerException)
    {
        RuleName = ruleName;
        ExpressionRole = expressionRole;
    }

    public DecisionEvaluationException(
        string ruleName,
        string expressionRole,
        string message)
        : base($"Rule '{ruleName}' {expressionRole} evaluation failed: {message}")
    {
        RuleName = ruleName;
        ExpressionRole = expressionRole;
    }

    public string RuleName { get; }

    public string ExpressionRole { get; }
}

public sealed class DecisionResultValidationException(
    string? ruleName,
    IReadOnlyList<ValidationIssue> issues)
    : Exception(
        ruleName is null
            ? "The contract default violates the result schema."
            : $"Rule '{ruleName}' produced a result that violates the result schema.")
{
    public string? RuleName { get; } = ruleName;

    public IReadOnlyList<ValidationIssue> Issues { get; } = issues;
}
