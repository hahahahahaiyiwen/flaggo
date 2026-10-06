using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Collections.Concurrent;
using System.Diagnostics;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Flaggo.ContractService.Tests;

public sealed class ContractServiceEndpointTests
{
    [Fact]
    public async Task ExecutesManagementContractFixtures()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var historical = await ReadFixtureDocumentAsync("07-get-version.json");
        var historicalContract = historical.RootElement
            .GetProperty("expected")
            .GetProperty("body")
            .GetProperty("contract")
            .GetRawText();
        factory.Clock.SetUtcNow(
            new DateTimeOffset(2026, 7, 15, 10, 0, 0, TimeSpan.Zero));
        using (var seedRequest = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/tetris.dropInterval",
            historicalContract))
        using (var seedResponse = await client.SendAsync(seedRequest))
        {
            Assert.Equal(HttpStatusCode.Created, seedResponse.StatusCode);
        }

        factory.Clock.SetUtcNow(ContractServiceFactory.Now);
        foreach (var fileName in new[]
                 {
                     "01-validate-valid.json",
                     "02-validate-invalid.json",
                     "04-put-invalid.json",
                     "03-put-ready.json",
                     "05-get-current.json",
                     "06-list-versions.json",
                     "07-get-version.json"
                 })
        {
            using var fixture = await ReadFixtureDocumentAsync(fileName);
            var requestModel = fixture.RootElement.GetProperty("request");
            var expected = fixture.RootElement.GetProperty("expected");
            var method = new HttpMethod(requestModel.GetProperty("method").GetString()!);
            var body = requestModel.TryGetProperty("body", out var requestBody)
                ? requestBody.GetRawText()
                : null;
            using var request = JsonRequest(
                method,
                requestModel.GetProperty("path").GetString()!,
                body);
            var correlationId = expected
                .GetProperty("headers")
                .GetProperty(CorrelationIds.HeaderName)
                .GetString()!;
            request.Headers.Add(CorrelationIds.HeaderName, correlationId);

            using var response = await client.SendAsync(request);
            using var actualBody = JsonDocument.Parse(
                await response.Content.ReadAsStringAsync());

            Assert.Equal(
                expected.GetProperty("status").GetInt32(),
                (int)response.StatusCode);
            Assert.True(
                JsonElement.DeepEquals(
                    expected.GetProperty("body"),
                    actualBody.RootElement),
                $"{fileName}:{Environment.NewLine}expected: "
                + $"{expected.GetProperty("body").GetRawText()}"
                + $"{Environment.NewLine}actual: {actualBody.RootElement.GetRawText()}");
            Assert.Equal(
                correlationId,
                Assert.Single(response.Headers.GetValues(CorrelationIds.HeaderName)));
        }
    }

    [Fact]
    public async Task ValidationComputesFixtureDigestWithoutPersistence()
    {
        var activities = new ConcurrentBag<Activity>();
        using var activityListener = Listen(
            "flaggo.contract-service",
            activities);
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("01-validate-valid.json");
        using var request = JsonRequest(
            HttpMethod.Post,
            fixture.Path,
            fixture.Body);
        request.Headers.Add(CorrelationIds.HeaderName, "client-validation");

        using var response = await client.SendAsync(request);
        var result = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal("valid", result.GetProperty("status").GetString());
        Assert.Equal(
            "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098",
            result.GetProperty("contractDigest").GetString());
        Assert.Empty(result.GetProperty("issues").EnumerateArray());
        Assert.Equal(
            "client-validation",
            Assert.Single(response.Headers.GetValues(CorrelationIds.HeaderName)));
        var activity = Assert.Single(
            activities,
            candidate => candidate.DisplayName == "flaggo.contract.validate");
        Assert.Equal(
            "tetris.dropInterval",
            activity.GetTagItem("flaggo.contract.name"));
        Assert.Equal(
            result.GetProperty("contractDigest").GetString(),
            activity.GetTagItem("flaggo.contract.digest"));
        Assert.Equal("success", activity.GetTagItem("flaggo.operation.outcome"));
        Assert.Equal(
            "client-validation",
            activity.GetTagItem("flaggo.request.correlation_id"));

        using var get = JsonRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/tetris.dropInterval");
        using var getResponse = await client.SendAsync(get);
        Assert.Equal(HttpStatusCode.NotFound, getResponse.StatusCode);
    }

    private static ActivityListener Listen(
        string sourceName,
        ConcurrentBag<Activity> activities)
    {
        var listener = new ActivityListener
        {
            ShouldListenTo = source => source.Name == sourceName,
            Sample = static (ref ActivityCreationOptions<ActivityContext> _) =>
                ActivitySamplingResult.AllDataAndRecorded,
            ActivityStopped = activities.Add
        };
        ActivitySource.AddActivityListener(listener);
        return listener;
    }

    [Fact]
    public async Task PublicationIsReadyAndIdempotentBySemanticDigest()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("03-put-ready.json");

        using var firstRequest = JsonRequest(
            HttpMethod.Put,
            fixture.Path,
            fixture.Body);
        using var first = await client.SendAsync(firstRequest);
        var created = await first.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.Created, first.StatusCode);
        Assert.NotNull(created);
        Assert.Equal(
            "sha256:aa59121ed2dfcf519654df1d3f9c60fa141aa4ddb199c207bd9d12b3a515714a",
            created.ContractDigest);
        Assert.Equal(
            "sha256:cb0a6cfab31c66e0c34b7d321e3c94dc4004a793a2ad2642a72d8001d12380b4",
            created.ActiveExecutableDigest);
        Assert.Equal(ContractServiceFactory.Now, created.AcceptedAt);
        Assert.Equal(
            $"/v3/decision-contracts/tetris.dropInterval/versions/{created.ContractDigest}",
            first.Headers.Location?.OriginalString);

        using var retryRequest = JsonRequest(
            HttpMethod.Put,
            fixture.Path,
            fixture.Body);
        using var retry = await client.SendAsync(retryRequest);
        var existing = await retry.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, retry.StatusCode);
        Assert.NotNull(existing);
        Assert.Equal(created.ContractDigest, existing.ContractDigest);
        Assert.Equal(created.ActiveExecutableDigest, existing.ActiveExecutableDigest);
        Assert.Equal(created.AcceptedAt, existing.AcceptedAt);
        Assert.Equal(
            StrictJson.SerializeToUtf8Bytes(created.Contract),
            StrictJson.SerializeToUtf8Bytes(existing.Contract));
        var active = await factory.Services
            .GetRequiredService<IExecutableStore>()
            .GetActiveAsync(created.ContractDigest);
        Assert.Equal(created.ActiveExecutableDigest, active?.ExecutableDigest);
        Assert.Empty(active!.Executable.Rules);
    }

    [Fact]
    public async Task ContractNameAuthorityConflictMatchesFixture()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var seed = await ReadFixtureAsync("03-put-ready.json");
        using (var seedRequest = JsonRequest(HttpMethod.Put, seed.Path, seed.Body))
        using (var seedResponse = await client.SendAsync(seedRequest))
        {
            Assert.Equal(HttpStatusCode.Created, seedResponse.StatusCode);
        }

        using var fixture = await ReadFixtureDocumentAsync(
            "contract-01-name-authority-conflict.json",
            "errors");
        var requestModel = fixture.RootElement.GetProperty("request");
        var expected = fixture.RootElement.GetProperty("expected");
        using var request = JsonRequest(
            HttpMethod.Put,
            requestModel.GetProperty("path").GetString()!,
            requestModel.GetProperty("body").GetRawText());
        var correlationId = requestModel
            .GetProperty("headers")
            .GetProperty(CorrelationIds.HeaderName)
            .GetString()!;
        request.Headers.Add(CorrelationIds.HeaderName, correlationId);

        using var response = await client.SendAsync(request);
        using var actualBody = JsonDocument.Parse(
            await response.Content.ReadAsStringAsync());

        Assert.Equal(HttpStatusCode.Conflict, response.StatusCode);
        Assert.True(JsonElement.DeepEquals(
            expected.GetProperty("body"),
            actualBody.RootElement));
        Assert.Equal(
            correlationId,
            Assert.Single(response.Headers.GetValues(CorrelationIds.HeaderName)));
    }

    [Fact]
    public async Task CurrentCatalogReturnsAllAuthoritiesAndSupportsConditionalPolling()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        using var fixture = await ReadFixtureDocumentAsync("01-validate-valid.json");
        var learningContract = fixture.RootElement
            .GetProperty("request")
            .GetProperty("body")
            .GetRawText();
        using (var learningRequest = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/tetris.dropInterval",
            learningContract))
        using (var learningResponse = await client.SendAsync(learningRequest))
        {
            learningResponse.EnsureSuccessStatusCode();
        }
        var otherScope = new AuthorityScope("acme", "worker", "production");
        await PutDefaultAsync(client, 3, otherScope);

        using var request = JsonRequest(
            HttpMethod.Get,
            "/v3/decision-contract-catalog/current");
        using var response = await client.SendAsync(request);
        var catalog = await response.Content.ReadFromJsonAsync<CurrentContractCatalog>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.NotNull(catalog);
        Assert.Collection(
            catalog.Contracts,
            entry => Assert.Equal(otherScope, entry.Contract.Authority),
            entry => Assert.Equal(ContractServiceFactory.Scope, entry.Contract.Authority));
        var etag = response.Headers.ETag?.Tag;
        Assert.StartsWith("\"catalog:", etag, StringComparison.Ordinal);

        using var conditional = JsonRequest(
            HttpMethod.Get,
            "/v3/decision-contract-catalog/current");
        conditional.Headers.TryAddWithoutValidation("If-None-Match", etag);
        using var unchanged = await client.SendAsync(conditional);

        Assert.Equal(HttpStatusCode.NotModified, unchanged.StatusCode);
        Assert.Equal(0, unchanged.Content.Headers.ContentLength);
        Assert.Equal(
            etag,
            unchanged.Headers.ETag?.Tag);
    }

    [Fact]
    public async Task AuthoredExpressionsReplaceDefaultAuthority()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var body =
            """
            {
              "authority": {
                "tenant": "local",
                "application": "tetris",
                "environment": "integration"
              },
              "name": "worker.batchSize",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                {
                  "name": "pressure",
                  "schema": { "type": "number", "minimum": 0, "maximum": 1 }
                }
              ],
              "result": {
                "schema": { "type": "integer", "minimum": 1, "maximum": 10 },
                "default": 3
              },
              "authoredExecutable": {
                "rules": [
                  {
                    "name": "high-pressure",
                    "when": { "expression": "attributes.pressure >= 0.7" },
                    "return": { "value": 6 }
                  }
                ]
              }
            }
            """;
        using var request = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/worker.batchSize",
            body);

        using var response = await client.SendAsync(request);
        var version = await response.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.Created, response.StatusCode);
        var active = await factory.Services
            .GetRequiredService<IExecutableStore>()
            .GetActiveAsync(version!.ContractDigest);
        Assert.Equal(version.ActiveExecutableDigest, active?.ExecutableDigest);
        Assert.Equal("high-pressure", Assert.Single(active!.Executable.Rules).Name);
    }

    [Fact]
    public async Task NewVersionMovesCurrentAndPreservesOldRuntimeAuthority()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var first = await PutDefaultAsync(client, 3);
        var second = await PutDefaultAsync(client, 4);

        Assert.NotEqual(first.ContractDigest, second.ContractDigest);
        using var currentRequest = JsonRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/worker.batchSize");
        using var currentResponse = await client.SendAsync(currentRequest);
        var current = await currentResponse.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);
        Assert.Equal(second.ContractDigest, current?.ContractDigest);

        using var oldRequest = JsonRequest(
            HttpMethod.Get,
            $"/v3/decision-contracts/worker.batchSize/versions/{first.ContractDigest}");
        using var oldResponse = await client.SendAsync(oldRequest);
        Assert.Equal(HttpStatusCode.OK, oldResponse.StatusCode);
        Assert.NotNull(await factory.Services
            .GetRequiredService<IExecutableStore>()
            .GetActiveAsync(first.ContractDigest));
    }

    [Fact]
    public async Task ListsStablePaginatedHistory()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        await PutDefaultAsync(client, 2);
        await PutDefaultAsync(client, 3);
        await PutDefaultAsync(client, 4);

        using var firstRequest = JsonRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/worker.batchSize/versions?limit=2");
        using var firstResponse = await client.SendAsync(firstRequest);
        var first = await firstResponse.Content
            .ReadFromJsonAsync<DecisionContractVersionList>(StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, firstResponse.StatusCode);
        Assert.Equal(2, first!.Versions.Count);
        Assert.NotNull(first.NextCursor);

        using var secondRequest = JsonRequest(
            HttpMethod.Get,
            $"/v3/decision-contracts/worker.batchSize/versions?limit=2"
            + $"&cursor={Uri.EscapeDataString(first.NextCursor)}");
        using var secondResponse = await client.SendAsync(secondRequest);
        var second = await secondResponse.Content
            .ReadFromJsonAsync<DecisionContractVersionList>(StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, secondResponse.StatusCode);
        Assert.Single(second!.Versions);
        Assert.Null(second.NextCursor);
        Assert.Empty(
            first.Versions.Select(version => version.ContractDigest)
                .Intersect(second.Versions.Select(version => version.ContractDigest)));
    }

    [Fact]
    public async Task ConcurrentIdenticalPublicationsConverge()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("03-put-ready.json");
        var responses = await Task.WhenAll(
            Enumerable.Range(0, 8).Select(async _ =>
            {
                using var request = JsonRequest(
                    HttpMethod.Put,
                    fixture.Path,
                    fixture.Body);
                return await client.SendAsync(request);
            }));

        try
        {
            Assert.Single(
                responses,
                response => response.StatusCode == HttpStatusCode.Created);
            Assert.Equal(
                7,
                responses.Count(response => response.StatusCode == HttpStatusCode.OK));
            var digests = await Task.WhenAll(responses.Select(async response =>
                (await response.Content.ReadFromJsonAsync<DecisionContractVersion>(
                    StrictJson.Options))!.ContractDigest));
            Assert.Single(digests.Distinct(StringComparer.Ordinal));
        }
        finally
        {
            foreach (var response in responses)
            {
                response.Dispose();
            }
        }
    }

    [Fact]
    public async Task AnalysisCandidateSubmissionPersistsInactiveAndRetriesIdempotently()
    {
        var activities = new ConcurrentBag<Activity>();
        using var activityListener = Listen(
            "flaggo.contract-service",
            activities);
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var version = await PutDefaultAsync(client, 3);
        var activeDigest = version.ActiveExecutableDigest;
        var submission = CreateCandidateSubmission(
            cycleId: "cycle-1",
            returnValue: 5);
        var path =
            $"/v3/decision-contracts/worker.batchSize/versions/{version.ContractDigest}"
            + "/candidates";

        using var firstRequest = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(submission, StrictJson.Options));
        using var firstResponse = await client.SendAsync(firstRequest);
        var created = await firstResponse.Content
            .ReadFromJsonAsync<AnalysisCandidateResult>(StrictJson.Options);

        Assert.Equal(HttpStatusCode.Created, firstResponse.StatusCode);
        Assert.NotNull(created);
        Assert.True(created.Created);
        Assert.Equal("candidate", created.LifecycleState);
        Assert.Equal(version.ContractDigest, created.ContractDigest);
        Assert.Equal(ContractServiceFactory.Now, created.CreatedAt);
        var store = factory.Services.GetRequiredService<IExecutableStore>();
        var persisted = await store.GetAsync(created.ExecutableDigest);
        Assert.Equal(ExecutableLifecycleState.Candidate, persisted?.State);
        Assert.Equal(
            activeDigest,
            (await store.GetActiveAsync(version.ContractDigest))?.ExecutableDigest);
        var submissionActivity = Assert.Single(
            activities,
            candidate =>
                candidate.DisplayName == "flaggo.candidate.submit"
                && Equals(
                    candidate.GetTagItem("flaggo.candidate.digest"),
                    created.ExecutableDigest));
        Assert.Equal(
            version.ContractDigest,
            submissionActivity.GetTagItem("flaggo.contract.digest"));
        Assert.Equal(
            created.ExecutableDigest,
            submissionActivity.GetTagItem("flaggo.candidate.digest"));
        Assert.Equal(
            "success",
            submissionActivity.GetTagItem("flaggo.operation.outcome"));
        Assert.Equal(
            submission.Provenance.CycleId,
            persisted?.Provenance?.GetProperty("cycleId").GetString());

        factory.Clock.SetUtcNow(ContractServiceFactory.Now.AddMinutes(5));
        using var retryRequest = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(submission, StrictJson.Options));
        using var retryResponse = await client.SendAsync(retryRequest);
        var retry = await retryResponse.Content
            .ReadFromJsonAsync<AnalysisCandidateResult>(StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, retryResponse.StatusCode);
        Assert.NotNull(retry);
        Assert.False(retry.Created);
        Assert.Equal(created.ExecutableDigest, retry.ExecutableDigest);
        Assert.Equal(created.CreatedAt, retry.CreatedAt);
        Assert.Equal(
            activeDigest,
            (await store.GetActiveAsync(version.ContractDigest))?.ExecutableDigest);
    }

    [Fact]
    public async Task ActivationWorkerActivatesNewestEligibleAnalysisCandidate()
    {
        var activities = new ConcurrentBag<Activity>();
        using var activityListener = Listen(
            "flaggo.contract-service",
            activities);
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("01-validate-valid.json");
        using var deployRequest = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/tetris.dropInterval",
            fixture.Body);
        using var deployResponse = await client.SendAsync(deployRequest);
        deployResponse.EnsureSuccessStatusCode();
        var version = (await deployResponse.Content
            .ReadFromJsonAsync<DecisionContractVersion>(StrictJson.Options))!;
        var path =
            $"/v3/decision-contracts/tetris.dropInterval/versions/"
            + $"{version.ContractDigest}/candidates";
        var older = await PostCandidateAsync(
            client,
            path,
            CreateCandidateSubmission("cycle-older", returnValue: 600));
        factory.Clock.SetUtcNow(ContractServiceFactory.Now.AddSeconds(1));
        var newestSubmission =
            CreateCandidateSubmission("cycle-newest", returnValue: 700);
        var newest = await PostCandidateAsync(
            client,
            path,
            newestSubmission);

        var worker = factory.Services
            .GetRequiredService<AnalysisCandidateActivationWorker>();
        await worker.RunOnceAsync();

        var store = factory.Services.GetRequiredService<IExecutableStore>();
        Assert.Equal(
            newest.ExecutableDigest,
            (await store.GetActiveAsync(version.ContractDigest))?.ExecutableDigest);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(older.ExecutableDigest))?.State);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(version.ActiveExecutableDigest))?.State);
        var activity = Assert.Single(
            activities,
            candidate =>
                candidate.DisplayName == "flaggo.candidate.activate"
                && Equals(
                    candidate.GetTagItem("flaggo.candidate.digest"),
                    newest.ExecutableDigest));
        Assert.Equal("success", activity.GetTagItem("flaggo.operation.outcome"));

        using var retryRequest = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(newestSubmission, StrictJson.Options));
        using var retryResponse = await client.SendAsync(retryRequest);
        var retry = await retryResponse.Content
            .ReadFromJsonAsync<AnalysisCandidateResult>(StrictJson.Options);
        Assert.Equal(HttpStatusCode.OK, retryResponse.StatusCode);
        Assert.NotNull(retry);
        Assert.False(retry.Created);
        Assert.Equal("active", retry.LifecycleState);
        Assert.Equal(newest.ExecutableDigest, retry.ExecutableDigest);
        Assert.Equal(newest.CreatedAt, retry.CreatedAt);

        await worker.RunOnceAsync();
        Assert.Equal(
            newest.ExecutableDigest,
            (await store.GetActiveAsync(version.ContractDigest))?.ExecutableDigest);
    }

    [Fact]
    public async Task ActivationWorkerRejectsCandidateWithoutAutoActivationPolicy()
    {
        var activities = new ConcurrentBag<Activity>();
        using var activityListener = Listen(
            "flaggo.contract-service",
            activities);
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var version = await PutDefaultAsync(client, 3);
        var path =
            $"/v3/decision-contracts/worker.batchSize/versions/"
            + $"{version.ContractDigest}/candidates";
        var older = await PostCandidateAsync(
            client,
            path,
            CreateCandidateSubmission("cycle-policy-older", returnValue: 4));
        factory.Clock.SetUtcNow(ContractServiceFactory.Now.AddSeconds(1));
        var newest = await PostCandidateAsync(
            client,
            path,
            CreateCandidateSubmission("cycle-policy-newest", returnValue: 5));

        await factory.Services
            .GetRequiredService<AnalysisCandidateActivationWorker>()
            .RunOnceAsync();

        var store = factory.Services.GetRequiredService<IExecutableStore>();
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(newest.ExecutableDigest))?.State);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(older.ExecutableDigest))?.State);
        Assert.Equal(
            version.ActiveExecutableDigest,
            (await store.GetActiveAsync(version.ContractDigest))?.ExecutableDigest);
        var activity = Assert.Single(
            activities,
            item =>
                item.DisplayName == "flaggo.candidate.activate"
                && Equals(
                    item.GetTagItem("flaggo.candidate.digest"),
                    newest.ExecutableDigest));
        Assert.Equal("rejected", activity.GetTagItem("flaggo.operation.outcome"));
        Assert.Equal("validation", activity.GetTagItem("flaggo.failure.category"));
    }

    [Fact]
    public void ActivationWorkerConfigurationDefaultsAndRejectsInvalidPacing()
    {
        var defaultOptions = CandidateActivationOptions.FromConfiguration(
            new ConfigurationBuilder().Build());
        Assert.Equal(TimeSpan.FromSeconds(5), defaultOptions.PollInterval);

        var invalid = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                [CandidateActivationOptions.PollIntervalEnvironmentVariable] = "0"
            })
            .Build();
        Action readInvalidOptions = () =>
            CandidateActivationOptions.FromConfiguration(invalid);
        var exception = Assert.Throws<InvalidOperationException>(readInvalidOptions);
        Assert.Contains(
            CandidateActivationOptions.PollIntervalEnvironmentVariable,
            exception.Message,
            StringComparison.Ordinal);
    }

    [Fact]
    public async Task AnalysisCandidateSubmissionRejectsConflictsAndInvalidExecutables()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var version = await PutDefaultAsync(client, 3);
        var path =
            $"/v3/decision-contracts/worker.batchSize/versions/{version.ContractDigest}"
            + "/candidates";
        var first = CreateCandidateSubmission("cycle-1", returnValue: 5);
        using (var request = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(first, StrictJson.Options)))
        using (var response = await client.SendAsync(request))
        {
            Assert.Equal(HttpStatusCode.Created, response.StatusCode);
        }

        var conflicting = CreateCandidateSubmission("cycle-1", returnValue: 6);
        using (var request = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(conflicting, StrictJson.Options)))
        using (var response = await client.SendAsync(request))
        {
            await AssertProblemAsync(
                response,
                HttpStatusCode.Conflict,
                ProblemTypes.CandidateAdmissionConflict);
        }

        var invalid = CreateCandidateSubmission("cycle-2", returnValue: 20);
        using (var request = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(invalid, StrictJson.Options)))
        using (var response = await client.SendAsync(request))
        {
            await AssertProblemAsync(
                response,
                HttpStatusCode.UnprocessableEntity,
                ProblemTypes.InvalidDecisionExecutable);
        }
    }

    [Fact]
    public async Task AnalysisCandidateSubmissionRequiresTheExactCurrentContractDigest()
    {
        var activities = new ConcurrentBag<Activity>();
        using var activityListener = Listen(
            "flaggo.contract-service",
            activities);
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var previous = await PutDefaultAsync(client, 3);
        var current = await PutDefaultAsync(client, 4);
        var submission = CreateCandidateSubmission("cycle-1", returnValue: 5);
        using var staleRequest = JsonRequest(
            HttpMethod.Post,
            $"/v3/decision-contracts/worker.batchSize/versions/{previous.ContractDigest}"
            + "/candidates",
            JsonSerializer.Serialize(submission, StrictJson.Options));
        using var staleResponse = await client.SendAsync(staleRequest);
        await AssertProblemAsync(
            staleResponse,
            HttpStatusCode.Conflict,
            ProblemTypes.StaleContractDigest);
        var staleActivity = Assert.Single(
            activities,
            candidate =>
                candidate.DisplayName == "flaggo.candidate.submit"
                && Equals(
                    candidate.GetTagItem("flaggo.operation.outcome"),
                    "stale"));
        Assert.Equal(
            "stale",
            staleActivity.GetTagItem("flaggo.operation.outcome"));
        Assert.Equal(
            "conflict",
            staleActivity.GetTagItem("flaggo.failure.category"));

        const string missingDigest =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        using var missingRequest = JsonRequest(
            HttpMethod.Post,
            $"/v3/decision-contracts/worker.batchSize/versions/{missingDigest}/candidates",
            JsonSerializer.Serialize(
                CreateCandidateSubmission("cycle-2", returnValue: 5),
                StrictJson.Options));
        using var missingResponse = await client.SendAsync(missingRequest);
        await AssertProblemAsync(
            missingResponse,
            HttpStatusCode.NotFound,
            ProblemTypes.ContractVersionNotFound);

        var store = factory.Services.GetRequiredService<IExecutableStore>();
        Assert.Equal(
            previous.ActiveExecutableDigest,
            (await store.GetActiveAsync(previous.ContractDigest))?.ExecutableDigest);
        Assert.Equal(
            current.ActiveExecutableDigest,
            (await store.GetActiveAsync(current.ContractDigest))?.ExecutableDigest);
    }

    [Fact]
    public async Task ReturnsStandardProblemsForInvalidContractsAndIdentity()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var invalid = await ReadFixtureAsync("04-put-invalid.json");
        using var invalidRequest = JsonRequest(
            HttpMethod.Put,
            invalid.Path,
            invalid.Body);
        using var invalidResponse = await client.SendAsync(invalidRequest);
        await AssertProblemAsync(
            invalidResponse,
            HttpStatusCode.UnprocessableEntity,
            ProblemTypes.InvalidDecisionContract);

        using var mismatchRequest = JsonRequest(
            HttpMethod.Post,
            "/v3/decision-contracts/other.name/validate",
            """{"authority":{"tenant":"local","application":"tetris","environment":"integration"},"name":"actual.name","expression_syntax":"flaggo.cel/v1","attributes":[],"result":{"schema":{"type":"boolean"},"default":false}}""");
        using var mismatchResponse = await client.SendAsync(mismatchRequest);
        await AssertProblemAsync(
            mismatchResponse,
            HttpStatusCode.Conflict,
            ProblemTypes.ContractNameMismatch);

        using var nullMemberRequest = JsonRequest(
            HttpMethod.Post,
            "/v3/decision-contracts/actual.name/validate",
            """{"authority":{"tenant":"local","application":"tetris","environment":"integration"},"name":"actual.name","expression_syntax":"flaggo.cel/v1","attributes":null,"result":{"schema":{"type":"boolean"},"default":false}}""");
        using var nullMemberResponse = await client.SendAsync(nullMemberRequest);
        await AssertProblemAsync(
            nullMemberResponse,
            HttpStatusCode.BadRequest,
            ProblemTypes.InvalidRequest);

        using var schemaMismatchRequest = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/schema.parity",
            """
            {
              "authority": {
                "tenant": "local",
                "application": "tetris",
                "environment": "integration"
              },
              "name": "schema.parity",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [
                { "name": "outcome", "schema": { "type": "integer" } }
              ],
              "result": {
                "schema": { "type": "boolean" },
                "default": false
              },
              "learning": {
                "policy": {
                  "mode": "auto-activation",
                  "evaluate": { "interval": "PT1M" }
                },
                "evidence": [
                  {
                    "name": "outcome",
                    "attribute": "outcome",
                    "correlateBy": [],
                    "source": {
                      "kind": "log",
                      "scope": "",
                      "name": "demo.outcome",
                      "correlation": {}
                    }
                  }
                ],
                "objective": {
                  "primary": {
                    "evidence": "outcome",
                    "direction": "maximize"
                  }
                }
              }
            }
            """);
        using var schemaMismatchResponse = await client.SendAsync(schemaMismatchRequest);
        await AssertProblemAsync(
            schemaMismatchResponse,
            HttpStatusCode.UnprocessableEntity,
            ProblemTypes.InvalidDecisionContract);
    }

    [Fact]
    public async Task ManagementOperationsAndHealthDoNotRequireAuthentication()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        using var live = await client.GetAsync("/health/live");
        using var ready = await client.GetAsync("/health/ready");
        Assert.Equal(HttpStatusCode.OK, live.StatusCode);
        Assert.Equal(HttpStatusCode.OK, ready.StatusCode);
    }

    [Fact]
    public async Task UnsupportedMethodReturnsProblemDetails()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        using var request = new HttpRequestMessage(
            HttpMethod.Delete,
            "/v3/decision-contracts/worker.batchSize");

        using var response = await client.SendAsync(request);

        await AssertProblemAsync(
            response,
            HttpStatusCode.MethodNotAllowed,
            ProblemTypes.MethodNotAllowed);
    }

    private static async Task<DecisionContractVersion> PutDefaultAsync(
        HttpClient client,
        int defaultValue,
        AuthorityScope? authority = null)
    {
        var scope = authority ?? ContractServiceFactory.Scope;
        var body =
            $$"""
            {
              "authority": {
                "tenant": "{{scope.Tenant}}",
                "application": "{{scope.Application}}",
                "environment": "{{scope.Environment}}"
              },
              "name": "worker.batchSize",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": { "type": "integer", "minimum": 1, "maximum": 10 },
                "default": {{defaultValue}}
              }
            }
            """;
        using var request = JsonRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/worker.batchSize",
            body);
        using var response = await client.SendAsync(request);
        response.EnsureSuccessStatusCode();
        return (await response.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options))!;
    }

    private static AnalysisCandidateSubmission CreateCandidateSubmission(
        string cycleId,
        int returnValue) =>
        new()
        {
            Rules =
            [
                new ExecutableRule
                {
                    Name = "analysis-rule",
                    When = new ExpressionWhen("true"),
                    Return = new LiteralReturn(
                        JsonSerializer.SerializeToElement(returnValue))
                }
            ],
            Provenance = new AnalysisCandidateProvenance
            {
                WorkspaceId =
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                CycleId = cycleId,
                AttemptId = $"attempt-{cycleId}",
                EvidenceCutoff = ContractServiceFactory.Now.AddMinutes(-1),
                EvidenceWatermark = 42,
                AnalysisManifestDigest =
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }
        };

    private static async Task<AnalysisCandidateResult> PostCandidateAsync(
        HttpClient client,
        string path,
        AnalysisCandidateSubmission submission)
    {
        using var request = JsonRequest(
            HttpMethod.Post,
            path,
            JsonSerializer.Serialize(submission, StrictJson.Options));
        using var response = await client.SendAsync(request);
        Assert.Equal(HttpStatusCode.Created, response.StatusCode);
        return (await response.Content.ReadFromJsonAsync<AnalysisCandidateResult>(
            StrictJson.Options))!;
    }

    private static HttpRequestMessage JsonRequest(
        HttpMethod method,
        string path,
        string? body = null)
    {
        var request = new HttpRequestMessage(method, path);
        if (body is not null)
        {
            request.Content = new StringContent(body, Encoding.UTF8, "application/json");
        }

        return request;
    }

    private static async Task<(string Path, string Body)> ReadFixtureAsync(string fileName)
    {
        using var document = await ReadFixtureDocumentAsync(fileName);
        var request = document.RootElement.GetProperty("request");
        return (
            request.GetProperty("path").GetString()!,
            request.GetProperty("body").GetRawText());
    }

    private static async Task<JsonDocument> ReadFixtureDocumentAsync(
        string fileName,
        string group = "management")
    {
        var path = Path.Combine(
            AppContext.BaseDirectory,
            "Fixtures",
            group,
            fileName);
        return JsonDocument.Parse(await File.ReadAllTextAsync(path));
    }

    private static async Task AssertProblemAsync(
        HttpResponseMessage response,
        HttpStatusCode status,
        string type)
    {
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(status, response.StatusCode);
        Assert.Equal("application/problem+json", response.Content.Headers.ContentType?.MediaType);
        Assert.Equal(type, body.GetProperty("type").GetString());
        Assert.Equal((int)status, body.GetProperty("status").GetInt32());
        Assert.Equal(
            ["type", "title", "status", "detail", "instance"],
            body.EnumerateObject().Select(property => property.Name).ToArray());
        Assert.True(response.Headers.Contains(CorrelationIds.HeaderName));
    }
}

public sealed class ContractServiceFactory : WebApplicationFactory<Program>
{
    public static readonly AuthorityScope Scope =
        new("local", "tetris", "integration");
    public static readonly DateTimeOffset Now =
        new(2026, 8, 1, 12, 0, 0, TimeSpan.Zero);

    private readonly string _databasePath =
        Path.Combine(Path.GetTempPath(), $"flaggo-contract-service-{Guid.NewGuid():N}.db");

    public MutableTimeProvider Clock { get; } = new(Now);

    protected override void ConfigureWebHost(IWebHostBuilder builder)
    {
        builder.UseEnvironment("Development");
        builder.UseSetting(
            "ConnectionStrings:Flaggo",
            $"Data Source={_databasePath};Pooling=False");
        builder.UseSetting(
            CandidateActivationOptions.PollIntervalEnvironmentVariable,
            "600000");
        builder.ConfigureTestServices(services =>
        {
            services.RemoveAll<TimeProvider>();
            services.AddSingleton<TimeProvider>(Clock);
        });
    }

    protected override void Dispose(bool disposing)
    {
        base.Dispose(disposing);
        DeleteIfExists(_databasePath);
        DeleteIfExists($"{_databasePath}-shm");
        DeleteIfExists($"{_databasePath}-wal");
    }

    private static void DeleteIfExists(string path)
    {
        if (File.Exists(path))
        {
            File.Delete(path);
        }
    }
}

public sealed class MutableTimeProvider(DateTimeOffset now) : TimeProvider
{
    private DateTimeOffset _now = now;

    public override DateTimeOffset GetUtcNow() => _now;

    public void SetUtcNow(DateTimeOffset value)
    {
        _now = value;
    }
}
