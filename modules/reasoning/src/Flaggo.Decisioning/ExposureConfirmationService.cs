using Flaggo.Audit;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning;

public interface IExposureConfirmationService
{
    Task<ExposureConfirmationOutcome> ConfirmAsync(
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);
}

public sealed class ExposureConfirmationService(
    IExposureStore exposureStore,
    IExposureAuditSink exposureAuditSink) : IExposureConfirmationService
{
    public async Task<ExposureConfirmationOutcome> ConfirmAsync(
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken)
    {
        var preparation = await exposureStore.PrepareConfirmationAsync(
            decisionId,
            request,
            appIds,
            environments,
            cancellationToken);
        var outcome = preparation.Outcome;
        if (preparation.AlreadyConfirmed)
        {
            return outcome;
        }

        await exposureAuditSink.RecordExposureAsync(
            new ExposureAuditRecord(
                outcome.Result.ExposureId,
                outcome.Result.DecisionId,
                outcome.Snapshot.AppId,
                outcome.Snapshot.Environment,
                outcome.AppliedAt,
                outcome.Result.ConfirmedAt),
            cancellationToken);
        await exposureStore.CommitConfirmationAsync(
            decisionId,
            outcome.Result.ExposureId,
            CancellationToken.None);

        return outcome;
    }
}
