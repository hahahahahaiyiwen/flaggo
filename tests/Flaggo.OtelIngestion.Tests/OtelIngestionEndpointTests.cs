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
    public async Task IngestsFlaggoAndApplicationLogsAsCandidateEvidence()
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
        Assert.Equal(2, body.GetProperty("created").GetInt32());

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var decision = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            signal: "decision.received"));
        Assert.Equal("decision-1", decision.ObservationId);
        Assert.Equal("logs", decision.TelemetryType);
        Assert.Equal("decision.received", decision.Signal);
        Assert.Equal("flaggo.decision.received", decision.Payload.GetProperty("eventName").GetString());
        Assert.Equal("game-1", AttributeValue(decision.Payload, "flaggo.correlation.gameId"));

        var outcome = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            telemetryType: "logs",
            signal: "outcome.observed"));
        Assert.Equal("outcome-1", outcome.ObservationId);
        Assert.Equal("outcome.observed", outcome.Signal);
        Assert.Equal("tetris.survival_ms", AttributeValue(outcome.Payload, "flaggo.evidence.binding"));
    }

    [Fact]
    public async Task IngestsNonFlaggoTelemetryAndReportsDuplicates()
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
        Assert.Equal(3, body.GetProperty("accepted").GetInt32());
        Assert.Equal(0, body.GetProperty("ignored").GetInt32());
        Assert.Equal(2, body.GetProperty("created").GetInt32());
        Assert.Equal(1, body.GetProperty("duplicates").GetInt32());

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var logs = await store.ListTelemetryAsync(Scope, 10, telemetryType: "logs");
        Assert.Contains(logs, record => record.Signal == "log");
    }

    [Fact]
    public async Task IngestsMetricsAndTracesAsRawCandidateEvidence()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();

        using var metricsResponse = await client.SendAsync(MetricsRequest());
        using var tracesResponse = await client.SendAsync(TracesRequest());

        Assert.Equal(HttpStatusCode.OK, metricsResponse.StatusCode);
        Assert.Equal(HttpStatusCode.OK, tracesResponse.StatusCode);

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var metric = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            telemetryType: "metrics",
            signal: "metric:tetris.board_pressure_mean_5s"));
        Assert.Equal("metrics", metric.TelemetryType);
        Assert.Equal("tetris.board_pressure_mean_5s", metric.Payload.GetProperty("name").GetString());

        var span = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            telemetryType: "traces",
            signal: "span:tetris.tick"));
        Assert.Equal("traces", span.TelemetryType);
        Assert.Equal("span-1", span.Payload.GetProperty("spanId").GetString());
    }

    [Fact]
    public async Task AcceptsUnauthenticatedTelemetryWhenResourceScopeIsPresent()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(DecisionRecord("decision-1"));
        request.Headers.Remove(TestAuthenticationHandler.HeaderName);

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal(1, body.GetProperty("accepted").GetInt32());
    }

    private static HttpRequestMessage LogsRequest(params object[] records)
    {
        var payload = new
        {
            resourceLogs = new[]
            {
                new
                {
                    resource = Resource(),
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

    private static HttpRequestMessage MetricsRequest()
    {
        var payload = new
        {
            resourceMetrics = new[]
            {
                new
                {
                    resource = Resource(),
                    scopeMetrics = new[]
                    {
                        new
                        {
                            metrics = new[]
                            {
                                new
                                {
                                    name = "tetris.board_pressure_mean_5s",
                                    gauge = new
                                    {
                                        dataPoints = new[]
                                        {
                                            new
                                            {
                                                timeUnixNano = "1770000000000000000",
                                                asDouble = 0.82,
                                                attributes = new[]
                                                {
                                                    Attribute("game.id", "game-1")
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
        var request = new HttpRequestMessage(HttpMethod.Post, "/v1/metrics")
        {
            Content = JsonContent.Create(payload)
        };
        request.Headers.Add(TestAuthenticationHandler.HeaderName, "authorized");
        return request;
    }

    private static HttpRequestMessage TracesRequest()
    {
        var payload = new
        {
            resourceSpans = new[]
            {
                new
                {
                    resource = Resource(),
                    scopeSpans = new[]
                    {
                        new
                        {
                            spans = new[]
                            {
                                new
                                {
                                    traceId = "trace-1",
                                    spanId = "span-1",
                                    name = "tetris.tick",
                                    startTimeUnixNano = "1770000000000000000",
                                    endTimeUnixNano = "1770000001000000000",
                                    attributes = new[]
                                    {
                                        Attribute("game.id", "game-1"),
                                        Attribute("current_level", "9")
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
        var request = new HttpRequestMessage(HttpMethod.Post, "/v1/traces")
        {
            Content = JsonContent.Create(payload)
        };
        request.Headers.Add(TestAuthenticationHandler.HeaderName, "authorized");
        return request;
    }

    private static object Resource() => new
    {
        attributes = new[]
        {
            Attribute("service.name", "test-application"),
            Attribute("deployment.environment.name", "test-environment")
        }
    };

    private static object DecisionRecord(string decisionId) => new
    {
        eventName = "flaggo.decision.received",
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
        eventName = "flaggo.outcome.observed",
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

    private static string? AttributeValue(JsonElement logRecord, string name)
    {
        foreach (var attribute in logRecord.GetProperty("attributes").EnumerateArray())
        {
            if (attribute.GetProperty("key").GetString() == name)
            {
                return attribute
                    .GetProperty("value")
                    .GetProperty("stringValue")
                    .GetString();
            }
        }
        return null;
    }

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
