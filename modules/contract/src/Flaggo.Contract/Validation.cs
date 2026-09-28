using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Flaggo.Contract;

public interface IContractExpressionValidator : IExpressionCanonicalizer
{
    ExpressionValidation Validate(
        string expression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose);

    ExpressionValidation ValidateSyntax(string expression);
}

public enum ExpressionPurpose
{
    Predicate,
    Result,
    Guardrail
}

public sealed record ExpressionValidation(
    bool IsValid,
    string? CanonicalExpression,
    IReadOnlySet<string> ReferencedAttributes,
    string? Error);

public static partial class ContractValidator
{
    public const int MaximumAttributes = 128;
    public const int MaximumCollectionSize = 256;

    public static bool IsDecisionName(string? value) =>
        value is { Length: >= 1 and <= 128 }
        && DecisionNamePattern().IsMatch(value);

    public static DecisionContractValidationResult Validate(
        DecisionContract contract,
        IContractExpressionValidator? expressionValidator = null)
    {
        ArgumentNullException.ThrowIfNull(contract);

        var issues = new List<ValidationIssue>();
        ValidateDecisionName(contract.Name, "/name", issues);
        if (!string.Equals(contract.ExpressionSyntax, "flaggo.cel/v1", StringComparison.Ordinal))
        {
            Error(issues, "unsupported-expression-syntax", "/expression_syntax",
                "expression_syntax must be flaggo.cel/v1.");
        }

        if (contract.Attributes.Count > MaximumAttributes)
        {
            Error(issues, "too-many-attributes", "/attributes",
                $"A contract may declare at most {MaximumAttributes} attributes.");
        }

        var attributes = new Dictionary<string, ValueSchema>(StringComparer.Ordinal);
        for (var index = 0; index < contract.Attributes.Count; index++)
        {
            var attribute = contract.Attributes[index];
            var path = $"/attributes/{index}";
            if (attribute is null)
            {
                Error(issues, "null-attribute", path,
                    "Attribute declarations must not be null.");
                continue;
            }

            ValidateAttributeName(attribute.Name, $"{path}/name", issues);
            if (attribute.Schema is null)
            {
                Error(issues, "null-attribute-schema", $"{path}/schema",
                    "Attribute schemas must not be null.");
            }
            else if (attribute.Name is not null
                && !attributes.TryAdd(attribute.Name, attribute.Schema))
            {
                Error(issues, "duplicate-attribute", $"{path}/name",
                    $"Attribute '{attribute.Name}' is declared more than once.");
            }

            if (attribute.Schema is not null)
            {
                ValueSchemaValidator.ValidateSchema(
                    attribute.Schema,
                    $"{path}/schema",
                    issues);
            }
        }

        ValueSchemaValidator.ValidateSchema(contract.Result.Schema, "/result/schema", issues);
        var defaultIssues = new List<ValidationIssue>();
        ValueSchemaValidator.ValidateValue(
            contract.Result.Schema,
            contract.Result.Default,
            "/result/default",
            defaultIssues);
        if (defaultIssues.Count > 0)
        {
            Error(
                issues,
                "default-out-of-schema",
                "/result/default",
                "The default value does not satisfy result.schema.");
        }

        ValidateAuthoredExecutable(contract, attributes, expressionValidator, issues);
        ValidateLearning(contract, attributes, expressionValidator, issues);

        if (issues.Any(issue => issue.Severity == "error"))
        {
            return new DecisionContractValidationResult
            {
                Status = "invalid",
                Issues = issues
            };
        }

        return new DecisionContractValidationResult
        {
            Status = "valid",
            ContractDigest = ContractDigests.ComputeContractDigest(contract, expressionValidator),
            Issues = issues
        };
    }

    public static void ValidateRuntimeInput(
        DecisionContract contract,
        RuntimeInput input,
        ICollection<ValidationIssue> issues)
    {
        if (input.Attributes.Count > MaximumAttributes + 1)
        {
            Error(issues, "too-many-attributes", "/attributes",
                $"Runtime input may contain at most {MaximumAttributes + 1} attributes.");
        }

        if (!input.Attributes.TryGetValue("_random", out var random)
            || random.ValueKind != JsonValueKind.Number
            || !random.TryGetDouble(out var randomValue)
            || !double.IsFinite(randomValue)
            || randomValue < 0
            || randomValue >= 1)
        {
            Error(issues, "invalid-internal-attribute", "/attributes/_random",
                "_random is required and must be a finite number in [0, 1).");
        }

        var declarations = contract.Attributes.ToDictionary(
            attribute => attribute.Name,
            attribute => attribute.Schema,
            StringComparer.Ordinal);
        foreach (var (name, value) in input.Attributes)
        {
            if (name == "_random")
            {
                continue;
            }

            if (name.StartsWith('_'))
            {
                Error(issues, "unknown-internal-attribute", $"/attributes/{EscapePointer(name)}",
                    $"Internal attribute '{name}' is not defined by this contract profile.");
                continue;
            }

            if (!declarations.TryGetValue(name, out var schema))
            {
                Error(issues, "undeclared-attribute", $"/attributes/{EscapePointer(name)}",
                    $"Attribute '{name}' is not declared by the contract.");
                continue;
            }

            ValueSchemaValidator.ValidateValue(
                schema,
                value,
                $"/attributes/{EscapePointer(name)}",
                issues);
        }

        if (input.CurrentExposure is not null)
        {
            var bytes = Encoding.UTF8.GetByteCount(input.CurrentExposure.ExposureId);
            if (bytes is < 1 or > 256)
            {
                Error(issues, "invalid-current-exposure", "/currentExposure/exposureId",
                    "exposureId must contain between 1 and 256 UTF-8 bytes.");
            }
        }
    }

    public static IReadOnlyList<ValidationIssue> ValidateExecutable(
        DecisionContract contract,
        DecisionExecutable executable,
        IContractExpressionValidator? expressionValidator = null)
    {
        ArgumentNullException.ThrowIfNull(contract);
        ArgumentNullException.ThrowIfNull(executable);

        var contractValidation = Validate(contract, expressionValidator);
        var issues = new List<ValidationIssue>(contractValidation.Issues);
        if (contractValidation.Status != "valid")
        {
            return issues;
        }

        if (!string.Equals(
                    executable.ContractDigest,
                    contractValidation.ContractDigest,
                    StringComparison.Ordinal))
        {
            Error(
                    issues,
                    "contract-digest-mismatch",
                    "/contractDigest",
                    "Executable contractDigest does not match the supplied contract.");
        }

        if (!string.Equals(executable.Kind, "rules", StringComparison.Ordinal))
        {
            Error(
                    issues,
                    "unsupported-executable-kind",
                    "/kind",
                    "Executable kind must be rules.");
        }

        if (executable.Rules.Count > MaximumCollectionSize)
        {
            Error(
                    issues,
                    "too-many-rules",
                    "/rules",
                    $"An executable may contain at most {MaximumCollectionSize} rules.");
        }

        var attributes = contract.Attributes.ToDictionary(
            attribute => attribute.Name,
            attribute => attribute.Schema,
            StringComparer.Ordinal);
        var names = new HashSet<string>(StringComparer.Ordinal);
        for (var index = 0; index < executable.Rules.Count; index++)
        {
            var rule = executable.Rules[index];
            var path = $"/rules/{index}";
            ValidateMemberName(rule.Name, $"{path}/name", issues);
            if (!names.Add(rule.Name))
            {
                    Error(
                        issues,
                        "duplicate-rule",
                        $"{path}/name",
                        $"Rule '{rule.Name}' is declared more than once.");
            }

            if (rule.When is not ExpressionWhen predicate)
            {
                    Error(
                        issues,
                        "invalid-executable-predicate",
                        $"{path}/when",
                        "Executable rules require a deterministic expression predicate.");
            }
            else
            {
                    ValidateExpression(
                        predicate.Expression,
                        attributes,
                        BooleanSchema,
                        ExpressionPurpose.Predicate,
                        $"{path}/when/expression",
                        expressionValidator,
                        issues);
            }

            switch (rule.Return)
            {
                    case LiteralReturn literal:
                        ValueSchemaValidator.ValidateValue(
                            contract.Result.Schema,
                            literal.Value,
                            $"{path}/return/value",
                            issues);
                        break;
                    case ExpressionReturn expression:
                        ValidateExpression(
                            expression.Expression,
                            attributes,
                            contract.Result.Schema,
                            ExpressionPurpose.Result,
                            $"{path}/return/expression",
                            expressionValidator,
                            issues);
                        break;
                    default:
                        Error(
                            issues,
                            "invalid-executable-return",
                            $"{path}/return",
                            "Executable rules require one literal or deterministic expression return.");
                        break;
            }
        }

        return issues;
    }

    private static void ValidateAuthoredExecutable(
        DecisionContract contract,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        IContractExpressionValidator? expressionValidator,
        ICollection<ValidationIssue> issues)
    {
        if (contract.AuthoredExecutable is null)
        {
            return;
        }

        if (contract.AuthoredExecutable.Rules.Count == 0)
        {
            Error(issues, "empty-authored-executable", "/authoredExecutable/rules",
                "An authored executable must contain at least one rule.");
        }

        if (contract.AuthoredExecutable.Rules.Count > MaximumCollectionSize)
        {
            Error(issues, "too-many-rules", "/authoredExecutable/rules",
                $"An authored executable may contain at most {MaximumCollectionSize} rules.");
        }

        var names = new HashSet<string>(StringComparer.Ordinal);
        for (var index = 0; index < contract.AuthoredExecutable.Rules.Count; index++)
        {
            var rule = contract.AuthoredExecutable.Rules[index];
            var path = $"/authoredExecutable/rules/{index}";
            if (rule is null)
            {
                Error(issues, "null-rule", path,
                    "Authored rules must not be null.");
                continue;
            }

            ValidateMemberName(rule.Name, $"{path}/name", issues);
            if (!names.Add(rule.Name))
            {
                Error(issues, "duplicate-rule", $"{path}/name",
                    $"Rule '{rule.Name}' is declared more than once.");
            }

            if (rule.When is ExpressionWhen predicate)
            {
                ValidateExpression(predicate.Expression, attributes, BooleanSchema,
                    ExpressionPurpose.Predicate, $"{path}/when/expression",
                    expressionValidator, issues);
            }
            else if (rule.When is NaturalLanguageWhen condition
                && string.IsNullOrWhiteSpace(condition.Condition))
            {
                Error(issues, "invalid-authored-condition", $"{path}/when/condition",
                    "Authored rule conditions must not be empty.");
            }
            else if (rule.When is not NaturalLanguageWhen)
            {
                Error(issues, "invalid-authored-predicate", $"{path}/when",
                    "Authored rules require one condition or expression predicate.");
            }

            switch (rule.Return)
            {
                case LiteralReturn literal:
                    ValueSchemaValidator.ValidateValue(
                        contract.Result.Schema,
                        literal.Value,
                        $"{path}/return/value",
                        issues);
                    break;
                case ExpressionReturn expression:
                    ValidateExpression(expression.Expression, attributes, contract.Result.Schema,
                        ExpressionPurpose.Result, $"{path}/return/expression",
                        expressionValidator, issues);
                    break;
            }
        }
    }

    private static void ValidateLearning(
        DecisionContract contract,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        IContractExpressionValidator? expressionValidator,
        ICollection<ValidationIssue> issues)
    {
        if (contract.Learning is null)
        {
            return;
        }

        if (contract.Learning.Policy.Mode != "auto-activation")
        {
            Error(issues, "unsupported-learning-policy", "/learning/policy/mode",
                "The initial learning policy mode is auto-activation.");
        }

        var interval = contract.Learning.Policy.Evaluate.Interval;
        if (!LearningIntervalPattern().IsMatch(interval)
            || !interval.Any(character => character is >= '1' and <= '9'))
        {
            Error(issues, "invalid-learning-interval", "/learning/policy/evaluate/interval",
                "The learning interval must be a positive ISO 8601 duration.");
        }

        if (contract.Learning.Evidence.Count == 0)
        {
            Error(issues, "empty-evidence", "/learning/evidence",
                "A learning declaration must contain at least one evidence entry.");
        }

        var evidenceNames = new HashSet<string>(StringComparer.Ordinal);
        for (var index = 0; index < contract.Learning.Evidence.Count; index++)
        {
            var evidence = contract.Learning.Evidence[index];
            var path = $"/learning/evidence/{index}";
            if (evidence is null)
            {
                Error(issues, "null-evidence", path,
                    "Evidence declarations must not be null.");
                continue;
            }

            ValidateMemberName(evidence.Name, $"{path}/name", issues);
            if (!evidenceNames.Add(evidence.Name))
            {
                Error(issues, "duplicate-evidence", $"{path}/name",
                    $"Evidence '{evidence.Name}' is declared more than once.");
            }

            if (string.IsNullOrEmpty(evidence.Attribute)
                || !attributes.ContainsKey(evidence.Attribute))
            {
                Error(issues, "unknown-evidence-attribute", $"{path}/attribute",
                    $"Evidence attribute '{evidence.Attribute}' is not declared.");
            }

            ValidateEvidenceBinding(evidence.Binding, $"{path}/binding", issues);

            var correlations = new HashSet<string>(StringComparer.Ordinal);
            for (var correlationIndex = 0; correlationIndex < evidence.CorrelateBy.Count; correlationIndex++)
            {
                var correlation = evidence.CorrelateBy[correlationIndex];
                if (string.IsNullOrEmpty(correlation))
                {
                    Error(issues, "invalid-correlation-attribute",
                        $"{path}/correlateBy/{correlationIndex}",
                        "Correlation attribute names must not be null or empty.");
                    continue;
                }

                if (!correlations.Add(correlation))
                {
                    Error(issues, "duplicate-correlation-attribute",
                        $"{path}/correlateBy/{correlationIndex}",
                        $"Correlation attribute '{correlation}' is repeated.");
                }

                if (!attributes.ContainsKey(correlation))
                {
                    Error(issues, "unknown-correlation-attribute",
                        $"{path}/correlateBy/{correlationIndex}",
                        $"Correlation attribute '{correlation}' is not declared.");
                }
            }
        }

        if (!evidenceNames.Contains(contract.Learning.Objective.Primary.Evidence))
        {
            Error(issues, "unknown-primary-evidence", "/learning/objective/primary/evidence",
                "The primary objective must reference declared evidence.");
        }

        if (contract.Learning.Objective.Primary.Direction is not ("minimize" or "maximize"))
        {
            Error(issues, "invalid-objective-direction", "/learning/objective/primary/direction",
                "Objective direction must be minimize or maximize.");
        }

        var guardrailNames = new HashSet<string>(StringComparer.Ordinal);
        var guardrails = contract.Learning.Objective.Guardrails ?? [];
        for (var index = 0; index < guardrails.Count; index++)
        {
            var guardrail = guardrails[index];
            var path = $"/learning/objective/guardrails/{index}";
            if (guardrail is null)
            {
                Error(issues, "null-guardrail", path,
                    "Guardrails must not be null.");
                continue;
            }

            ValidateMemberName(guardrail.Name, $"{path}/name", issues);
            if (!guardrailNames.Add(guardrail.Name))
            {
                Error(issues, "duplicate-guardrail", $"{path}/name",
                    $"Guardrail '{guardrail.Name}' is declared more than once.");
            }

            if (string.IsNullOrWhiteSpace(guardrail.Expression))
            {
                Error(issues, "invalid-guardrail-expression", $"{path}/expression",
                    "Guardrail expression must not be empty.");
            }
            else if (expressionValidator is not null)
            {
                var result = expressionValidator.ValidateSyntax(guardrail.Expression);
                if (!result.IsValid)
                {
                    Error(
                        issues,
                        "invalid-guardrail-expression",
                        $"{path}/expression",
                        result.Error ?? "Guardrail expression is invalid.");
                }
            }
        }
    }

    private static void ValidateExpression(
        string? expression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose,
        string path,
        IContractExpressionValidator? expressionValidator,
        ICollection<ValidationIssue> issues)
    {
        if (string.IsNullOrWhiteSpace(expression))
        {
            Error(issues, "invalid-expression", path,
                "Expression must not be null or empty.");
            return;
        }

        if (Encoding.UTF8.GetByteCount(expression) > 4096)
        {
            Error(issues, "expression-too-large", path,
                "Expression exceeds the 4096-byte limit.");
            return;
        }

        if (expressionValidator is null)
        {
            return;
        }

        var result = expressionValidator.Validate(expression, attributes, expectedResult, purpose);
        if (!result.IsValid)
        {
            Error(issues, "invalid-expression", path, result.Error ?? "Expression is invalid.");
        }
    }

    private static void ValidateDecisionName(
        string? value,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (!IsDecisionName(value))
        {
            Error(issues, "invalid-decision-name", path,
                "Decision name must match ^[A-Za-z][A-Za-z0-9._-]*$.");
        }
    }

    private static void ValidateAttributeName(
        string? value,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (value is null
            || value.Length is < 1 or > 128
            || !AttributeNamePattern().IsMatch(value))
        {
            Error(issues, "invalid-attribute-name", path,
                "Attribute name must match ^[A-Za-z][A-Za-z0-9_]*$.");
        }
    }

    private static void ValidateMemberName(
        string? value,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (value is null
            || value.Length is < 1 or > 128
            || !DecisionNamePattern().IsMatch(value))
        {
            Error(issues, "invalid-member-name", path,
                "Member name must match ^[A-Za-z][A-Za-z0-9._-]*$.");
        }
    }

    private static void ValidateEvidenceBinding(
        string? value,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (value is null
            || value.Length is < 1 or > 256
            || !DecisionNamePattern().IsMatch(value))
        {
            Error(issues, "invalid-evidence-binding", path,
                "Evidence binding must match ^[A-Za-z][A-Za-z0-9._-]*$ and contain at most 256 characters.");
        }
    }

    internal static void Error(
        ICollection<ValidationIssue> issues,
        string code,
        string path,
        string message) =>
        issues.Add(new ValidationIssue
        {
            Code = code,
            Severity = "error",
            Path = path,
            Message = message
        });

    internal static string EscapePointer(string value) =>
        value.Replace("~", "~0", StringComparison.Ordinal)
            .Replace("/", "~1", StringComparison.Ordinal);

    private static readonly ValueSchema BooleanSchema = new() { Type = "boolean" };

    [GeneratedRegex("^[A-Za-z][A-Za-z0-9._-]*$", RegexOptions.CultureInvariant)]
    private static partial Regex DecisionNamePattern();

    [GeneratedRegex("^[A-Za-z][A-Za-z0-9_]*$", RegexOptions.CultureInvariant)]
    private static partial Regex AttributeNamePattern();

    [GeneratedRegex(
        "^P(?=\\d|T\\d)(?:\\d+Y)?(?:\\d+M)?(?:\\d+W)?(?:\\d+D)?(?:T(?=\\d)(?:\\d+H)?(?:\\d+M)?(?:\\d+(?:\\.\\d+)?S)?)?$",
        RegexOptions.CultureInvariant)]
    private static partial Regex LearningIntervalPattern();
}

public static class ValueSchemaValidator
{
    public static void ValidateSchema(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues,
        int depth = 0)
    {
        if (depth > StrictJson.MaximumDepth)
        {
            ContractValidator.Error(issues, "schema-too-deep", path,
                $"Value schema exceeds {StrictJson.MaximumDepth} levels.");
            return;
        }

        switch (schema.Type)
        {
            case "null":
            case "boolean":
                RejectNumericAndCollectionKeywords(schema, path, issues);
                break;
            case "integer":
                ValidateIntegerSchema(schema, path, issues);
                ValidateNumericSchema(schema, path, issues);
                break;
            case "number":
                ValidateNumericSchema(schema, path, issues);
                break;
            case "string":
                ValidateRange(
                    schema.MinLength,
                    schema.MaxLength,
                    StrictJson.MaximumStringBytes,
                    path,
                    "length",
                    issues);
                RejectNumericKeywords(schema, path, issues);
                RejectCollectionKeywords(schema, path, issues);
                break;
            case "array":
                if (schema.Items is null)
                {
                    ContractValidator.Error(issues, "missing-items", $"{path}/items",
                        "Array schemas require items.");
                }
                else
                {
                    ValidateSchema(schema.Items, $"{path}/items", issues, depth + 1);
                }

                ValidateRange(
                    schema.MinItems,
                    schema.MaxItems,
                    ContractValidator.MaximumCollectionSize,
                    path,
                    "items",
                    issues);
                RejectNumericKeywords(schema, path, issues);
                RejectObjectKeywords(schema, path, issues);
                break;
            case "object":
                if (schema.Properties is null || schema.AdditionalProperties is not false)
                {
                    ContractValidator.Error(issues, "open-object-schema", path,
                        "Object schemas require properties and additionalProperties: false.");
                }
                else
                {
                    if (schema.Properties.Count > ContractValidator.MaximumCollectionSize)
                    {
                        ContractValidator.Error(issues, "too-many-schema-properties",
                            $"{path}/properties", "Object schema has too many properties.");
                    }

                    foreach (var property in schema.Properties)
                    {
                        var propertyPath =
                            $"{path}/properties/{ContractValidator.EscapePointer(property.Key)}";
                        if (property.Value is null)
                        {
                            ContractValidator.Error(
                                issues,
                                "null-property-schema",
                                propertyPath,
                                "Property schemas must not be null.");
                        }
                        else
                        {
                            ValidateSchema(
                                property.Value,
                                propertyPath,
                                issues,
                                depth + 1);
                        }
                    }
                }

                var required = schema.Required ?? [];
                if (required.Count != required.Distinct(StringComparer.Ordinal).Count())
                {
                    ContractValidator.Error(issues, "duplicate-required-property",
                        $"{path}/required", "Required property names must be unique.");
                }

                if (schema.Properties is not null)
                {
                    foreach (var name in required)
                    {
                        if (string.IsNullOrEmpty(name))
                        {
                            ContractValidator.Error(
                                issues,
                                "invalid-required-property",
                                $"{path}/required",
                                "Required property names must not be null or empty.");
                        }
                        else if (!schema.Properties.ContainsKey(name))
                        {
                            ContractValidator.Error(
                                issues,
                                "unknown-required-property",
                                $"{path}/required",
                                $"Required property '{name}' is not declared.");
                        }
                    }
                }

                ValidateRange(
                    schema.MinProperties,
                    schema.MaxProperties,
                    ContractValidator.MaximumCollectionSize,
                    path,
                    "properties",
                    issues);
                RejectNumericKeywords(schema, path, issues);
                RejectArrayKeywords(schema, path, issues);
                break;
            default:
                ContractValidator.Error(issues, "unsupported-value-type", $"{path}/type",
                    $"Value schema type '{schema.Type}' is not supported.");
                break;
        }
    }

    public static void ValidateValue(
        ValueSchema schema,
        JsonElement value,
        string path,
        ICollection<ValidationIssue> issues,
        int depth = 0)
    {
        if (depth > StrictJson.MaximumDepth)
        {
            ContractValidator.Error(issues, "value-too-deep", path,
                $"Value exceeds {StrictJson.MaximumDepth} levels.");
            return;
        }

        switch (schema.Type)
        {
            case "null":
                RequireKind(value, JsonValueKind.Null, path, issues);
                break;
            case "boolean":
                if (value.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                {
                    TypeError(path, "boolean", issues);
                }

                break;
            case "integer":
                if (value.ValueKind != JsonValueKind.Number || !value.TryGetInt64(out var integer))
                {
                    TypeError(path, "signed 64-bit integer", issues);
                    break;
                }

                ValidateNumber(integer, schema, path, issues);
                break;
            case "number":
                if (value.ValueKind != JsonValueKind.Number
                    || !value.TryGetDouble(out var number)
                    || !double.IsFinite(number))
                {
                    TypeError(path, "finite number", issues);
                    break;
                }

                ValidateNumber(number, schema, path, issues);
                break;
            case "string":
                if (value.ValueKind != JsonValueKind.String)
                {
                    TypeError(path, "string", issues);
                    break;
                }

                var text = value.GetString()!;
                var runeLength = text.EnumerateRunes().Count();
                if (Encoding.UTF8.GetByteCount(text) > StrictJson.MaximumStringBytes
                    || schema.MinLength is { } minLength && runeLength < minLength
                    || schema.MaxLength is { } maxLength && runeLength > maxLength)
                {
                    ContractValidator.Error(issues, "string-out-of-range", path,
                        "String does not satisfy the configured length bounds.");
                }

                break;
            case "array":
                if (value.ValueKind != JsonValueKind.Array)
                {
                    TypeError(path, "array", issues);
                    break;
                }

                var items = value.EnumerateArray().ToArray();
                if (items.Length > ContractValidator.MaximumCollectionSize
                    || schema.MinItems is { } minItems && items.Length < minItems
                    || schema.MaxItems is { } maxItems && items.Length > maxItems)
                {
                    ContractValidator.Error(issues, "array-out-of-range", path,
                        "Array does not satisfy the configured item bounds.");
                }

                if (schema.UniqueItems is true)
                {
                    var unique = new HashSet<string>(StringComparer.Ordinal);
                    if (items.Any(item => !unique.Add(Jcs.Net.JsonCanonicalizer.Canonicalize(item))))
                    {
                        ContractValidator.Error(issues, "array-items-not-unique", path,
                            "Array items must be unique.");
                    }
                }

                if (schema.Items is not null)
                {
                    for (var index = 0; index < items.Length; index++)
                    {
                        ValidateValue(schema.Items, items[index], $"{path}/{index}", issues, depth + 1);
                    }
                }

                break;
            case "object":
                if (value.ValueKind != JsonValueKind.Object)
                {
                    TypeError(path, "object", issues);
                    break;
                }

                var properties = value.EnumerateObject().ToArray();
                if (properties.Length > ContractValidator.MaximumCollectionSize
                    || schema.MinProperties is { } minProperties && properties.Length < minProperties
                    || schema.MaxProperties is { } maxProperties && properties.Length > maxProperties)
                {
                    ContractValidator.Error(issues, "object-out-of-range", path,
                        "Object does not satisfy the configured property bounds.");
                }

                var values = properties.ToDictionary(property => property.Name, StringComparer.Ordinal);
                foreach (var required in schema.Required ?? [])
                {
                    if (!values.ContainsKey(required))
                    {
                        ContractValidator.Error(issues, "required-property-missing",
                            $"{path}/{ContractValidator.EscapePointer(required)}",
                            $"Required property '{required}' is missing.");
                    }
                }

                foreach (var property in properties)
                {
                    if (schema.Properties?.TryGetValue(property.Name, out var propertySchema) is true)
                    {
                        ValidateValue(propertySchema, property.Value,
                            $"{path}/{ContractValidator.EscapePointer(property.Name)}",
                            issues,
                            depth + 1);
                    }
                    else
                    {
                        ContractValidator.Error(issues, "additional-property",
                            $"{path}/{ContractValidator.EscapePointer(property.Name)}",
                            $"Property '{property.Name}' is not declared.");
                    }
                }

                break;
            default:
                TypeError(path, schema.Type, issues);
                break;
        }
    }

    private static void ValidateNumericSchema(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        var lower = schema.Minimum ?? schema.ExclusiveMinimum;
        var upper = schema.Maximum ?? schema.ExclusiveMaximum;
        if (lower is not null && upper is not null && lower > upper)
        {
            ContractValidator.Error(issues, "invalid-schema-range", path,
                "Lower numeric bound must not exceed upper bound.");
        }

        if (schema.MultipleOf is <= 0)
        {
            ContractValidator.Error(issues, "invalid-multiple-of", $"{path}/multipleOf",
                "multipleOf must be positive.");
        }

        RejectStringKeywords(schema, path, issues);
        RejectCollectionKeywords(schema, path, issues);
    }

    private static void ValidateIntegerSchema(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        ValidateIntegerKeyword(schema.Minimum, "minimum", path, issues);
        ValidateIntegerKeyword(schema.Maximum, "maximum", path, issues);
        ValidateIntegerKeyword(
            schema.ExclusiveMinimum,
            "exclusiveMinimum",
            path,
            issues);
        ValidateIntegerKeyword(
            schema.ExclusiveMaximum,
            "exclusiveMaximum",
            path,
            issues);
        ValidateIntegerKeyword(schema.MultipleOf, "multipleOf", path, issues);
    }

    private static void ValidateIntegerKeyword(
        double? value,
        string keyword,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (value is not null && value != Math.Truncate(value.Value))
        {
            ContractValidator.Error(
                issues,
                "non-integer-schema-keyword",
                $"{path}/{keyword}",
                $"{keyword} must be an integer for an integer schema.");
        }
    }

    private static void ValidateNumber(
        double value,
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (schema.Minimum is { } minimum && value < minimum
            || schema.Maximum is { } maximum && value > maximum
            || schema.ExclusiveMinimum is { } exclusiveMinimum && value <= exclusiveMinimum
            || schema.ExclusiveMaximum is { } exclusiveMaximum && value >= exclusiveMaximum)
        {
            ContractValidator.Error(issues, "number-out-of-range", path,
                "Number does not satisfy the configured bounds.");
        }

        if (schema.MultipleOf is { } multipleOf)
        {
            var quotient = value / multipleOf;
            if (Math.Abs(quotient - Math.Round(quotient)) > 1e-10)
            {
                ContractValidator.Error(issues, "number-not-multiple", path,
                    $"Number must be a multiple of {multipleOf.ToString(CultureInfo.InvariantCulture)}.");
            }
        }
    }

    private static void ValidateRange(
        int? minimum,
        int? maximum,
        int profileMaximum,
        string path,
        string noun,
        ICollection<ValidationIssue> issues)
    {
        if (minimum is < 0
            || maximum is < 0
            || minimum > maximum
            || minimum > profileMaximum
            || maximum > profileMaximum)
        {
            ContractValidator.Error(issues, "invalid-schema-range", path,
                $"Invalid {noun} bounds; values must be between 0 and {profileMaximum}.");
        }
    }

    private static void RequireKind(
        JsonElement value,
        JsonValueKind kind,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (value.ValueKind != kind)
        {
            TypeError(path, kind.ToString().ToLowerInvariant(), issues);
        }
    }

    private static void TypeError(
        string path,
        string expected,
        ICollection<ValidationIssue> issues) =>
        ContractValidator.Error(issues, "invalid-value-type", path,
            $"Value must be a {expected}.");

    private static void RejectNumericAndCollectionKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        RejectNumericKeywords(schema, path, issues);
        RejectStringKeywords(schema, path, issues);
        RejectCollectionKeywords(schema, path, issues);
    }

    private static void RejectNumericKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (schema.Minimum is not null || schema.Maximum is not null
            || schema.ExclusiveMinimum is not null || schema.ExclusiveMaximum is not null
            || schema.MultipleOf is not null)
        {
            ContractValidator.Error(issues, "inapplicable-schema-keyword", path,
                $"Numeric keywords are not valid for type '{schema.Type}'.");
        }
    }

    private static void RejectStringKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (schema.MinLength is not null || schema.MaxLength is not null)
        {
            ContractValidator.Error(issues, "inapplicable-schema-keyword", path,
                $"String keywords are not valid for type '{schema.Type}'.");
        }
    }

    private static void RejectCollectionKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        RejectArrayKeywords(schema, path, issues);
        RejectObjectKeywords(schema, path, issues);
    }

    private static void RejectArrayKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (schema.Items is not null || schema.MinItems is not null
            || schema.MaxItems is not null || schema.UniqueItems is not null)
        {
            ContractValidator.Error(issues, "inapplicable-schema-keyword", path,
                $"Array keywords are not valid for type '{schema.Type}'.");
        }
    }

    private static void RejectObjectKeywords(
        ValueSchema schema,
        string path,
        ICollection<ValidationIssue> issues)
    {
        if (schema.Properties is not null || schema.Required is not null
            || schema.AdditionalProperties is not null
            || schema.MinProperties is not null || schema.MaxProperties is not null)
        {
            ContractValidator.Error(issues, "inapplicable-schema-keyword", path,
                $"Object keywords are not valid for type '{schema.Type}'.");
        }
    }
}
