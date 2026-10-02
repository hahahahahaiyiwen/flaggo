using System.Net;
using System.Net.Http.Json;
using System.Security.Claims;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

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
        using (var seedRequest = AuthorizedRequest(
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
            using var request = AuthorizedRequest(
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
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("01-validate-valid.json");
        using var request = AuthorizedRequest(
            HttpMethod.Post,
            fixture.Path,
            fixture.Body);
        request.Headers.Add(CorrelationIds.HeaderName, "client-validation");

        using var response = await client.SendAsync(request);
        var result = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal("valid", result.GetProperty("status").GetString());
        Assert.Equal(
            "sha256:3c93eda4a9f8ab8b602a08644db59d19ab1219406b26b32aff8ef2c52c4c2040",
            result.GetProperty("contractDigest").GetString());
        Assert.Empty(result.GetProperty("issues").EnumerateArray());
        Assert.Equal(
            "client-validation",
            Assert.Single(response.Headers.GetValues(CorrelationIds.HeaderName)));

        using var get = AuthorizedRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/tetris.dropInterval");
        using var getResponse = await client.SendAsync(get);
        Assert.Equal(HttpStatusCode.NotFound, getResponse.StatusCode);
    }

    [Fact]
    public async Task PublicationIsReadyAndIdempotentBySemanticDigest()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var fixture = await ReadFixtureAsync("03-put-ready.json");

        using var firstRequest = AuthorizedRequest(
            HttpMethod.Put,
            fixture.Path,
            fixture.Body);
        using var first = await client.SendAsync(firstRequest);
        var created = await first.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.Created, first.StatusCode);
        Assert.NotNull(created);
        Assert.Equal(
            "sha256:5ac7477d55d7dfb1b4993e87118182b55a848fdc36fa5617196f27f1400e2ab1",
            created.ContractDigest);
        Assert.Equal(
            "sha256:bf693713abb032d7257141839c38f8b3943d463d464f1b4dc2dba356e3902398",
            created.ActiveExecutableDigest);
        Assert.Equal(ContractServiceFactory.Now, created.AcceptedAt);
        Assert.Equal(
            $"/v3/decision-contracts/tetris.dropInterval/versions/{created.ContractDigest}",
            first.Headers.Location?.OriginalString);

        using var retryRequest = AuthorizedRequest(
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
            .GetActiveAsync(ContractServiceFactory.Scope, created.ContractDigest);
        Assert.Equal(created.ActiveExecutableDigest, active?.ExecutableDigest);
        Assert.Empty(active!.Executable.Rules);
    }

    [Fact]
    public async Task AuthoredExpressionsReplaceDefaultAuthority()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var body =
            """
            {
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
        using var request = AuthorizedRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/worker.batchSize",
            body);

        using var response = await client.SendAsync(request);
        var version = await response.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);

        Assert.Equal(HttpStatusCode.Created, response.StatusCode);
        var active = await factory.Services
            .GetRequiredService<IExecutableStore>()
            .GetActiveAsync(ContractServiceFactory.Scope, version!.ContractDigest);
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
        using var currentRequest = AuthorizedRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/worker.batchSize");
        using var currentResponse = await client.SendAsync(currentRequest);
        var current = await currentResponse.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options);
        Assert.Equal(second.ContractDigest, current?.ContractDigest);

        using var oldRequest = AuthorizedRequest(
            HttpMethod.Get,
            $"/v3/decision-contracts/worker.batchSize/versions/{first.ContractDigest}");
        using var oldResponse = await client.SendAsync(oldRequest);
        Assert.Equal(HttpStatusCode.OK, oldResponse.StatusCode);
        Assert.NotNull(await factory.Services
            .GetRequiredService<IExecutableStore>()
            .GetActiveAsync(ContractServiceFactory.Scope, first.ContractDigest));
    }

    [Fact]
    public async Task ListsStablePaginatedHistory()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        await PutDefaultAsync(client, 2);
        await PutDefaultAsync(client, 3);
        await PutDefaultAsync(client, 4);

        using var firstRequest = AuthorizedRequest(
            HttpMethod.Get,
            "/v3/decision-contracts/worker.batchSize/versions?limit=2");
        using var firstResponse = await client.SendAsync(firstRequest);
        var first = await firstResponse.Content
            .ReadFromJsonAsync<DecisionContractVersionList>(StrictJson.Options);

        Assert.Equal(HttpStatusCode.OK, firstResponse.StatusCode);
        Assert.Equal(2, first!.Versions.Count);
        Assert.NotNull(first.NextCursor);

        using var secondRequest = AuthorizedRequest(
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
                using var request = AuthorizedRequest(
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
    public async Task ReturnsStandardProblemsForInvalidContractsAndIdentity()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        var invalid = await ReadFixtureAsync("04-put-invalid.json");
        using var invalidRequest = AuthorizedRequest(
            HttpMethod.Put,
            invalid.Path,
            invalid.Body);
        using var invalidResponse = await client.SendAsync(invalidRequest);
        await AssertProblemAsync(
            invalidResponse,
            HttpStatusCode.UnprocessableEntity,
            ProblemTypes.InvalidDecisionContract);

        using var mismatchRequest = AuthorizedRequest(
            HttpMethod.Post,
            "/v3/decision-contracts/other.name/validate",
            """{"name":"actual.name","expression_syntax":"flaggo.cel/v1","attributes":[],"result":{"schema":{"type":"boolean"},"default":false}}""");
        using var mismatchResponse = await client.SendAsync(mismatchRequest);
        await AssertProblemAsync(
            mismatchResponse,
            HttpStatusCode.Conflict,
            ProblemTypes.ContractNameMismatch);

        using var nullMemberRequest = AuthorizedRequest(
            HttpMethod.Post,
            "/v3/decision-contracts/actual.name/validate",
            """{"name":"actual.name","expression_syntax":"flaggo.cel/v1","attributes":null,"result":{"schema":{"type":"boolean"},"default":false}}""");
        using var nullMemberResponse = await client.SendAsync(nullMemberRequest);
        await AssertProblemAsync(
            nullMemberResponse,
            HttpStatusCode.BadRequest,
            ProblemTypes.InvalidRequest);

        using var schemaMismatchRequest = AuthorizedRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/schema.parity",
            """
            {
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
    public async Task EnforcesOperationScopesAndReportsHealth()
    {
        using var factory = new ContractServiceFactory();
        using var client = factory.CreateClient();
        using var anonymous = new HttpRequestMessage(
            HttpMethod.Get,
            "/v3/decision-contracts/worker.batchSize");
        using var anonymousResponse = await client.SendAsync(anonymous);
        await AssertProblemAsync(
            anonymousResponse,
            HttpStatusCode.Unauthorized,
            ProblemTypes.AuthenticationRequired);

        using var forbidden = AuthorizedRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/worker.batchSize",
            """{"name":"worker.batchSize"}""",
            "read-only");
        using var forbiddenResponse = await client.SendAsync(forbidden);
        await AssertProblemAsync(
            forbiddenResponse,
            HttpStatusCode.Forbidden,
            ProblemTypes.InsufficientScope);

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
        int defaultValue)
    {
        var body =
            $$"""
            {
              "name": "worker.batchSize",
              "expression_syntax": "flaggo.cel/v1",
              "attributes": [],
              "result": {
                "schema": { "type": "integer", "minimum": 1, "maximum": 10 },
                "default": {{defaultValue}}
              }
            }
            """;
        using var request = AuthorizedRequest(
            HttpMethod.Put,
            "/v3/decision-contracts/worker.batchSize",
            body);
        using var response = await client.SendAsync(request);
        response.EnsureSuccessStatusCode();
        return (await response.Content.ReadFromJsonAsync<DecisionContractVersion>(
            StrictJson.Options))!;
    }

    private static HttpRequestMessage AuthorizedRequest(
        HttpMethod method,
        string path,
        string? body = null,
        string authentication = "authorized")
    {
        var request = new HttpRequestMessage(method, path);
        request.Headers.Add(TestContractAuthenticationHandler.HeaderName, authentication);
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

    private static async Task<JsonDocument> ReadFixtureDocumentAsync(string fileName)
    {
        var path = Path.Combine(
            AppContext.BaseDirectory,
            "Fixtures",
            "management",
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
    public static readonly DecisionScope Scope =
        new("test-application", "test-environment");
    public static readonly DateTimeOffset Now =
        new(2026, 8, 1, 12, 0, 0, TimeSpan.Zero);

    private readonly string _databasePath =
        Path.Combine(Path.GetTempPath(), $"flaggo-contract-service-{Guid.NewGuid():N}.db");

    public MutableTimeProvider Clock { get; } = new(Now);

    protected override void ConfigureWebHost(IWebHostBuilder builder)
    {
        builder.UseEnvironment("Development");
        builder.UseSetting(
            "Flaggo:Authentication:LocalDevelopmentBypass",
            "true");
        builder.UseSetting(
            "ConnectionStrings:Flaggo",
            $"Data Source={_databasePath};Pooling=False");
        builder.ConfigureTestServices(services =>
        {
            services.RemoveAll<TimeProvider>();
            services.AddSingleton<TimeProvider>(Clock);
            services
                .AddAuthentication(options =>
                {
                    options.DefaultAuthenticateScheme =
                        TestContractAuthenticationHandler.SchemeName;
                    options.DefaultChallengeScheme =
                        TestContractAuthenticationHandler.SchemeName;
                    options.DefaultForbidScheme =
                        TestContractAuthenticationHandler.SchemeName;
                })
                .AddScheme<
                    AuthenticationSchemeOptions,
                    TestContractAuthenticationHandler>(
                    TestContractAuthenticationHandler.SchemeName,
                    _ => { });
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

public sealed class TestContractAuthenticationHandler(
    IOptionsMonitor<AuthenticationSchemeOptions> options,
    ILoggerFactory logger,
    UrlEncoder encoder)
    : AuthenticationHandler<AuthenticationSchemeOptions>(options, logger, encoder)
{
    public const string SchemeName = "ContractTest";
    public const string HeaderName = "X-Test-Authentication";

    protected override Task<AuthenticateResult> HandleAuthenticateAsync()
    {
        var value = Request.Headers[HeaderName].FirstOrDefault();
        if (value is null)
        {
            return Task.FromResult(AuthenticateResult.NoResult());
        }

        var claims = new List<Claim>
        {
            new(ClaimTypes.NameIdentifier, "test"),
            new(FlaggoClaimTypes.Application, ContractServiceFactory.Scope.Application),
            new(FlaggoClaimTypes.Environment, ContractServiceFactory.Scope.Environment)
        };
        claims.Add(new Claim(
            "scope",
            value == "authorized"
                ? "flaggo.contracts:validate flaggo.contracts:accept flaggo.contracts:read"
                : "flaggo.contracts:read"));
        var principal = new ClaimsPrincipal(new ClaimsIdentity(claims, SchemeName));
        return Task.FromResult(AuthenticateResult.Success(
            new AuthenticationTicket(principal, SchemeName)));
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
