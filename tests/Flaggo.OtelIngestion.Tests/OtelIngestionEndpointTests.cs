using System.Net;
using System.Net.Http.Json;
using System.Security.Claims;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Flaggo.OtelIngestion.Tests;

public sealed class OtelIngestionEndpointTests
{
    private static readonly DecisionScope Scope = new("test-application", "test-environment");

    [Fact]
    public async Task IngestsDecisionAndOutcomeObservationsFromOtlpJsonLogs()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(
            DecisionRecord("decision-1"),
            OutcomeRecord("outcome-1"));

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal(2, body.GetProperty("accepted").GetInt32());
        Assert.Equal(1, body.GetProperty("decisionObservationsCreated").GetInt32());
        Assert.Equal(1, body.GetProperty("outcomeObservationsCreated").GetInt32());

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var decision = Assert.Single(await store.ListDecisionsAsync(Scope, 10));
        Assert.Equal("decision-1", decision.DecisionId);
        Assert.Equal("tetris.dropInterval", decision.ContractName);
        Assert.Equal(850, decision.Result.GetInt32());
        Assert.Equal("game-1", decision.CorrelationAttributes["gameId"].GetString());

        var outcome = Assert.Single(await store.ListOutcomesAsync(Scope, 10));
        Assert.Equal("outcome-1", outcome.ObservationId);
        Assert.Equal("decision-1", outcome.DecisionId);
        Assert.Equal("tetris.survival_ms", outcome.Binding);
        Assert.Equal(18_400, outcome.Value.GetInt32());
    }

    [Fact]
    public async Task IgnoresNonFlaggoTelemetryAndReportsDuplicates()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(
            NonFlaggoRecord(),
            DecisionRecord("decision-1"),
            DecisionRecord("decision-1"));

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal(2, body.GetProperty("accepted").GetInt32());
        Assert.Equal(1, body.GetProperty("ignored").GetInt32());
        Assert.Equal(1, body.GetProperty("decisionObservationsCreated").GetInt32());
        Assert.Equal(1, body.GetProperty("duplicates").GetInt32());
    }

    [Fact]
    public async Task ReturnsProblemDetailsForInvalidFlaggoTelemetry()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(new
        {
            timeUnixNano = "1770000000000000000",
            attributes = new[]
            {
                Attribute("flaggo.signal", "decision.received")
            }
        });

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.BadRequest, response.StatusCode);
        Assert.Equal(ProblemTypes.InvalidRequest, body.GetProperty("type").GetString());
        Assert.Contains("flaggo.decision.id", body.GetProperty("detail").GetString());
    }

    [Theory]
    [InlineData(null, HttpStatusCode.Unauthorized, ProblemTypes.AuthenticationRequired)]
    [InlineData("forbidden", HttpStatusCode.Forbidden, ProblemTypes.InsufficientScope)]
    public async Task EnforcesEvidenceWriteScope(
        string? authentication,
        HttpStatusCode status,
        string problemType)
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(DecisionRecord("decision-1"));
        request.Headers.Remove(TestAuthenticationHandler.HeaderName);
        if (authentication is not null)
        {
            request.Headers.Add(TestAuthenticationHandler.HeaderName, authentication);
        }

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(status, response.StatusCode);
        Assert.Equal(problemType, body.GetProperty("type").GetString());
    }

    private static HttpRequestMessage LogsRequest(params object[] records)
    {
        var payload = new
        {
            resourceLogs = new[]
            {
                new
                {
                    scopeLogs = new[]
                    {
                        new
                        {
                            logRecords = records
                        }
                    }
                }
            }
        };
        var request = new HttpRequestMessage(HttpMethod.Post, "/v1/logs")
        {
            Content = JsonContent.Create(payload)
        };
        request.Headers.Add(TestAuthenticationHandler.HeaderName, "authorized");
        return request;
    }

    private static object DecisionRecord(string decisionId) => new
    {
        timeUnixNano = "1770000000000000000",
        attributes = new[]
        {
            Attribute("flaggo.signal", "decision.received"),
            Attribute("flaggo.decision.id", decisionId),
            Attribute("flaggo.contract.name", "tetris.dropInterval"),
            Attribute("flaggo.contract.digest", Digest('0')),
            Attribute("flaggo.executable.digest", Digest('1')),
            Attribute("flaggo.result.json", "850"),
            Attribute("flaggo.result.hash", Digest('2')),
            Attribute("flaggo.evaluation.source", "rule"),
            Attribute("flaggo.evaluation.rule", "high-pressure"),
            Attribute("flaggo.correlation.gameId", "game-1")
        }
    };

    private static object OutcomeRecord(string observationId) => new
    {
        timeUnixNano = "1770000300000000000",
        attributes = new[]
        {
            Attribute("flaggo.signal", "outcome.observed"),
            Attribute("flaggo.observation.id", observationId),
            Attribute("flaggo.evidence.binding", "tetris.survival_ms"),
            Attribute("flaggo.evidence.value.json", "18400"),
            Attribute("flaggo.decision.id", "decision-1"),
            Attribute("flaggo.contract.name", "tetris.dropInterval"),
            Attribute("flaggo.contract.digest", Digest('0')),
            Attribute("flaggo.correlation.gameId", "game-1")
        }
    };

    private static object NonFlaggoRecord() => new
    {
        timeUnixNano = "1770000000000000000",
        attributes = new[]
        {
            Attribute("service.event", "ordinary-app-log")
        }
    };

    private static object Attribute(string key, string value) => new
    {
        key,
        value = new { stringValue = value }
    };

    private static string Digest(char value) => $"sha256:{new string(value, 64)}";
}

public sealed class OtelIngestionFactory : WebApplicationFactory<Program>
{
    private readonly string _databasePath =
        Path.Combine(Path.GetTempPath(), $"flaggo-otel-{Guid.NewGuid():N}.db");

    public string ConnectionString => $"Data Source={_databasePath};Pooling=False";

    protected override void ConfigureWebHost(IWebHostBuilder builder)
    {
        builder.UseEnvironment("Development");
        builder.UseSetting(
            "Flaggo:Authentication:LocalDevelopmentBypass",
            "true");
        builder.UseSetting("ConnectionStrings:Flaggo", ConnectionString);
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
            claims.Add(new Claim("scope", "flaggo.evidence:write"));
        }

        var principal = new ClaimsPrincipal(new ClaimsIdentity(claims, SchemeName));
        return Task.FromResult(AuthenticateResult.Success(
            new AuthenticationTicket(principal, SchemeName)));
    }
}
