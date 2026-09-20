using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

public interface ILifecycleAuditReader
{
    Task<LifecycleAuditTrail> ReadAsync(
        string appId,
        string environment,
        CancellationToken cancellationToken);
}

public static class LifecycleAuditIntegrity
{
    public static void Validate(LifecycleAuditTrail trail)
    {
        if (trail is null ||
            trail.Reviews is null || trail.Activations is null ||
            trail.Transitions is null || trail.Records is null)
        {
            throw Invalid("The lifecycle journal is incomplete.");
        }

        Unique(trail.Reviews.Select(item => item?.Receipt?.ReviewId), "review");
        Unique(trail.Activations.Select(item => item?.ActivationId), "activation");
        Unique(trail.Transitions.Select(item => item?.Request?.TransitionId), "transition");
        Unique(trail.Records.Select(item => item?.AuditId), "audit");
        Unique(trail.Activations.Where(item => item.Status == LifecycleMutationStatus.Applied)
            .Select(item => item.StateId), "activated state");
        var records = trail.Records.ToDictionary(item => item.AuditId, StringComparer.Ordinal);
        var referenced = new HashSet<string>(StringComparer.Ordinal);
        var proposals = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var review in trail.Reviews)
        {
            var commit = review.Commit;
            var receipt = review.Receipt;
            if (commit is null || commit.Request is null || commit.Actor is null ||
                commit.Decision is null || commit.Decision.Reasons is null ||
                commit.Request.Proposal?.Context is null || commit.Evidence is null ||
                receipt.Reasons is null || !commit.Actor.CanReview ||
                !LifecycleJson.IsDigest(commit.DefinitionSnapshotDigest) ||
                receipt.ReviewedAt == default ||
                receipt.ReviewId != commit.Request.ReviewId ||
                receipt.ProposalId != commit.Request.Proposal.Context.ProposalId ||
                receipt.Disposition != commit.Decision.Disposition ||
                !Enum.IsDefined(receipt.Disposition) ||
                !Enum.IsDefined(commit.Request.Replacement) ||
                receipt.Reasons.Any(string.IsNullOrWhiteSpace) ||
                receipt.Disposition != LifecycleDisposition.Approved && receipt.Reasons.Count == 0 ||
                receipt.AuditId != LifecycleIdentity.AuditId(review.RequestFingerprint, LifecycleAuditKind.Review) ||
                review.RequestFingerprint != LifecycleIdentity.ReviewRequest(commit.Request, commit.Actor.Identity) ||
                receipt.ReviewDigest != LifecycleIdentity.ReviewDigest(commit, receipt.ReviewedAt) ||
                LifecycleJson.Digest(receipt.Reasons) != LifecycleJson.Digest(commit.Decision.Reasons) ||
                LifecycleJson.Digest(receipt.EffectivePolicy) != LifecycleJson.Digest(commit.Decision.EffectivePolicy))
            {
                throw Invalid("A lifecycle review or its immutable receipt is inconsistent.");
            }

            try
            {
                DecisionProposalValidation.Validate(commit.Request.Proposal);
            }
            catch (DecisionProposalValidationException error)
            {
                throw new InvalidDataException("The lifecycle journal contains an invalid proposal.", error);
            }
            var proposal = commit.Request.Proposal;
            var context = proposal.Context;
            foreach (var snapshot in commit.Evidence)
            {
                ProposalEvidenceValidation.Validate(snapshot);
            }
            var proposalDigest = LifecycleJson.Digest(proposal);
            if (proposals.TryGetValue(receipt.ProposalId, out var prior))
            {
                if (prior != proposalDigest)
                {
                    throw Invalid("A lifecycle proposal identity has conflicting content.");
                }
            }
            else
            {
                proposals.Add(receipt.ProposalId, proposalDigest);
                RequireReviewAudit(LifecycleAuditKind.Proposal);
            }

            RequireReviewAudit(LifecycleAuditKind.Review);

            if (receipt.Disposition == LifecycleDisposition.Approved)
            {
                var approval = receipt.Approval;
                if (receipt.EffectivePolicy?.AutomaticApprovalAllowed != true ||
                    receipt.Reasons.Count != 0 ||
                    approval is null ||
                    approval.Mode != "automatic" ||
                    approval.ApprovalId != $"approval-{receipt.ReviewDigest[7..]}" ||
                    approval.ReviewId != receipt.ReviewId ||
                    approval.ReviewDigest != receipt.ReviewDigest ||
                    approval.ProposalDigest != proposalDigest ||
                    approval.PolicyRevision != receipt.EffectivePolicy.Revision ||
                    approval.Initiator != commit.Actor.Identity ||
                    approval.ApprovedAt != receipt.ReviewedAt ||
                    context.CreatedAt > receipt.ReviewedAt ||
                    context.ExpiresAt <= receipt.ReviewedAt ||
                    commit.Evidence.Any(snapshot => !ProposalEvidenceValidation.IsCurrent(
                        snapshot, receipt.ReviewedAt, receipt.EffectivePolicy.MaximumEvidenceAgeSeconds)))
                {
                    throw Invalid("An automatic approval is missing or does not bind its reviewed proposal.");
                }

                RequireReviewAudit(LifecycleAuditKind.AutomaticApproval);
            }
            else if (receipt.Approval is not null)
            {
                throw Invalid("A non-approved review cannot carry an approval.");
            }

            void RequireReviewAudit(LifecycleAuditKind kind) =>
                Require(new LifecycleAuditRecord(
                    LifecycleIdentity.AuditId(review.RequestFingerprint, kind),
                    kind, receipt.ReviewId, context.Definition, context.ControlTarget!,
                    commit.Actor.Identity, receipt.ReviewedAt, receipt.Reasons,
                    context.ProposalId, receipt.ReviewId, receipt.Approval?.ApprovalId,
                    Disposition: receipt.Disposition,
                    PolicyRevision: receipt.EffectivePolicy?.Revision));
        }

        var activatedProposals = new HashSet<string>(StringComparer.Ordinal);
        foreach (var activation in trail.Activations)
        {
            var review = trail.Reviews.SingleOrDefault(item =>
                item.Receipt.ReviewId == activation.ReviewId)
                ?? throw Invalid("An activation references an unknown review.");
            var request = new LifecycleActivationRequest(activation.ActivationId, activation.ReviewId);
            if (activation.Actor is null || activation.Reasons is null ||
                activation.Reasons.Any(string.IsNullOrWhiteSpace) ||
                activation.Fingerprint != LifecycleIdentity.ActivationRequest(request, activation.Actor) ||
                activation.Definition != review.Commit.Request.Proposal.Context.Definition ||
                !Enum.IsDefined(activation.Status) ||
                activation.RecordedAt == default)
            {
                throw Invalid("An activation receipt is inconsistent.");
            }

            var kind = activation.Status == LifecycleMutationStatus.Applied
                ? LifecycleAuditKind.Activation
                : LifecycleAuditKind.ActivationRejected;
            if (activation.AuditId != LifecycleIdentity.AuditId(activation.Fingerprint, kind) ||
                activation.ApprovalId != review.Receipt.Approval?.ApprovalId)
            {
                throw Invalid("An activation audit does not match its immutable receipt.");
            }
            RequireActivationAudit(kind);
            if (activation.Status == LifecycleMutationStatus.Applied)
            {
                if (string.IsNullOrWhiteSpace(activation.StateId) ||
                    activation.Generation is null or <= 0 ||
                    activation.ApprovalId is null ||
                    activation.ApprovalId != review.Receipt.Approval?.ApprovalId ||
                    activation.Reasons.Count != 0 ||
                    !activatedProposals.Add(review.Receipt.ProposalId) ||
                    activation.RecordedAt < review.Receipt.ReviewedAt ||
                    review.Commit.Request.Proposal.Context.ExpiresAt <= activation.RecordedAt ||
                    review.Commit.Evidence.Any(snapshot => !ProposalEvidenceValidation.IsCurrent(
                        snapshot, activation.RecordedAt, review.Receipt.EffectivePolicy?.MaximumEvidenceAgeSeconds)))
                {
                    throw Invalid("An applied activation must carry its recorded approval and state identity.");
                }
                if (activation.PredecessorStateId is not null &&
                    review.Commit.Baseline?.LifecycleStatus == GovernedDecisionStateStatus.Active)
                {
                    var replacementKind = review.Commit.Request.Replacement == LifecycleReplacementKind.Rollback
                        ? LifecycleAuditKind.Rollback
                        : LifecycleAuditKind.Supersession;
                    RequireActivationAudit(replacementKind);
                }
            }
            else if (activation.StateId is not null ||
                     activation.Generation is not null ||
                     activation.PredecessorStateId is not null ||
                     activation.Reasons.Count == 0)
            {
                throw Invalid("A rejected activation cannot claim replacement authority.");
            }

            void RequireActivationAudit(LifecycleAuditKind auditKind) =>
                Require(new LifecycleAuditRecord(
                    LifecycleIdentity.AuditId(activation.Fingerprint, auditKind),
                    auditKind, activation.ActivationId, activation.Definition,
                    review.Commit.Request.Proposal.Context.ControlTarget!,
                    activation.Actor, activation.RecordedAt, activation.Reasons,
                    review.Receipt.ProposalId, activation.ReviewId, activation.ApprovalId,
                    activation.StateId, activation.PredecessorStateId,
                    PolicyRevision: review.Receipt.EffectivePolicy?.Revision));
        }

        foreach (var transition in trail.Transitions)
        {
            if (transition.Request.Address is null || transition.Actor is null ||
                transition.Reasons is null || transition.Reasons.Any(string.IsNullOrWhiteSpace) ||
                string.IsNullOrWhiteSpace(transition.Request.Reason) ||
                transition.Fingerprint != LifecycleIdentity.TransitionRequest(transition.Request, transition.Actor) ||
                transition.RecordedAt == default ||
                !Enum.IsDefined(transition.Status) ||
                transition.Status == LifecycleMutationStatus.Rejected && transition.Reasons.Count == 0 ||
                transition.Status == LifecycleMutationStatus.Applied && transition.Reasons.Count != 0 ||
                transition.Request.Status is not
                    (GovernedDecisionStateStatus.Completed or GovernedDecisionStateStatus.Expired))
            {
                throw Invalid("A lifecycle transition receipt is inconsistent.");
            }
            var kind = transition.Status == LifecycleMutationStatus.Rejected
                ? LifecycleAuditKind.TransitionRejected
                : transition.Request.Status == GovernedDecisionStateStatus.Completed
                    ? LifecycleAuditKind.Completion
                    : LifecycleAuditKind.Expiry;
            var activation = trail.Activations.SingleOrDefault(item =>
                item.Status == LifecycleMutationStatus.Applied && item.StateId == transition.Request.StateId)
                ?? throw Invalid("A terminal transition references state without an audited activation.");
            var review = trail.Reviews.Single(item => item.Receipt.ReviewId == activation.ReviewId);
            if (transition.AuditId != LifecycleIdentity.AuditId(transition.Fingerprint, kind) ||
                activation.Definition.AppId != transition.Request.Address.AppId ||
                activation.Definition.Environment != transition.Request.Address.Environment ||
                activation.Definition.DecisionKey != transition.Request.Address.DecisionKey ||
                review.Commit.Request.Proposal.Context.ControlTarget != transition.Request.Address.ControlTarget ||
                transition.RecordedAt < activation.RecordedAt)
            {
                throw Invalid("A lifecycle transition audit does not match its outcome.");
            }
            Require(new LifecycleAuditRecord(
                transition.AuditId, kind, transition.Request.TransitionId, activation.Definition,
                transition.Request.Address.ControlTarget!, transition.Actor,
                transition.RecordedAt, transition.Reasons, review.Receipt.ProposalId,
                StateId: transition.Request.StateId));
        }
        if (referenced.Count != records.Count)
        {
            throw Invalid("The lifecycle journal contains an orphan audit record.");
        }

        void Require(LifecycleAuditRecord expected)
        {
            var actor = expected.Actor;
            var definition = expected.Definition;
            if (!records.TryGetValue(expected.AuditId, out var record) ||
                record.Definition is null || record.Actor is null || record.Reasons is null ||
                expected.ControlTarget is null ||
                string.IsNullOrWhiteSpace(expected.ControlTarget.Type) ||
                string.IsNullOrWhiteSpace(expected.ControlTarget.Id) ||
                string.IsNullOrWhiteSpace(actor.Subject) || string.IsNullOrWhiteSpace(actor.Issuer) ||
                actor.AppId != definition.AppId || actor.Environment != definition.Environment ||
                LifecycleJson.Digest(record) != LifecycleJson.Digest(expected) ||
                !referenced.Add(expected.AuditId))
            {
                throw Invalid("A required lifecycle audit record is missing, duplicated, or inconsistent.");
            }
        }
    }

    private static void Unique(IEnumerable<string?> identities, string kind)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        if (identities.Any(id => string.IsNullOrWhiteSpace(id) || !seen.Add(id)))
        {
            throw Invalid($"Lifecycle {kind} identities must be nonempty and unique.");
        }
    }

    private static InvalidDataException Invalid(string message) => new(message);
}
