using System.Diagnostics;
using System.Diagnostics.Metrics;
using Flaggo.Contract;
using Flaggo.Decision;
using Flaggo.ServiceHosting;
using Microsoft.Data.Sqlite;

namespace Flaggo.DecisionService;

public sealed class DecisionServiceObservability
{
    private readonly FlaggoInstrumentation _instrumentation;
    private readonly Counter<long> _evaluations;
    private readonly Histogram<double> _duration;

    public DecisionServiceObservability(FlaggoInstrumentation instrumentation)
    {
        _instrumentation = instrumentation;
        _evaluations = instrumentation.Meter.CreateCounter<long>(
            "flaggo.decision.evaluations",
            "{evaluation}");
        _duration = instrumentation.Meter.CreateHistogram<double>(
            "flaggo.decision.evaluation.duration",
            "s");
    }

    public FlaggoOperation StartEvaluation(
        string contractName,
        string contractDigest)
    {
        var correlationId = Activity.Current?.GetTagItem(
            "flaggo.request.correlation_id");
        var activity = _instrumentation.StartActivity(
            "flaggo.decision.evaluate");
        activity?.SetTag("flaggo.operation.name", "decision.evaluate");
        activity?.SetTag("flaggo.contract.name", contractName);
        activity?.SetTag("flaggo.contract.digest", contractDigest);
        activity?.SetTag("flaggo.request.correlation_id", correlationId);
        return new FlaggoOperation(activity, _evaluations, _duration);
    }

    public static string EvaluationSource(RuntimeDecision decision) =>
        decision.Evaluation is RuleEvaluation ? "rule" : "default";

    public static (string Outcome, string Category) Classify(Exception exception) =>
        exception switch
        {
            HttpContractException
                or ArgumentException
                or RuntimeInputValidationException => ("invalid", "validation"),
            DecisionContractVersionNotFoundException => ("not_found", "not_found"),
            ActiveExecutableNotFoundException
                or SqliteException
                or IOException => ("unavailable", "dependency"),
            TimeoutException => ("unavailable", "timeout"),
            OperationCanceledException => ("cancelled", "cancelled"),
            RuntimeIntegrityException
                or DecisionResultValidationException => ("failure", "integrity"),
            DecisionEvaluationException => ("failure", "internal"),
            _ => ("failure", "internal")
        };
}
