using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;

Console.InputEncoding = new UTF8Encoding(false, true);
Console.OutputEncoding = new UTF8Encoding(false, true);
var compiler = new FlaggoExpressionCompiler();
while (Console.ReadLine() is { } line)
{
    using var request = JsonDocument.Parse(line);
    var value = request.RootElement;
    var id = value.GetProperty("id").GetString();
    if (value.TryGetProperty("domainModel", out var domainModel))
    {
        try
        {
            var bytes = Encoding.UTF8.GetBytes(value.GetProperty("document").GetString()!);
            object model = domainModel.GetString() switch
            {
                "contract" => StrictJson.Deserialize<DecisionContract>(bytes),
                "executable" => StrictJson.Deserialize<DecisionExecutable>(bytes),
                "runtimeInput" => StrictJson.Deserialize<RuntimeInput>(bytes),
                "schema" => StrictJson.Deserialize<ValueSchema>(bytes),
                "json" => StrictJson.Deserialize<JsonElement>(bytes),
                _ => throw new InvalidOperationException("Unknown domain model.")
            };
            var action = value.GetProperty("domainAction").GetString();
            if (action is "validate" or "validateValue")
            {
                object validation = model switch
                {
                    DecisionContract contract => ContractValidator.Validate(contract),
                    ValueSchema schema when action == "validateValue" =>
                        ValidateValue(schema, value.GetProperty("validationValue").GetString()!),
                    ValueSchema schema => ValidateSchema(schema),
                    RuntimeInput input => ValidateInput(
                        StrictJson.Deserialize<DecisionContract>(Encoding.UTF8.GetBytes(
                            value.GetProperty("validationContract").GetString()!)), input),
                    DecisionExecutable executable => ContractValidator.ValidateExecutable(
                        StrictJson.Deserialize<DecisionContract>(Encoding.UTF8.GetBytes(
                            value.GetProperty("validationContract").GetString()!)), executable),
                    _ => throw new InvalidOperationException("Model cannot be validated.")
                };
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    id, status = "value",
                    value = JsonSerializer.SerializeToElement(validation, validation.GetType(), StrictJson.Options)
                }));
                continue;
            }
            string? digest = action == "digest"
                ? model switch
                {
                    DecisionContract contract => ContractDigests.ComputeContractDigest(contract),
                    DecisionExecutable executable => ContractDigests.ComputeExecutableDigest(executable),
                    _ => throw new InvalidOperationException("Model does not have a digest.")
                }
                : null;
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                id, status = "value",
                value = new
                {
                    document = JsonSerializer.SerializeToElement(model, model.GetType(), StrictJson.Options),
                    digest
                }
            }));
        }
        catch (JsonException error)
        {
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                id, status = "json-error", message = error.Message
            }));
        }
        catch (Exception error) when (error is InvalidOperationException or NotSupportedException
            or ArgumentException or NullReferenceException)
        {
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                id, status = "runtime-fault", exceptionType = error.GetType().FullName,
                message = error.Message
            }));
        }

        static IReadOnlyList<ValidationIssue> ValidateSchema(ValueSchema schema)
        {
            var issues = new List<ValidationIssue>();
            ValueSchemaValidator.ValidateSchema(schema, "/schema", issues);
            return issues;
        }

        static IReadOnlyList<ValidationIssue> ValidateValue(ValueSchema schema, string text)
        {
            using var document = JsonDocument.Parse(text);
            var issues = new List<ValidationIssue>();
            ValueSchemaValidator.ValidateValue(schema, document.RootElement, "/value", issues);
            return issues;
        }

        static IReadOnlyList<ValidationIssue> ValidateInput(DecisionContract contract, RuntimeInput input)
        {
            var issues = new List<ValidationIssue>();
            ContractValidator.ValidateRuntimeInput(contract, input, issues);
            return issues;
        }
        continue;
    }
    if (value.TryGetProperty("canonicalJson", out var canonicalJson))
    {
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "value",
            value = Encoding.UTF8.GetString(Jcs.Net.JsonCanonicalizer.CanonicalizeToUtf8(canonicalJson))
        }));
        continue;
    }
    var schemas = value.GetProperty("schemas").EnumerateObject().ToDictionary(
        property => property.Name,
        property => JsonSerializer.Deserialize<ValueSchema>(
            property.Value.GetRawText(), StrictJson.Options)!,
        StringComparer.Ordinal);
    CompiledFlaggoExpression compiled;
    try
    {
        compiled = value.TryGetProperty("checked", out var stored)
            ? compiler.Materialize(
                JsonSerializer.Deserialize<CheckedFlaggoExpression>(
                    stored.GetRawText(), StrictJson.Options)!,
                schemas,
                new ValueSchema { Type = value.GetProperty("resultType").GetString()! },
                ExpressionPurpose.Result)
            : compiler.Compile(
            value.GetProperty("expression").GetString()!,
            schemas,
            new ValueSchema { Type = value.GetProperty("resultType").GetString()! },
            ExpressionPurpose.Result);
    }
    catch (ExpressionCompilationException error)
    {
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "compile-error", message = error.Message
        }));
        continue;
    }

    if (value.GetProperty("compileOnly").GetBoolean())
    {
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "compiled", canonical = compiled.CanonicalExpression,
            @checked = JsonSerializer.SerializeToElement(compiled.CheckedExpression, StrictJson.Options)
        }));
        continue;
    }

    var attributes = value.GetProperty("attributes").EnumerateObject().ToDictionary(
        property => property.Name, property => property.Value.Clone(), StringComparer.Ordinal);
    try
    {
        var result = compiled.Evaluate(attributes);
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "value", value = result, canonical = compiled.CanonicalExpression,
            @checked = JsonSerializer.SerializeToElement(compiled.CheckedExpression, StrictJson.Options)
        }));
    }
    catch (ExpressionEvaluationException error)
    {
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "evaluation-error", message = error.Message,
            @checked = JsonSerializer.SerializeToElement(compiled.CheckedExpression, StrictJson.Options)
        }));
    }
    catch (ArgumentException error)
    {
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, status = "runtime-fault", exceptionType = error.GetType().FullName,
            message = error.Message,
            @checked = JsonSerializer.SerializeToElement(compiled.CheckedExpression, StrictJson.Options)
        }));
    }
}
