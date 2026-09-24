using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Audit;
using Flaggo.ControlPlane;
using Flaggo.DataPlane;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Flaggo.Decisioning.Tests;

public sealed class HostBoundaryTests
{
    [Fact]
    public async Task DataPlane_DecisionAndReadinessCompositionIsRequestScoped()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        using var firstScope = factory.Services.CreateScope();
        using var secondScope = factory.Services.CreateScope();

        var firstDecision = firstScope.ServiceProvider
            .GetRequiredService<DecisionService>();
        var firstReadiness = firstScope.ServiceProvider
            .GetRequiredService<IRuntimeReadinessProbe>();

        Assert.Same(
            firstDecision,
            firstScope.ServiceProvider.GetRequiredService<DecisionService>());
        Assert.Same(
            firstReadiness,
            firstScope.ServiceProvider
                .GetRequiredService<IRuntimeReadinessProbe>());
        Assert.NotSame(
            firstDecision,
            secondScope.ServiceProvider.GetRequiredService<DecisionService>());
        Assert.NotSame(
            firstReadiness,
            secondScope.ServiceProvider
                .GetRequiredService<IRuntimeReadinessProbe>());
    }

    [Fact]
    public async Task DataPlane_DoesNotHostManagementEndpoints()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        using var client = factory.CreateClient();

        using var response = await client.PostAsync(
            "/v1/definition-bundles:validate",
            new StringContent("{}", Encoding.UTF8, "application/json"));

        Assert.Equal(HttpStatusCode.NotFound, response.StatusCode);
    }

    [Fact]
    public async Task ControlPlane_DoesNotHostRuntimeEndpoints()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<ControlPlaneAssemblyMarker>(registryFile.Path);
        using var client = factory.CreateClient();

        using var response = await client.PostAsync(
            "/v1/decisions/tetris.dropInterval:decide",
            new StringContent("{}", Encoding.UTF8, "application/json"));

        Assert.Equal(HttpStatusCode.NotFound, response.StatusCode);
    }

    [Fact]
    public async Task ApprovedControlPlaneBundle_BecomesDecidableInSeparateDataPlaneHost()
    {
        using var registryFile = new TestRegistryFile();
        await using var dataFactory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        await using var controlFactory =
            new LocalHostFactory<ControlPlaneAssemblyMarker>(registryFile.Path);
        using var dataClient = dataFactory.CreateClient();
        using var controlClient = controlFactory.CreateClient();
        using var initialReadiness = await dataClient.GetAsync("/health/ready");
        Assert.Equal(HttpStatusCode.OK, initialReadiness.StatusCode);

        var fixturePath = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            "definition-bundle",
            "04-apply-approved-receipt.json");
        using var fixture = JsonDocument.Parse(File.ReadAllBytes(fixturePath));
        var bundle = JsonNode.Parse(
            fixture.RootElement
                .GetProperty("request")
                .GetProperty("body")
                .GetRawText())!.AsObject();
        bundle["decisions"]!.AsObject().First().Value!["result"]!["default"] = 850;
        using var applyRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/definition-bundles:apply")
        {
            Content = new StringContent(
                bundle.ToJsonString(),
                Encoding.UTF8,
                "application/json")
        };
        applyRequest.Headers.Add("Idempotency-Key", "cross-host-approval");

        using var applyResponse = await controlClient.SendAsync(applyRequest);
        Assert.Equal(HttpStatusCode.Accepted, applyResponse.StatusCode);
        var pending = await applyResponse.Content.ReadFromJsonAsync<JsonElement>();
        var approvalRequestId = pending.GetProperty("approvalRequestId").GetString()!;
        var bundleDigest = pending.GetProperty("bundleDigest").GetString()!;
        using var approvalResponse = await controlClient.PostAsJsonAsync(
            $"/v1/definition-bundle-approvals/{approvalRequestId}:approve",
            new
            {
                expectedBundleDigest = bundleDigest,
                comment = "Cross-host visibility test."
            });
        Assert.Equal(HttpStatusCode.OK, approvalResponse.StatusCode);
        var approval = await approvalResponse.Content.ReadFromJsonAsync<JsonElement>();
        var accepted = approval.GetProperty("receipt")
            .GetProperty("acceptedDefinitions")
            .GetProperty("tetris.dropInterval");
        var decideRequest = new
        {
            expectedContract = new
            {
                definitionId = accepted.GetProperty("definitionId").GetString(),
                contractDigest = accepted.GetProperty("contractDigest").GetString(),
                revision = accepted.GetProperty("revision").GetString(),
                bundleDigest
            },
            runtimeContext = new { },
            inputs = new { boardPressure = 0.8, currentLevel = 3, recentPlacementTimeMs = 1200, recoveryFailures = 2 },
            client = new
            {
                appId = "tetris-demo",
                environment = "dev"
            }
        };

        using var decideResponse = await dataClient.PostAsJsonAsync(
            "/v1/decisions/tetris.dropInterval:decide",
            decideRequest);

        Assert.Equal(HttpStatusCode.OK, decideResponse.StatusCode);
        var decision = await decideResponse.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(850, decision.GetProperty("value").GetInt32());
        Assert.Equal(
            accepted.GetProperty("revision").GetString(),
            decision.GetProperty("definition").GetProperty("revision").GetString());
        Assert.Equal(
            accepted.GetProperty("contractDigest").GetString(),
            decision.GetProperty("definitionStatus")
                .GetProperty("contractDigest")
                .GetString());
    }

    [Fact]
    public async Task ExposureAudit_IsCreatedOnlyForConfirmedReceipt()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        using var client = factory.CreateClient();
        var exposures = factory.Services.GetRequiredService<IExposureStore>();
        var audit = factory.Services.GetRequiredService<InMemoryAuditSink>();
        await exposures.CreatePendingAsync(
            "decision-confirmed",
            "confirm-confirmed",
            Snapshot(),
            CancellationToken.None);
        await exposures.CreatePendingAsync(
            "decision-unused",
            "confirm-unused",
            Snapshot(),
            CancellationToken.None);

        using var response = await client.PostAsJsonAsync(
            "/v1/exposures/decision-confirmed:confirm",
            new
            {
                confirmToken = "confirm-confirmed",
                appliedAt = DateTimeOffset.UtcNow.UtcDateTime.ToString("O")
            });

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var record = Assert.Single(audit.ExposureRecords);
        Assert.Equal("decision-confirmed", record.DecisionId);
        Assert.DoesNotContain(
            audit.ExposureRecords,
            item => item.DecisionId == "decision-unused");
    }

    [Fact]
    public async Task ConflictingExposureAuditIdentity_ReturnsFrozenConflict()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<IExposureAuditSink>();
                    services.AddSingleton<IExposureAuditSink>(
                        new ThrowingExposureAuditSink(
                            new ExposureAuditConflictException("exposure-1")));
                });
        using var client = factory.CreateClient();
        var exposures = factory.Services.GetRequiredService<IExposureStore>();
        await exposures.CreatePendingAsync(
            "decision-conflict",
            "confirm-conflict",
            Snapshot(),
            CancellationToken.None);

        using var response = await client.PostAsJsonAsync(
            "/v1/exposures/decision-conflict:confirm",
            new
            {
                confirmToken = "confirm-conflict",
                appliedAt = DateTimeOffset.UtcNow.UtcDateTime.ToString("O")
            });
        var problem = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.Conflict, response.StatusCode);
        Assert.Equal(
            "exposure-confirmation-conflict",
            problem.GetProperty("code").GetString());
        Assert.Null(
            Assert.IsType<InMemoryExposureStore>(exposures)
                .Find("decision-conflict")!
                .Confirmation);
    }

    [Fact]
    public async Task ExposureConfirmation_RetriesPreparedObservationAfterClockWindow()
    {
        using var registryFile = new TestRegistryFile();
        var time = new MutableTimeProvider(
            new DateTimeOffset(2026, 8, 6, 0, 0, 0, TimeSpan.Zero));
        var audit = new FailOnceExposureAuditSink();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<TimeProvider>();
                    services.AddSingleton<TimeProvider>(time);
                    services.RemoveAll<IExposureAuditSink>();
                    services.AddSingleton<IExposureAuditSink>(audit);
                });
        using var client = factory.CreateClient();
        var exposures = factory.Services.GetRequiredService<IExposureStore>();
        await exposures.CreatePendingAsync(
            "decision-retry",
            "confirm-retry",
            Snapshot(),
            CancellationToken.None);
        var request = new
        {
            confirmToken = "confirm-retry",
            appliedAt = "2026-08-06T00:00:00Z"
        };

        using var failed = await client.PostAsJsonAsync(
            "/v1/exposures/decision-retry:confirm",
            request);
        Assert.Equal(HttpStatusCode.ServiceUnavailable, failed.StatusCode);

        time.Advance(TimeSpan.FromMinutes(6));
        using var conflict = await client.PostAsJsonAsync(
            "/v1/exposures/decision-retry:confirm",
            new
            {
                confirmToken = "confirm-retry",
                appliedAt = "2026-08-06T00:00:01Z"
            });
        Assert.Equal(HttpStatusCode.Conflict, conflict.StatusCode);

        using var retried = await client.PostAsJsonAsync(
            "/v1/exposures/decision-retry:confirm",
            request);
        Assert.Equal(HttpStatusCode.OK, retried.StatusCode);
        Assert.Single(audit.Records);
    }

    [Theory]
    [InlineData("allow", true)]
    [InlineData("forbid", false)]
    public async Task MalformedLocalEvidence_UsesRequiredEvidenceFallbackPolicy(
        string requiredEvidenceUnavailable,
        bool clientFallbackEligible)
    {
        using var registryFile = new TestRegistryFile();
        using var evidenceFile =
            new CommittedTestJsonFile("evidence-omitted-quality");
        await evidenceFile.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy-test": { "sampleSize": 20 }
              }
            }
            """);
        var definition = RequiredEvidenceDefinition(requiredEvidenceUnavailable);
        var state = new GovernedDecisionState(
            definition.Identity.DefinitionId,
            definition.Identity.Revision,
            definition.Identity.ContractDigest,
            JsonSerializer.SerializeToElement(800),
            new DecisionTargetRef("cohort", "new_players"),
            "strategy",
            "strategy-test",
            new NumericRuleStrategy("boardPressure", 0.5, 850, 750));
        var registry = new InMemoryDefinitionRegistry([definition]);
        var stateStore = new InMemoryStateStore(
            [("tetris.dropInterval", state)]);
        var evidence = new LocalFileEvidenceProvider(
            new LocalFileEvidenceProviderOptions(evidenceFile.Path));
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<IRuntimeDefinitionReader>();
                    services.AddSingleton<IRuntimeDefinitionReader>(registry);
                    services.RemoveAll<IStateStore>();
                    services.AddSingleton<IStateStore>(stateStore);
                    services.RemoveAll<IEvidenceProvider>();
                    services.AddSingleton<IEvidenceProvider>(evidence);
                });
        using var client = factory.CreateClient();

        using var response = await client.PostAsJsonAsync(
            "/v1/decisions/tetris.dropInterval:decide",
            new
            {
                expectedContract = new
                {
                    definition.Identity.DefinitionId,
                    definition.Identity.ContractDigest,
                    definition.Identity.Revision
                },
                runtimeContext = new { },
                runtimeTarget = new { type = "cohort", id = "new_players" },
                inputs = new { boardPressure = 0.8 },
                client = new { appId = "tetris-demo", environment = "dev" }
            });

        Assert.Equal(HttpStatusCode.ServiceUnavailable, response.StatusCode);
        var problem = await response.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(
            "required-evidence-unavailable",
            problem.GetProperty("code").GetString());
        Assert.Equal(
            clientFallbackEligible,
            problem.GetProperty("clientFallback").GetProperty("eligible").GetBoolean());
    }

    [Fact]
    public async Task RequiredEvidenceUnavailable_ReleasesIdempotencyClaimForRecovery()
    {
        using var registryFile = new TestRegistryFile();
        using var evidenceFile =
            new CommittedTestJsonFile("evidence-idempotency-recovery");
        await evidenceFile.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {}
            }
            """);
        var definition = RequiredEvidenceDefinition("forbid");
        var state = new GovernedDecisionState(
            definition.Identity.DefinitionId,
            definition.Identity.Revision,
            definition.Identity.ContractDigest,
            JsonSerializer.SerializeToElement(800),
            new DecisionTargetRef("cohort", "new_players"),
            "strategy",
            "strategy-test",
            new NumericRuleStrategy("boardPressure", 0.5, 850, 750));
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<IRuntimeDefinitionReader>();
                    services.AddSingleton<IRuntimeDefinitionReader>(
                        new InMemoryDefinitionRegistry([definition]));
                    services.RemoveAll<IStateStore>();
                    services.AddSingleton<IStateStore>(
                        new InMemoryStateStore([("tetris.dropInterval", state)]));
                    services.RemoveAll<IEvidenceProvider>();
                    services.AddSingleton<IEvidenceProvider>(
                        new LocalFileEvidenceProvider(
                            new LocalFileEvidenceProviderOptions(evidenceFile.Path)));
                });
        using var client = factory.CreateClient();
        var requestBody = JsonSerializer.Serialize(
            new
            {
                expectedContract = new
                {
                    definition.Identity.DefinitionId,
                    definition.Identity.ContractDigest,
                    definition.Identity.Revision
                },
                runtimeContext = new { },
                runtimeTarget = new { type = "cohort", id = "new_players" },
                inputs = new { boardPressure = 0.8 },
                client = new { appId = "tetris-demo", environment = "dev" }
            },
            RuntimeHttp.JsonOptions);

        using var unavailableRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/decisions/tetris.dropInterval:decide")
        {
            Content = new StringContent(requestBody, Encoding.UTF8, "application/json")
        };
        unavailableRequest.Headers.Add("Idempotency-Key", "evidence-recovery");
        using var unavailable = await client.SendAsync(unavailableRequest);

        Assert.Equal(HttpStatusCode.ServiceUnavailable, unavailable.StatusCode);
        Assert.False(unavailable.Headers.Contains("Idempotency-Key-Expires-At"));
        var problem = await unavailable.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(
            "required-evidence-unavailable",
            problem.GetProperty("code").GetString());

        await evidenceFile.WriteAsync(
            """
            {
              "version": 1,
              "evidenceByStrategy": {
                "strategy-test": {
                  "evidenceQuality": 0.9,
                  "modelUncertainty": 0.1,
                  "sampleSize": 100
                }
              }
            }
            """);
        using var recoveredRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/decisions/tetris.dropInterval:decide")
        {
            Content = new StringContent(requestBody, Encoding.UTF8, "application/json")
        };
        recoveredRequest.Headers.Add("Idempotency-Key", "evidence-recovery");
        using var recovered = await client.SendAsync(recoveredRequest);

        Assert.Equal(HttpStatusCode.OK, recovered.StatusCode);
        Assert.True(recovered.Headers.Contains("Idempotency-Key-Expires-At"));
    }

    [Theory]
    [InlineData("timeout", true, "service-unavailable")]
    [InlineData("io", false, "unclassified-io-failure")]
    public async Task DataPlane_DecideAvailabilityFailurePreservesFallbackRules(
        string failureKind,
        bool clientFallbackEligible,
        string fallbackReason)
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<IAuditSink>();
                    services.AddSingleton<IAuditSink>(
                        new ThrowingAuditSink(
                            AvailabilityException(failureKind)));
                });
        using var client = factory.CreateClient();
        var identity = LocalRegistryHosting.DefaultDefinitions()
            .Single(definition => definition.LifecycleStatus == "active")
            .Identity;
        var correlationId = $"decide-{failureKind}-correlation";

        using var request = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/decisions/tetris.dropInterval:decide")
        {
            Content = JsonContent.Create(
                new
                {
                    expectedContract = new
                    {
                        identity.DefinitionId,
                        identity.ContractDigest,
                        identity.Revision
                    },
                    runtimeContext = new { },
                    runtimeTarget = new { type = "cohort", id = "new_players" },
                    inputs = new { boardPressure = 0.8, currentLevel = 3, recentPlacementTimeMs = 1200, recoveryFailures = 2 },
                    client = new { appId = "tetris-demo", environment = "dev" }
                })
        };
        request.Headers.Add("X-Flaggo-Correlation-Id", correlationId);

        using var response = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.ServiceUnavailable, response.StatusCode);
        Assert.Equal(
            correlationId,
            Assert.Single(response.Headers.GetValues("X-Flaggo-Correlation-Id")));
        Assert.Equal(
            "1",
            Assert.Single(response.Headers.GetValues("Retry-After")));
        var problem = await response.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(
            correlationId,
            problem.GetProperty("correlationId").GetString());
        Assert.Equal(1, problem.GetProperty("retryAfterSeconds").GetInt32());
        Assert.Equal(
            clientFallbackEligible,
            problem.GetProperty("clientFallback").GetProperty("eligible").GetBoolean());
        Assert.Equal(
            fallbackReason,
            problem.GetProperty("clientFallback").GetProperty("reason").GetString());
    }

    [Theory]
    [InlineData("timeout", "service-unavailable")]
    [InlineData("io", "unclassified-io-failure")]
    public async Task DataPlane_ExposureAvailabilityFailureNeverAllowsFallback(
        string failureKind,
        string fallbackReason)
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(
                registryFile.Path,
                services =>
                {
                    services.RemoveAll<IExposureAuditSink>();
                    services.AddSingleton<IExposureAuditSink>(
                        new ThrowingExposureAuditSink(
                            AvailabilityException(failureKind)));
                });
        using var client = factory.CreateClient();
        var exposures = factory.Services.GetRequiredService<IExposureStore>();
        await exposures.CreatePendingAsync(
            "decision-operation-aware",
            "confirm-operation-aware",
            Snapshot(),
            CancellationToken.None);
        var correlationId = $"exposure-{failureKind}-correlation";
        using var request = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/exposures/decision-operation-aware:confirm")
        {
            Content = JsonContent.Create(
                new
                {
                    confirmToken = "confirm-operation-aware"
                })
        };
        request.Headers.Add("X-Flaggo-Correlation-Id", correlationId);

        using var response = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.ServiceUnavailable, response.StatusCode);
        Assert.Equal(
            correlationId,
            Assert.Single(response.Headers.GetValues("X-Flaggo-Correlation-Id")));
        Assert.Equal(
            "1",
            Assert.Single(response.Headers.GetValues("Retry-After")));
        var problem = await response.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(
            correlationId,
            problem.GetProperty("correlationId").GetString());
        Assert.Equal(1, problem.GetProperty("retryAfterSeconds").GetInt32());
        Assert.False(
            problem.GetProperty("clientFallback").GetProperty("eligible").GetBoolean());
        Assert.Equal(
            fallbackReason,
            problem.GetProperty("clientFallback").GetProperty("reason").GetString());
        Assert.Null(
            Assert.IsType<InMemoryExposureStore>(exposures)
                .Find("decision-operation-aware")!
                .Confirmation);
    }

    private static Exception AvailabilityException(string failureKind) =>
        failureKind switch
        {
            "timeout" => new TimeoutException("dependency timed out"),
            "io" => new IOException("dependency unavailable"),
            _ => throw new ArgumentOutOfRangeException(nameof(failureKind))
        };

    private static DecisionSnapshot Snapshot() => new(
        "local-development",
        "tetris-demo",
        "dev",
        new RuntimeContractIdentity(
            "definition",
            $"sha256:{new string('a', 64)}",
            "revision"),
        JsonSerializer.SerializeToElement(800),
        "number",
        new ServerFallbackInfo("server", false, false, null),
        new Dictionary<string, JsonElement>(),
        new Dictionary<string, JsonElement>(),
        new DecisionTargetRef("session", "game-1"),
        new DecisionTargetRef("cohort", "new_players"),
        [],
        ["session:game-1", "cohort:new_players", "global"],
        new PolicyEvaluationResult("approved", [], []));

    private static RuntimeDecisionDefinition RequiredEvidenceDefinition(
        string requiredEvidenceUnavailable) =>
        new(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            new RuntimeContractIdentity(
                "definition-evidence",
                $"sha256:{new string('b', 64)}",
                "revision-evidence"),
            "number",
            JsonSerializer.SerializeToElement(800),
            "safe-default",
            [new RegisteredInput("boardPressure", "number", "request", "Current occupancy.", 0, 1)],
            [],
            NumberActionSpace: new NumberActionSpaceContract(700, 900),
            Policy: new DecisionPolicyContract(
                MinimumEvidenceQuality: 0.7,
                RequiredEvidenceUnavailable: requiredEvidenceUnavailable),
            TargetHierarchy: ["cohort", "global"],
            InferenceTarget: "cohort",
            FallbackOrder: ["global"]);

    private sealed class LocalHostFactory<TEntryPoint>(
        string registryPath,
        Action<IServiceCollection>? configureServices = null)
        : WebApplicationFactory<TEntryPoint>
        where TEntryPoint : class
    {
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting(
                "Flaggo:Authentication:LocalDevelopmentBypass",
                "true");
            builder.UseSetting(
                "Flaggo:Registry:LocalFilePath",
                registryPath);
            builder.UseSetting("Flaggo:Telemetry:CommitDescriptorPath",
                Path.Combine(Path.GetDirectoryName(registryPath)!, "telemetry", "inputs.commit.json"));
            if (configureServices is not null)
            {
                builder.ConfigureServices(configureServices);
            }
        }
    }

    private sealed class ThrowingAuditSink(Exception error) : IAuditSink
    {
        public Task RecordDecisionAsync(
            DecisionAuditRecord record,
            CancellationToken cancellationToken) =>
            Task.FromException(error);
    }

    private sealed class ThrowingExposureAuditSink(Exception error) :
        IExposureAuditSink
    {
        public Task RecordExposureAsync(
            ExposureAuditRecord record,
            CancellationToken cancellationToken) =>
            Task.FromException(error);
    }

    private sealed class FailOnceExposureAuditSink : IExposureAuditSink
    {
        private bool _fail = true;

        public List<ExposureAuditRecord> Records { get; } = [];

        public Task RecordExposureAsync(
            ExposureAuditRecord record,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (_fail)
            {
                _fail = false;
                throw new IOException("audit unavailable");
            }

            Records.Add(record);
            return Task.CompletedTask;
        }
    }

    private sealed class MutableTimeProvider(DateTimeOffset utcNow) : TimeProvider
    {
        private DateTimeOffset _utcNow = utcNow;

        public override DateTimeOffset GetUtcNow() => _utcNow;

        public void Advance(TimeSpan duration) => _utcNow += duration;
    }
}
