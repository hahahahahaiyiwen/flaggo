using Flaggo.Audit;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed partial class InMemoryGovernedStateLifecycleStore
{
    public Task<LifecycleReviewRecord?> GetReviewAsync(
        string reviewId,
        CancellationToken cancellationToken) =>
        Read(() => _journal.Reviews.SingleOrDefault(item =>
            item.Receipt.ReviewId == reviewId), cancellationToken);

    public Task<LifecycleActivationReceipt?> GetActivationAsync(
        string activationId,
        CancellationToken cancellationToken) =>
        Read(() => _journal.Activations.SingleOrDefault(item =>
            item.ActivationId == activationId), cancellationToken);

    public Task<LifecycleTransitionReceipt?> GetTransitionAsync(
        string transitionId,
        CancellationToken cancellationToken) =>
        Read(() => _journal.Transitions.SingleOrDefault(item =>
            item.Request.TransitionId == transitionId), cancellationToken);

    public Task<LifecycleAuditTrail> ReadAsync(
        string appId,
        string environment,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            RequireScope(appId, environment);
            return Task.FromResult(LifecycleJson.Copy(_journal));
        }
    }

    public Task<LifecycleReviewReceipt> CommitReviewAsync(
        LifecycleReviewCommit commit,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ArgumentNullException.ThrowIfNull(commit);
        ArgumentNullException.ThrowIfNull(commit.Request);
        DecisionProposalValidation.Validate(commit.Request.Proposal);
        return Transact(
            store => store.RecordReview(LifecycleJson.Copy(commit)),
            cancellationToken);
    }

    public Task<LifecycleActivationReceipt> CommitActivationAsync(
        LifecycleActivationCommit commit,
        CancellationToken cancellationToken) =>
        Transact(
            store => store.RecordActivation(LifecycleJson.Copy(commit), cancellationToken),
            cancellationToken);

    public Task<LifecycleTransitionReceipt> CommitTransitionAsync(
        LifecycleTransitionCommit commit,
        CancellationToken cancellationToken) =>
        Transact(
            store => store.RecordTransition(LifecycleJson.Copy(commit), cancellationToken),
            cancellationToken);

    private Task<T?> Read<T>(Func<T?> read, CancellationToken cancellationToken)
        where T : class
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            var value = read();
            return Task.FromResult(value is null ? null : LifecycleJson.Copy(value));
        }
    }

    private Task<T> Transact<T>(
        Func<InMemoryGovernedStateLifecycleStore, T> change,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            var candidate = new InMemoryGovernedStateLifecycleStore(
                CapturePersistenceSnapshot(), _timeProvider, _identityGenerator);
            var result = change(candidate);
            candidate.ValidateJournalState();
            var copy = LifecycleJson.Copy(result);
            cancellationToken.ThrowIfCancellationRequested();
            _latestStateIds = candidate._latestStateIds;
            _states = candidate._states;
            _activations = candidate._activations;
            _transitions = candidate._transitions;
            _proposalFingerprints = candidate._proposalFingerprints;
            _journal = candidate._journal;
            return Task.FromResult(copy);
        }
    }

    private LifecycleReviewReceipt RecordReview(LifecycleReviewCommit commit)
    {
        ArgumentNullException.ThrowIfNull(commit);
        ArgumentException.ThrowIfNullOrWhiteSpace(commit.Request.ReviewId);
        DecisionProposalValidation.Validate(commit.Request.Proposal);
        var proposal = commit.Request.Proposal;
        var context = proposal.Context;
        RequireActor(commit.Actor, context.Definition.AppId, context.Definition.Environment, review: true);
        RequireScope(context.Definition.AppId, context.Definition.Environment);
        if (context.ControlTarget is null)
        {
            throw Validation("invalid-proposal", "Lifecycle governance requires an explicit control target.");
        }

        var fingerprint = LifecycleIdentity.ReviewRequest(commit.Request, commit.Actor.Identity);
        var prior = _journal.Reviews.SingleOrDefault(item =>
            item.Receipt.ReviewId == commit.Request.ReviewId);
        if (prior is not null)
        {
            RequireFingerprint(prior.RequestFingerprint, fingerprint, "review-conflict");
            return prior.Receipt;
        }

        var existingProposal = _journal.Reviews.FirstOrDefault(item =>
            item.Receipt.ProposalId == context.ProposalId);
        if (existingProposal is not null &&
            LifecycleJson.Digest(existingProposal.Commit.Request.Proposal) != LifecycleJson.Digest(proposal))
        {
            throw Conflict("proposal-conflict", "The proposal identity is bound to different immutable content.");
        }
        if (!Enum.IsDefined(commit.Request.Replacement) ||
            !Enum.IsDefined(commit.Decision.Disposition) ||
            !LifecycleJson.IsDigest(commit.DefinitionSnapshotDigest))
        {
            throw Validation("invalid-review", "The lifecycle review contains invalid contract data.");
        }

        var now = _timeProvider.GetUtcNow().ToUniversalTime();
        foreach (var snapshot in commit.Evidence)
        {
            ProposalEvidenceValidation.Validate(snapshot);
        }
        var digest = LifecycleIdentity.ReviewDigest(commit, now);
        var approved = commit.Decision.Disposition == LifecycleDisposition.Approved;
        if (approved && (commit.Decision.EffectivePolicy?.AutomaticApprovalAllowed != true ||
                         commit.Decision.Reasons.Count != 0 ||
                         context.CreatedAt > now ||
                         context.ExpiresAt <= now ||
                         commit.Evidence.Any(snapshot => !ProposalEvidenceValidation.IsCurrent(
                             snapshot, now, commit.Decision.EffectivePolicy?.MaximumEvidenceAgeSeconds))))
        {
            throw Validation("invalid-approval", "The review cannot materialize an eligible automatic approval.");
        }

        var approval = approved
            ? new AutomaticLifecycleApproval(
                $"approval-{digest[7..]}",
                commit.Request.ReviewId,
                LifecycleJson.Digest(proposal),
                digest,
                commit.Decision.EffectivePolicy!.Revision,
                commit.Actor.Identity,
                now)
            : null;
        var receipt = new LifecycleReviewReceipt(
            commit.Request.ReviewId,
            context.ProposalId,
            commit.Decision.Disposition,
            commit.Decision.Reasons,
            digest,
            LifecycleIdentity.AuditId(fingerprint, LifecycleAuditKind.Review),
            now,
            approval,
            commit.Decision.EffectivePolicy);
        var records = new List<LifecycleAuditRecord>();
        if (existingProposal is null)
        {
            records.Add(ReviewAudit(LifecycleAuditKind.Proposal));
        }
        records.Add(ReviewAudit(LifecycleAuditKind.Review));
        if (approval is not null)
        {
            records.Add(ReviewAudit(LifecycleAuditKind.AutomaticApproval));
        }
        _journal = _journal with
        {
            Reviews = [.. _journal.Reviews, new LifecycleReviewRecord(commit, fingerprint, receipt)],
            Records = [.. _journal.Records, .. records]
        };
        return receipt;

        LifecycleAuditRecord ReviewAudit(LifecycleAuditKind kind) =>
            new(
                LifecycleIdentity.AuditId(fingerprint, kind),
                kind,
                receipt.ReviewId,
                context.Definition,
                context.ControlTarget,
                commit.Actor.Identity,
                now,
                receipt.Reasons,
                context.ProposalId,
                receipt.ReviewId,
                approval?.ApprovalId,
                Disposition: receipt.Disposition,
                PolicyRevision: receipt.EffectivePolicy?.Revision);
    }

    private LifecycleActivationReceipt RecordActivation(
        LifecycleActivationCommit commit,
        CancellationToken cancellationToken)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(commit.Request.ActivationId);
        ArgumentException.ThrowIfNullOrWhiteSpace(commit.Request.ReviewId);
        var review = _journal.Reviews.SingleOrDefault(item =>
            item.Receipt.ReviewId == commit.Request.ReviewId)
            ?? throw Validation("review-not-found", "The lifecycle review does not exist.");
        var context = review.Commit.Request.Proposal.Context;
        RequireActor(commit.Actor, context.Definition.AppId, context.Definition.Environment, review: false);
        var fingerprint = LifecycleIdentity.ActivationRequest(commit.Request, commit.Actor.Identity);
        var prior = _journal.Activations.SingleOrDefault(item =>
            item.ActivationId == commit.Request.ActivationId);
        if (prior is not null)
        {
            RequireFingerprint(prior.Fingerprint, fingerprint, "activation-conflict");
            return prior;
        }

        var reasons = new List<string>();
        if (_proposalFingerprints.ContainsKey(context.ProposalId))
        {
            reasons.Add("duplicate-proposal");
        }
        if (review.Receipt.Approval is null ||
            review.Receipt.Disposition != LifecycleDisposition.Approved)
        {
            reasons.Add("review_not_approved");
        }
        if (commit.CurrentDecision.Disposition != LifecycleDisposition.Approved)
        {
            reasons.AddRange(commit.CurrentDecision.Reasons);
            reasons.Add("activation_policy_blocked");
        }
        if (commit.InputsDigest != LifecycleIdentity.InputsDigest(review.Commit) ||
            LifecycleJson.Digest(commit.CurrentDecision.EffectivePolicy) !=
                LifecycleJson.Digest(review.Receipt.EffectivePolicy))
        {
            reasons.Add("review_stale");
        }

        GovernedDecisionState? activated = null;
        var now = _timeProvider.GetUtcNow().ToUniversalTime();
        if (context.CreatedAt > now || context.ExpiresAt <= now)
        {
            reasons.Add("expired-proposal");
        }
        if (review.Commit.Evidence.Any(snapshot => !ProposalEvidenceValidation.IsCurrent(
                snapshot, now, review.Receipt.EffectivePolicy?.MaximumEvidenceAgeSeconds)))
        {
            reasons.Add("stale_evidence");
        }
        if (reasons.Count == 0)
        {
            try
            {
                var current = ResolveExpectedBaseline(Address(context), context.ExpectedBaseline);
                if (LifecycleJson.Digest(current) != LifecycleJson.Digest(review.Commit.Baseline))
                {
                    throw Conflict("stale-baseline", "The reviewed baseline changed before activation.");
                }
                activated = Activate(
                    new GovernedStateActivationRequest(
                        commit.Request.ActivationId,
                        review.Receipt.Approval!.ApprovalId,
                        review.Commit.Request.Proposal,
                        review.Commit.Request.Replacement == LifecycleReplacementKind.Rollback
                            ? GovernedDecisionStateStatus.RolledBack
                            : GovernedDecisionStateStatus.Superseded),
                    cancellationToken);
            }
            catch (GovernedStateConflictException error)
            {
                reasons.Add(error.Code);
            }
            catch (GovernedStateValidationException error)
            {
                reasons.Add(error.Code);
            }
        }

        var kind = activated is null ? LifecycleAuditKind.ActivationRejected : LifecycleAuditKind.Activation;
        var receipt = new LifecycleActivationReceipt(
            commit.Request.ActivationId,
            commit.Request.ReviewId,
            fingerprint,
            context.Definition,
            commit.Actor.Identity,
            activated is null ? LifecycleMutationStatus.Rejected : LifecycleMutationStatus.Applied,
            reasons.Distinct(StringComparer.Ordinal).ToArray(),
            LifecycleIdentity.AuditId(fingerprint, kind),
            activated?.ActivatedAt ?? now,
            activated?.StateId,
            activated?.Generation,
            activated?.PredecessorStateId,
            review.Receipt.Approval?.ApprovalId);
        var records = new List<LifecycleAuditRecord> { ActivationAudit(kind) };
        if (activated?.PredecessorStateId is not null &&
            review.Commit.Baseline?.LifecycleStatus == GovernedDecisionStateStatus.Active)
        {
            records.Add(ActivationAudit(
                review.Commit.Request.Replacement == LifecycleReplacementKind.Rollback
                    ? LifecycleAuditKind.Rollback
                    : LifecycleAuditKind.Supersession));
        }
        _journal = _journal with
        {
            Activations = [.. _journal.Activations, receipt],
            Records = [.. _journal.Records, .. records]
        };
        return receipt;

        LifecycleAuditRecord ActivationAudit(LifecycleAuditKind auditKind) =>
            new(
                LifecycleIdentity.AuditId(fingerprint, auditKind),
                auditKind,
                receipt.ActivationId,
                context.Definition,
                context.ControlTarget!,
                receipt.Actor,
                receipt.RecordedAt,
                receipt.Reasons,
                context.ProposalId,
                receipt.ReviewId,
                receipt.ApprovalId,
                receipt.StateId,
                receipt.PredecessorStateId,
                PolicyRevision: review.Receipt.EffectivePolicy?.Revision);
    }

    private LifecycleTransitionReceipt RecordTransition(
        LifecycleTransitionCommit commit,
        CancellationToken cancellationToken)
    {
        var request = commit.Request;
        ArgumentException.ThrowIfNullOrWhiteSpace(request.TransitionId);
        ArgumentException.ThrowIfNullOrWhiteSpace(request.Reason);
        RequireActor(commit.Actor, request.Address.AppId, request.Address.Environment, review: false);
        RequireScope(request.Address.AppId, request.Address.Environment);
        var fingerprint = LifecycleIdentity.TransitionRequest(request, commit.Actor.Identity);
        var prior = _journal.Transitions.SingleOrDefault(item =>
            item.Request.TransitionId == request.TransitionId);
        if (prior is not null)
        {
            RequireFingerprint(prior.Fingerprint, fingerprint, "transition-conflict");
            return prior;
        }
        if (request.Status is not (GovernedDecisionStateStatus.Completed or GovernedDecisionStateStatus.Expired))
        {
            throw Validation("invalid-lifecycle-transition", "Only completion and expiry are supported.");
        }
        if (!_states.TryGetValue(request.StateId, out var entry) || entry.Address != request.Address)
        {
            throw Conflict("target-conflict", "The transition does not identify state owned by the target.");
        }

        var reasons = new List<string>();
        try
        {
            Transition(
                new GovernedStateTransitionRequest(
                    request.TransitionId, request.Address, request.StateId,
                    request.ExpectedGeneration, request.Status),
                cancellationToken);
        }
        catch (GovernedStateConflictException error)
        {
            reasons.Add(error.Code);
        }
        catch (GovernedStateValidationException error)
        {
            reasons.Add(error.Code);
        }
        var applied = reasons.Count == 0;
        var kind = !applied ? LifecycleAuditKind.TransitionRejected
            : request.Status == GovernedDecisionStateStatus.Completed
                ? LifecycleAuditKind.Completion : LifecycleAuditKind.Expiry;
        var receipt = new LifecycleTransitionReceipt(
            request,
            commit.Actor.Identity,
            fingerprint,
            applied ? LifecycleMutationStatus.Applied : LifecycleMutationStatus.Rejected,
            reasons,
            LifecycleIdentity.AuditId(fingerprint, kind),
            _timeProvider.GetUtcNow().ToUniversalTime());
        var state = entry.State;
        var definition = _journal.Activations.Single(item =>
            item.Status == LifecycleMutationStatus.Applied && item.StateId == state.StateId).Definition;
        var audit = new LifecycleAuditRecord(
            receipt.AuditId, kind, request.TransitionId, definition,
            request.Address.ControlTarget!, receipt.Actor, receipt.RecordedAt,
            receipt.Reasons, state.ProposalId, StateId: state.StateId);
        _journal = _journal with
        {
            Transitions = [.. _journal.Transitions, receipt],
            Records = [.. _journal.Records, audit]
        };
        return receipt;
    }

    private void RequireScope(string appId, string environment)
    {
        if (string.IsNullOrWhiteSpace(appId) || string.IsNullOrWhiteSpace(environment) ||
            _states.Values.Any(item => item.Address.AppId != appId || item.Address.Environment != environment) ||
            _journal.Reviews.Any(item =>
                item.Commit.Request.Proposal.Context.Definition.AppId != appId ||
                item.Commit.Request.Proposal.Context.Definition.Environment != environment))
        {
            throw Validation("resource-scope-mismatch",
                "A lifecycle journal has exactly one application and environment scope.");
        }
    }

    private static void RequireActor(
        LifecycleActor actor,
        string appId,
        string environment,
        bool review)
    {
        if (actor is null ||
            string.IsNullOrWhiteSpace(actor.Subject) ||
            string.IsNullOrWhiteSpace(actor.Issuer) ||
            actor.AppId != appId || actor.Environment != environment ||
            (review ? !actor.CanReview : !actor.CanActivate))
        {
            throw Validation("actor-not-authorized", "The authenticated actor lacks lifecycle authority.");
        }
    }

    private static void RequireFingerprint(string stored, string supplied, string code)
    {
        if (stored != supplied)
        {
            throw Conflict(code, "The operation identity is bound to different content or actor identity.");
        }
    }

    private void ValidateJournalState()
    {
        LifecycleAuditIntegrity.Validate(_journal);
        var statuses = new Dictionary<string, GovernedDecisionStateStatus>(StringComparer.Ordinal);
        var latest = new Dictionary<GovernedStateAddress, string>();
        var applied = _journal.Activations
            .Where(item => item.Status == LifecycleMutationStatus.Applied)
            .ToDictionary(item => item.StateId!, StringComparer.Ordinal);
        if (applied.Count != _states.Count || _activations.Count != applied.Count ||
            _transitions.Count != _journal.Transitions.Count(item => item.Status == LifecycleMutationStatus.Applied))
        {
            throw new InvalidDataException("Governed state or replay metadata is missing its lifecycle journal.");
        }
        foreach (var audit in _journal.Records)
        {
            try
            {
                RequireScope(audit.Definition.AppId, audit.Definition.Environment);
            }
            catch (GovernedStateValidationException error)
            {
                throw new InvalidDataException("A persisted lifecycle journal contains mixed resource scopes.", error);
            }
            if (audit.Kind == LifecycleAuditKind.Activation)
            {
                var receipt = applied[audit.StateId!];
                if (!_states.TryGetValue(receipt.StateId!, out var entry))
                {
                    throw new InvalidDataException("The lifecycle journal references missing governed state.");
                }
                var review = _journal.Reviews.Single(item => item.Receipt.ReviewId == receipt.ReviewId);
                latest.TryGetValue(entry.Address, out var predecessor);
                var expectedGeneration = predecessor is null ? 1 : checked(_states[predecessor].State.Generation + 1);
                var baseline = predecessor is null
                    ? null
                    : _states[predecessor].State with { LifecycleStatus = statuses[predecessor] };
                if (receipt.PredecessorStateId != predecessor || receipt.Generation != expectedGeneration ||
                    review.Commit.Request.Proposal.Context.ExpectedBaseline !=
                        new GovernedStateBaseline(predecessor, expectedGeneration - 1) ||
                    LifecycleJson.Digest(review.Commit.Baseline) != LifecycleJson.Digest(baseline))
                {
                    throw new InvalidDataException("The lifecycle journal has an invalid generation or predecessor chain.");
                }
                var replacement = review.Commit.Request.Replacement == LifecycleReplacementKind.Rollback
                    ? GovernedDecisionStateStatus.RolledBack : GovernedDecisionStateStatus.Superseded;
                var request = new GovernedStateActivationRequest(
                    receipt.ActivationId, receipt.ApprovalId!,
                    review.Commit.Request.Proposal, replacement);
                var expected = CreateState(
                    request, receipt.StateId!, expectedGeneration,
                    predecessor, receipt.RecordedAt);
                if (entry.Address != Address(review.Commit.Request.Proposal.Context) ||
                    LifecycleJson.Digest(expected) != LifecycleJson.Digest(
                        entry.State with { LifecycleStatus = GovernedDecisionStateStatus.Active }) ||
                    !_activations.TryGetValue(receipt.ActivationId, out var replay) ||
                    replay.StateId != receipt.StateId || replay.Fingerprint != Fingerprint(request) ||
                    replay.ProposalId != expected.ProposalId ||
                    replay.ProposalFingerprint != Fingerprint(request.Proposal))
                {
                    throw new InvalidDataException("Governed state is not the complete audited activation.");
                }
                if (predecessor is not null && statuses[predecessor] == GovernedDecisionStateStatus.Active)
                {
                    statuses[predecessor] = replacement;
                }
                statuses.Add(receipt.StateId!, GovernedDecisionStateStatus.Active);
                latest[entry.Address] = receipt.StateId!;
            }
            else if (audit.Kind is LifecycleAuditKind.Completion or LifecycleAuditKind.Expiry)
            {
                var receipt = _journal.Transitions.Single(item => item.AuditId == audit.AuditId);
                var request = receipt.Request;
                if (!latest.TryGetValue(request.Address, out var currentId) ||
                    currentId != request.StateId ||
                    statuses[currentId] != GovernedDecisionStateStatus.Active ||
                    _states[currentId].State.Generation != request.ExpectedGeneration ||
                    !_transitions.TryGetValue(request.TransitionId, out var replay) ||
                    replay.StateId != request.StateId ||
                    replay.Fingerprint != Fingerprint(new GovernedStateTransitionRequest(
                        request.TransitionId, request.Address, request.StateId,
                        request.ExpectedGeneration, request.Status)))
                {
                    throw new InvalidDataException("The lifecycle journal contains an invalid terminal transition.");
                }
                statuses[currentId] = request.Status;
            }
        }
        if (_states.Any(item => !statuses.TryGetValue(item.Key, out var status) ||
                                item.Value.State.LifecycleStatus != status))
        {
            throw new InvalidDataException("Governed lifecycle status does not match its audited history.");
        }
    }
}
