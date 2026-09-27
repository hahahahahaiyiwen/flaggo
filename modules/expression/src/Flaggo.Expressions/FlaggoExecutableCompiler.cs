using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Contract;

namespace Flaggo.Expressions;

public sealed partial class FlaggoExecutableCompiler(
    FlaggoExpressionCompiler expressionCompiler)
{
    public ExecutableCompilation Compile(
        DecisionContract contract,
        DecisionExecutable executable)
    {
        var issues = ContractValidator.ValidateExecutable(
            contract,
            executable,
            expressionCompiler);
        if (issues.Any(issue => issue.Severity == "error"))
        {
            throw new ExecutableCompilationException(issues);
        }

        var attributes = AttributeSchemas(contract);
        var compiledRules = new List<CompiledExecutableRule>(executable.Rules.Count);
        var checkedRules = new List<CheckedExecutableRule>(executable.Rules.Count);
        var normalizedRules = new List<ExecutableRule>(executable.Rules.Count);
        foreach (var rule in executable.Rules)
        {
            var predicate = expressionCompiler.Compile(
                ((ExpressionWhen)rule.When).Expression,
                attributes,
                BooleanSchema,
                ExpressionPurpose.Predicate);
            CompiledFlaggoExpression? resultExpression = null;
            RuleReturn normalizedReturn = rule.Return;
            CheckedFlaggoExpression? checkedResult = null;
            if (rule.Return is ExpressionReturn expression)
            {
                resultExpression = expressionCompiler.Compile(
                    expression.Expression,
                    attributes,
                    contract.Result.Schema,
                    ExpressionPurpose.Result);
                normalizedReturn = new ExpressionReturn(resultExpression.CanonicalExpression);
                checkedResult = resultExpression.CheckedExpression;
            }

            var normalizedRule = rule with
            {
                When = new ExpressionWhen(predicate.CanonicalExpression),
                Return = normalizedReturn
            };
            normalizedRules.Add(normalizedRule);
            checkedRules.Add(new CheckedExecutableRule
            {
                Name = rule.Name,
                Predicate = predicate.CheckedExpression,
                Result = checkedResult
            });
            compiledRules.Add(CreateCompiledRule(
                normalizedRule,
                predicate,
                resultExpression));
        }

        var normalizedExecutable = executable with { Rules = normalizedRules };
        var executableDigest = ContractDigests.ComputeExecutableDigest(normalizedExecutable);
        return new ExecutableCompilation(
            normalizedExecutable,
            executableDigest,
            new CheckedDecisionExecutable { Rules = checkedRules },
            new CompiledDecisionExecutable(
                normalizedExecutable,
                executableDigest,
                compiledRules));
    }

    public CompiledDecisionExecutable Materialize(
        DecisionContract contract,
        string contractDigest,
        DecisionExecutable executable,
        string executableDigest,
        CheckedDecisionExecutable checkedExecutable)
    {
        ValidateStoredShape(
            contract,
            contractDigest,
            executable,
            executableDigest,
            checkedExecutable);

        var attributes = AttributeSchemas(contract);
        var compiledRules = new List<CompiledExecutableRule>(executable.Rules.Count);
        for (var index = 0; index < executable.Rules.Count; index++)
        {
            var rule = executable.Rules[index];
            var checkedRule = checkedExecutable.Rules[index];
            var predicateSource = ((ExpressionWhen)rule.When).Expression;
            if (!string.Equals(
                    predicateSource,
                    checkedRule.Predicate.CanonicalExpression,
                    StringComparison.Ordinal))
            {
                throw new ExecutableMaterializationException(
                    $"Rule '{rule.Name}' predicate is not its stored canonical expression.");
            }

            var predicate = MaterializeExpression(
                checkedRule.Predicate,
                attributes,
                BooleanSchema,
                ExpressionPurpose.Predicate,
                rule.Name);
            CompiledFlaggoExpression? resultExpression = null;
            if (rule.Return is ExpressionReturn expression)
            {
                if (checkedRule.Result is null
                    || !string.Equals(
                        expression.Expression,
                        checkedRule.Result.CanonicalExpression,
                        StringComparison.Ordinal))
                {
                    throw new ExecutableMaterializationException(
                        $"Rule '{rule.Name}' return is not its stored canonical expression.");
                }

                resultExpression = MaterializeExpression(
                    checkedRule.Result,
                    attributes,
                    contract.Result.Schema,
                    ExpressionPurpose.Result,
                    rule.Name);
            }
            else if (checkedRule.Result is not null)
            {
                throw new ExecutableMaterializationException(
                    $"Literal rule '{rule.Name}' unexpectedly has a checked return expression.");
            }

            compiledRules.Add(CreateCompiledRule(rule, predicate, resultExpression));
        }

        return new CompiledDecisionExecutable(executable, executableDigest, compiledRules);
    }

    private CompiledFlaggoExpression MaterializeExpression(
        CheckedFlaggoExpression checkedExpression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose,
        string ruleName)
    {
        try
        {
            return expressionCompiler.Materialize(
                checkedExpression,
                attributes,
                expectedResult,
                purpose);
        }
        catch (ExpressionCompilationException exception)
        {
            throw new ExecutableMaterializationException(
                $"Rule '{ruleName}' contains an invalid checked expression.",
                exception);
        }
    }

    private static void ValidateStoredShape(
        DecisionContract contract,
        string contractDigest,
        DecisionExecutable executable,
        string executableDigest,
        CheckedDecisionExecutable checkedExecutable)
    {
        if (!ContractDigests.IsSha256Digest(contractDigest)
            || !string.Equals(
                executable.ContractDigest,
                contractDigest,
                StringComparison.Ordinal))
        {
            throw new ExecutableMaterializationException(
                "Executable contract identity does not match the accepted contract.");
        }

        if (!string.Equals(executable.Kind, "rules", StringComparison.Ordinal))
        {
            throw new ExecutableMaterializationException(
                $"Executable kind '{executable.Kind}' is not supported.");
        }

        if (executable.Rules.Count > ContractValidator.MaximumCollectionSize)
        {
            throw new ExecutableMaterializationException(
                $"Executable exceeds the {ContractValidator.MaximumCollectionSize}-rule limit.");
        }

        if (!string.Equals(
                ContractDigests.ComputeExecutableDigest(executable),
                executableDigest,
                StringComparison.Ordinal))
        {
            throw new ExecutableMaterializationException(
                "Executable digest does not match its canonical stored content.");
        }

        if (checkedExecutable.Rules.Count != executable.Rules.Count)
        {
            throw new ExecutableMaterializationException(
                "Checked rule count does not match the executable.");
        }

        var names = new HashSet<string>(StringComparer.Ordinal);
        var issues = new List<ValidationIssue>();
        for (var index = 0; index < executable.Rules.Count; index++)
        {
            var rule = executable.Rules[index];
            var checkedRule = checkedExecutable.Rules[index];
            if (rule.Name.Length is < 1 or > 128 || !MemberNamePattern().IsMatch(rule.Name))
            {
                throw new ExecutableMaterializationException(
                    $"Executable rule name '{rule.Name}' is invalid.");
            }

            if (!names.Add(rule.Name))
            {
                throw new ExecutableMaterializationException(
                    $"Executable rule name '{rule.Name}' is duplicated.");
            }

            if (!string.Equals(rule.Name, checkedRule.Name, StringComparison.Ordinal))
            {
                throw new ExecutableMaterializationException(
                    $"Checked rule at index {index} does not match executable rule '{rule.Name}'.");
            }

            if (rule.When is not ExpressionWhen)
            {
                throw new ExecutableMaterializationException(
                    $"Executable rule '{rule.Name}' has no deterministic predicate.");
            }

            if (rule.Return is LiteralReturn literal)
            {
                ValueSchemaValidator.ValidateValue(
                    contract.Result.Schema,
                    literal.Value,
                    $"/rules/{index}/return/value",
                    issues);
            }
            else if (rule.Return is not ExpressionReturn)
            {
                throw new ExecutableMaterializationException(
                    $"Executable rule '{rule.Name}' has an unsupported return.");
            }
        }

        if (issues.Count > 0)
        {
            throw new ExecutableMaterializationException(
                "Executable contains a literal result that violates the contract.");
        }
    }

    private static CompiledExecutableRule CreateCompiledRule(
        ExecutableRule rule,
        CompiledFlaggoExpression predicate,
        CompiledFlaggoExpression? resultExpression)
    {
        var referencedAttributes = new HashSet<string>(
            predicate.ReferencedAttributes,
            StringComparer.Ordinal);
        if (resultExpression is not null)
        {
            referencedAttributes.UnionWith(resultExpression.ReferencedAttributes);
        }

        return new CompiledExecutableRule(
            rule.Name,
            predicate,
            rule.Return,
            resultExpression,
            referencedAttributes);
    }

    private static IReadOnlyDictionary<string, ValueSchema> AttributeSchemas(
        DecisionContract contract) =>
        contract.Attributes.ToDictionary(
            attribute => attribute.Name,
            attribute => attribute.Schema,
            StringComparer.Ordinal);

    private static readonly ValueSchema BooleanSchema = new() { Type = "boolean" };

    [GeneratedRegex("^[A-Za-z][A-Za-z0-9._-]*$", RegexOptions.CultureInvariant)]
    private static partial Regex MemberNamePattern();
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record CheckedDecisionExecutable
{
    public required IReadOnlyList<CheckedExecutableRule> Rules { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record CheckedExecutableRule
{
    public required string Name { get; init; }

    public required CheckedFlaggoExpression Predicate { get; init; }

    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public CheckedFlaggoExpression? Result { get; init; }
}

public sealed record ExecutableCompilation(
    DecisionExecutable Executable,
    string ExecutableDigest,
    CheckedDecisionExecutable CheckedExecutable,
    CompiledDecisionExecutable CompiledExecutable);

public sealed record CompiledDecisionExecutable(
    DecisionExecutable Executable,
    string ExecutableDigest,
    IReadOnlyList<CompiledExecutableRule> Rules);

public sealed record CompiledExecutableRule(
    string Name,
    CompiledFlaggoExpression Predicate,
    RuleReturn Return,
    CompiledFlaggoExpression? ResultExpression,
    IReadOnlySet<string> ReferencedAttributes);

public sealed class ExecutableCompilationException : Exception
{
    public ExecutableCompilationException(IReadOnlyList<ValidationIssue> issues)
        : base(string.Join(
            "; ",
            issues
                .Where(issue => issue.Severity == "error")
                .Select(issue => $"{issue.Path}: {issue.Message}")))
    {
        Issues = issues;
    }

    public IReadOnlyList<ValidationIssue> Issues { get; }
}

public sealed class ExecutableMaterializationException : Exception
{
    public ExecutableMaterializationException(string message)
        : base(message)
    {
    }

    public ExecutableMaterializationException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}
