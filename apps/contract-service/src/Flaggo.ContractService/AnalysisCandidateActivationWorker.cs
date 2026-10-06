using System.Globalization;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;

namespace Flaggo.ContractService;

public sealed record CandidateActivationOptions(
    TimeSpan PollInterval,
    int PageSize = 100)
{
    public const string PollIntervalEnvironmentVariable =
        "FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS";
    public const int DefaultPollIntervalMilliseconds = 5_000;

    public static CandidateActivationOptions FromConfiguration(
        IConfiguration configuration)
    {
        ArgumentNullException.ThrowIfNull(configuration);
        var value = configuration[PollIntervalEnvironmentVariable];
        if (string.IsNullOrWhiteSpace(value))
        {
            return new CandidateActivationOptions(
                TimeSpan.FromMilliseconds(DefaultPollIntervalMilliseconds));
        }

        if (!long.TryParse(
                value,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var milliseconds)
            || milliseconds <= 0)
        {
            throw new InvalidOperationException(
                $"{PollIntervalEnvironmentVariable} must be a positive integer.");
        }

        return new CandidateActivationOptions(TimeSpan.FromMilliseconds(milliseconds));
    }
}

public sealed class AnalysisCandidateActivationWorker(
    IAnalysisCandidateActivationStore activationStore,
    IExecutableStore executableStore,
    IContractVersionStore contractStore,
    FlaggoExecutableCompiler executableCompiler,
    CandidateActivationOptions options,
    TimeProvider timeProvider,
    ContractServiceObservability observability) : BackgroundService
{
    private string? _nextContractName;

    public override async Task StartAsync(CancellationToken cancellationToken)
    {
        try
        {
            await RunOnceAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception exception)
        {
            observability.ActivationScanFailed(exception);
        }

        await base.StartAsync(cancellationToken);
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(options.PollInterval, timeProvider, stoppingToken);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                return;
            }

            try
            {
                await RunOnceAsync(stoppingToken);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                return;
            }
            catch (Exception exception)
            {
                observability.ActivationScanFailed(exception);
            }
        }
    }

    public async Task RunOnceAsync(CancellationToken cancellationToken = default)
    {
        var page = await activationStore.ListPendingAsync(
            options.PageSize,
            _nextContractName,
            cancellationToken);
        _nextContractName = page.NextContractName;
        foreach (var candidate in page.Candidates)
        {
            await ProcessAsync(candidate, cancellationToken);
        }
    }

    private async Task ProcessAsync(
        AnalysisCandidateActivationSnapshot snapshot,
        CancellationToken cancellationToken)
    {
        var admission = snapshot.Admission;
        using var operation = observability.StartActivation(
            admission.ContractName,
            admission.ContractDigest,
            admission.ExecutableDigest);
        try
        {
            var assessment = await AssessAsync(snapshot, cancellationToken);
            var result = await activationStore.ResolveAsync(
                new AnalysisCandidateResolutionCommand(
                    admission,
                    snapshot.CandidateStateVersion,
                    snapshot.ActiveExecutable,
                    assessment.Action),
                cancellationToken);
            switch (result.Outcome)
            {
                case AnalysisCandidateResolutionOutcome.Activated:
                    operation.Complete("success");
                    observability.ActivationCompleted(
                        admission.ContractName,
                        admission.ContractDigest,
                        admission.ExecutableDigest);
                    break;
                case AnalysisCandidateResolutionOutcome.Rejected:
                    operation.Complete(
                        "rejected",
                        assessment.FailureCategory);
                    observability.ActivationResolved(
                        admission.ContractName,
                        admission.ContractDigest,
                        admission.ExecutableDigest,
                        "rejected",
                        assessment.FailureCategory,
                        result.SupersededCandidateCount);
                    break;
                case AnalysisCandidateResolutionOutcome.Superseded:
                    operation.Complete("superseded");
                    observability.ActivationResolved(
                        admission.ContractName,
                        admission.ContractDigest,
                        admission.ExecutableDigest,
                        "superseded",
                        failureCategory: null,
                        result.SupersededCandidateCount);
                    break;
                case AnalysisCandidateResolutionOutcome.Yielded:
                case AnalysisCandidateResolutionOutcome.AlreadyResolved:
                    operation.Complete("yielded");
                    observability.ActivationResolved(
                        admission.ContractName,
                        admission.ContractDigest,
                        admission.ExecutableDigest,
                        "yielded",
                        failureCategory: null,
                        result.SupersededCandidateCount);
                    break;
                default:
                    throw new InvalidDataException(
                        $"Unknown Candidate resolution outcome '{result.Outcome}'.");
            }
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            observability.ActivationRejected(
                admission.ContractName,
                admission.ContractDigest,
                admission.ExecutableDigest,
                exception);
        }
    }

    private async Task<CandidateAssessment> AssessAsync(
        AnalysisCandidateActivationSnapshot snapshot,
        CancellationToken cancellationToken)
    {
        try
        {
            var admission = snapshot.Admission;
            var accepted = await contractStore.GetAsync(
                admission.ContractName,
                admission.ContractDigest,
                cancellationToken);
            if (accepted is null)
            {
                return CandidateAssessment.Reject("not_found");
            }

            if (!string.Equals(
                    accepted.Contract.Learning?.Policy.Mode,
                    "auto-activation",
                    StringComparison.Ordinal))
            {
                return CandidateAssessment.Reject("validation");
            }

            var candidate = await executableStore.GetAsync(
                admission.ExecutableDigest,
                cancellationToken);
            if (candidate is null)
            {
                return CandidateAssessment.Reject("not_found");
            }

            if (candidate.State != ExecutableLifecycleState.Candidate
                || candidate.StateVersion != snapshot.CandidateStateVersion)
            {
                return CandidateAssessment.Activate;
            }

            if (!string.Equals(
                    candidate.Executable.ContractDigest,
                    admission.ContractDigest,
                    StringComparison.Ordinal))
            {
                return CandidateAssessment.Reject("integrity");
            }

            var compilation = executableCompiler.Compile(
                accepted.Contract,
                candidate.Executable);
            if (!string.Equals(
                    compilation.ExecutableDigest,
                    candidate.ExecutableDigest,
                    StringComparison.Ordinal)
                || !JsonSerializer.SerializeToUtf8Bytes(
                        compilation.CheckedExecutable,
                        StrictJson.Options)
                    .AsSpan()
                    .SequenceEqual(
                        JsonSerializer.SerializeToUtf8Bytes(
                            candidate.CheckedExecutable,
                            StrictJson.Options)))
            {
                return CandidateAssessment.Reject("integrity");
            }

            return CandidateAssessment.Activate;
        }
        catch (ExecutableCompilationException)
        {
            return CandidateAssessment.Reject("validation");
        }
        catch (InvalidDataException)
        {
            return CandidateAssessment.Reject("integrity");
        }
        catch (JsonException)
        {
            return CandidateAssessment.Reject("integrity");
        }
        catch (ArgumentException)
        {
            return CandidateAssessment.Reject("validation");
        }
    }

    private sealed record CandidateAssessment(
        AnalysisCandidateResolutionAction Action,
        string? FailureCategory)
    {
        public static CandidateAssessment Activate { get; } =
            new(AnalysisCandidateResolutionAction.Activate, FailureCategory: null);

        public static CandidateAssessment Reject(string failureCategory) =>
            new(AnalysisCandidateResolutionAction.Reject, failureCategory);
    }
}
