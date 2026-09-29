using System.IO.Compression;
using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;

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
            OutcomeRecord("decision-1"));

        using var response = await client.SendAsync(request);
        var body = await response.Content.ReadFromJsonAsync<JsonElement>();

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Empty(body.EnumerateObject());

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var decision = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            signal: "decision.received"));
        Assert.NotEqual("decision-1", decision.ObservationId);
        Assert.Equal("logs", decision.TelemetryType);
        Assert.Equal("decision.received", decision.Signal);
        var decisionRecord = StoredRecord(decision);
        Assert.Equal(
            "flaggo.decision.received",
            decisionRecord.GetProperty("eventName").GetString());
        Assert.Equal("decision-1", AttributeValue(decisionRecord, "flaggo.decision.id"));
        Assert.Equal(
            "game-1",
            AttributeValue(decisionRecord, "flaggo.correlation.gameId"));
        Assert.Equal(
            "instance-1",
            AttributeValue(
                decision.Envelope.GetProperty("resource"),
                "service.instance.id"));
        Assert.Equal(
            "@flaggo/test",
            decision.Envelope.GetProperty("scope").GetProperty("name").GetString());
        Assert.Equal(
            "https://opentelemetry.io/schemas/1.27.0",
            decision.Envelope.GetProperty("resourceSchemaUrl").GetString());
        Assert.Equal(
            "https://flaggo.dev/schemas/telemetry/v1",
            decision.Envelope.GetProperty("scopeSchemaUrl").GetString());

        var outcome = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            telemetryType: "logs",
            signal: "outcome.observed"));
        Assert.NotEqual(decision.ObservationId, outcome.ObservationId);
        Assert.NotEqual("decision-1", outcome.ObservationId);
        Assert.Equal("outcome.observed", outcome.Signal);
        Assert.Equal(
            "decision-1",
            AttributeValue(StoredRecord(outcome), "flaggo.decision.id"));
        Assert.Equal(
            "tetris.survival_ms",
            AttributeValue(StoredRecord(outcome), "flaggo.evidence.binding"));
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
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);

        var store = new SqliteEvidenceStore(factory.ConnectionString);
        var logs = await store.ListTelemetryAsync(Scope, 10, telemetryType: "logs");
        Assert.Equal(2, logs.Count);
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
        Assert.Equal(
            "tetris.board_pressure_mean_5s",
            StoredRecord(metric).GetProperty("name").GetString());

        var span = Assert.Single(await store.ListTelemetryAsync(
            Scope,
            10,
            telemetryType: "traces",
            signal: "span:tetris.tick"));
        Assert.Equal("traces", span.TelemetryType);
        Assert.Equal("traces:trace-1:span-1", span.ObservationId);
        Assert.Equal("span-1", StoredRecord(span).GetProperty("spanId").GetString());
    }

    [Fact]
    public async Task AcceptsUnauthenticatedTelemetryWhenResourceScopeIsPresent()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(DecisionRecord("decision-1"));

        using var response = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
    }

    [Fact]
    public async Task AcceptsGzipCompressedOtlpJson()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        var json = JsonSerializer.SerializeToUtf8Bytes(
            LogsPayload(DecisionRecord("decision-1")));
        using var compressed = new MemoryStream();
        using (var gzip = new GZipStream(
            compressed,
            CompressionLevel.Fastest,
            leaveOpen: true))
        {
            gzip.Write(json);
        }
        using var request = new HttpRequestMessage(HttpMethod.Post, "/v1/logs")
        {
            Content = new ByteArrayContent(compressed.ToArray())
        };
        request.Content.Headers.ContentType =
            new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
        request.Content.Headers.ContentEncoding.Add("gzip");

        using var response = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var store = new SqliteEvidenceStore(factory.ConnectionString);
        Assert.Single(await store.ListTelemetryAsync(Scope, 10));
    }

    [Fact]
    public async Task AcceptsLogStringsBeyondContractJsonLimit()
    {
        using var factory = new OtelIngestionFactory();
        using var client = factory.CreateClient();
        using var request = LogsRequest(new
        {
            timeUnixNano = "1770000000000000000",
            body = new { stringValue = new string('x', 20_000) },
            attributes = new[]
            {
                Attribute("service.event", "large-stack-trace")
            }
        });

        using var response = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
    }

    private static HttpRequestMessage LogsRequest(params object[] records)
    {
        var request = new HttpRequestMessage(HttpMethod.Post, "/v1/logs")
        {
            Content = JsonContent.Create(LogsPayload(records))
        };
        return request;
    }

    private static object LogsPayload(params object[] records) =>
        new
        {
            resourceLogs = new object[]
            {
                new
                {
                    resource = Resource(),
                    schemaUrl = "https://opentelemetry.io/schemas/1.27.0",
                    scopeLogs = new[]
                    {
                        new
                        {
                            scope = InstrumentationScope(),
                            schemaUrl = "https://flaggo.dev/schemas/telemetry/v1",
                            logRecords = records
                        }
                    }
                }
            }
        };

    private static HttpRequestMessage MetricsRequest()
    {
        var payload = new
        {
            resourceMetrics = new[]
            {
                new
                {
                    resource = Resource(),
                    schemaUrl = "https://opentelemetry.io/schemas/1.27.0",
                    scopeMetrics = new[]
                    {
                        new
                        {
                            scope = InstrumentationScope(),
                            schemaUrl = "https://flaggo.dev/schemas/telemetry/v1",
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
                    schemaUrl = "https://opentelemetry.io/schemas/1.27.0",
                    scopeSpans = new[]
                    {
                        new
                        {
                            scope = InstrumentationScope(),
                            schemaUrl = "https://flaggo.dev/schemas/telemetry/v1",
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
        return request;
    }

    private static object Resource() => new
    {
        attributes = new[]
        {
            Attribute("service.name", "test-application"),
            Attribute("deployment.environment.name", "test-environment"),
            Attribute("service.instance.id", "instance-1")
        }
    };

    private static object InstrumentationScope() => new
    {
        name = "@flaggo/test",
        version = "1.0.0",
        attributes = new[]
        {
            Attribute("scope.attribute", "scope-value")
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

    private static object OutcomeRecord(string decisionId) => new
    {
        eventName = "flaggo.outcome.observed",
        timeUnixNano = "1770000300000000000",
        attributes = new[]
        {
            Attribute("flaggo.signal", "outcome.observed"),
            Attribute("flaggo.evidence.binding", "tetris.survival_ms"),
            Attribute("flaggo.evidence.value.json", "18400"),
            Attribute("flaggo.decision.id", decisionId),
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

    private static JsonElement StoredRecord(EvidenceTelemetryRecord record) =>
        record.Envelope.GetProperty("record");

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
        builder.UseSetting("ConnectionStrings:Flaggo", ConnectionString);
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
