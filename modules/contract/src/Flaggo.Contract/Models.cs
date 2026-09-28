using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Contract;

public readonly record struct DecisionScope(string Application, string Environment)
{
    public override string ToString() => $"{Application}/{Environment}";
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionContract
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("expression_syntax")]
    public required string ExpressionSyntax { get; init; }

    [JsonPropertyName("attributes")]
    public required IReadOnlyList<ContractAttribute> Attributes { get; init; }

    [JsonPropertyName("result")]
    public required ContractResult Result { get; init; }

    [JsonPropertyName("authoredExecutable")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public AuthoredExecutable? AuthoredExecutable { get; init; }

    [JsonPropertyName("learning")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public LearningDefinition? Learning { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ContractAttribute
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("schema")]
    public required ValueSchema Schema { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ContractResult
{
    [JsonPropertyName("schema")]
    public required ValueSchema Schema { get; init; }

    [JsonPropertyName("default")]
    public required JsonElement Default { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ValueSchema
{
    [JsonPropertyName("type")]
    public required string Type { get; init; }

    [JsonPropertyName("minimum")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Minimum { get; init; }

    [JsonPropertyName("maximum")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Maximum { get; init; }

    [JsonPropertyName("exclusiveMinimum")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? ExclusiveMinimum { get; init; }

    [JsonPropertyName("exclusiveMaximum")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? ExclusiveMaximum { get; init; }

    [JsonPropertyName("multipleOf")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? MultipleOf { get; init; }

    [JsonPropertyName("minLength")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MinLength { get; init; }

    [JsonPropertyName("maxLength")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MaxLength { get; init; }

    [JsonPropertyName("items")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ValueSchema? Items { get; init; }

    [JsonPropertyName("minItems")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MinItems { get; init; }

    [JsonPropertyName("maxItems")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MaxItems { get; init; }

    [JsonPropertyName("uniqueItems")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public bool? UniqueItems { get; init; }

    [JsonPropertyName("properties")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyDictionary<string, ValueSchema>? Properties { get; init; }

    [JsonPropertyName("required")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<string>? Required { get; init; }

    [JsonPropertyName("additionalProperties")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public bool? AdditionalProperties { get; init; }

    [JsonPropertyName("minProperties")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MinProperties { get; init; }

    [JsonPropertyName("maxProperties")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? MaxProperties { get; init; }

    [JsonPropertyName("description")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Description { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record AuthoredExecutable
{
    [JsonPropertyName("rules")]
    public required IReadOnlyList<AuthoredRule> Rules { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record AuthoredRule
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("description")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Description { get; init; }

    [JsonPropertyName("when")]
    public required RuleWhen When { get; init; }

    [JsonPropertyName("return")]
    public required RuleReturn Return { get; init; }
}

[JsonConverter(typeof(RuleWhenJsonConverter))]
public abstract record RuleWhen;

public sealed record NaturalLanguageWhen(string Condition) : RuleWhen;

public sealed record ExpressionWhen(string Expression) : RuleWhen;

[JsonConverter(typeof(RuleReturnJsonConverter))]
public abstract record RuleReturn;

public sealed record LiteralReturn(JsonElement Value) : RuleReturn;

public sealed record ExpressionReturn(string Expression) : RuleReturn;

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record LearningDefinition
{
    [JsonPropertyName("policy")]
    public required LearningPolicy Policy { get; init; }

    [JsonPropertyName("evidence")]
    public required IReadOnlyList<EvidenceDefinition> Evidence { get; init; }

    [JsonPropertyName("objective")]
    public required LearningObjective Objective { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record LearningPolicy
{
    [JsonPropertyName("mode")]
    public required string Mode { get; init; }

    [JsonPropertyName("evaluate")]
    public required LearningEvaluationPolicy Evaluate { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record LearningEvaluationPolicy
{
    [JsonPropertyName("interval")]
    public required string Interval { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record EvidenceDefinition
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("description")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Description { get; init; }

    [JsonPropertyName("attribute")]
    public required string Attribute { get; init; }

    [JsonPropertyName("binding")]
    public required string Binding { get; init; }

    [JsonPropertyName("correlateBy")]
    public required IReadOnlyList<string> CorrelateBy { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record LearningObjective
{
    [JsonPropertyName("primary")]
    public required PrimaryObjective Primary { get; init; }

    [JsonPropertyName("guardrails")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<Guardrail>? Guardrails { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record PrimaryObjective
{
    [JsonPropertyName("evidence")]
    public required string Evidence { get; init; }

    [JsonPropertyName("direction")]
    public required string Direction { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record Guardrail
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("description")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Description { get; init; }

    [JsonPropertyName("expression")]
    public required string Expression { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionExecutable
{
    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("kind")]
    public string Kind { get; init; } = "rules";

    [JsonPropertyName("rules")]
    public required IReadOnlyList<ExecutableRule> Rules { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ExecutableRule
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("when")]
    public required ExpressionWhen When { get; init; }

    [JsonPropertyName("return")]
    public required RuleReturn Return { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record RuntimeInput
{
    [JsonPropertyName("attributes")]
    public required IReadOnlyDictionary<string, JsonElement> Attributes { get; init; }

    [JsonPropertyName("currentExposure")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public CurrentExposure? CurrentExposure { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record CurrentExposure
{
    [JsonPropertyName("exposureId")]
    public required string ExposureId { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record RuntimeDecision
{
    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("executableDigest")]
    public required string ExecutableDigest { get; init; }

    [JsonPropertyName("result")]
    public required JsonElement Result { get; init; }

    [JsonPropertyName("evaluation")]
    public required EvaluationProvenance Evaluation { get; init; }
}

[JsonConverter(typeof(EvaluationProvenanceJsonConverter))]
public abstract record EvaluationProvenance;

public sealed record RuleEvaluation(string Rule) : EvaluationProvenance;

public sealed record DefaultEvaluation : EvaluationProvenance;

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ValidationIssue
{
    [JsonPropertyName("code")]
    public required string Code { get; init; }

    [JsonPropertyName("severity")]
    public required string Severity { get; init; }

    [JsonPropertyName("path")]
    public required string Path { get; init; }

    [JsonPropertyName("message")]
    public required string Message { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionContractValidationResult
{
    [JsonPropertyName("status")]
    public required string Status { get; init; }

    [JsonPropertyName("contractDigest")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? ContractDigest { get; init; }

    [JsonPropertyName("issues")]
    public required IReadOnlyList<ValidationIssue> Issues { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionContractVersionSummary
{
    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("status")]
    public string Status { get; init; } = "ready";

    [JsonPropertyName("acceptedAt")]
    [JsonConverter(typeof(UtcDateTimeOffsetJsonConverter))]
    public required DateTimeOffset AcceptedAt { get; init; }

    [JsonPropertyName("activeExecutableDigest")]
    public required string ActiveExecutableDigest { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionContractVersion
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("status")]
    public string Status { get; init; } = "ready";

    [JsonPropertyName("acceptedAt")]
    [JsonConverter(typeof(UtcDateTimeOffsetJsonConverter))]
    public required DateTimeOffset AcceptedAt { get; init; }

    [JsonPropertyName("activeExecutableDigest")]
    public required string ActiveExecutableDigest { get; init; }

    [JsonPropertyName("contract")]
    public required DecisionContract Contract { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionContractVersionList
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("currentContractDigest")]
    public required string CurrentContractDigest { get; init; }

    [JsonPropertyName("versions")]
    public required IReadOnlyList<DecisionContractVersionSummary> Versions { get; init; }

    [JsonPropertyName("nextCursor")]
    [JsonIgnore(Condition = JsonIgnoreCondition.Never)]
    public string? NextCursor { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record LivenessResult
{
    [JsonPropertyName("status")]
    public string Status { get; init; } = "live";

    [JsonPropertyName("service")]
    public required string Service { get; init; }

    [JsonPropertyName("version")]
    public required string Version { get; init; }

    [JsonPropertyName("observedAt")]
    [JsonConverter(typeof(UtcDateTimeOffsetJsonConverter))]
    public required DateTimeOffset ObservedAt { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ReadinessCheck
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("status")]
    public required string Status { get; init; }

    [JsonPropertyName("required")]
    public required bool Required { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ReadinessResult
{
    [JsonPropertyName("status")]
    public required string Status { get; init; }

    [JsonPropertyName("observedAt")]
    [JsonConverter(typeof(UtcDateTimeOffsetJsonConverter))]
    public required DateTimeOffset ObservedAt { get; init; }

    [JsonPropertyName("checks")]
    public required IReadOnlyList<ReadinessCheck> Checks { get; init; }
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ProblemDetailsDocument
{
    [JsonPropertyName("type")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Type { get; init; }

    [JsonPropertyName("title")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Title { get; init; }

    [JsonPropertyName("status")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? Status { get; init; }

    [JsonPropertyName("detail")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Detail { get; init; }

    [JsonPropertyName("instance")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Instance { get; init; }
}
