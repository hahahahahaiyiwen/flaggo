using System.Net;
using System.Security.Claims;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.ControlPlane;
using Flaggo.DataPlane;
using Flaggo.Decisioning;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Flaggo.Decisioning.Tests;

public sealed class ServiceFixtureConformanceTests
{
    private static readonly IReadOnlyDictionary<string, FixturePlan> Plans =
        BuildPlans();

    public static IEnumerable<object[]> ManifestCases()
    {
        using var manifest = LoadManifest();
        return manifest.RootElement.GetProperty("cases")
            .EnumerateArray()
            .Select(item => new object[] { item.GetProperty("name").GetString()! })
            .ToArray();
    }

    [Fact]
    public void Manifest_HasOneExplicitExecutablePlanPerFixture()
    {
        using var manifest = LoadManifest();
        var cases = manifest.RootElement.GetProperty("cases").EnumerateArray().ToArray();
        var names = cases.Select(CaseName).ToArray();
        var files = cases.Select(FixtureRelativePath).ToArray();

        Assert.Equal(manifest.RootElement.GetProperty("totalCases").GetInt32(), cases.Length);
        Assert.Equal(names.Length, names.Distinct(StringComparer.Ordinal).Count());
        Assert.Equal(files.Length, files.Distinct(StringComparer.Ordinal).Count());
        Assert.Equal(
            names.Order(StringComparer.Ordinal),
            Plans.Keys.Order(StringComparer.Ordinal));

        var fixtureRoot = Path.Combine(TestPaths.RepositoryRoot, "contracts", "fixtures");
        var actualFixtureFiles = Directory
            .EnumerateFiles(fixtureRoot, "*.json", SearchOption.AllDirectories)
            .Select(path => Path.GetRelativePath(
                Path.Combine(TestPaths.RepositoryRoot, "contracts"),
                path).Replace('\\', '/'))
            .Order(StringComparer.Ordinal)
            .ToArray();
        Assert.Equal(files.Order(StringComparer.Ordinal), actualFixtureFiles);
    }

    [Theory]
    [MemberData(nameof(ManifestCases))]
    public async Task ManifestCase_ExecutesItsOwnedBehavior(string name)
    {
        var fixture = LoadFixture(name);
        var plan = Plans[name];
        switch (plan.Host)
        {
            case FixtureHost.DataPlane:
                await ExecuteDataPlaneAsync(fixture, plan);
                break;
            case FixtureHost.ControlPlane:
                await ExecuteControlPlaneAsync(fixture, plan);
                break;
            case FixtureHost.SdkLocal:
                await ExecuteSdkLocalBoundaryAsync(fixture);
                break;
            case FixtureHost.SchemaNegative:
                await ExecuteRawJsonNegativeCasesAsync(fixture);
                break;
            default:
                throw new ArgumentOutOfRangeException();
        }
    }

    private static async Task ExecuteDataPlaneAsync(
        JsonElement fixture,
        FixturePlan plan)
    {
        using var registryFile = new TestRegistryFile();
        var time = RuntimeTime(fixture);
        await using var factory = new FixtureDataPlaneFactory(
            registryFile.Path,
            fixture,
            plan,
            time);
        using var client = factory.CreateClient();
        await PrepareDataPlaneAsync(factory.Services, fixture, plan, time);

        if (plan.Request == RequestExecution.ConcurrentReplay)
        {
            using var firstRequest = CreateRequest(fixture);
            using var secondRequest = CreateRequest(fixture);
            var responses = await Task.WhenAll(
                client.SendAsync(firstRequest),
                client.SendAsync(secondRequest));
            using var first = responses[0];
            using var second = responses[1];
            await AssertResponseAsync(fixture, first);
            await AssertResponseAsync(fixture, second);
            var firstBody = await ReadJsonAsync(first);
            var secondBody = await ReadJsonAsync(second);
            Assert.Equal(
                firstBody.GetProperty("decisionId").GetString(),
                secondBody.GetProperty("decisionId").GetString());
            return;
        }

        if (plan.Request is RequestExecution.Replay or
            RequestExecution.IdempotencyConflict or
            RequestExecution.TtlReplacement)
        {
            using var setupRequest = CreateRequest(
                fixture,
                mutateBody: plan.Request == RequestExecution.IdempotencyConflict);
            using var setupResponse = await client.SendAsync(setupRequest);
            Assert.Equal(HttpStatusCode.OK, setupResponse.StatusCode);
            if (plan.Request == RequestExecution.TtlReplacement)
            {
                time.Advance(TimeSpan.FromHours(24));
            }
        }

        using var request = CreateRequest(fixture);
        using var response = await client.SendAsync(request);
        await AssertResponseAsync(fixture, response);

        if (plan.Request == RequestExecution.Replay)
        {
            var replay = await ReadJsonAsync(response);
            Assert.Equal(
                fixture.GetProperty("expected")
                    .GetProperty("body")
                    .GetProperty("decisionId")
                    .GetString(),
                replay.GetProperty("decisionId").GetString());
        }
    }

    private static async Task ExecuteControlPlaneAsync(
        JsonElement fixture,
        FixturePlan plan)
    {
        using var registryFile = new TestRegistryFile();
        var time = new MutableTimeProvider(
            new DateTimeOffset(2026, 7, 30, 18, 30, 0, TimeSpan.Zero));
        await using var factory = new FixtureControlPlaneFactory(
            registryFile.Path,
            fixture,
            plan,
            time);
        using var client = factory.CreateClient();
        await PrepareControlPlaneAsync(factory.Services, fixture, plan, time);

        using var request = CreateRequest(fixture);
        using var response = await client.SendAsync(request);
        await AssertResponseAsync(fixture, response);
    }

    private static async Task ExecuteSdkLocalBoundaryAsync(JsonElement fixture)
    {
        Assert.True(fixture.GetProperty("sdkLocal").GetBoolean());
        Assert.Contains(
            "no network request is issued",
            fixture.GetProperty("invariant").GetString()!,
            StringComparison.OrdinalIgnoreCase);

        using var registryFile = new TestRegistryFile();
        var plan = new FixturePlan(
            FixtureHost.DataPlane,
            SetupKind.ConflictingDefinition);
        var time = RuntimeTime(fixture);
        await using var factory = new FixtureDataPlaneFactory(
            registryFile.Path,
            fixture,
            plan,
            time);
        using var client = factory.CreateClient();
        using var request = CreateRequest(fixture);
        using var response = await client.SendAsync(request);
        Assert.Equal(HttpStatusCode.Conflict, response.StatusCode);
        var body = await ReadJsonAsync(response);
        Assert.Equal("contract-conflict", body.GetProperty("code").GetString());

        var expected = fixture.GetProperty("expected");
        Assert.False(expected.GetProperty("networkCallIssued").GetBoolean());
        Assert.False(expected.GetProperty("clientFallbackAllowed").GetBoolean());
    }

    private static async Task ExecuteRawJsonNegativeCasesAsync(JsonElement fixture)
    {
        Assert.True(fixture.GetProperty("schemaNegative").GetBoolean());
        using var registryFile = new TestRegistryFile();
        var plan = new FixturePlan(FixtureHost.DataPlane, SetupKind.RuntimeDecision);
        var time = RuntimeTime(fixture);
        await using var factory = new FixtureDataPlaneFactory(
            registryFile.Path,
            fixture,
            plan,
            time);
        using var client = factory.CreateClient();

        foreach (var invalidJson in fixture.GetProperty("rejectedJsonDocuments")
                     .EnumerateArray())
        {
            using var request = new HttpRequestMessage(
                HttpMethod.Post,
                "/v1/decisions/tetris.dropInterval:decide")
            {
                Content = new StringContent(
                    invalidJson.GetString()!,
                    Encoding.UTF8,
                    "application/json")
            };
            request.Headers.Add("X-Flaggo-Correlation-Id", "schema-negative");
            using var response = await client.SendAsync(request);
            Assert.Equal(HttpStatusCode.BadRequest, response.StatusCode);
            var body = await ReadJsonAsync(response);
            Assert.Equal("malformed-json", body.GetProperty("code").GetString());
        }

        Assert.NotEmpty(fixture.GetProperty("rejectedSamples").EnumerateArray());
        Assert.NotEmpty(fixture.GetProperty("rejectedFixtureMutations").EnumerateArray());
    }

    private static async Task PrepareDataPlaneAsync(
        IServiceProvider services,
        JsonElement fixture,
        FixturePlan plan,
        MutableTimeProvider time)
    {
        if (plan.Setup is not (
                SetupKind.ExposurePending or
                SetupKind.ExposureReplay or
                SetupKind.ExposureConflict or
                SetupKind.ExposureInvalidAppliedAt))
        {
            return;
        }

        var store = services.GetRequiredService<IExposureStore>();
        var requestBody = fixture.GetProperty("request").GetProperty("body");
        var confirmToken = plan.Setup == SetupKind.ExposurePending &&
                           fixture.GetProperty("name").GetString() ==
                           "exposure-confirm-invalid-token"
            ? "confirm-abc"
            : requestBody.GetProperty("confirmToken").GetString()!;
        await store.CreatePendingAsync(
            "decision-123",
            confirmToken,
            ExposureSnapshot(),
            CancellationToken.None);
        if (plan.Setup is SetupKind.ExposureReplay or SetupKind.ExposureConflict)
        {
            var appliedAt = plan.Setup == SetupKind.ExposureConflict
                ? "2026-07-29T19:20:00Z"
                : requestBody.TryGetProperty("appliedAt", out var value)
                    ? value.GetString()
                    : null;
            var confirmationService =
                services.GetRequiredService<IExposureConfirmationService>();
            await confirmationService.ConfirmAsync(
                "decision-123",
                new ExposureConfirmationRequest(confirmToken, appliedAt),
                new HashSet<string>(StringComparer.Ordinal) { "tetris-demo" },
                new HashSet<string>(StringComparer.Ordinal) { "dev" },
                CancellationToken.None);
        }
    }

    private static async Task PrepareControlPlaneAsync(
        IServiceProvider services,
        JsonElement fixture,
        FixturePlan plan,
        MutableTimeProvider time)
    {
        var manager = services.GetRequiredService<IDefinitionBundleManager>();
        var approvals = services.GetRequiredService<IDefinitionApprovalManager>();
        if (plan.Setup == SetupKind.ApplyConflict)
        {
            await manager.ApplyAsync(
                "apply-key-1",
                ManagementBundle("04-apply-approved-receipt.json"));
            return;
        }

        if (plan.Setup == SetupKind.ExpiredResubmission)
        {
            var body = fixture.GetProperty("request").GetProperty("body");
            await manager.ApplyAsync("apply-key-sem-1", body);
            time.Advance(TimeSpan.FromDays(8));
            return;
        }

        if (plan.Setup is not (
                SetupKind.PendingApproval or
                SetupKind.ExpiredApproval or
                SetupKind.ApprovedApproval or
                SetupKind.RejectedApproval))
        {
            return;
        }

        var pending = Assert.IsType<RequiresApprovalResult>(
            (await manager.ApplyAsync(
                "approval-fixture-setup",
                ManagementBundle("06-apply-requires-approval.json"))).Body);
        if (plan.Setup == SetupKind.ExpiredApproval)
        {
            if (fixture.GetProperty("expected").TryGetProperty("body", out var expiredBody) &&
                expiredBody.TryGetProperty("expiredAt", out var expiredAt))
            {
                time.Set(DateTimeOffset.Parse(expiredAt.GetString()!));
            }
            else
            {
                time.Advance(TimeSpan.FromDays(8));
            }
        }
        else if (plan.Setup == SetupKind.ApprovedApproval)
        {
            var expected = fixture.GetProperty("expected");
            var expectedBody = expected.TryGetProperty("body", out var body)
                ? body
                : default;
            if (expectedBody.ValueKind == JsonValueKind.Object &&
                expectedBody.TryGetProperty("decidedAt", out var decidedAt))
            {
                time.Set(DateTimeOffset.Parse(decidedAt.GetString()!));
            }

            var actor = expectedBody.ValueKind == JsonValueKind.Object &&
                        expectedBody.TryGetProperty("approval", out var approval)
                ? ReadActor(approval.GetProperty("actor"))
                : new ApprovalActor("user:operator-1", "Release Operator");
            var comment = fixture.GetProperty("request")
                .TryGetProperty("body", out var requestBody) &&
                requestBody.TryGetProperty("comment", out var commentValue)
                    ? commentValue.GetString()
                    : null;
            await approvals.ApproveAsync(
                pending.ApprovalRequestId,
                pending.BundleDigest,
                actor,
                comment);
        }
        else if (plan.Setup == SetupKind.RejectedApproval)
        {
            await approvals.RejectAsync(
                pending.ApprovalRequestId,
                pending.BundleDigest,
                new ApprovalActor("user:operator-1", "Release Operator"),
                "operator-rejected",
                null);
        }

        if (plan.Setup == SetupKind.PendingApproval &&
            fixture.GetProperty("expected").TryGetProperty(
                "body",
                out var pendingExpectedBody) &&
            pendingExpectedBody.TryGetProperty(
                "decidedAt",
                out var pendingDecidedAt))
        {
            time.Set(DateTimeOffset.Parse(pendingDecidedAt.GetString()!));
        }
    }

    private static HttpRequestMessage CreateRequest(
        JsonElement fixture,
        bool mutateBody = false)
    {
        var request = fixture.GetProperty("request");
        var message = new HttpRequestMessage(
            new HttpMethod(request.GetProperty("method").GetString()!),
            request.GetProperty("path").GetString());
        if (request.TryGetProperty("body", out var body))
        {
            var bodyText = body.GetRawText();
            if (mutateBody)
            {
                using var document = JsonDocument.Parse(bodyText);
                var dictionary = JsonSerializer.Deserialize<Dictionary<string, JsonElement>>(
                    document.RootElement.GetRawText())!;
                var runtimeContext = dictionary["runtimeContext"]
                    .Deserialize<Dictionary<string, JsonElement>>()!;
                runtimeContext["deviceType"] =
                    JsonSerializer.SerializeToElement("conflict");
                dictionary["runtimeContext"] =
                    JsonSerializer.SerializeToElement(runtimeContext);
                bodyText = JsonSerializer.Serialize(dictionary);
            }

            message.Content = new StringContent(bodyText, Encoding.UTF8, "application/json");
        }

        foreach (var header in request.GetProperty("headers").EnumerateObject())
        {
            if (header.Name is "Authorization" or "Content-Type")
            {
                continue;
            }

            message.Headers.TryAddWithoutValidation(header.Name, header.Value.GetString());
        }

        return message;
    }

    private static async Task AssertResponseAsync(
        JsonElement fixture,
        HttpResponseMessage response)
    {
        var expected = fixture.GetProperty("expected");
        Assert.Equal(
            expected.GetProperty("status").GetInt32(),
            (int)response.StatusCode);
        AssertHeaders(expected.GetProperty("headers"), response);
        if (!expected.TryGetProperty("body", out var expectedBody))
        {
            return;
        }

        var actual = await ReadJsonAsync(response);
        if (expectedBody.TryGetProperty("code", out _))
        {
            AssertProblem(fixture, expectedBody, actual);
        }
        else if (expectedBody.TryGetProperty("decisionMode", out _))
        {
            AssertDecision(expectedBody, actual);
        }
        else if (expectedBody.TryGetProperty("exposureId", out _))
        {
            AssertExposure(expectedBody, actual);
        }
        else if (expectedBody.TryGetProperty("service", out _) ||
                 expectedBody.TryGetProperty("checks", out _))
        {
            AssertHealth(expectedBody, actual);
        }
        else if (expectedBody.TryGetProperty("approvalRequestId", out _))
        {
            AssertApproval(expectedBody, actual);
        }
        else if (expectedBody.TryGetProperty("acceptedDefinitions", out _) ||
                 expectedBody.TryGetProperty("validatedDefinitions", out _) ||
                 expectedBody.TryGetProperty("compatibility", out _) ||
                 expectedBody.TryGetProperty("issues", out _))
        {
            AssertManagement(expectedBody, actual);
        }
        else
        {
            Assert.True(
                JsonElement.DeepEquals(expectedBody, actual),
                $"Response body differs.\nExpected: {expectedBody}\nActual: {actual}");
        }
    }

    private static void AssertHeaders(
        JsonElement expectedHeaders,
        HttpResponseMessage response)
    {
        foreach (var expectedHeader in expectedHeaders.EnumerateObject())
        {
            if (expectedHeader.Name == "Content-Type")
            {
                Assert.Equal(
                    expectedHeader.Value.GetString(),
                    response.Content.Headers.ContentType?.MediaType);
                continue;
            }

            Assert.True(
                response.Headers.TryGetValues(expectedHeader.Name, out var values),
                $"Required response header '{expectedHeader.Name}' is missing.");
            var actual = Assert.Single(values);
            if (expectedHeader.Name == "X-Flaggo-Correlation-Id")
            {
                Assert.False(string.IsNullOrWhiteSpace(actual));
            }
            else if (expectedHeader.Name == "Idempotency-Key-Expires-At")
            {
                Assert.Equal(
                    DateTimeOffset.Parse(expectedHeader.Value.GetString()!),
                    DateTimeOffset.Parse(actual));
            }
            else
            {
                Assert.Equal(expectedHeader.Value.GetString(), actual);
            }
        }
    }

    private static void AssertProblem(
        JsonElement fixture,
        JsonElement expected,
        JsonElement actual)
    {
        foreach (var required in new[]
                 {
                     "type", "status", "code", "title", "detail", "instance",
                     "correlationId"
                 })
        {
            Assert.True(actual.TryGetProperty(required, out _));
        }

        Assert.Equal(expected.GetProperty("status").GetInt32(), actual.GetProperty("status").GetInt32());
        Assert.Equal(expected.GetProperty("code").GetString(), actual.GetProperty("code").GetString());
        Assert.Equal(
            fixture.GetProperty("request").GetProperty("path").GetString(),
            actual.GetProperty("instance").GetString());
        if (expected.TryGetProperty("retryAfterSeconds", out var retry))
        {
            Assert.Equal(retry.GetInt32(), actual.GetProperty("retryAfterSeconds").GetInt32());
        }

        if (expected.TryGetProperty("clientFallback", out var fallback))
        {
            Assert.True(actual.TryGetProperty("clientFallback", out var actualFallback));
            Assert.Equal(
                fallback.GetProperty("eligible").GetBoolean(),
                actualFallback.GetProperty("eligible").GetBoolean());
            Assert.Equal(
                fallback.GetProperty("reason").GetString(),
                actualFallback.GetProperty("reason").GetString());
        }

        if (expected.TryGetProperty("issues", out var issues))
        {
            var actualIssues = actual.GetProperty("issues").EnumerateArray().ToArray();
            foreach (var issue in issues.EnumerateArray())
            {
                Assert.Contains(
                    actualIssues,
                    candidate =>
                        candidate.GetProperty("code").GetString() ==
                        issue.GetProperty("code").GetString() &&
                        candidate.GetProperty("path").GetString() ==
                        issue.GetProperty("path").GetString());
            }
        }
    }

    private static void AssertDecision(JsonElement expected, JsonElement actual)
    {
        foreach (var property in new[]
                 {
                     "decisionKey", "definition", "decisionId", "value", "valueType",
                     "decisionMode", "confidence", "targetProvenance",
                     "resolutionChain", "fallback", "policy", "definitionStatus",
                     "exposure", "reason", "auditId"
                 })
        {
            Assert.True(actual.TryGetProperty(property, out var actualValue));
            Assert.True(
                JsonElement.DeepEquals(expected.GetProperty(property), actualValue),
                $"Decision property '{property}' differs.");
        }

        foreach (var optional in new[] { "strategyId", "runtimeTarget", "controlTarget" })
        {
            if (expected.TryGetProperty(optional, out var expectedValue))
            {
                Assert.True(actual.TryGetProperty(optional, out var actualValue));
                Assert.True(JsonElement.DeepEquals(expectedValue, actualValue));
            }
            else
            {
                Assert.False(actual.TryGetProperty(optional, out _));
            }
        }

        if (!expected.TryGetProperty("evidence", out _))
        {
            Assert.False(actual.TryGetProperty("evidence", out _));
        }
    }

    private static void AssertExposure(JsonElement expected, JsonElement actual)
    {
        Assert.Equal(expected.GetProperty("exposureId").GetString(), actual.GetProperty("exposureId").GetString());
        Assert.Equal(expected.GetProperty("decisionId").GetString(), actual.GetProperty("decisionId").GetString());
        Assert.Equal(expected.GetProperty("status").GetString(), actual.GetProperty("status").GetString());
        Assert.Equal(
            DateTimeOffset.Parse(expected.GetProperty("confirmedAt").GetString()!),
            DateTimeOffset.Parse(actual.GetProperty("confirmedAt").GetString()!));
    }

    private static void AssertHealth(JsonElement expected, JsonElement actual)
    {
        Assert.Equal(expected.GetProperty("status").GetString(), actual.GetProperty("status").GetString());
        Assert.Equal(
            DateTimeOffset.Parse(expected.GetProperty("observedAt").GetString()!),
            DateTimeOffset.Parse(actual.GetProperty("observedAt").GetString()!));
        if (expected.TryGetProperty("service", out var service))
        {
            Assert.Equal(service.GetString(), actual.GetProperty("service").GetString());
            Assert.Equal(
                expected.GetProperty("version").GetString(),
                actual.GetProperty("version").GetString());
        }

        if (expected.TryGetProperty("checks", out var checks))
        {
            Assert.True(JsonElement.DeepEquals(checks, actual.GetProperty("checks")));
        }
    }

    private static void AssertManagement(JsonElement expected, JsonElement actual)
    {
        Assert.Equal(expected.GetProperty("status").GetString(), actual.GetProperty("status").GetString());
        foreach (var property in new[] { "bundleDigest", "compatibility" })
        {
            if (expected.TryGetProperty(property, out var expectedValue))
            {
                Assert.Equal(expectedValue.GetString(), actual.GetProperty(property).GetString());
            }
        }

        if (expected.TryGetProperty("issues", out var issues))
        {
            var actualIssues = actual.GetProperty("issues").EnumerateArray().ToArray();
            foreach (var issue in issues.EnumerateArray())
            {
                Assert.Contains(
                    actualIssues,
                    candidate =>
                        candidate.GetProperty("code").GetString() ==
                        issue.GetProperty("code").GetString() &&
                        candidate.GetProperty("path").GetString() ==
                        issue.GetProperty("path").GetString());
            }
        }

        if (expected.TryGetProperty("acceptedDefinitions", out var accepted))
        {
            foreach (var definition in accepted.EnumerateObject())
            {
                var actualDefinition = actual.GetProperty("acceptedDefinitions")
                    .GetProperty(definition.Name);
                Assert.True(
                    JsonElement.DeepEquals(definition.Value, actualDefinition),
                    $"Accepted definition '{definition.Name}' differs. Expected: {definition.Value}; Actual: {actualDefinition}");
            }
        }

        if (expected.TryGetProperty("validatedDefinitions", out var validated))
        {
            foreach (var definition in validated.EnumerateObject())
            {
                var actualDefinition = actual.GetProperty("validatedDefinitions")
                    .GetProperty(definition.Name);
                Assert.True(JsonElement.DeepEquals(definition.Value, actualDefinition));
            }
        }
    }

    private static void AssertApproval(JsonElement expected, JsonElement actual)
    {
        foreach (var property in new[]
                 {
                     "approvalRequestId", "application", "environment",
                     "bundleDigest", "status"
                 })
        {
            Assert.Equal(
                expected.GetProperty(property).GetString(),
                actual.GetProperty(property).GetString());
        }

        foreach (var timestamp in new[]
                 {
                     "createdAt", "expiresAt", "decidedAt", "expiredAt"
                 })
        {
            if (expected.TryGetProperty(timestamp, out var expectedValue))
            {
                Assert.Equal(
                    DateTimeOffset.Parse(expectedValue.GetString()!),
                    DateTimeOffset.Parse(actual.GetProperty(timestamp).GetString()!));
            }
        }

        Assert.NotEmpty(actual.GetProperty("changes").EnumerateArray());
        if (expected.TryGetProperty("supersedesApprovalRequestId", out var supersedes))
        {
            Assert.Equal(
                supersedes.GetString(),
                actual.GetProperty("supersedesApprovalRequestId").GetString());
        }

        if (expected.TryGetProperty("approval", out var approval))
        {
            Assert.True(
                JsonElement.DeepEquals(
                    approval,
                    actual.GetProperty("approval")));
        }

        if (expected.TryGetProperty("rejection", out var rejection))
        {
            Assert.True(
                JsonElement.DeepEquals(
                    rejection,
                    actual.GetProperty("rejection")));
        }

        if (expected.TryGetProperty("receipt", out var receipt))
        {
            var actualReceipt = actual.GetProperty("receipt");
            Assert.Equal(
                receipt.GetProperty("status").GetString(),
                actualReceipt.GetProperty("status").GetString());
            Assert.Equal(
                receipt.GetProperty("bundleDigest").GetString(),
                actualReceipt.GetProperty("bundleDigest").GetString());
            var snapshotDefinition = ApprovalSnapshotBundle()
                .GetProperty("definitions")[0];
            foreach (var definition in receipt.GetProperty("acceptedDefinitions")
                         .EnumerateObject())
            {
                var expectedIdentity = definition.Value;
                var actualIdentity = actualReceipt
                    .GetProperty("acceptedDefinitions")
                    .GetProperty(definition.Name);
                Assert.Equal(
                    expectedIdentity.GetProperty("definitionId").GetString(),
                    actualIdentity.GetProperty("definitionId").GetString());
                Assert.Equal(
                    expectedIdentity.GetProperty("revision").GetString(),
                    actualIdentity.GetProperty("revision").GetString());
                Assert.Equal(
                    CanonicalJson.ContractDigest(snapshotDefinition),
                    actualIdentity.GetProperty("contractDigest").GetString());
            }
        }
    }

    private static JsonElement BuildRuntimeDefinitionBody(JsonElement fixture) =>
        fixture.GetProperty("request").TryGetProperty("body", out var body)
            ? body
            : default;

    private static IDefinitionRegistry RuntimeRegistry(
        JsonElement fixture,
        FixturePlan plan)
    {
        var expectedBody = fixture.GetProperty("expected")
            .TryGetProperty("body", out var body)
            ? body
            : default;
        if (plan.Setup == SetupKind.RateLimited)
        {
            return new ThrowingRegistry(
                new DecisionContractException(
                    429,
                    "rate-limited",
                    expectedBody.GetProperty("detail").GetString()!,
                    retryAfterSeconds: 2));
        }

        if (plan.Setup == SetupKind.ServiceUnavailable)
        {
            return new ThrowingRegistry(new TimeoutException("Registry unavailable."));
        }

        if (plan.Setup == SetupKind.InternalFailure)
        {
            return new ThrowingRegistry(new InvalidOperationException("Unexpected failure."));
        }

        if (plan.Setup == SetupKind.UnknownKey)
        {
            return new InMemoryDefinitionRegistry([]);
        }

        var requestBody = BuildRuntimeDefinitionBody(fixture);
        if (requestBody.ValueKind != JsonValueKind.Object ||
            !requestBody.TryGetProperty("expectedContract", out var contract))
        {
            return new InMemoryDefinitionRegistry(LocalRegistryHosting.DefaultDefinitions());
        }

        var decisionKey = fixture.GetProperty("request")
            .GetProperty("path")
            .GetString()!
            .Split("/v1/decisions/", StringSplitOptions.None)[1]
            .Split(":decide", StringSplitOptions.None)[0];
        var identity = contract.Deserialize<RuntimeContractIdentity>(
            RuntimeHttp.JsonOptions)!;
        var lifecycle = "active";
        if (plan.Setup == SetupKind.UnknownDefinition)
        {
            identity = identity with
            {
                DefinitionId = "def_other",
                Revision = "rev_other"
            };
        }
        else if (plan.Setup == SetupKind.ConflictingDefinition)
        {
            identity = identity with
            {
                ContractDigest =
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            };
        }
        else if (plan.Setup == SetupKind.RetiredDefinition)
        {
            lifecycle = "retired";
        }

        var fallback = expectedBody.ValueKind == JsonValueKind.Object &&
                       expectedBody.TryGetProperty("value", out var expectedValue)
            ? expectedValue.Clone()
            : JsonSerializer.SerializeToElement(800);
        var valueType = expectedBody.ValueKind == JsonValueKind.Object &&
                        expectedBody.TryGetProperty("valueType", out var expectedType)
            ? expectedType.GetString()!
            : "number";
        var runtimeFields = LocalRegistryHosting.DefaultDefinitions()[0].RuntimeContext;
        var inputs = LocalRegistryHosting.DefaultDefinitions()[0].Inputs;
        var policy = plan.Setup switch
        {
            SetupKind.EvidenceRequiredEligible =>
                new DecisionPolicyContract(
                    MinimumEvidenceQuality: 0.5,
                    RequiredEvidenceUnavailable: "allow"),
            SetupKind.EvidenceRequiredForbidden =>
                new DecisionPolicyContract(
                    MinimumEvidenceQuality: 0.5,
                    RequiredEvidenceUnavailable: "forbid"),
            _ => null
        };
        var definition = new RegisteredDecisionDefinition(
            requestBody.TryGetProperty("client", out var client)
                ? client.GetProperty("appId").GetString()!
                : "tetris-demo",
            requestBody.TryGetProperty("client", out client)
                ? client.GetProperty("environment").GetString()!
                : "dev",
            decisionKey,
            identity,
            valueType,
            fallback,
            expectedBody.ValueKind == JsonValueKind.Object &&
            expectedBody.TryGetProperty("fallback", out var expectedFallback) &&
            expectedFallback.TryGetProperty("reason", out var reason) &&
            reason.ValueKind == JsonValueKind.String
                ? reason.GetString()!
                : "safe_default_drop_interval",
            decisionKey == "tetris.dropInterval" ? inputs : [],
            decisionKey == "tetris.dropInterval" ? runtimeFields : [],
            lifecycle,
            valueType == "number" ? new NumberActionSpaceContract(0, 5000) : null,
            policy);
        return new InMemoryDefinitionRegistry([definition]);
    }

    private static GovernedDecisionState? RuntimeState(
        JsonElement fixture,
        FixturePlan plan)
    {
        var expected = fixture.GetProperty("expected");
        if (!expected.TryGetProperty("body", out var body) ||
            (!body.TryGetProperty("decisionMode", out _) &&
             plan.Setup is not (
                 SetupKind.EvidenceRequiredEligible or
                 SetupKind.EvidenceRequiredForbidden)))
        {
            return null;
        }

        var requestBody = fixture.GetProperty("request").GetProperty("body");
        var contract = requestBody.GetProperty("expectedContract");
        var value = body.TryGetProperty("value", out var expectedValue)
            ? expectedValue.Clone()
            : JsonSerializer.SerializeToElement(800);
        DecisionTargetRef? controlTarget = body.TryGetProperty(
            "controlTarget",
            out var target)
            ? target.Deserialize<DecisionTargetRef>(RuntimeHttp.JsonOptions)
            : null;
        return new GovernedDecisionState(
            contract.GetProperty("definitionId").GetString()!,
            contract.GetProperty("revision").GetString()!,
            contract.GetProperty("contractDigest").GetString()!,
            value,
            controlTarget,
            body.TryGetProperty("decisionMode", out var mode)
                ? mode.GetString()!
                : "strategy",
            body.TryGetProperty("strategyId", out var strategy)
                ? strategy.GetString()
                : "fixture-strategy");
    }

    private static InMemoryDefinitionRegistry ControlRegistry(
        JsonElement fixture,
        FixturePlan plan,
        MutableTimeProvider time)
    {
        IReadOnlyList<RegisteredDecisionDefinition> definitions = plan.Setup switch
        {
            SetupKind.EmptyRegistry => [],
            SetupKind.PreviousDefinition or
            SetupKind.PendingApproval or
            SetupKind.ExpiredApproval or
            SetupKind.ApprovedApproval or
            SetupKind.RejectedApproval or
            SetupKind.ExpiredResubmission =>
                [PreviousDefinition()],
            SetupKind.MultiDefinition => MultiDefinitions(fixture),
            SetupKind.LineageMismatch => LineageMismatchDefinitions(),
            SetupKind.MetadataOnly => LocalRegistryHosting.DefaultDefinitions(),
            _ => [IdenticalDefinition()]
        };
        return new InMemoryDefinitionRegistry(
            definitions,
            new FixtureDefinitionIdentityGenerator(),
            time);
    }

    private static RegisteredDecisionDefinition IdenticalDefinition()
    {
        var definition = LocalRegistryHosting.DefaultDefinitions()[0];
        return definition with
        {
            Identity = definition.Identity with
            {
                BundleDigest =
                    "sha256:b906aceba616dda027d6001cb8cd94a72cd96bc10a37b983a9a7c0fefd8e9a6f"
            }
        };
    }

    private static RegisteredDecisionDefinition PreviousDefinition()
    {
        var definition = LocalRegistryHosting.DefaultDefinitions()[0];
        return definition with
        {
            Identity = new RuntimeContractIdentity(
                definition.Identity.DefinitionId,
                "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e",
                "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        };
    }

    private static IReadOnlyList<RegisteredDecisionDefinition> MultiDefinitions(
        JsonElement fixture)
    {
        var bundle = fixture.GetProperty("request").GetProperty("body");
        var accepted = fixture.GetProperty("expected")
            .GetProperty("body")
            .GetProperty("acceptedDefinitions");
        return bundle.GetProperty("definitions").EnumerateArray().Select(definition =>
        {
            var key = definition.GetProperty("key").GetString()!;
            var identity = accepted.GetProperty(key);
            return new RegisteredDecisionDefinition(
                "tetris-demo",
                "dev",
                key,
                new RuntimeContractIdentity(
                    identity.GetProperty("definitionId").GetString()!,
                    identity.GetProperty("contractDigest").GetString()!,
                    identity.GetProperty("revision").GetString()!,
                    fixture.GetProperty("expected")
                        .GetProperty("body")
                        .GetProperty("bundleDigest")
                        .GetString()),
                definition.GetProperty("valueType").GetString()!,
                definition.GetProperty("fallback").GetProperty("value").Clone(),
                "fixture",
                [],
                []);
        }).ToArray();
    }

    private static IReadOnlyList<RegisteredDecisionDefinition>
        LineageMismatchDefinitions()
    {
        var current = IdenticalDefinition() with
        {
            Identity = IdenticalDefinition().Identity with
            {
                DefinitionId = "def_current_tetris"
            }
        };
        var foreign = IdenticalDefinition() with
        {
            DecisionKey = "other.key"
        };
        return [current, foreign];
    }

    private static JsonElement ManagementBundle(string file)
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            "definition-bundle",
            file);
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.GetProperty("request").GetProperty("body").Clone();
    }

    private static JsonElement ApprovalSnapshotBundle()
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            "approvals",
            "10-snapshot-digest-headers.json");
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.GetProperty("expected").GetProperty("body").Clone();
    }

    private static MutableTimeProvider RuntimeTime(JsonElement fixture)
    {
        var expected = fixture.GetProperty("expected");
        if (expected.TryGetProperty("body", out var body))
        {
            foreach (var timestamp in new[] { "observedAt", "confirmedAt" })
            {
                if (body.ValueKind == JsonValueKind.Object &&
                    body.TryGetProperty(timestamp, out var value))
                {
                    return new MutableTimeProvider(
                        DateTimeOffset.Parse(value.GetString()!));
                }
            }
        }

        if (expected.TryGetProperty("headers", out var headers) &&
            headers.TryGetProperty("Idempotency-Key-Expires-At", out var expiry))
        {
            var retainedAt = DateTimeOffset.Parse(expiry.GetString()!);
            if (fixture.GetProperty("name").GetString() ==
                "decide-idempotency-ttl-expiry-new-request")
            {
                retainedAt = retainedAt.AddHours(-24);
            }

            return new MutableTimeProvider(
                retainedAt.AddHours(-24));
        }

        return new MutableTimeProvider(
            new DateTimeOffset(2026, 7, 30, 18, 30, 0, TimeSpan.Zero));
    }

    private static DecisionSnapshot ExposureSnapshot() =>
        new(
            "tetris-demo",
            "dev",
            LocalRegistryHosting.DefaultDefinitions()[0].Identity,
            JsonSerializer.SerializeToElement(700),
            "number",
            new ServerFallbackInfo("server", false, false, null),
            new Dictionary<string, JsonElement>(),
            [],
            null,
            new DecisionTargetRef("cohort", "new_players"),
            [],
            ["global"],
            new PolicyEvaluationResult("approved", [], []));

    private static ApprovalActor ReadActor(JsonElement actor) =>
        new(
            actor.GetProperty("subject").GetString()!,
            actor.TryGetProperty("displayName", out var displayName)
                ? displayName.GetString()
                : null);

    private static async Task<JsonElement> ReadJsonAsync(
        HttpResponseMessage response)
    {
        var bytes = await response.Content.ReadAsByteArrayAsync();
        using var document = JsonDocument.Parse(bytes);
        return document.RootElement.Clone();
    }

    private static JsonElement LoadFixture(string name)
    {
        using var manifest = LoadManifest();
        var fixtureCase = manifest.RootElement.GetProperty("cases")
            .EnumerateArray()
            .Single(item => CaseName(item) == name);
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            FixtureRelativePath(fixtureCase)
                .Replace('/', Path.DirectorySeparatorChar));
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.Clone();
    }

    private static JsonDocument LoadManifest() =>
        JsonDocument.Parse(
            File.ReadAllBytes(
                Path.Combine(
                    TestPaths.RepositoryRoot,
                    "contracts",
                    "conformance",
                    "fixture-manifest-v1.json")));

    private static string CaseName(JsonElement fixtureCase) =>
        fixtureCase.GetProperty("name").GetString()!;

    private static string FixtureRelativePath(JsonElement fixtureCase) =>
        fixtureCase.GetProperty("file").GetString()!;

    private static IReadOnlyDictionary<string, FixturePlan> BuildPlans() =>
        new Dictionary<string, FixturePlan>(StringComparer.Ordinal)
        {
            ["decide-active-fixed-value"] = Data(SetupKind.RuntimeDecision),
            ["decide-active-numeric-strategy"] = Data(SetupKind.RuntimeDecision),
            ["decide-targetless-global-value"] = Data(SetupKind.RuntimeDecision),
            ["decide-resolution-fallback-with-confidence"] = Data(SetupKind.RuntimeDecision),
            ["decide-policy-decision-fallback-null-confidence"] = Data(SetupKind.RuntimeDecision),
            ["decide-idempotency-replay-returns-original"] = Data(SetupKind.RuntimeDecision, RequestExecution.Replay),
            ["decide-concurrent-fingerprint-converge"] = Data(SetupKind.RuntimeDecision, RequestExecution.ConcurrentReplay),
            ["decide-idempotency-ttl-expiry-new-request"] = Data(SetupKind.RuntimeDecision, RequestExecution.TtlReplacement),
            ["decide-cohort-claim-provenance"] = Data(SetupKind.RuntimeDecision),
            ["decide-default-response-omits-evidence-view"] = Data(SetupKind.RuntimeDecision),
            ["decide-unknown-decision-key"] = Data(SetupKind.UnknownKey),
            ["decide-missing-contract-identity"] = Data(SetupKind.RuntimeDecision),
            ["decide-unknown-definition"] = Data(SetupKind.UnknownDefinition),
            ["decide-conflicting-contract-identity"] = Data(SetupKind.ConflictingDefinition),
            ["decide-retired-definition"] = Data(SetupKind.RetiredDefinition),
            ["decide-duplicate-inference-input"] = Data(SetupKind.RuntimeDecision),
            ["decide-invalid-inference-input"] = Data(SetupKind.RuntimeDecision),
            ["decide-idempotency-key-body-conflict"] = Data(SetupKind.RuntimeDecision, RequestExecution.IdempotencyConflict),
            ["decide-idempotency-in-progress"] = Data(SetupKind.IdempotencyInProgress),
            ["decide-authentication-required"] = Data(SetupKind.RuntimeDecision, authentication: FixtureAuthentication.None),
            ["decide-insufficient-scope"] = Data(SetupKind.RuntimeDecision, authentication: FixtureAuthentication.InsufficientScope),
            ["confirm-insufficient-scope"] = Data(SetupKind.ExposurePending, authentication: FixtureAuthentication.InsufficientScope),
            ["decide-scope-mismatch"] = Data(SetupKind.RuntimeDecision, authentication: FixtureAuthentication.ScopeMismatch),
            ["decide-rate-limited"] = Data(SetupKind.RateLimited),
            ["decide-service-unavailable-eligible"] = Data(SetupKind.ServiceUnavailable),
            ["decide-ineligible-no-client-fallback"] = Data(SetupKind.InternalFailure),
            ["decide-required-evidence-unavailable-forbidden"] = Data(SetupKind.EvidenceRequiredForbidden),
            ["decide-required-evidence-unavailable-eligible"] = Data(SetupKind.EvidenceRequiredEligible),
            ["decide-evidence-required-readiness-unchanged"] = Data(SetupKind.EvidenceRequiredForbidden),
            ["sdk-local-callsite-binding-mismatch"] = new(FixtureHost.SdkLocal, SetupKind.ConflictingDefinition),
            ["decide-schema-negative-cases"] = new(FixtureHost.SchemaNegative, SetupKind.RuntimeDecision),
            ["exposure-confirm-success"] = Data(SetupKind.ExposurePending),
            ["exposure-confirm-idempotent-replay"] = Data(SetupKind.ExposureReplay),
            ["exposure-confirm-invalid-token"] = Data(SetupKind.ExposurePending),
            ["exposure-confirm-conflict"] = Data(SetupKind.ExposureConflict),
            ["exposure-confirm-invalid-applied-at"] = Data(SetupKind.ExposureInvalidAppliedAt),
            ["health-liveness"] = Data(SetupKind.Health),
            ["health-readiness-ready"] = Data(SetupKind.Health),
            ["health-readiness-degraded"] = Data(SetupKind.Health),
            ["health-readiness-not-ready"] = Data(SetupKind.Health),
            ["validate-insufficient-scope"] = Control(SetupKind.IdenticalDefinition, FixtureAuthentication.InsufficientScope),
            ["apply-insufficient-scope"] = Control(SetupKind.IdenticalDefinition, FixtureAuthentication.InsufficientScope),
            ["approve-insufficient-scope"] = Control(SetupKind.PendingApproval, FixtureAuthentication.InsufficientScope),
            ["get-approval-insufficient-scope"] = Control(SetupKind.PendingApproval, FixtureAuthentication.InsufficientScope),
            ["get-approval-snapshot-insufficient-scope"] = Control(SetupKind.PendingApproval, FixtureAuthentication.InsufficientScope),
            ["reject-insufficient-scope"] = Control(SetupKind.PendingApproval, FixtureAuthentication.InsufficientScope),
            ["validate-bundle-identical"] = Control(SetupKind.IdenticalDefinition),
            ["validate-bundle-invalid"] = Control(SetupKind.IdenticalDefinition),
            ["apply-invalid-bundle"] = Control(SetupKind.IdenticalDefinition),
            ["apply-approved-receipt"] = Control(SetupKind.IdenticalDefinition),
            ["apply-idempotency-conflict"] = Control(SetupKind.ApplyConflict),
            ["apply-requires-approval"] = Control(SetupKind.PreviousDefinition),
            ["apply-multi-definition-receipt"] = Control(SetupKind.MultiDefinition),
            ["apply-metadata-only"] = Control(SetupKind.MetadataOnly),
            ["apply-expired-resubmission-linked"] = Control(SetupKind.ExpiredResubmission),
            ["validate-new-key-omitted-lineage"] = Control(SetupKind.EmptyRegistry),
            ["validate-unknown-lineage"] = Control(SetupKind.EmptyRegistry),
            ["validate-lineage-mismatch"] = Control(SetupKind.LineageMismatch),
            ["validate-stable-issue-codes"] = Control(SetupKind.IdenticalDefinition),
            ["approval-get-review"] = Control(SetupKind.PendingApproval),
            ["approval-approve-success"] = Control(SetupKind.PendingApproval),
            ["approval-reject"] = Control(SetupKind.PendingApproval),
            ["approval-get-expired"] = Control(SetupKind.ExpiredApproval),
            ["approval-not-found"] = Control(SetupKind.PreviousDefinition),
            ["approval-bundle-conflict"] = Control(SetupKind.PendingApproval),
            ["approval-approve-idempotent-replay"] = Control(SetupKind.ApprovedApproval),
            ["approval-terminal-conflict"] = Control(SetupKind.ApprovedApproval),
            ["approval-approve-expired"] = Control(SetupKind.ExpiredApproval),
            ["approval-snapshot-digest-headers"] = Control(SetupKind.PendingApproval)
        };

    private static FixturePlan Data(
        SetupKind setup,
        RequestExecution request = RequestExecution.Single,
        FixtureAuthentication authentication = FixtureAuthentication.Valid) =>
        new(FixtureHost.DataPlane, setup, request, authentication);

    private static FixturePlan Control(
        SetupKind setup,
        FixtureAuthentication authentication = FixtureAuthentication.Valid) =>
        new(FixtureHost.ControlPlane, setup, Authentication: authentication);

    private sealed class FixtureDataPlaneFactory(
        string registryPath,
        JsonElement fixture,
        FixturePlan plan,
        MutableTimeProvider time)
        : WebApplicationFactory<DataPlaneAssemblyMarker>
    {
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting("Flaggo:Authentication:LocalDevelopmentBypass", "true");
            builder.UseSetting("Flaggo:Registry:LocalFilePath", registryPath);
            builder.ConfigureTestServices(services =>
            {
                ConfigureAuthentication(services, fixture, plan);
                services.RemoveAll<LocalFileDefinitionRegistry>();
                services.RemoveAll<IDefinitionRegistry>();
                services.RemoveAll<IRegistryHealth>();
                var registry = RuntimeRegistry(fixture, plan);
                services.AddSingleton(registry);
                services.AddSingleton<IRegistryHealth>(
                    registry as IRegistryHealth ?? new AvailableRegistryHealth());

                services.RemoveAll<TimeProvider>();
                services.AddSingleton<TimeProvider>(time);
                services.RemoveAll<IStateStore>();
                services.RemoveAll<IStateHealth>();
                var state = new FixtureStateStore(RuntimeState(fixture, plan));
                services.AddSingleton<IStateStore>(state);
                services.AddSingleton<IStateHealth>(state);
                services.RemoveAll<IRuntimeIdGenerator>();
                services.AddSingleton<IRuntimeIdGenerator>(
                    new FixtureRuntimeIdGenerator(fixture));
                services.RemoveAll<IStrategyExecutor>();
                services.AddSingleton<IStrategyExecutor>(
                    new FixtureStrategyExecutor(fixture));
                services.RemoveAll<IPolicyEvaluator>();
                services.AddSingleton<IPolicyEvaluator>(
                    new FixturePolicyEvaluator(fixture));
                if (plan.Setup == SetupKind.IdempotencyInProgress)
                {
                    services.RemoveAll<IDecideIdempotencyStore>();
                    services.AddSingleton<IDecideIdempotencyStore>(
                        new InProgressIdempotencyStore());
                }

                if (plan.Setup == SetupKind.Health &&
                    fixture.GetProperty("expected")
                        .GetProperty("body")
                        .TryGetProperty("checks", out var checks))
                {
                    services.RemoveAll<IRuntimeReadinessProbe>();
                    services.AddSingleton<IRuntimeReadinessProbe>(
                        new FixtureReadinessProbe(checks));
                }
            });
        }
    }

    private sealed class FixtureControlPlaneFactory(
        string registryPath,
        JsonElement fixture,
        FixturePlan plan,
        MutableTimeProvider time)
        : WebApplicationFactory<ControlPlaneAssemblyMarker>
    {
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting("Flaggo:Authentication:LocalDevelopmentBypass", "true");
            builder.UseSetting("Flaggo:Registry:LocalFilePath", registryPath);
            builder.ConfigureTestServices(services =>
            {
                ConfigureAuthentication(services, fixture, plan);
                services.RemoveAll<LocalFileDefinitionRegistry>();
                services.RemoveAll<IDefinitionBundleManager>();
                services.RemoveAll<IDefinitionApprovalManager>();
                var registry = ControlRegistry(fixture, plan, time);
                services.AddSingleton(registry);
                services.AddSingleton<IDefinitionBundleManager>(registry);
                services.AddSingleton<IDefinitionApprovalManager>(registry);
            });
        }
    }

    private static void ConfigureAuthentication(
        IServiceCollection services,
        JsonElement fixture,
        FixturePlan plan)
    {
        services.AddSingleton(new FixtureAuthenticationState(
            plan.Authentication,
            ActorFromFixture(fixture),
            ResourceScopeFromFixture(fixture).AppId,
            ResourceScopeFromFixture(fixture).Environment));
        services.AddAuthentication(options =>
            {
                options.DefaultAuthenticateScheme = "Fixture";
                options.DefaultChallengeScheme = "Fixture";
                options.DefaultForbidScheme = "Fixture";
            })
            .AddScheme<AuthenticationSchemeOptions, FixtureAuthenticationHandler>(
                "Fixture",
                _ => { });
    }

    private static ApprovalActor ActorFromFixture(JsonElement fixture)
    {
        if (fixture.GetProperty("expected").TryGetProperty("body", out var body))
        {
            foreach (var property in new[] { "approval", "rejection" })
            {
                if (body.ValueKind == JsonValueKind.Object &&
                    body.TryGetProperty(property, out var decision))
                {
                    return ReadActor(decision.GetProperty("actor"));
                }
            }
        }

        return new ApprovalActor("user:operator-1", "Release Operator");
    }

    private static (string AppId, string Environment) ResourceScopeFromFixture(
        JsonElement fixture)
    {
        if (fixture.GetProperty("request").TryGetProperty("body", out var requestBody))
        {
            if (requestBody.ValueKind == JsonValueKind.Object &&
                requestBody.TryGetProperty("client", out var client))
            {
                return (
                    client.GetProperty("appId").GetString()!,
                    client.GetProperty("environment").GetString()!);
            }

            if (requestBody.ValueKind == JsonValueKind.Object &&
                requestBody.TryGetProperty("application", out var application))
            {
                return (
                    application.GetProperty("id").GetString()!,
                    application.GetProperty("environment").GetString()!);
            }
        }

        if (fixture.GetProperty("expected").TryGetProperty("body", out var expectedBody) &&
            expectedBody.ValueKind == JsonValueKind.Object &&
            expectedBody.TryGetProperty("application", out var appId) &&
            expectedBody.TryGetProperty("environment", out var environment))
        {
            return (appId.GetString()!, environment.GetString()!);
        }

        return ("tetris-demo", "dev");
    }

    private sealed record FixturePlan(
        FixtureHost Host,
        SetupKind Setup,
        RequestExecution Request = RequestExecution.Single,
        FixtureAuthentication Authentication = FixtureAuthentication.Valid);

    private enum FixtureHost
    {
        DataPlane,
        ControlPlane,
        SdkLocal,
        SchemaNegative
    }

    private enum SetupKind
    {
        RuntimeDecision,
        UnknownKey,
        UnknownDefinition,
        ConflictingDefinition,
        RetiredDefinition,
        RateLimited,
        ServiceUnavailable,
        InternalFailure,
        EvidenceRequiredForbidden,
        EvidenceRequiredEligible,
        IdempotencyInProgress,
        ExposurePending,
        ExposureReplay,
        ExposureConflict,
        ExposureInvalidAppliedAt,
        Health,
        IdenticalDefinition,
        PreviousDefinition,
        MetadataOnly,
        MultiDefinition,
        EmptyRegistry,
        LineageMismatch,
        ApplyConflict,
        ExpiredResubmission,
        PendingApproval,
        ExpiredApproval,
        ApprovedApproval,
        RejectedApproval
    }

    private enum RequestExecution
    {
        Single,
        Replay,
        ConcurrentReplay,
        IdempotencyConflict,
        TtlReplacement
    }

    private enum FixtureAuthentication
    {
        Valid,
        None,
        InsufficientScope,
        ScopeMismatch
    }

    private sealed class MutableTimeProvider(DateTimeOffset utcNow) : TimeProvider
    {
        private DateTimeOffset _utcNow = utcNow;

        public override DateTimeOffset GetUtcNow() => _utcNow;

        public void Advance(TimeSpan duration) => _utcNow += duration;

        public void Set(DateTimeOffset value) => _utcNow = value;
    }

    private sealed class FixtureDefinitionIdentityGenerator :
        IDefinitionIdentityGenerator
    {
        private int _approval;

        public string CreateDefinitionId() =>
            "def_01JQ8Z0000000000000000000B";

        public string CreateRevision() =>
            "rev_01JQ99ZZ11223344556677889A";

        public string CreateApprovalRequestId() =>
            Interlocked.Increment(ref _approval) == 1
                ? "apr_01JQ91C2D3E4F5G6H7J8K9M0N1"
                : "apr_01JQ95XR7Y8Z9A0B1C2D3E4F5G";
    }

    private sealed class FixtureRuntimeIdGenerator(JsonElement fixture) :
        IRuntimeIdGenerator
    {
        private int _decision;
        private readonly bool _ttlReplacement =
            fixture.GetProperty("name").GetString() ==
            "decide-idempotency-ttl-expiry-new-request";
        private readonly JsonElement _body = fixture.GetProperty("expected")
            .TryGetProperty("body", out var body)
            ? body
            : default;

        public string CreateDecisionId() =>
            _body.ValueKind == JsonValueKind.Object &&
            _body.TryGetProperty("decisionId", out var value)
                ? _ttlReplacement && _decision++ == 0
                    ? "decision-expired-original"
                    : value.GetString()!
                : "decision-123";

        public string CreateAuditId() =>
            _body.ValueKind == JsonValueKind.Object &&
            _body.TryGetProperty("auditId", out var value)
                ? value.GetString()!
                : "audit-789";

        public string CreateConfirmToken() =>
            _body.ValueKind == JsonValueKind.Object &&
            _body.TryGetProperty("exposure", out var exposure) &&
            exposure.TryGetProperty("confirmToken", out var value)
                ? value.GetString()!
                : "confirm-abc";

        public string CreateExposureId() =>
            _body.ValueKind == JsonValueKind.Object &&
            _body.TryGetProperty("exposureId", out var value)
                ? value.GetString()!
                : "exposure-456";
    }

    private sealed class FixtureStateStore(GovernedDecisionState? state) :
        IStateStore,
        IStateHealth
    {
        public Task<GovernedDecisionState?> GetActiveAsync(
            string decisionKey,
            string definitionId,
            string revision,
            IReadOnlyList<DecisionTargetRef?> resolutionTargets,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            return Task.FromResult(state);
        }

        public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            return Task.FromResult(true);
        }
    }

    private sealed class FixtureStrategyExecutor(JsonElement fixture) :
        IStrategyExecutor
    {
        private readonly JsonElement _body = fixture.GetProperty("expected")
            .TryGetProperty("body", out var body)
            ? body
            : default;

        public Task<StrategyExecutionResult> ExecuteAsync(
            StrategyExecutionRequest request,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var confidence = _body.ValueKind == JsonValueKind.Object &&
                             _body.TryGetProperty("confidence", out var report) &&
                             report.ValueKind == JsonValueKind.Object
                ? report.Deserialize<ConfidenceReport>(RuntimeHttp.JsonOptions)
                : null;
            return Task.FromResult(
                new StrategyExecutionResult(
                    _body.TryGetProperty("value", out var value)
                        ? value.Clone()
                        : request.State.Value,
                    _body.TryGetProperty("decisionMode", out var mode)
                        ? mode.GetString()!
                        : request.State.Mode,
                    _body.TryGetProperty("strategyId", out var strategy)
                        ? strategy.GetString()
                                : null,
                    confidence,
                    _body.TryGetProperty("reason", out var reason)
                        ? reason.GetString()!
                        : "Fixture strategy completed."));
        }
    }

    private sealed class FixturePolicyEvaluator(JsonElement fixture) :
        IPolicyEvaluator
    {
        private readonly JsonElement _body = fixture.GetProperty("expected")
            .TryGetProperty("body", out var body)
            ? body
            : default;

        public Task<PolicyDecision> EvaluateAsync(
            PolicyEvaluationRequest request,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var result = _body.ValueKind == JsonValueKind.Object &&
                         _body.TryGetProperty("policy", out var policy)
                ? policy.Deserialize<PolicyEvaluationResult>(RuntimeHttp.JsonOptions)!
                : new PolicyEvaluationResult("approved", [], []);
            return Task.FromResult(
                new PolicyDecision(result.Result == "approved", result));
        }
    }

    private sealed class ThrowingRegistry(Exception exception) :
        IDefinitionRegistry
    {
        public Task<DefinitionLookup> ResolveAsync(
            string appId,
            string environment,
            string decisionKey,
            string definitionId,
            string revision,
            CancellationToken cancellationToken) =>
            Task.FromException<DefinitionLookup>(exception);
    }

    private sealed class AvailableRegistryHealth : IRegistryHealth
    {
        public Task<bool> IsAvailableAsync(CancellationToken cancellationToken) =>
            Task.FromResult(true);
    }

    private sealed class InProgressIdempotencyStore : IDecideIdempotencyStore
    {
        public Task<IdempotentDecisionResult> ExecuteAsync(
            string idempotencyNamespace,
            string key,
            string fingerprint,
            Func<CancellationToken, Task<DecideTerminalOutcome>> operation,
            CancellationToken cancellationToken) =>
            Task.FromException<IdempotentDecisionResult>(
                new IdempotencyInProgressException());
    }

    private sealed class FixtureReadinessProbe(JsonElement checks) :
        IRuntimeReadinessProbe
    {
        private readonly IReadOnlyList<RuntimeDependencyCheck> _checks =
            checks.EnumerateArray().Select(check =>
                new RuntimeDependencyCheck(
                    check.GetProperty("name").GetString()!,
                    check.GetProperty("status").GetString()!,
                    check.GetProperty("required").GetBoolean())).ToArray();

        public Task<IReadOnlyList<RuntimeDependencyCheck>> CheckAsync(
            CancellationToken cancellationToken) =>
            Task.FromResult(_checks);
    }

    private sealed record FixtureAuthenticationState(
        FixtureAuthentication Authentication,
        ApprovalActor Actor,
        string AppId,
        string Environment);

    private sealed class FixtureAuthenticationHandler(
        IOptionsMonitor<AuthenticationSchemeOptions> options,
        ILoggerFactory logger,
        UrlEncoder encoder,
        FixtureAuthenticationState state)
        : AuthenticationHandler<AuthenticationSchemeOptions>(
            options,
            logger,
            encoder)
    {
        protected override Task<AuthenticateResult> HandleAuthenticateAsync()
        {
            if (state.Authentication == FixtureAuthentication.None)
            {
                return Task.FromResult(AuthenticateResult.NoResult());
            }

            var scope = state.Authentication == FixtureAuthentication.InsufficientScope
                ? "polari.unrelated"
                : "polari.decisions:decide polari.exposures:confirm " +
                  "polari.definitions:validate polari.definitions:apply " +
                  "polari.definitions:approve";
            var appId = state.Authentication == FixtureAuthentication.ScopeMismatch
                ? "another-app"
                : state.AppId;
            var environment = state.Authentication == FixtureAuthentication.ScopeMismatch
                ? "prod"
                : state.Environment;
            var claims = new List<Claim>
            {
                new(ClaimTypes.NameIdentifier, state.Actor.Subject),
                new(ClaimTypes.Name, state.Actor.DisplayName ?? state.Actor.Subject),
                new("scope", scope),
                new("polari_app_id", appId),
                new("polari_environment", environment),
                new("polari_tenant_id", "fixture-tenant")
            };
            var identity = new ClaimsIdentity(claims, Scheme.Name);
            return Task.FromResult(
                AuthenticateResult.Success(
                    new AuthenticationTicket(
                        new ClaimsPrincipal(identity),
                        Scheme.Name)));
        }
    }
}
