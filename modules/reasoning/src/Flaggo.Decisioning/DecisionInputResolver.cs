using System.Collections.Frozen;
using System.Text.Json;
using Flaggo.Evidence;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning;

public sealed record ResolvedDecisionInputs(
    IReadOnlyDictionary<string, JsonElement> Values,
    IReadOnlyDictionary<string, InputProvenance> Provenance);

public sealed class DecisionInputResolver(IInputEvidenceReader evidence)
{
    public async Task<ResolvedDecisionInputs> ResolveAsync(
        ApplicationScope scope,
        RuntimeDecisionDefinition definition,
        TargetResolutionPlan targetPlan,
        IReadOnlyDictionary<string, JsonElement>? callerInputs,
        DateTimeOffset evaluatedAt,
        CancellationToken cancellationToken)
    {
        var values = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        var provenance = new Dictionary<string, InputProvenance>(StringComparer.Ordinal);
        var inputs = callerInputs ?? FrozenDictionary<string, JsonElement>.Empty;
        var declared = definition.Inputs.ToDictionary(input => input.Key, StringComparer.Ordinal);
        foreach (var key in inputs.Keys)
        {
            if (!declared.TryGetValue(key, out var input)) throw Invalid(key, "Unknown input.");
            if (input.Source != "request") throw Invalid(key, "Evidence-owned inputs cannot be supplied by callers.", "input-source-conflict");
        }
        var reads = new List<EvidenceInputRequest>();
        foreach (var input in definition.Inputs)
        {
            if (input.Source == "request")
            {
                if (!inputs.TryGetValue(input.Key, out var value) ||
                    !DecisionValues.Matches(value, input.ValueType, input.Minimum, input.Maximum))
                    throw Invalid(input.Key, "Required input is missing or violates its primitive type/bounds.");
                values.Add(input.Key, value.Clone());
                provenance.Add(input.Key, new InputProvenance("request"));
                continue;
            }
            if (input.Source != "evidence" || input.Binding is null)
                throw new DecisionContractException(409, "contract-conflict", "The registered input has an invalid source.");
            var binding = definition.Evidence.SingleOrDefault(binding => binding.Key == input.Binding)
                ?? throw new DecisionContractException(409, "contract-conflict", "The registered input binding is missing.");
            var target = targetPlan.ResolvedTargets?.SingleOrDefault(target => target.Type == binding.TargetType);
            if (target is null)
                throw new DecisionContractException(422, "invalid-runtime-context",
                    $"Evidence input '{input.Key}' requires a resolved '{binding.TargetType}' target.");
            reads.Add(new(input.Key, binding, target));
        }
        if (reads.Count > 0)
        {
            InputEvidenceResult resolved;
            try
            {
                resolved = await evidence.ReadInputsAsync(new InputEvidenceRequest(
                    scope,
                    definition.Identity, reads, evaluatedAt), cancellationToken);
            }
            catch (InputEvidenceUnavailableException error)
            {
                throw Unavailable(reads[0], "unavailable", error.Message);
            }
            foreach (var read in reads)
            {
                if (!resolved.Inputs.TryGetValue(read.InputKey, out var observation) ||
                    observation.Status != "available")
                {
                    throw Unavailable(read, observation?.Status ?? "missing", "Required materialized input is unusable.");
                }
                if (observation.Value is not JsonElement value ||
                    !DecisionValues.Matches(value, read.Binding.ValueType, read.Binding.Minimum, read.Binding.Maximum))
                    throw Unavailable(read, "invalid-value", "The materialized value violates its binding.");
                values.Add(read.InputKey, value.Clone());
                provenance.Add(read.InputKey, observation.Provenance);
            }
        }
        return new(values.ToFrozenDictionary(StringComparer.Ordinal), provenance.ToFrozenDictionary(StringComparer.Ordinal));
    }

    private static DecisionContractException Invalid(string key, string message, string issue = "invalid-inference-input") =>
        new(422, "invalid-inference-input", message,
            [new ProblemIssue(issue, "error", $"/inputs/{Pointer(key)}", message, InputKey: key)]);

    private static DecisionContractException Unavailable(EvidenceInputRequest input, string reason, string detail) =>
        new(503, "required-evidence-unavailable",
            $"Binding '{input.Binding.Key}' for input '{input.InputKey}' is {reason}.",
            [new ProblemIssue("required-evidence-unavailable", "error", $"/inputs/{Pointer(input.InputKey)}",
                $"{input.Binding.Key}: {reason}. {detail}", InputKey: input.InputKey)],
            clientFallback: new ClientFallbackEligibility(false, "required-input-evidence-unavailable"));

    private static string Pointer(string value) => value.Replace("~", "~0").Replace("/", "~1");
}
