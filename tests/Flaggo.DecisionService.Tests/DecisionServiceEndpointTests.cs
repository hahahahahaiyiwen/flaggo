using System.Net;
using System.Net.Http.Json;
using System.Security.Claims;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Flaggo.DecisionService.Tests;

public sealed class DecisionServiceEndpointTests
{
    [Fact]
    public async Task CreatesExactVersionDecisionAndPreservesCorrelationId()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var deployed = await DeployAsync(factory, activate: true);
        using var request = DecisionRequest(
            deployed.Contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25,"pressure":0.9}}""");
        request.Headers.Add(CorrelationIds.HeaderName, "client-correlation");

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal(
            "client-correlation",
            Assert.Single(response.Headers.GetValues(CorrelationIds.HeaderName)));
        Assert.Equal(deployed.ContractDigest, body.GetProperty("contractDigest").GetString());
        Assert.Equal(deployed.ExecutableDigest, body.GetProperty("executableDigest").GetString());
        Assert.Equal(250, body.GetProperty("result").GetInt32());
        Assert.Equal("rule", body.GetProperty("evaluation").GetProperty("source").GetString());
        Assert.Equal(
            "high-pressure",
            body.GetProperty("evaluation").GetProperty("rule").GetString());
    }

    [Fact]
    public async Task ReturnsStandardProblemDetailsForProtocolAndInputFailures()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var deployed = await DeployAsync(factory, activate: true);

        using var unsupported = DecisionRequest(
            deployed.Contract.Name,
            deployed.ContractDigest,
            "{}",
            "text/plain");
        using var unsupportedResponse = await client.SendAsync(unsupported);
        await AssertProblemAsync(
            unsupportedResponse,
            HttpStatusCode.UnsupportedMediaType,
            ProblemTypes.UnsupportedMediaType);

        using var invalid = DecisionRequest(
            deployed.Contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"pressure":0.9}}""");
        using var invalidResponse = await client.SendAsync(invalid);
        await AssertProblemAsync(
            invalidResponse,
            HttpStatusCode.UnprocessableEntity,
            ProblemTypes.InvalidRuntimeInput);

        using var unknownMember = DecisionRequest(
            deployed.Contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25},"unknown":true}""");
        using var unknownMemberResponse = await client.SendAsync(unknownMember);
        await AssertProblemAsync(
            unknownMemberResponse,
            HttpStatusCode.BadRequest,
            ProblemTypes.InvalidRequest);
    }

    [Fact]
    public async Task DistinguishesInvalidAndUnknownContractIdentity()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var deployed = await DeployAsync(factory, activate: true);

        using var invalidName = DecisionRequest(
            "invalid$name",
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25}}""");
        using var invalidNameResponse = await client.SendAsync(invalidName);
        await AssertProblemAsync(
            invalidNameResponse,
            HttpStatusCode.BadRequest,
            ProblemTypes.InvalidRequest);

        using var wrongName = DecisionRequest(
            "checkout.other",
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25}}""");
        using var wrongNameResponse = await client.SendAsync(wrongName);
        await AssertProblemAsync(
            wrongNameResponse,
            HttpStatusCode.NotFound,
            ProblemTypes.ContractVersionNotFound);
    }

    [Fact]
    public async Task ReturnsNotActiveWithRetryAfter()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var deployed = await DeployAsync(factory, activate: false);
        using var request = DecisionRequest(
            deployed.Contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25}}""");

        using var response = await client.SendAsync(request);

        await AssertProblemAsync(
            response,
            HttpStatusCode.ServiceUnavailable,
            ProblemTypes.ExecutableNotActive);
        Assert.Equal("1", Assert.Single(response.Headers.GetValues("Retry-After")));
    }

    [Theory]
    [InlineData("evaluation", ProblemTypes.EvaluationFailed)]
    [InlineData("result", ProblemTypes.InvalidRuntimeResult)]
    public async Task MapsRuntimeFailuresWithoutReturningTheDefault(
        string failure,
        string expectedProblemType)
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var expressionCompiler = factory.Services
            .GetRequiredService<FlaggoExpressionCompiler>();
        var contract = new DecisionContract
        {
            Name = "checkout.failure",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes =
            [
                new ContractAttribute
                {
                    Name = "value",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            Result = new ContractResult
            {
                Schema = failure == "result"
                    ? new ValueSchema { Type = "integer", Minimum = 0 }
                    : new ValueSchema { Type = "integer" },
                Default = JsonSerializer.SerializeToElement(99)
            }
        };
        var executable = new DecisionExecutable
        {
            ContractDigest = ContractDigests.ComputeContractDigest(
                contract,
                expressionCompiler),
            Rules =
            [
                new ExecutableRule
                {
                    Name = "failing-rule",
                    When = new ExpressionWhen("true"),
                    Return = new ExpressionReturn(
                        failure == "evaluation"
                            ? "1 / attributes.value"
                            : "attributes.value")
                }
            ]
        };
        var compilation = new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
        var deployed = await PersistAsync(factory, contract, compilation, activate: true);
        using var request = DecisionRequest(
            contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25,"value":0}}""");
        if (failure == "result")
        {
            request.Content = new StringContent(
                """{"attributes":{"_random":0.25,"value":-1}}""",
                Encoding.UTF8,
                "application/json");
        }

        using var response = await client.SendAsync(request);

        await AssertProblemAsync(
            response,
            HttpStatusCode.InternalServerError,
            expectedProblemType);
    }

    [Fact]
    public async Task RejectsCorruptCheckedExecutable()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        var expressionCompiler = factory.Services
            .GetRequiredService<FlaggoExpressionCompiler>();
        var contract = new DecisionContract
        {
            Name = "checkout.integrity",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes = [],
            Result = new ContractResult
            {
                Schema = new ValueSchema { Type = "integer" },
                Default = JsonSerializer.SerializeToElement(0)
            }
        };
        var executable = new DecisionExecutable
        {
            ContractDigest = ContractDigests.ComputeContractDigest(
                contract,
                expressionCompiler),
            Rules =
            [
                new ExecutableRule
                {
                    Name = "always",
                    When = new ExpressionWhen("true"),
                    Return = new LiteralReturn(JsonSerializer.SerializeToElement(1))
                }
            ]
        };
        var compilation = new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
        var checkedRule = compilation.CheckedExecutable.Rules[0];
        var corruptBytes = checkedRule.Predicate.CheckedAst.ToArray();
        corruptBytes[^1] ^= 0xff;
        compilation = compilation with
        {
            CheckedExecutable = compilation.CheckedExecutable with
            {
                Rules =
                [
                    checkedRule with
                    {
                        Predicate = checkedRule.Predicate with
                        {
                            CheckedAst = corruptBytes
                        }
                    }
                ]
            }
        };
        var deployed = await PersistAsync(factory, contract, compilation, activate: true);
        using var request = DecisionRequest(
            contract.Name,
            deployed.ContractDigest,
            """{"attributes":{"_random":0.25}}""");

        using var response = await client.SendAsync(request);

        await AssertProblemAsync(
            response,
            HttpStatusCode.InternalServerError,
            ProblemTypes.ExecutableIntegrityFailure);
    }

    [Theory]
    [InlineData(null, HttpStatusCode.Unauthorized, ProblemTypes.AuthenticationRequired)]
    [InlineData("forbidden", HttpStatusCode.Forbidden, ProblemTypes.InsufficientScope)]
    public async Task EnforcesAuthenticationAndScope(
        string? authentication,
        HttpStatusCode expectedStatus,
        string expectedProblemType)
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        using var request = new HttpRequestMessage(
            HttpMethod.Post,
            "/v3/decision-contracts/checkout.delay/versions/"
            + "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            + "/decisions")
        {
            Content = new StringContent(
                """{"attributes":{"_random":0.25}}""",
                Encoding.UTF8,
                "application/json")
        };
        if (authentication is not null)
        {
            request.Headers.Add(TestAuthenticationHandler.HeaderName, authentication);
        }

        using var response = await client.SendAsync(request);

        await AssertProblemAsync(response, expectedStatus, expectedProblemType);
    }

    [Fact]
    public async Task ExposesLiveAndReadyHealthWithoutAuthentication()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();

        using var live = await client.GetAsync("/health/live");
        using var ready = await client.GetAsync("/health/ready");
        var liveBody = await live.Content.ReadFromJsonAsync<JsonElement>();
        var readyBody = await ready.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, live.StatusCode);
        Assert.Equal("live", liveBody.GetProperty("status").GetString());
        Assert.Equal("flaggo-decision-service", liveBody.GetProperty("service").GetString());
        Assert.EndsWith("Z", liveBody.GetProperty("observedAt").GetString());
        Assert.Equal(HttpStatusCode.OK, ready.StatusCode);
        Assert.Equal("ready", readyBody.GetProperty("status").GetString());
        var checks = readyBody.GetProperty("checks");
        Assert.Equal(2, checks.GetArrayLength());
        Assert.Equal("contract-store", checks[0].GetProperty("name").GetString());
        Assert.Equal("executable-store", checks[1].GetProperty("name").GetString());
    }

    [Fact]
    public async Task UnknownRouteReturnsProblemDetails()
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();

        using var response = await client.GetAsync("/unknown");

        await AssertProblemAsync(
            response,
            HttpStatusCode.NotFound,
            ProblemTypes.ResourceNotFound);
    }

    [Theory]
    [InlineData("01-rule-result.json")]
    [InlineData("02-default-result.json")]
    [InlineData("03-missing-attribute-default.json")]
    [InlineData("04-current-exposure.json")]
    [InlineData("05-retry-after-activation-change.json")]
    public async Task RuntimeDecisionFixturesProduceExpectedSemantics(string fixtureName)
    {
        using var factory = new DecisionServiceFactory();
        using var client = factory.CreateClient();
        using var fixture = JsonDocument.Parse(
            await File.ReadAllTextAsync(
                Path.Combine(AppContext.BaseDirectory, "Fixtures", "runtime", fixtureName)));
        var expected = fixture.RootElement.GetProperty("expected").GetProperty("body");
        var expectedEvaluation = expected.GetProperty("evaluation");
        var expectedSource = expectedEvaluation.GetProperty("source").GetString();
        var expectedResult = expected.GetProperty("result").GetInt32();
        var contract = new DecisionContract
        {
            Name = "tetris.dropInterval",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes =
            [
                new ContractAttribute
                {
                    Name = "user_id",
                    Schema = new ValueSchema { Type = "string" }
                },
                new ContractAttribute
                {
                    Name = "recovery_failures",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            Result = new ContractResult
            {
                Schema = new ValueSchema { Type = "integer" },
                Default = JsonSerializer.SerializeToElement(800)
            }
        };
        var expressionCompiler = factory.Services
            .GetRequiredService<FlaggoExpressionCompiler>();
        var ruleName = expectedSource == "rule"
            ? expectedEvaluation.GetProperty("rule").GetString()!
            : "slow-after-recovery-failure";
        var executable = new DecisionExecutable
        {
            ContractDigest = ContractDigests.ComputeContractDigest(
                contract,
                expressionCompiler),
            Rules =
            [
                new ExecutableRule
                {
                    Name = ruleName,
                    When = new ExpressionWhen("attributes.recovery_failures > 0"),
                    Return = new LiteralReturn(
                        JsonSerializer.SerializeToElement(
                            expectedSource == "rule" ? expectedResult : 700))
                }
            ]
        };
        var compilation = new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
        var deployed = await PersistAsync(factory, contract, compilation, activate: true);
        using var request = DecisionRequest(
            contract.Name,
            deployed.ContractDigest,
            fixture.RootElement.GetProperty("request").GetProperty("body").GetRawText());

        using var response = await client.SendAsync(request);
        var actual = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal(expectedResult, actual.GetProperty("result").GetInt32());
        Assert.Equal(
            expectedSource,
            actual.GetProperty("evaluation").GetProperty("source").GetString());
        if (expectedSource == "rule")
        {
            Assert.Equal(
                ruleName,
                actual.GetProperty("evaluation").GetProperty("rule").GetString());
        }
    }

    private static HttpRequestMessage DecisionRequest(
        string contractName,
        string contractDigest,
        string body,
        string mediaType = "application/json")
    {
        var request = new HttpRequestMessage(
            HttpMethod.Post,
            $"/v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions")
        {
            Content = new StringContent(body, Encoding.UTF8, mediaType)
        };
        request.Headers.Add(TestAuthenticationHandler.HeaderName, "authorized");
        return request;
    }

    private static async Task<DeployedFixture> DeployAsync(
        DecisionServiceFactory factory,
        bool activate)
    {
        var expressionCompiler = factory.Services.GetRequiredService<FlaggoExpressionCompiler>();
        var contract = new DecisionContract
        {
            Name = "checkout.delay",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes =
            [
                new ContractAttribute
                {
                    Name = "pressure",
                    Schema = new ValueSchema { Type = "number" }
                }
            ],
            Result = new ContractResult
            {
                Schema = new ValueSchema { Type = "integer" },
                Default = JsonSerializer.SerializeToElement(500)
            }
        };
        var contractDigest = ContractDigests.ComputeContractDigest(
            contract,
            expressionCompiler);
        var executable = new DecisionExecutable
        {
            ContractDigest = contractDigest,
            Rules =
            [
                new ExecutableRule
                {
                    Name = "high-pressure",
                    When = new ExpressionWhen("attributes.pressure >= 0.8"),
                    Return = new LiteralReturn(JsonSerializer.SerializeToElement(250))
                }
            ]
        };
        var compilation = new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
        return await PersistAsync(factory, contract, compilation, activate);
    }

    private static async Task<DeployedFixture> PersistAsync(
        DecisionServiceFactory factory,
        DecisionContract contract,
        ExecutableCompilation compilation,
        bool activate)
    {
        var contractStore = factory.Services.GetRequiredService<IContractVersionStore>();
        var executableStore = factory.Services.GetRequiredService<IExecutableStore>();
        var scope = new DecisionScope("test-application", "test-environment");
        var contractDigest = compilation.Executable.ContractDigest;
        await contractStore.PutAsync(new AcceptedContractVersion(
            scope,
            contractDigest,
            new DateTimeOffset(2026, 3, 5, 10, 0, 0, TimeSpan.Zero),
            contract));
        await executableStore.PutCandidateAsync(new StoredExecutable(
            scope,
            compilation.ExecutableDigest,
            compilation.Executable,
            compilation.CheckedExecutable,
            ExecutableLifecycleState.Candidate,
            StateVersion: 0,
            CreatedAt: new DateTimeOffset(2026, 3, 5, 10, 1, 0, TimeSpan.Zero),
            ActivatedAt: null));
        if (activate)
        {
            await executableStore.ActivateAsync(
                scope,
                contractDigest,
                compilation.ExecutableDigest);
        }

        return new DeployedFixture(contract, contractDigest, compilation.ExecutableDigest);
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

    private sealed record DeployedFixture(
        DecisionContract Contract,
        string ContractDigest,
        string ExecutableDigest);
}

public sealed class DecisionServiceFactory : WebApplicationFactory<Program>
{
    private readonly string _databasePath =
        Path.Combine(Path.GetTempPath(), $"flaggo-service-{Guid.NewGuid():N}.db");

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
            services
                .AddAuthentication(options =>
                {
                    options.DefaultAuthenticateScheme =
                        TestAuthenticationHandler.SchemeName;
                    options.DefaultChallengeScheme =
                        TestAuthenticationHandler.SchemeName;
                    options.DefaultForbidScheme =
                        TestAuthenticationHandler.SchemeName;
                })
                .AddScheme<AuthenticationSchemeOptions, TestAuthenticationHandler>(
                    TestAuthenticationHandler.SchemeName,
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

public sealed class TestAuthenticationHandler(
    IOptionsMonitor<AuthenticationSchemeOptions> options,
    ILoggerFactory logger,
    UrlEncoder encoder)
    : AuthenticationHandler<AuthenticationSchemeOptions>(options, logger, encoder)
{
    public const string SchemeName = "Test";
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
            new(FlaggoClaimTypes.Application, "test-application"),
            new(FlaggoClaimTypes.Environment, "test-environment")
        };
        if (string.Equals(value, "authorized", StringComparison.Ordinal))
        {
            claims.Add(new Claim("scope", "flaggo.decisions:decide"));
        }

        var principal = new ClaimsPrincipal(new ClaimsIdentity(claims, SchemeName));
        return Task.FromResult(AuthenticateResult.Success(
            new AuthenticationTicket(principal, SchemeName)));
    }
}
