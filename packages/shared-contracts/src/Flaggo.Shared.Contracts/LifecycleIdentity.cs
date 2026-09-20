namespace Flaggo.Shared.Contracts;

public static class LifecycleIdentity
{
    public static string ReviewRequest(
        LifecycleReviewRequest request,
        LifecycleActorIdentity actor) =>
        LifecycleJson.Digest(new { Operation = "review", Request = request, Actor = actor });

    public static string ActivationRequest(
        LifecycleActivationRequest request,
        LifecycleActorIdentity actor) =>
        LifecycleJson.Digest(new { Operation = "activation", Request = request, Actor = actor });

    public static string TransitionRequest(
        LifecycleTransitionRequest request,
        LifecycleActorIdentity actor) =>
        LifecycleJson.Digest(new { Operation = "transition", Request = request, Actor = actor });

    public static string ReviewDigest(LifecycleReviewCommit commit, DateTimeOffset reviewedAt) =>
        LifecycleJson.Digest(new { Commit = commit, ReviewedAt = reviewedAt });

    public static string InputsDigest(LifecycleReviewCommit commit) =>
        LifecycleJson.Digest(new
        {
            commit.DefinitionSnapshotDigest,
            commit.PolicyContext,
            commit.Evidence,
            commit.Baseline
        });

    public static string AuditId(string operationFingerprint, LifecycleAuditKind kind) =>
        $"lifecycle-{LifecycleJson.Digest(new { operationFingerprint, kind })[7..]}";
}
