namespace Flaggo.Shared.Contracts;

public static class ProposalEvidenceValidation
{
    public static void Validate(ProposalEvidenceSnapshot snapshot)
    {
        if (snapshot is null ||
            string.IsNullOrWhiteSpace(snapshot.Reference) ||
            snapshot.Definition is not { Contract: not null } definition ||
            string.IsNullOrWhiteSpace(definition.AppId) ||
            string.IsNullOrWhiteSpace(definition.Environment) ||
            string.IsNullOrWhiteSpace(definition.DecisionKey) ||
            string.IsNullOrWhiteSpace(definition.Contract.DefinitionId) ||
            string.IsNullOrWhiteSpace(definition.Contract.Revision) ||
            !LifecycleJson.IsDigest(definition.Contract.ContractDigest) ||
            definition.Contract.BundleDigest is not null && !LifecycleJson.IsDigest(definition.Contract.BundleDigest) ||
            snapshot.ControlTarget is null ||
            string.IsNullOrWhiteSpace(snapshot.ControlTarget.Type) ||
            string.IsNullOrWhiteSpace(snapshot.ControlTarget.Id) ||
            snapshot.ObservedAt == default ||
            snapshot.ExpiresAt <= snapshot.ObservedAt ||
            snapshot.Evidence is not { } evidence)
        {
            throw new InvalidDataException("A proposal evidence snapshot is missing valid identity or observation metadata.");
        }
        if (!Probability(evidence.EvidenceQuality) ||
            evidence.ModelUncertainty is double uncertainty && !Probability(uncertainty) ||
            evidence.ExpectedOutcome is double outcome && !Probability(outcome) ||
            evidence.SampleSize is double size && (!double.IsFinite(size) || size < 0))
        {
            throw new InvalidDataException("Proposal evidence quality, uncertainty, outcome, and sample size are invalid.");
        }
    }

    private static bool Probability(double value) => double.IsFinite(value) && value is >= 0 and <= 1;

    public static bool IsCurrent(
        ProposalEvidenceSnapshot snapshot,
        DateTimeOffset now,
        double? maximumAgeSeconds) =>
        snapshot.ObservedAt <= now &&
        (snapshot.ExpiresAt is null || snapshot.ExpiresAt > now) &&
        (maximumAgeSeconds is null || (now - snapshot.ObservedAt).TotalSeconds <= maximumAgeSeconds);
}
