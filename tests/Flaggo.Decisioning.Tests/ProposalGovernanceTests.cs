using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Evidence;
using Flaggo.Lifecycle;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Logging;

namespace Flaggo.Decisioning.Tests;

public sealed class ProposalGovernanceTests
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task EvidenceMustStillBeFreshWhenActivationCommits(bool useMaximumAge)
    {
        var fixture = new Fixture();
        if (useMaximumAge)
        {
            fixture.Policy.Context = fixture.Policy.Context with
            {
                OperatorControls = LifecycleTestData.Layer() with { MaximumEvidenceAgeSeconds = 1 }
            };
        }
        var evidence = new MutableEvidence
        {
            Snapshots =
            [
                new("evidence", LifecycleTestData.Definition, LifecycleTestData.Target, LifecycleTestData.Now,
                    useMaximumAge ? null : LifecycleTestData.Now.AddSeconds(1), new DecisionEvidenceSnapshot(0.9))
            ]
        };
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with { Context = proposal.Context with { EvidenceReferences = ["evidence"] } };
        await fixture.CreateService(evidence: evidence).ReviewAsync(new("review", proposal), CancellationToken.None);
        var delayed = fixture.CreateService(
            evidence: evidence,
            policy: new AfterEvaluationPolicy(() => fixture.Clock.Now = LifecycleTestData.Now.AddSeconds(2)));

        var result = await delayed.ActivateAsync(new("activation", "review"), CancellationToken.None);

        Assert.Equal(LifecycleMutationStatus.Rejected, result.Status);
        Assert.Contains("stale_evidence", result.Reasons);
        Assert.Null(await fixture.Store.GetBaselineAsync(Address(), CancellationToken.None));
    }

    [Fact]
    public async Task EvidenceExpiringBetweenEvaluationAndReviewCommitCannotCreateApproval()
    {
        var fixture = new Fixture();
        var evidence = new MutableEvidence
        {
            Snapshots =
            [
                new("evidence", LifecycleTestData.Definition, LifecycleTestData.Target, LifecycleTestData.Now,
                    LifecycleTestData.Now.AddSeconds(1), new DecisionEvidenceSnapshot(0.9))
            ]
        };
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with { Context = proposal.Context with { EvidenceReferences = ["evidence"] } };
        var service = fixture.CreateService(
            evidence: evidence,
            policy: new AfterEvaluationPolicy(() => fixture.Clock.Now = LifecycleTestData.Now.AddSeconds(2)));

        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(() =>
            service.ReviewAsync(new("review", proposal), CancellationToken.None));

        Assert.Equal("invalid-approval", error.Code);
        Assert.Null(await fixture.Store.GetReviewAsync("review", CancellationToken.None));
    }

    [Fact]
    public async Task EvidenceChangesAfterApprovalRequireANewReview()
    {
        var fixture = new Fixture();
        var evidence = new MutableEvidence
        {
            Snapshots =
            [
                new("evidence", LifecycleTestData.Definition, LifecycleTestData.Target,
                    LifecycleTestData.Now.AddMinutes(-1), LifecycleTestData.Now.AddMinutes(5),
                    new DecisionEvidenceSnapshot(0.9))
            ]
        };
        var service = fixture.CreateService(evidence: evidence);
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with { Context = proposal.Context with { EvidenceReferences = ["evidence"] } };
        await service.ReviewAsync(new("review", proposal), CancellationToken.None);
        evidence.Snapshots = [evidence.Snapshots[0] with { Evidence = new DecisionEvidenceSnapshot(0.95) }];

        var changed = await service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        Assert.Equal(LifecycleMutationStatus.Rejected, changed.Status);
        Assert.Contains("review_stale", changed.Reasons);
        Assert.Null(await fixture.Store.GetBaselineAsync(Address(), CancellationToken.None));

        await service.ReviewAsync(new("new-review", proposal), CancellationToken.None);
        evidence.Snapshots = [];
        var missing = await service.ActivateAsync(new("new-activation", "new-review"), CancellationToken.None);
        Assert.Equal(LifecycleMutationStatus.Rejected, missing.Status);
        Assert.Contains("required_evidence_unavailable", missing.Reasons);
    }

    [Fact]
    public async Task AutomaticReviewApprovalActivationFeedsTheExistingRuntime()
    {
        var fixture = new Fixture(strategy: true);
        var proposal = new NumericStrategyDecisionProposal(
            LifecycleTestData.Proposal().Context,
            JsonSerializer.SerializeToElement(800),
            "strategy",
            new NumericRuleStrategy("pressure", 0.7, 850, 750));
        var reviewed = await fixture.Service.ReviewAsync(
            new LifecycleReviewRequest("review", proposal), CancellationToken.None);
        var activated = await fixture.Service.ActivateAsync(
            new LifecycleActivationRequest("activation", "review"), CancellationToken.None);
        var audit = new InMemoryAuditSink();
        var runtime = new DecisionService(
            fixture.Registry,
            new GovernedStateRuntimeProjection(fixture.Store, "app", "dev"),
            new InMemoryExposureStore(fixture.Clock, () => Guid.NewGuid().ToString("N")),
            audit,
            new GuidRuntimeIdGenerator(),
            fixture.Clock,
            new DefaultTargetResolver(new Dictionary<string, string> { ["canary"] = "canary" }),
            new InMemoryEvidenceProvider(new Dictionary<string, DecisionEvidenceSnapshot>
            {
                ["strategy"] = new(0.9)
            }),
            new DeterministicStrategyExecutor(),
            new DefaultPolicyEvaluator(fixture.Clock),
            NullLogger<DecisionService>.Instance);
        var result = await runtime.DecideAsync(
            "decision",
            new DecideRequest(
                LifecycleTestData.Definition.Contract,
                new Dictionary<string, JsonElement>(),
                new DecisionClient("app", "dev"),
                LifecycleTestData.Target,
                [new SignalInput(new SignalRef("pressure"), JsonSerializer.SerializeToElement(0.9))]));

        Assert.Equal("automatic", reviewed.Approval!.Mode);
        Assert.Equal(LifecycleMutationStatus.Applied, activated.Status);
        Assert.Equal("strategy", result.DecisionMode);
        Assert.Equal(850, result.Value.GetInt32());
        Assert.Equal("approved", result.Policy.Result);
        Assert.Single(audit.Records);
    }

    [Fact]
    public async Task HumanRequiredStaysPendingEvenIfAProducerClaimsToBeAnOperator()
    {
        var fixture = new Fixture();
        fixture.Policy.Context = fixture.Policy.Context with
        {
            OperatorControls = LifecycleTestData.Layer() with { AutomaticApprovalAllowed = false }
        };
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                Source = new DecisionProposalSource(DecisionProposalSourceKind.Operator, "claimed-operator")
            }
        };
        var review = await fixture.Service.ReviewAsync(new("review", proposal), CancellationToken.None);
        var activation = await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);

        Assert.Equal(LifecycleDisposition.PendingApproval, review.Disposition);
        Assert.Null(review.Approval);
        Assert.Equal(LifecycleMutationStatus.Rejected, activation.Status);
        Assert.Null(await fixture.Store.GetBaselineAsync(Address(), CancellationToken.None));
    }

    [Fact]
    public async Task ChangedPolicyRequiresANewReviewInsteadOfReusingAnApproval()
    {
        var fixture = new Fixture();
        await fixture.Service.ReviewAsync(new("review", LifecycleTestData.Proposal()), CancellationToken.None);
        fixture.Policy.Context = fixture.Policy.Context with
        {
            OperatorControls = LifecycleTestData.Layer("changed-policy") with { Paused = true }
        };
        var rejected = await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);

        Assert.Equal(LifecycleMutationStatus.Rejected, rejected.Status);
        Assert.Contains("review_stale", rejected.Reasons);
        Assert.Contains("lifecycle_paused", rejected.Reasons);
        Assert.Null(await fixture.Store.GetBaselineAsync(Address(), CancellationToken.None));
    }

    [Fact]
    public async Task CommittedReplayDoesNotReevaluateExpiredProposalsOrUnavailableDependencies()
    {
        var fixture = new Fixture();
        var request = new LifecycleReviewRequest("review", LifecycleTestData.Proposal());
        var review = await fixture.Service.ReviewAsync(request, CancellationToken.None);
        var activation = await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        fixture.Clock.Now = LifecycleTestData.Now.AddDays(1);
        fixture.Policy.Fail = true;

        var replayedReview = await fixture.Service.ReviewAsync(request, CancellationToken.None);
        var replayedActivation = await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        Assert.Equal(LifecycleJson.Digest(review), LifecycleJson.Digest(replayedReview));
        Assert.Equal(LifecycleJson.Digest(activation), LifecycleJson.Digest(replayedActivation));
    }

    [Fact]
    public async Task RevokedOrCrossScopeActorsCannotReadReplayOrActivate()
    {
        var fixture = new Fixture();
        var request = new LifecycleReviewRequest("review", LifecycleTestData.Proposal());
        await fixture.Service.ReviewAsync(request, CancellationToken.None);
        fixture.Actors.Actor = LifecycleTestData.Actor with { CanActivate = false };
        var denied = await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None));
        Assert.Equal("actor-not-authorized", denied.Code);
        fixture.Actors.Actor = LifecycleTestData.Actor with { AppId = "other-app" };
        await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ReviewAsync(request, CancellationToken.None));
        Assert.Null(await fixture.Store.GetBaselineAsync(Address(), CancellationToken.None));
    }

    [Fact]
    public async Task DependencyFailureDoesNotConsumeAnActivationIdentity()
    {
        var fixture = new Fixture();
        await fixture.Service.ReviewAsync(new("review", LifecycleTestData.Proposal()), CancellationToken.None);
        fixture.Policy.Fail = true;
        await Assert.ThrowsAsync<IOException>(() =>
            fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None));
        Assert.Null(await fixture.Store.GetActivationAsync("activation", CancellationToken.None));
        fixture.Policy.Fail = false;
        Assert.Equal(LifecycleMutationStatus.Applied,
            (await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None)).Status);
    }

    [Fact]
    public async Task ExpiredApprovalCannotCreateNewAuthority()
    {
        var fixture = new Fixture();
        await fixture.Service.ReviewAsync(new("review", LifecycleTestData.Proposal()), CancellationToken.None);
        fixture.Clock.Now = LifecycleTestData.Now.AddHours(2);
        var result = await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        Assert.Equal(LifecycleMutationStatus.Rejected, result.Status);
        Assert.Contains("expired-proposal", result.Reasons);
    }

    [Fact]
    public async Task ChangedRequestOrActorCannotReuseAnOperationIdentity()
    {
        var fixture = new Fixture();
        var proposal = LifecycleTestData.Proposal();
        await fixture.Service.ReviewAsync(new("review", proposal), CancellationToken.None);
        await fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None);
        var changed = await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ReviewAsync(new("review", proposal with
            {
                Value = JsonSerializer.SerializeToElement(850)
            }), CancellationToken.None));
        Assert.Equal("review-conflict", changed.Code);

        var provenanceConflict = await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ReviewAsync(new("review", proposal with
            {
                Context = proposal.Context with
                {
                    Definition = proposal.Context.Definition with
                    {
                        Contract = proposal.Context.Definition.Contract with { BuildId = "another-build" }
                    }
                }
            }), CancellationToken.None));
        Assert.Equal("review-conflict", provenanceConflict.Code);

        fixture.Actors.Actor = LifecycleTestData.Actor with { Subject = "different-actor" };
        var actorConflict = await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None));
        Assert.Equal("activation-conflict", actorConflict.Code);

        fixture.Actors.Actor = LifecycleTestData.Actor with { CanReview = false, CanActivate = false };
        await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ReviewAsync(new("review", proposal), CancellationToken.None));
        await Assert.ThrowsAsync<LifecycleGovernanceException>(() =>
            fixture.Service.ActivateAsync(new("activation", "review"), CancellationToken.None));
    }

    [Theory]
    [InlineData("timeout")]
    [InlineData("caller")]
    [InlineData("shutdown")]
    public async Task BoundedWaitDoesNotReleaseANonCooperativeWritersLease(string interruption)
    {
        using var file = TestJsonFile.CreateCommitted("governance-bounded-commit");
        var fixture = new Fixture();
        var stable = new LocalFileGovernedStateLifecycleStore(new(file.Path), fixture.Clock);
        await fixture.CreateService(stable).ReviewAsync(
            new("review-1", LifecycleTestData.Proposal()), CancellationToken.None);
        using var caller = new CancellationTokenSource();
        using var shutdown = new CancellationTokenSource();
        var publisher = new DelayedPublisher();
        var delayed = new LocalFileGovernedStateLifecycleStore(
            new(file.Path), fixture.Clock, new GuidGovernedStateIdentityGenerator(), publisher);
        var service = fixture.CreateService(delayed, new(
            interruption == "timeout" ? TimeSpan.FromSeconds(1) : TimeSpan.FromSeconds(10),
            shutdown.Token));
        var pending = service.ActivateAsync(new("activation-1", "review-1"), caller.Token);
        try
        {
            await publisher.Started.Task.WaitAsync(TimeSpan.FromSeconds(5));
            if (interruption == "caller")
            {
                caller.Cancel();
            }
            if (interruption == "shutdown")
            {
                shutdown.Cancel();
            }
            if (interruption == "timeout")
            {
                var error = await Assert.ThrowsAsync<TimeoutException>(() =>
                    pending.WaitAsync(TimeSpan.FromSeconds(5)));
                Assert.Contains("lifecycle commit deadline", error.Message);
            }
            else
            {
                var error = await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
                    pending.WaitAsync(TimeSpan.FromSeconds(5)));
                Assert.Contains("outcome may be unknown", error.Message);
            }
            Assert.Throws<IOException>(() =>
            {
                using var lease = new FileStream(
                    $"{file.Path}.lock", FileMode.Open, FileAccess.ReadWrite, FileShare.None);
            });
            Assert.Null(await stable.GetActivationAsync("activation-1", CancellationToken.None));
        }
        finally
        {
            publisher.Release.TrySetResult();
            await publisher.Completed.Task.WaitAsync(TimeSpan.FromSeconds(5));
        }

        var replay = await LifecycleTestData.ActivateAsync(stable);
        Assert.Equal(LifecycleMutationStatus.Applied, replay.Status);
        var journal = await stable.ReadAsync("app", "dev", CancellationToken.None);
        Assert.Single(journal.Activations);
        Assert.Equal(4, journal.Records.Count);
        Assert.Equal(LifecycleJson.Digest(replay),
            LifecycleJson.Digest(await service.ActivateAsync(new("activation-1", "review-1"), CancellationToken.None)));
    }

    [Fact]
    public async Task LateCommitFailureIsObservedAndSameIdentityCanBeRetried()
    {
        using var file = TestJsonFile.CreateCommitted("governance-late-failure");
        var fixture = new Fixture();
        var stable = new LocalFileGovernedStateLifecycleStore(new(file.Path), fixture.Clock);
        await fixture.CreateService(stable).ReviewAsync(
            new("review-1", LifecycleTestData.Proposal()), CancellationToken.None);
        var publisher = new DelayedPublisher(fail: true);
        var logger = new LateFailureLogger();
        var delayed = new LocalFileGovernedStateLifecycleStore(
            new(file.Path), fixture.Clock, new GuidGovernedStateIdentityGenerator(), publisher);
        var service = fixture.CreateService(delayed, new(TimeSpan.FromSeconds(1)), logger);
        try
        {
            var error = await Assert.ThrowsAsync<TimeoutException>(() =>
                service.ActivateAsync(new("activation-1", "review-1"), CancellationToken.None)
                    .WaitAsync(TimeSpan.FromSeconds(5)));
            Assert.Contains("lifecycle commit deadline", error.Message);
        }
        finally
        {
            publisher.Release.TrySetResult();
            await publisher.Completed.Task.WaitAsync(TimeSpan.FromSeconds(5));
        }

        var failure = await logger.LateFailure.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.IsType<IOException>(failure.GetBaseException());
        Assert.Null(await stable.GetActivationAsync("activation-1", CancellationToken.None));
        Assert.Equal(LifecycleMutationStatus.Applied, (await LifecycleTestData.ActivateAsync(stable)).Status);
    }

    private static GovernedStateAddress Address() => new("app", "dev", "decision", LifecycleTestData.Target);

    private sealed class Fixture
    {
        public Fixture(bool strategy = false)
        {
            Store = new InMemoryGovernedStateLifecycleStore(Clock);
            Registry = new InMemoryDefinitionRegistry(
                [LifecycleTestData.Runtime()],
                intelligenceDefinitions: [LifecycleTestData.Intelligence(strategy)]);
            Service = CreateService();
        }

        public ProposalGovernance CreateService(
            IGovernedStateLifecycleStore? store = null,
            LifecycleCommitOptions? options = null,
            ILogger<ProposalGovernance>? logger = null,
            IProposalEvidenceReader? evidence = null,
            ILifecyclePolicyEvaluator? policy = null) =>
            new(
                Registry, Registry, Policy,
                evidence ?? new InMemoryProposalEvidenceReader([]),
                Actors, policy ?? new DefaultLifecyclePolicyEvaluator(), store ?? Store, Clock,
                logger ?? NullLogger<ProposalGovernance>.Instance, options);

        public MutableClock Clock { get; } = new();
        public MutablePolicy Policy { get; } = new();
        public TestActors Actors { get; } = new();
        public InMemoryGovernedStateLifecycleStore Store { get; }
        public InMemoryDefinitionRegistry Registry { get; }
        public ProposalGovernance Service { get; }
    }

    private sealed class MutableClock : TimeProvider
    {
        public DateTimeOffset Now { get; set; } = LifecycleTestData.Now;
        public override DateTimeOffset GetUtcNow() => Now;
    }

    private sealed class MutablePolicy : ILifecyclePolicyContextProvider
    {
        public LifecyclePolicyContext Context { get; set; } = LifecycleTestData.Policy;
        public bool Fail { get; set; }

        public Task<LifecyclePolicyContext?> GetAsync(
            GovernedDefinitionIdentity definition, CancellationToken cancellationToken) =>
            Fail ? throw new IOException("Injected policy dependency failure.")
                : Task.FromResult<LifecyclePolicyContext?>(LifecycleJson.Copy(Context));
    }

    private sealed class TestActors : ILifecycleActorProvider
    {
        public LifecycleActor Actor { get; set; } = LifecycleTestData.Actor;
        public Task<LifecycleActor> GetAsync(
            string appId, string environment, CancellationToken cancellationToken) =>
            Task.FromResult(Actor);
    }

    private sealed class MutableEvidence : IProposalEvidenceReader
    {
        public IReadOnlyList<ProposalEvidenceSnapshot> Snapshots { get; set; } = [];
        public Task<IReadOnlyList<ProposalEvidenceSnapshot>> GetAsync(
            ProposalEvidenceRequest request, CancellationToken cancellationToken) =>
            Task.FromResult<IReadOnlyList<ProposalEvidenceSnapshot>>(LifecycleJson.Copy(Snapshots.ToArray()));
    }

    private sealed class AfterEvaluationPolicy(Action afterEvaluation) : ILifecyclePolicyEvaluator
    {
        public async Task<LifecyclePolicyDecision> EvaluateAsync(
            LifecyclePolicyEvaluationRequest request, CancellationToken cancellationToken)
        {
            var result = await new DefaultLifecyclePolicyEvaluator().EvaluateAsync(request, cancellationToken);
            Assert.Equal(LifecycleDisposition.Approved, result.Disposition);
            afterEvaluation();
            return result;
        }
    }

    private sealed class DelayedPublisher(bool fail = false) : IGovernedStateDocumentPublisher
    {
        public TaskCompletionSource Started { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public TaskCompletionSource Completed { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        public async Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken)
        {
            Started.TrySetResult();
            await Release.Task;
            try
            {
                if (fail)
                {
                    throw new IOException("Injected late publication failure.");
                }
                await CommittedFileSnapshotWriter.PublishAsync(path, bytes, CancellationToken.None);
            }
            finally
            {
                Completed.TrySetResult();
            }
        }
    }

    private sealed class LateFailureLogger : ILogger<ProposalGovernance>
    {
        public TaskCompletionSource<Exception> LateFailure { get; } =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;
        public bool IsEnabled(LogLevel logLevel) => true;
        public void Log<TState>(
            LogLevel logLevel, EventId eventId, TState state, Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            if (exception is not null && formatter(state, exception).Contains("after its caller stopped waiting"))
            {
                LateFailure.TrySetResult(exception);
            }
        }
    }
}
