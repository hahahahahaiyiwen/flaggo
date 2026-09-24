using System.Collections.Frozen;
using System.Text;
using System.Text.Json;
using Flaggo.Shared.Contracts;
using Json.Schema;

namespace Flaggo.Registry;

internal static class DecisionManifest
{
    private static readonly JsonSchema Schema = LoadSchema();

    public static IReadOnlyList<ProblemIssue> Validate(JsonElement bundle)
    {
        var issues = new List<ProblemIssue>();
        try
        {
            StrictJson.Validate(Encoding.UTF8.GetBytes(bundle.GetRawText()));
            _ = CanonicalJson.Canonicalize(bundle);
            var evaluation = Schema.Evaluate(bundle, new EvaluationOptions { OutputFormat = OutputFormat.List });
            if (!evaluation.IsValid)
            {
                AddSchemaErrors(evaluation, issues);
                return issues;
            }
            _ = CanonicalJson.BundleDigest(bundle);
        }
        catch (JsonException error)
        {
            issues.Add(Issue("/", error.Message));
            return issues;
        }

        foreach (var entry in bundle.GetProperty("decisions").EnumerateObject())
        {
            var definition = entry.Value;
            var path = $"/decisions/{Pointer(entry.Name)}";
            var result = definition.GetProperty("result");
            if (Text(result, "type") == "number")
            {
                var minimum = result.GetProperty("min").GetDouble();
                var maximum = result.GetProperty("max").GetDouble();
                var value = result.GetProperty("default");
                if (minimum > maximum || value.GetDouble() < minimum || value.GetDouble() > maximum ||
                    (result.TryGetProperty("step", out var step) &&
                     !CanonicalJson.IsStepAligned(value, result.GetProperty("min"), step)))
                {
                    issues.Add(Issue($"{path}/result", "The default must satisfy ordered bounds and exact step alignment."));
                }
            }
            else if (result.TryGetProperty("allowedValues", out var allowed) &&
                     !allowed.EnumerateArray().Any(value => value.GetString() == Text(result, "default")))
            {
                issues.Add(Issue($"{path}/result/default", "The default must be an allowed string value."));
            }

            var targeting = definition.GetProperty("targeting");
            var hierarchy = Strings(targeting.GetProperty("hierarchy"));
            var primary = Text(targeting, "primary")!;
            var fallbacks = Strings(targeting.GetProperty("fallbackOrder"));
            if (!hierarchy.Contains(primary, StringComparer.Ordinal) ||
                fallbacks.Any(target => target == primary || !hierarchy.Contains(target, StringComparer.Ordinal)))
            {
                issues.Add(Issue($"{path}/targeting", "Primary and fallback targets must belong to the hierarchy and be distinct."));
            }
            var context = ReadContext(definition);
            var mappedTargets = context.Where(field => field.TargetType is not null).ToArray();
            if (mappedTargets.Select(field => field.TargetType).Distinct(StringComparer.Ordinal).Count() != mappedTargets.Length ||
                mappedTargets.Any(field => !hierarchy.Contains(field.TargetType!, StringComparer.Ordinal)))
            {
                issues.Add(Issue($"{path}/context", "Each declared target must have one context source within the hierarchy."));
            }

            var evidence = ReadEvidence(definition);
            foreach (var binding in evidence)
            {
                var bindingPath = $"{path}/evidence/{Pointer(binding.Key)}";
                ValidateRange(binding.Minimum, binding.Maximum, bindingPath, issues);
                if (!hierarchy.Contains(binding.TargetType, StringComparer.Ordinal))
                {
                    issues.Add(Issue($"{bindingPath}/target", "Evidence target is outside the definition hierarchy."));
                }
                if ((binding.Source.Kind != "span" || binding.Source.EventName is null) &&
                    (binding.TargetIdAttribute?.From == "eventAttributes" || binding.ExposureIdAttribute?.From == "eventAttributes"))
                {
                    issues.Add(Issue(bindingPath, "Event attributes are available only for a span-event binding."));
                }
            }
            var bindings = evidence.ToDictionary(binding => binding.Key, StringComparer.Ordinal);
            foreach (var input in Properties(definition, "inputs"))
            {
                var inputPath = $"{path}/inputs/{Pointer(input.Name)}";
                if (Text(input.Value, "source") == "request")
                {
                    var (minimum, maximum) = Range(input.Value);
                    ValidateRange(minimum, maximum, inputPath, issues);
                    continue;
                }
                var key = Text(input.Value, "binding")!;
                if (!bindings.TryGetValue(key, out var binding))
                {
                    issues.Add(Issue(inputPath, "Unknown evidence binding."));
                }
                else if (binding.ExposureIdAttribute is not null)
                {
                    issues.Add(Issue(inputPath,
                        "Confirmed-exposure bindings are outcome evidence, not required runtime inputs: the first decision cannot create its own prerequisite exposure."));
                }
                else if (binding.TargetType != "global" &&
                         !context.Any(field => field.TargetType == binding.TargetType && field.Required))
                {
                    issues.Add(Issue(inputPath, "An evidence-owned input requires explicit caller context for its non-global target."));
                }
            }
            var objectives = ReadObjectives(definition);
            foreach (var objective in (objectives.Secondary ?? []).Prepend(objectives.Primary).OfType<DecisionObjective>())
            {
                if (!bindings.TryGetValue(objective.EvidenceKey, out var binding) || binding.ValueType != "number")
                {
                    issues.Add(Issue($"{path}/intent", "Numeric objectives must reference numeric evidence bindings."));
                }
            }
            var policy = definition.GetProperty("policy");
            if (Text(policy, "kind") == "reference")
            {
                issues.Add(new ProblemIssue("invalid-policy", "error", $"{path}/policy",
                    "The governed policy reference is not available; supply explicit inline constraints."));
            }
            if (Text(policy, "kind") == "inline")
            {
                var constraints = policy.GetProperty("constraints").EnumerateArray().ToArray();
                if (constraints.Select(item => Text(item, "kind")).Distinct(StringComparer.Ordinal).Count() != constraints.Length)
                {
                    issues.Add(Issue($"{path}/policy/constraints", "Constraint kinds must be unique."));
                }
                if (constraints.Any(item => Text(item, "kind") == "number-bounds" &&
                                           item.GetProperty("min").GetDouble() > item.GetProperty("max").GetDouble()))
                {
                    issues.Add(Issue($"{path}/policy/constraints", "Policy bounds must be ordered."));
                }
            }
        }
        return issues;
    }

    public static IReadOnlyList<RegisteredRuntimeContextField> ReadContext(JsonElement definition) =>
        Properties(definition, "context").Select(field => new RegisteredRuntimeContextField(
            field.Name, Text(field.Value, "type")!,
            field.Value.TryGetProperty("required", out var required) && required.GetBoolean(),
            Text(field.Value, "target"))).ToArray();

    public static IReadOnlyList<RegisteredEvidenceBinding> ReadEvidence(JsonElement definition) =>
        Properties(definition, "evidence").Select(entry =>
        {
            var binding = entry.Value;
            var source = binding.GetProperty("source");
            var scope = source.GetProperty("scope");
            var selected = source.GetProperty("value");
            var target = binding.GetProperty("target");
            var attribution = binding.GetProperty("attribution");
            var (minimum, maximum) = Range(binding);
            return new RegisteredEvidenceBinding(
                entry.Name, Text(binding, "meaning")!, Text(binding, "type")!, Text(binding, "unit"),
                minimum, maximum,
                new TelemetrySourceContract(
                    Text(source, "kind")!, Text(scope, "name")!, Text(scope, "version"),
                    Text(source, "name"), Text(source, "eventName"),
                    source.TryGetProperty("bodyEquals", out var body) ? body.Clone() : null,
                    Attributes(source, "resourceAttributes"), Attributes(source, "attributes"),
                    Attributes(source, "eventAttributes"), Text(selected, "from")!,
                    Text(selected, "key"), selected.TryGetProperty("path", out var bodyPath) ? Strings(bodyPath) : []),
                Text(target, "type")!,
                target.TryGetProperty("idAttribute", out var id) ? Selector(id) : null,
                checked((long)binding.GetProperty("freshness").GetProperty("maxAgeSeconds").GetDouble()),
                attribution.TryGetProperty("exposureIdAttribute", out var exposure) ? Selector(exposure) : null);
        }).ToArray();

    public static IReadOnlyList<RegisteredInput> ReadInputs(
        JsonElement definition, IReadOnlyList<RegisteredEvidenceBinding> evidence) =>
        Properties(definition, "inputs").Select(entry =>
        {
            var input = entry.Value;
            if (Text(input, "source") == "evidence")
            {
                var binding = evidence.Single(binding => binding.Key == Text(input, "binding"));
                return new RegisteredInput(entry.Name, binding.ValueType, "evidence", binding.Meaning,
                    binding.Minimum, binding.Maximum, binding.Unit, binding.Key);
            }
            var (minimum, maximum) = Range(input);
            return new RegisteredInput(entry.Name, Text(input, "type")!, "request", Text(input, "meaning")!,
                minimum, maximum, Text(input, "unit"));
        }).ToArray();

    public static DecisionObjectives ReadObjectives(JsonElement definition)
    {
        if (!definition.TryGetProperty("intent", out var intent)) return new();
        if (Text(intent, "type") == "natural-language") return new(NaturalLanguage: Text(intent, "text"));
        return new(Primary: Objective(intent.GetProperty("primary")),
            Secondary: intent.TryGetProperty("secondary", out var secondary)
                ? secondary.EnumerateArray().Select(Objective).ToArray() : [],
            Rationale: Text(intent, "rationale"));
    }

    public static DecisionActionSpaceContract ReadResult(JsonElement result) =>
        new(Text(result, "type")!, result.GetProperty("default").Clone(),
            Number(result, "min"), Number(result, "max"), Number(result, "step"),
            result.TryGetProperty("allowedValues", out var allowed) ? Strings(allowed) : null);

    private static DecisionObjective Objective(JsonElement value) =>
        new(Text(value, "evidence")!, Text(value, "direction")!, Number(value, "target"));

    private static TelemetryAttributeSelector Selector(JsonElement value) =>
        new(Text(value, "from")!, Text(value, "key")!);

    private static IReadOnlyDictionary<string, JsonElement> Attributes(JsonElement value, string name) =>
        Properties(value, name).ToFrozenDictionary(item => item.Name, item => item.Value.Clone(), StringComparer.Ordinal);

    private static IEnumerable<JsonProperty> Properties(JsonElement value, string name) =>
        value.TryGetProperty(name, out var property) ? property.EnumerateObject() : [];

    private static (double? Minimum, double? Maximum) Range(JsonElement value) =>
        value.TryGetProperty("range", out var range) ? (range[0].GetDouble(), range[1].GetDouble()) : (null, null);

    private static void ValidateRange(double? minimum, double? maximum, string path, ICollection<ProblemIssue> issues)
    {
        if (minimum > maximum) issues.Add(Issue($"{path}/range", "Numeric bounds must be ordered."));
    }

    private static string? Text(JsonElement value, string name) =>
        value.TryGetProperty(name, out var property) ? property.GetString() : null;

    private static double? Number(JsonElement value, string name) =>
        value.TryGetProperty(name, out var property) ? property.GetDouble() : null;

    private static string[] Strings(JsonElement value) =>
        value.EnumerateArray().Select(item => item.GetString()!).ToArray();

    private static string Pointer(string value) => value.Replace("~", "~0").Replace("/", "~1");

    private static ProblemIssue Issue(string path, string message) => new("invalid-definition", "error", path, message);

    private static void AddSchemaErrors(EvaluationResults evaluation, ICollection<ProblemIssue> issues)
    {
        if (evaluation.IsValid) return;
        if (evaluation.Errors is not null)
        {
            foreach (var error in evaluation.Errors)
                issues.Add(Issue(evaluation.InstanceLocation.ToString(), error.Value));
        }
        if (evaluation.Details is not null)
        {
            foreach (var detail in evaluation.Details) AddSchemaErrors(detail, issues);
        }
        if (issues.Count == 0) issues.Add(Issue("/", "Manifest does not satisfy the v2 schema."));
    }

    private static JsonSchema LoadSchema()
    {
        using var stream = typeof(DecisionManifest).Assembly.GetManifestResourceStream("Flaggo.Registry.DecisionManifest.schema.json")
            ?? throw new InvalidOperationException("The embedded decision manifest schema is missing.");
        using var document = JsonDocument.Parse(stream);
        return JsonSchema.Build(document.RootElement.Clone(),
            new BuildOptions { Dialect = Dialect.Draft202012, SchemaRegistry = new() });
    }
}
