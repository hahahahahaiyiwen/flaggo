using System.Text;
using System.Text.Json;
using Celly;
using Celly.Ast;
using Celly.Checking;
using Celly.Common;
using Celly.Interpreter;
using Celly.Protobuf;
using Celly.Types;
using Celly.Values;
using Flaggo.Contract;
using Google.Protobuf;
using System.Security.Cryptography;

namespace Flaggo.Expressions;

public sealed class FlaggoExpressionCompiler : IContractExpressionValidator
{
    public const int MaximumExpressionBytes = 4096;
    public const int MaximumAstDepth = 32;
    public const int MaximumAstNodes = 256;
    public const ulong MaximumStaticCost = 1_000;
    public const long MaximumRuntimeIterations = 10_000;
    public const int MaximumCheckedExpressionBytes = 65_536;

    private static readonly HashSet<string> AllowedFunctions =
    [
        Operators.Conditional,
        Operators.LogicalAnd,
        Operators.LogicalOr,
        Operators.LogicalNot,
        Operators.Equals,
        Operators.NotEquals,
        Operators.Less,
        Operators.LessEquals,
        Operators.Greater,
        Operators.GreaterEquals,
        Operators.Add,
        Operators.Subtract,
        Operators.Multiply,
        Operators.Divide,
        Operators.Modulo,
        Operators.Negate,
        "bool",
        "double",
        "int",
        "string",
        "size"
    ];

    public ExpressionValidation Validate(
        string expression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose)
    {
        try
        {
            var compiled = Compile(expression, attributes, expectedResult, purpose);
            return new ExpressionValidation(
                true,
                compiled.CanonicalExpression,
                compiled.ReferencedAttributes,
                null);
        }
        catch (ExpressionCompilationException exception)
        {
            return new ExpressionValidation(
                false,
                null,
                new HashSet<string>(StringComparer.Ordinal),
                exception.Message);
        }
    }

    public ExpressionValidation ValidateSyntax(string expression)
    {
        try
        {
            var canonicalExpression = Canonicalize(expression);
            return new ExpressionValidation(
                true,
                canonicalExpression,
                new HashSet<string>(StringComparer.Ordinal),
                null);
        }
        catch (ExpressionCompilationException exception)
        {
            return new ExpressionValidation(
                false,
                null,
                new HashSet<string>(StringComparer.Ordinal),
                exception.Message);
        }
    }

    public string Canonicalize(string expression)
    {
        EnsureExpressionSize(expression);
        var environment = CreateEnvironment(new Dictionary<string, ValueSchema>());
        var parsed = environment.Parse(expression);
        if (parsed.Ast is null)
        {
            throw new ExpressionCompilationException(FormatIssues(parsed.Issues));
        }

        return Unparser.Unparse(parsed.Ast);
    }

    public CompiledFlaggoExpression Compile(
        string expression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose)
    {
        EnsureExpressionSize(expression);
        var environment = CreateEnvironment(attributes);
        var parsed = environment.Parse(expression);
        if (parsed.Ast is null)
        {
            throw new ExpressionCompilationException(FormatIssues(parsed.Issues));
        }

        var referencedAttributes = new HashSet<string>(StringComparer.Ordinal);
        var metrics = InspectProfile(parsed.Ast.Expr, referencedAttributes);
        if (metrics.Depth > MaximumAstDepth)
        {
            throw new ExpressionCompilationException(
                $"Expression exceeds the maximum AST depth of {MaximumAstDepth}.");
        }

        if (metrics.Nodes > MaximumAstNodes)
        {
            throw new ExpressionCompilationException(
                $"Expression exceeds the maximum AST node count of {MaximumAstNodes}.");
        }

        foreach (var attribute in referencedAttributes)
        {
            if (attribute != "_random" && !attributes.ContainsKey(attribute))
            {
                throw new ExpressionCompilationException(
                    $"Expression references undeclared attribute '{attribute}'.");
            }
        }

        var checkedResult = environment.Check(parsed.Ast);
        if (checkedResult.HasErrors)
        {
            throw new ExpressionCompilationException(FormatIssues(checkedResult.Issues));
        }

        var actualType = checkedResult.TypeOf(parsed.Ast.Expr);
        var expectedType = purpose == ExpressionPurpose.Predicate
            ? CelType.Bool
            : ToCelType(expectedResult);
        if (!IsCompatible(actualType, expectedType))
        {
            throw new ExpressionCompilationException(
                $"Expression has type '{actualType}', expected '{expectedType}'.");
        }

        var cost = environment.EstimateCost(parsed.Ast);
        if (cost.IsUnbounded || cost.Max > MaximumStaticCost)
        {
            throw new ExpressionCompilationException(
                $"Expression static cost {cost} exceeds {MaximumStaticCost}.");
        }

        var canonicalExpression = Unparser.Unparse(parsed.Ast);
        var checkedExpressionBytes = AstConverter.ToCheckedExpr(parsed.Ast).ToByteArray();
        if (checkedExpressionBytes.Length > MaximumCheckedExpressionBytes)
        {
            throw new ExpressionCompilationException(
                $"Checked expression exceeds the {MaximumCheckedExpressionBytes}-byte limit.");
        }

        return new CompiledFlaggoExpression(
            expression,
            canonicalExpression,
            referencedAttributes,
            purpose,
            new CheckedFlaggoExpression
            {
                CanonicalExpression = canonicalExpression,
                ReferencedAttributes = referencedAttributes
                    .OrderBy(name => name, StringComparer.Ordinal)
                    .ToArray(),
                CheckedAst = checkedExpressionBytes,
                CheckedAstDigest = Digest(checkedExpressionBytes)
            },
            environment.Program(parsed.Ast));
    }

    public CompiledFlaggoExpression Materialize(
        CheckedFlaggoExpression checkedExpression,
        IReadOnlyDictionary<string, ValueSchema> attributes,
        ValueSchema expectedResult,
        ExpressionPurpose purpose)
    {
        ArgumentNullException.ThrowIfNull(checkedExpression);
        if (checkedExpression.CheckedAst.Length is 0 or > MaximumCheckedExpressionBytes)
        {
            throw new ExpressionCompilationException(
                $"Checked expression must contain between 1 and "
                + $"{MaximumCheckedExpressionBytes} bytes.");
        }

        var digest = Digest(checkedExpression.CheckedAst);
        if (!string.Equals(digest, checkedExpression.CheckedAstDigest, StringComparison.Ordinal))
        {
            throw new ExpressionCompilationException(
                "Checked expression digest does not match its stored representation.");
        }

        CelAbstractSyntax ast;
        try
        {
            ast = AstConverter.FromCheckedExpr(
                Cel.Expr.CheckedExpr.Parser.ParseFrom(checkedExpression.CheckedAst));
        }
        catch (InvalidProtocolBufferException exception)
        {
            throw new ExpressionCompilationException(
                "Checked expression is not a valid CEL checked expression.",
                exception);
        }

        var referencedAttributes = new HashSet<string>(StringComparer.Ordinal);
        var metrics = InspectProfile(ast.Expr, referencedAttributes);
        if (metrics.Depth > MaximumAstDepth || metrics.Nodes > MaximumAstNodes)
        {
            throw new ExpressionCompilationException(
                "Checked expression exceeds the flaggo.cel/v1 AST limits.");
        }

        var expectedReferences = checkedExpression.ReferencedAttributes
            .OrderBy(name => name, StringComparer.Ordinal)
            .ToArray();
        if (!referencedAttributes.SetEquals(expectedReferences)
            || expectedReferences.Distinct(StringComparer.Ordinal).Count()
                != expectedReferences.Length)
        {
            throw new ExpressionCompilationException(
                "Checked expression attribute references do not match its stored metadata.");
        }

        foreach (var attribute in referencedAttributes)
        {
            if (attribute != "_random" && !attributes.ContainsKey(attribute))
            {
                throw new ExpressionCompilationException(
                    $"Checked expression references undeclared attribute '{attribute}'.");
            }
        }

        var canonicalExpression = Unparser.Unparse(ast);
        if (!string.Equals(
                canonicalExpression,
                checkedExpression.CanonicalExpression,
                StringComparison.Ordinal))
        {
            throw new ExpressionCompilationException(
                "Checked expression does not match its canonical expression.");
        }

        var expectedType = purpose == ExpressionPurpose.Predicate
            ? CelType.Bool
            : ToCelType(expectedResult);
        if (ast.TypeMap is null
            || !ast.TypeMap.TryGetValue(ast.Expr.Id, out var actualType)
            || !IsCompatible(actualType, expectedType))
        {
            throw new ExpressionCompilationException(
                $"Checked expression does not produce the expected '{expectedType}' type.");
        }

        var environment = CreateEnvironment(attributes);
        var cost = environment.EstimateCost(ast);
        if (cost.IsUnbounded || cost.Max > MaximumStaticCost)
        {
            throw new ExpressionCompilationException(
                $"Checked expression static cost {cost} exceeds {MaximumStaticCost}.");
        }

        return new CompiledFlaggoExpression(
            canonicalExpression,
            canonicalExpression,
            referencedAttributes,
            purpose,
            checkedExpression,
            environment.Program(ast));
    }

    private static CelEnv CreateEnvironment(IReadOnlyDictionary<string, ValueSchema> attributes)
    {
        var declarations = new List<VariableDecl>
        {
            new("attributes", CelType.Map(CelType.String, CelType.Dyn)),
            new("attributes._random", CelType.Double)
        };
        declarations.AddRange(attributes.Select(attribute =>
            new VariableDecl($"attributes.{attribute.Key}", ToCelType(attribute.Value))));

        return CelEnv.Create(new CelEnvSettings
        {
            Declarations = declarations,
            DisableMacros = true,
            EnableOptionalSyntax = false,
            EvalLimits = new EvalLimits { MaxIterations = MaximumRuntimeIterations }
        });
    }

    private static AstMetrics InspectProfile(
        Expr expression,
        ISet<string> referencedAttributes,
        int depth = 1)
    {
        if (depth > MaximumAstDepth + 1)
        {
            return new AstMetrics(1, depth);
        }

        return expression switch
        {
            ConstExpr constant => InspectConstant(constant, depth),
            IdentExpr identifier => throw new ExpressionCompilationException(
                $"Identifier '{identifier.Name}' is not available; use attributes.<name>."),
            SelectExpr select => InspectSelection(select, referencedAttributes, depth),
            CallExpr call => InspectCall(call, referencedAttributes, depth),
            ListExpr => throw Unsupported("list literals"),
            MapExpr => throw Unsupported("map literals"),
            StructExpr => throw Unsupported("message literals"),
            ComprehensionExpr => throw Unsupported("comprehensions and macros"),
            _ => throw Unsupported("this expression form")
        };
    }

    private static AstMetrics InspectConstant(ConstExpr expression, int depth)
    {
        if (expression.Value.Kind is ConstantKind.Bytes or ConstantKind.Uint)
        {
            throw Unsupported($"{expression.Value.Kind.ToString().ToLowerInvariant()} literals");
        }

        return new AstMetrics(1, depth);
    }

    private static AstMetrics InspectSelection(
        SelectExpr expression,
        ISet<string> referencedAttributes,
        int depth)
    {
        if (expression.TestOnly
            || expression.Operand is not IdentExpr { Name: "attributes" })
        {
            throw new ExpressionCompilationException(
                "Only direct attributes.<name> selection is supported.");
        }

        referencedAttributes.Add(expression.Field);
        return new AstMetrics(2, depth + 1);
    }

    private static AstMetrics InspectCall(
        CallExpr expression,
        ISet<string> referencedAttributes,
        int depth)
    {
        if (expression.Target is not null || !AllowedFunctions.Contains(expression.Function))
        {
            throw new ExpressionCompilationException(
                $"Function or operator '{expression.Function}' is not supported by flaggo.cel/v1.");
        }

        var metrics = new AstMetrics(1, depth);
        foreach (var argument in expression.Args)
        {
            metrics = metrics.Merge(InspectProfile(argument, referencedAttributes, depth + 1));
        }

        return metrics;
    }

    private static bool IsCompatible(CelType actual, CelType expected)
    {
        if (actual.Kind == expected.Kind)
        {
            return true;
        }

        return expected.Kind == CelTypeKind.Double && actual.Kind == CelTypeKind.Int;
    }

    private static CelType ToCelType(ValueSchema schema) => schema.Type switch
    {
        "null" => CelType.Null,
        "boolean" => CelType.Bool,
        "integer" => CelType.Int,
        "number" => CelType.Double,
        "string" => CelType.String,
        "array" => CelType.List(schema.Items is null ? CelType.Dyn : ToCelType(schema.Items)),
        "object" => CelType.Map(CelType.String, CelType.Dyn),
        _ => CelType.Dyn
    };

    private static void EnsureExpressionSize(string expression)
    {
        if (string.IsNullOrWhiteSpace(expression))
        {
            throw new ExpressionCompilationException("Expression must not be empty.");
        }

        if (Encoding.UTF8.GetByteCount(expression) > MaximumExpressionBytes)
        {
            throw new ExpressionCompilationException(
                $"Expression exceeds the {MaximumExpressionBytes}-byte limit.");
        }
    }

    private static string FormatIssues(IEnumerable<object> issues) =>
        string.Join("; ", issues.Select(issue => issue.ToString()));

    private static ExpressionCompilationException Unsupported(string feature) =>
        new($"{feature} are not supported by flaggo.cel/v1.");

    private static string Digest(ReadOnlySpan<byte> value) =>
        $"sha256:{Convert.ToHexStringLower(SHA256.HashData(value))}";

    private readonly record struct AstMetrics(int Nodes, int Depth)
    {
        public AstMetrics Merge(AstMetrics other) =>
            new(Nodes + other.Nodes, Math.Max(Depth, other.Depth));
    }
}

public sealed class CompiledFlaggoExpression
{
    private readonly CelProgram _program;

    internal CompiledFlaggoExpression(
        string source,
        string canonicalExpression,
        IReadOnlySet<string> referencedAttributes,
        ExpressionPurpose purpose,
        CheckedFlaggoExpression checkedExpression,
        CelProgram program)
    {
        Source = source;
        CanonicalExpression = canonicalExpression;
        ReferencedAttributes = referencedAttributes;
        Purpose = purpose;
        CheckedExpression = checkedExpression;
        _program = program;
    }

    public string Source { get; }

    public string CanonicalExpression { get; }

    public IReadOnlySet<string> ReferencedAttributes { get; }

    public ExpressionPurpose Purpose { get; }

    public CheckedFlaggoExpression CheckedExpression { get; }

    public JsonElement Evaluate(
        IReadOnlyDictionary<string, JsonElement> attributes,
        CancellationToken cancellationToken = default)
    {
        var nativeAttributes = attributes.ToDictionary(
            attribute => attribute.Key,
            attribute => ToNative(attribute.Value),
            StringComparer.Ordinal);
        var bindings = new Dictionary<string, object?>(StringComparer.Ordinal)
        {
            ["attributes"] = nativeAttributes
        };
        foreach (var attribute in nativeAttributes)
        {
            bindings[$"attributes.{attribute.Key}"] = attribute.Value;
        }

        var value = _program.Eval(
            bindings,
            new EvalLimits
            {
                MaxIterations = FlaggoExpressionCompiler.MaximumRuntimeIterations,
                CancellationToken = cancellationToken
            });
        if (value is ErrorValue error)
        {
            throw new ExpressionEvaluationException(error.Message);
        }

        if (value is UnknownValue)
        {
            throw new ExpressionEvaluationException("Expression produced an unknown value.");
        }

        try
        {
            return JsonSerializer.SerializeToElement(value.ToNative(), StrictJson.Options);
        }
        catch (Exception exception) when (exception is JsonException or NotSupportedException)
        {
            throw new ExpressionEvaluationException(
                "Expression produced a value outside the Flaggo JSON profile.",
                exception);
        }
    }

    private static object? ToNative(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.Null => null,
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        JsonValueKind.String => value.GetString(),
        JsonValueKind.Number when value.TryGetInt64(out var integer) => integer,
        JsonValueKind.Number when value.TryGetDouble(out var number) && double.IsFinite(number) => number,
        JsonValueKind.Array => value.EnumerateArray().Select(ToNative).ToArray(),
        JsonValueKind.Object => value.EnumerateObject().ToDictionary(
            property => property.Name,
            property => ToNative(property.Value),
            StringComparer.Ordinal),
        _ => throw new ExpressionEvaluationException(
            $"JSON kind '{value.ValueKind}' cannot be converted to CEL.")
    };
}

public sealed class ExpressionCompilationException : Exception
{
    public ExpressionCompilationException(string message)
        : base(message)
    {
    }

    public ExpressionCompilationException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

public sealed class ExpressionEvaluationException : Exception
{
    public ExpressionEvaluationException(string message)
        : base(message)
    {
    }

    public ExpressionEvaluationException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

public sealed record CheckedFlaggoExpression
{
    public required string CanonicalExpression { get; init; }

    public required IReadOnlyList<string> ReferencedAttributes { get; init; }

    public required byte[] CheckedAst { get; init; }

    public required string CheckedAstDigest { get; init; }
}
