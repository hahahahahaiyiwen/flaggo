using System.Diagnostics;
using System.Diagnostics.Metrics;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.ServiceHosting;
using Microsoft.Data.Sqlite;

namespace Flaggo.ContractService;

public sealed class ContractServiceObservability
{
    private readonly FlaggoInstrumentation _instrumentation;
    private readonly Counter<long> _operations;
    private readonly Histogram<double> _operationDuration;
    private readonly Counter<long> _candidateSubmissions;
    private readonly Counter<long> _activationAttempts;

    public ContractServiceObservability(FlaggoInstrumentation instrumentation)
    {
        _instrumentation = instrumentation;
        _operations = instrumentation.Meter.CreateCounter<long>(
            "flaggo.contract.operations",
            "{operation}");
        _operationDuration = instrumentation.Meter.CreateHistogram<double>(
            "flaggo.contract.operation.duration",
            "s");
        _candidateSubmissions = instrumentation.Meter.CreateCounter<long>(
            "flaggo.candidate.submissions",
            "{submission}");
        _activationAttempts = instrumentation.Meter.CreateCounter<long>(
            "flaggo.activation.attempts",
            "{attempt}");
    }

    public FlaggoOperation StartContractOperation(
        string spanName,
        string operationName,
        string? contractName = null,
        string? contractDigest = null)
    {
        var correlationId = Activity.Current?.GetTagItem(
            "flaggo.request.correlation_id");
        var activity = _instrumentation.StartActivity(spanName);
        activity?.SetTag("flaggo.operation.name", operationName);
        activity?.SetTag("flaggo.contract.name", contractName);
        activity?.SetTag("flaggo.contract.digest", contractDigest);
        activity?.SetTag("flaggo.request.correlation_id", correlationId);
        return new FlaggoOperation(
            activity,
            _operations,
            _operationDuration,
            operationName);
    }

    public FlaggoOperation StartCandidateSubmission(
        string contractName,
        string contractDigest,
        string candidateDigest)
    {
        var correlationId = Activity.Current?.GetTagItem(
            "flaggo.request.correlation_id");
        var activity = _instrumentation.StartActivity("flaggo.candidate.submit");
        activity?.SetTag("flaggo.operation.name", "candidate.submit");
        activity?.SetTag("flaggo.contract.name", contractName);
        activity?.SetTag("flaggo.contract.digest", contractDigest);
        activity?.SetTag("flaggo.candidate.digest", candidateDigest);
        activity?.SetTag("flaggo.request.correlation_id", correlationId);
        return new FlaggoOperation(
            activity,
            _operations,
            _operationDuration,
            "candidate.submit",
            _candidateSubmissions);
    }

    public FlaggoOperation StartActivation(
        string contractName,
        string contractDigest,
        string candidateDigest)
    {
        var correlationId = Activity.Current?.GetTagItem(
            "flaggo.request.correlation_id");
        var activity = _instrumentation.StartActivity("flaggo.candidate.activate");
        activity?.SetTag("flaggo.operation.name", "candidate.activate");
        activity?.SetTag("flaggo.contract.name", contractName);
        activity?.SetTag("flaggo.contract.digest", contractDigest);
        activity?.SetTag("flaggo.candidate.digest", candidateDigest);
        activity?.SetTag("flaggo.executable.digest", candidateDigest);
        activity?.SetTag("flaggo.request.correlation_id", correlationId);
        return new FlaggoOperation(
            activity,
            _operations,
            _operationDuration,
            "candidate.activate",
            _activationAttempts);
    }

    public void CandidateAdmitted(
        string contractName,
        string contractDigest,
        string candidateDigest) =>
        _instrumentation.LogEvent(
            LogLevel.Information,
            "flaggo.candidate.admitted",
            attributes: ContractAttributes(
                contractName,
                contractDigest,
                "success",
                candidateDigest));

    public void CandidateRejected(
        string contractName,
        string contractDigest,
        string candidateDigest,
        Exception exception)
    {
        var (outcome, category) = Classify(exception);
        var attributes = ContractAttributes(
            contractName,
            contractDigest,
            outcome,
            candidateDigest);
        attributes["flaggo.failure.category"] = category;
        _instrumentation.LogEvent(
            LogLevel.Warning,
            "flaggo.candidate.rejected",
            exception,
            attributes);
    }

    public void ActivationCompleted(
        string contractName,
        string contractDigest,
        string executableDigest) =>
        _instrumentation.LogEvent(
            LogLevel.Information,
            "flaggo.activation.completed",
            attributes: new Dictionary<string, object?>
            {
                ["flaggo.contract.name"] = contractName,
                ["flaggo.contract.digest"] = contractDigest,
                ["flaggo.candidate.digest"] = executableDigest,
                ["flaggo.executable.digest"] = executableDigest,
                ["flaggo.operation.outcome"] = "success"
            });

    public void ActivationRejected(
        string contractName,
        string contractDigest,
        string executableDigest,
        Exception exception)
    {
        var (outcome, category) = Classify(exception);
        _instrumentation.LogEvent(
            LogLevel.Warning,
            "flaggo.activation.rejected",
            exception,
            new Dictionary<string, object?>
            {
                ["flaggo.contract.name"] = contractName,
                ["flaggo.contract.digest"] = contractDigest,
                ["flaggo.candidate.digest"] = executableDigest,
                ["flaggo.executable.digest"] = executableDigest,
                ["flaggo.operation.outcome"] = outcome,
                ["flaggo.failure.category"] = category
            });
    }

    public static (string Outcome, string Category) Classify(Exception exception) =>
        exception switch
        {
            ContractNameMismatchException
                or InvalidDecisionContractException
                or ArgumentException
                or FormatException => ("invalid", "validation"),
            ActivationConflictException => ("conflict", "conflict"),
            ContractNameAuthorityConflictException => ("conflict", "conflict"),
            SqliteException or IOException => ("unavailable", "dependency"),
            TimeoutException => ("unavailable", "timeout"),
            OperationCanceledException => ("cancelled", "cancelled"),
            InvalidDataException => ("failure", "integrity"),
            _ => ("failure", "internal")
        };

    private static Dictionary<string, object?> ContractAttributes(
        string contractName,
        string contractDigest,
        string outcome,
        string candidateDigest) =>
        new()
        {
            ["flaggo.contract.name"] = contractName,
            ["flaggo.contract.digest"] = contractDigest,
            ["flaggo.candidate.digest"] = candidateDigest,
            ["flaggo.operation.outcome"] = outcome
        };
}
