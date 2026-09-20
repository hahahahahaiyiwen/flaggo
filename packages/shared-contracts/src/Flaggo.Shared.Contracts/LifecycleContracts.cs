using System.Text.Json.Serialization;

namespace Flaggo.Shared.Contracts;

public enum LifecycleDisposition
{
    Approved,
    Limited,
    PendingApproval,
    Hold,
    Rejected
}

public enum LifecycleReplacementKind
{
    Supersede,
    Rollback
}

public enum LifecycleMutationStatus
{
    Applied,
    Rejected
}

public sealed record LifecycleActorIdentity(
    string Subject,
    string Issuer,
    string AppId,
    string Environment);

public sealed record LifecycleActor(
    string Subject,
    string Issuer,
    string AppId,
    string Environment,
    bool CanReview,
    bool CanActivate)
{
    [JsonIgnore]
    public LifecycleActorIdentity Identity =>
        new(Subject, Issuer, AppId, Environment);
}

public sealed record LifecyclePolicyLayer(
    string Revision,
    IReadOnlyList<string> AllowedTargetKinds,
    bool AutomaticApprovalAllowed = false,
    bool AllowSupersession = false,
    bool AllowRollback = false,
    IReadOnlyList<DecisionTargetRef>? AllowedTargets = null,
    double? Minimum = null,
    double? Maximum = null,
    double? MinimumEvidenceQuality = null,
    double? MaximumModelUncertainty = null,
    double? MinimumExpectedOutcome = null,
    double? MinimumSampleSize = null,
    double? MaximumActivationDelta = null,
    double? MinimumActivationIntervalSeconds = null,
    double? MaximumEvidenceAgeSeconds = null,
    bool Paused = false);

public sealed record LifecyclePolicyContext(
    string AppId,
    string Environment,
    string DecisionKey,
    LifecyclePolicyLayer EnvironmentPolicy,
    LifecyclePolicyLayer OperatorControls);

public sealed record ProposalEvidenceSnapshot(
    string Reference,
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef ControlTarget,
    DateTimeOffset ObservedAt,
    DateTimeOffset? ExpiresAt,
    DecisionEvidenceSnapshot Evidence);

public sealed record LifecyclePolicyDecision(
    LifecycleDisposition Disposition,
    IReadOnlyList<string> Reasons,
    LifecyclePolicyLayer? EffectivePolicy);

public sealed record LifecycleReviewRequest(
    string ReviewId,
    DecisionProposal Proposal,
    LifecycleReplacementKind Replacement = LifecycleReplacementKind.Supersede);

public sealed record LifecycleReviewCommit(
    LifecycleReviewRequest Request,
    LifecycleActor Actor,
    LifecyclePolicyDecision Decision,
    string DefinitionSnapshotDigest,
    LifecyclePolicyContext? PolicyContext,
    IReadOnlyList<ProposalEvidenceSnapshot> Evidence,
    GovernedDecisionState? Baseline);

public sealed record AutomaticLifecycleApproval(
    string ApprovalId,
    string ReviewId,
    string ProposalDigest,
    string ReviewDigest,
    string PolicyRevision,
    LifecycleActorIdentity Initiator,
    DateTimeOffset ApprovedAt,
    string Mode = "automatic");

public sealed record LifecycleReviewReceipt(
    string ReviewId,
    string ProposalId,
    LifecycleDisposition Disposition,
    IReadOnlyList<string> Reasons,
    string ReviewDigest,
    string AuditId,
    DateTimeOffset ReviewedAt,
    AutomaticLifecycleApproval? Approval,
    LifecyclePolicyLayer? EffectivePolicy);

public sealed record LifecycleReviewRecord(
    LifecycleReviewCommit Commit,
    string RequestFingerprint,
    LifecycleReviewReceipt Receipt);

public sealed record LifecycleActivationRequest(
    string ActivationId,
    string ReviewId);

public sealed record LifecycleActivationCommit(
    LifecycleActivationRequest Request,
    LifecycleActor Actor,
    string InputsDigest,
    LifecyclePolicyDecision CurrentDecision);

public sealed record LifecycleActivationReceipt(
    string ActivationId,
    string ReviewId,
    string Fingerprint,
    GovernedDefinitionIdentity Definition,
    LifecycleActorIdentity Actor,
    LifecycleMutationStatus Status,
    IReadOnlyList<string> Reasons,
    string AuditId,
    DateTimeOffset RecordedAt,
    string? StateId = null,
    long? Generation = null,
    string? PredecessorStateId = null,
    string? ApprovalId = null);

public sealed record LifecycleTransitionRequest(
    string TransitionId,
    GovernedStateAddress Address,
    string StateId,
    long ExpectedGeneration,
    GovernedDecisionStateStatus Status,
    string Reason);

public sealed record LifecycleTransitionCommit(
    LifecycleTransitionRequest Request,
    LifecycleActor Actor);

public sealed record LifecycleTransitionReceipt(
    LifecycleTransitionRequest Request,
    LifecycleActorIdentity Actor,
    string Fingerprint,
    LifecycleMutationStatus Status,
    IReadOnlyList<string> Reasons,
    string AuditId,
    DateTimeOffset RecordedAt);

public enum LifecycleAuditKind
{
    Proposal,
    Review,
    AutomaticApproval,
    Activation,
    ActivationRejected,
    Supersession,
    Rollback,
    Completion,
    Expiry,
    TransitionRejected
}

public sealed record LifecycleAuditRecord(
    string AuditId,
    LifecycleAuditKind Kind,
    string OperationId,
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef ControlTarget,
    LifecycleActorIdentity Actor,
    DateTimeOffset RecordedAt,
    IReadOnlyList<string> Reasons,
    string? ProposalId = null,
    string? ReviewId = null,
    string? ApprovalId = null,
    string? StateId = null,
    string? PredecessorStateId = null,
    LifecycleDisposition? Disposition = null,
    string? PolicyRevision = null);

public sealed record LifecycleAuditTrail(
    IReadOnlyList<LifecycleReviewRecord> Reviews,
    IReadOnlyList<LifecycleActivationReceipt> Activations,
    IReadOnlyList<LifecycleTransitionReceipt> Transitions,
    IReadOnlyList<LifecycleAuditRecord> Records);
