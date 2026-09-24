using System.IO.Compression;
using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Security.Claims;
using System.Text.Encodings.Web;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.DataPlane;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Google.Protobuf;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using OpenTelemetry.Proto.Collector.Logs.V1;
using OpenTelemetry.Proto.Collector.Metrics.V1;
using OpenTelemetry.Proto.Collector.Trace.V1;
using OpenTelemetry.Proto.Common.V1;
using OpenTelemetry.Proto.Logs.V1;
using OpenTelemetry.Proto.Metrics.V1;
using OpenTelemetry.Proto.Trace.V1;
using static Flaggo.Decisioning.Tests.InputEvidenceTestData;

namespace Flaggo.Decisioning.Tests;

public sealed class OtlpHostTests
{
    internal static async Task ExecuteScopeFixtureAsync(JsonElement fixture)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        var request = fixture.GetProperty("request");
        var path = request.GetProperty("path").GetString()!;
        var message = request.GetProperty("protobuf").GetRawText();
        IMessage payload = path.Split('/')[^1] switch
        {
            "metrics" => JsonParser.Default.Parse<ExportMetricsServiceRequest>(message),
            "traces" => JsonParser.Default.Parse<ExportTraceServiceRequest>(message),
            "logs" => JsonParser.Default.Parse<ExportLogsServiceRequest>(message),
            _ => throw new InvalidDataException("Unsupported OTLP fixture route.")
        };
        using var http = new HttpRequestMessage(new HttpMethod(request.GetProperty("method").GetString()!), path)
        {
            Content = new ByteArrayContent(payload.ToByteArray())
        };
        foreach (var header in request.GetProperty("headers").EnumerateObject())
        {
            if (!http.Headers.TryAddWithoutValidation(header.Name, header.Value.GetString()))
                Assert.True(http.Content.Headers.TryAddWithoutValidation(header.Name, header.Value.GetString()));
        }
        using var response = await client.SendAsync(http);
        var expected = fixture.GetProperty("expected");
        Assert.Equal(expected.GetProperty("status").GetInt32(), (int)response.StatusCode);
        foreach (var header in expected.GetProperty("headers").EnumerateObject())
        {
            var values = response.Headers.TryGetValues(header.Name, out var found)
                ? found : response.Content.Headers.GetValues(header.Name);
            Assert.Equal(header.Value.GetString(), Assert.Single(values));
        }
        var status = JsonParser.Default.Parse<Google.Rpc.Status>(expected.GetProperty("protobuf").GetRawText());
        Assert.Equal(status.ToByteArray(), await response.Content.ReadAsByteArrayAsync());
        Assert.Equal("missing", (await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), Binding())).Status);
    }

    [Fact]
    public async Task HostStop_ReleasesWriterLeaseBeforeProviderDisposal()
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        using var response = await SendAsync(client, "metrics", Payload("metrics"));
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);

        var materializer = factory.Services.GetRequiredService<InputEvidenceMaterializer>();
        await factory.Services.GetRequiredService<IHost>().StopAsync();

        Assert.False(materializer.IsInitialized);
        using var lease = new FileStream($"{factory.TelemetryPath}.lock",
            FileMode.Open, FileAccess.ReadWrite, FileShare.None);
    }

    [Theory]
    [InlineData("metrics", false)]
    [InlineData("metrics", true)]
    [InlineData("traces", false)]
    [InlineData("traces", true)]
    [InlineData("logs", false)]
    [InlineData("logs", true)]
    public async Task ProtobufRoute_CommitsNativeObservationBeforeSuccess(string signal, bool gzip)
    {
        using var directory = new TestRegistryFile();
        var binding = BindingFor(signal);
        await using var factory = new OtlpFactory(directory.Path, binding);
        using var client = factory.CreateClient();
        using var response = await SendAsync(client, signal, Payload(signal), gzip: gzip);
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal("application/x-protobuf", response.Content.Headers.ContentType!.MediaType);
        var bytes = await response.Content.ReadAsByteArrayAsync();
        switch (signal)
        {
            case "metrics": Assert.Null(ExportMetricsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess); break;
            case "traces": Assert.Null(ExportTraceServiceResponse.Parser.ParseFrom(bytes).PartialSuccess); break;
            case "logs": Assert.Null(ExportLogsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess); break;
        }
        var materialized = await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), binding);
        Assert.Equal("available", materialized.Status);
        Assert.Equal(0.75, materialized.Value!.Value.GetDouble());
        Assert.Equal("observed", materialized.Provenance.Coverage);
        Assert.True(File.Exists(factory.TelemetryPath));
    }

    [Theory]
    [InlineData("reordered-metadata", "available")]
    [InlineData("different-bytes", "ambiguous")]
    [InlineData("different-types", "ambiguous")]
    public async Task GaugeStreamIdentity_PreservesNativeTypesAndIgnoresAttributeOrder(string mutation, string expected)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        factory.Clock.Now = ObservedAt.AddTicks(1);
        for (var index = 0; index < 2; index++)
        {
            var payload = MetricPayloadAt(Timestamp + (ulong)index * 100, index == 0 ? 0.25 : 0.75);
            AnyValue identity = mutation switch
            {
                "different-bytes" => new() { BytesValue = ByteString.CopyFrom([(byte)index]) },
                "different-types" => index == 0 ? new() { IntValue = 1 } : new() { DoubleValue = 1 },
                _ => new()
                {
                    KvlistValue = new()
                    {
                        Values = { (index == 0 ? new[] { "a", "b" } : ["b", "a"])
                            .Select(key => new KeyValue { Key = key, Value = new() { StringValue = key } }) }
                    }
                }
            };
            payload.ResourceMetrics[0].ScopeMetrics[0].Metrics[0].Gauge.DataPoints[0].Attributes.Add(
                new KeyValue { Key = "identity", Value = identity });
            using var response = await SendAsync(client, "metrics", payload.ToByteArray());
            Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        }
        var value = await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), Binding(), factory.Clock.Now);
        Assert.Equal(expected, value.Status);
        if (expected == "available") Assert.Equal(0.75, value.Value!.Value.GetDouble());
        else Assert.Null(value.Value);
    }

    [Theory]
    [InlineData("none", "worker", "dev", 401)]
    [InlineData("decide", "worker", "dev", 403)]
    [InlineData("ingest", "foreign", "dev", 403)]
    [InlineData("ingest", "worker", "prod", 403)]
    [InlineData("no-tenant-ingest", "worker", "dev", 403)]
    public async Task IngestAuthentication_IsSeparateAndScopeIsNotAuthorizedByResourceAttributes(
        string token, string app, string environment, int status)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        using var response = await SendAsync(client, "metrics", Payload("metrics"), token, app: app, environment: environment);
        Assert.Equal(status, (int)response.StatusCode);
        var error = Google.Rpc.Status.Parser.ParseFrom(await response.Content.ReadAsByteArrayAsync());
        Assert.Equal(status == 401 ? 16 : 7, error.Code);
        Assert.NotEmpty(error.Message);
        Assert.Equal("missing", (await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), Binding())).Status);
    }

    [Theory]
    [InlineData("json", 415)]
    [InlineData("encoding", 415)]
    [InlineData("protobuf", 400)]
    [InlineData("gzip", 400)]
    [InlineData("duplicate-attributes", 400)]
    [InlineData("duplicate-scope-attributes", 400)]
    public async Task MalformedTransport_ReturnsProtobufStatus(string invalid, int status)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        var bytes = Payload("metrics");
        if (invalid == "protobuf") bytes = [0xff];
        if (invalid is "duplicate-attributes" or "duplicate-scope-attributes")
        {
            var request = ExportMetricsServiceRequest.Parser.ParseFrom(bytes);
            var scope = request.ResourceMetrics[0].ScopeMetrics[0];
            var attributes = invalid == "duplicate-attributes"
                ? scope.Metrics[0].Gauge.DataPoints[0].Attributes : scope.Scope.Attributes;
            attributes.Add(new KeyValue { Key = "duplicate", Value = new() { IntValue = 1 } });
            attributes.Add(new KeyValue { Key = "duplicate", Value = new() { IntValue = 2 } });
            bytes = request.ToByteArray();
        }
        using var message = new HttpRequestMessage(HttpMethod.Post, "/otlp/worker/dev/v1/metrics");
        message.Headers.Authorization = new("Bearer", "ingest");
        message.Content = new ByteArrayContent(bytes);
        message.Content.Headers.ContentType = new(invalid == "json" ? "application/json" : "application/x-protobuf");
        if (invalid is "encoding" or "gzip") message.Content.Headers.ContentEncoding.Add(invalid == "gzip" ? "gzip" : "br");
        using var response = await client.SendAsync(message);
        Assert.Equal(status, (int)response.StatusCode);
        Assert.Equal(3, Google.Rpc.Status.Parser.ParseFrom(await response.Content.ReadAsByteArrayAsync()).Code);
    }

    [Theory]
    [InlineData("encoded-chunked")]
    [InlineData("decompressed")]
    [InlineData("records")]
    public async Task Bounds_RejectRequestWithoutPublishingPartialWork(string limit)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding(),
            maximumBytes: limit == "records" ? 4096 : 256, maximumRecords: 1);
        using var client = factory.CreateClient();
        var payload = limit == "records" ? MetricPayload(0.25, 0.75).ToByteArray() : new byte[1024];
        if (limit == "encoded-chunked") Random.Shared.NextBytes(payload);
        using var content = new StreamContent(new MemoryStream(Compress(payload)));
        content.Headers.ContentType = new("application/x-protobuf");
        content.Headers.ContentEncoding.Add("gzip");
        using var request = new HttpRequestMessage(HttpMethod.Post, "/otlp/worker/dev/v1/metrics") { Content = content };
        request.Headers.Authorization = new("Bearer", "ingest");
        request.Headers.TransferEncodingChunked = true;
        using var response = await client.SendAsync(request);
        Assert.Equal(HttpStatusCode.RequestEntityTooLarge, response.StatusCode);
        Assert.Equal(8, Google.Rpc.Status.Parser.ParseFrom(await response.Content.ReadAsByteArrayAsync()).Code);
        Assert.Equal("missing", (await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), Binding())).Status);
    }

    [Theory]
    [InlineData("metrics")]
    [InlineData("traces")]
    [InlineData("logs")]
    public async Task InvalidMatchedRecord_HasSignalSpecificPartialSuccess(string signal)
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, BindingFor(signal));
        using var client = factory.CreateClient();
        using var response = await SendAsync(client, signal, Payload(signal, value: 5));
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var bytes = await response.Content.ReadAsByteArrayAsync();
        var (rejected, diagnostics) = signal switch
        {
            "metrics" => (ExportMetricsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.RejectedDataPoints,
                ExportMetricsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.ErrorMessage),
            "traces" => (ExportTraceServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.RejectedSpans,
                ExportTraceServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.ErrorMessage),
            _ => (ExportLogsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.RejectedLogRecords,
                ExportLogsServiceResponse.Parser.ParseFrom(bytes).PartialSuccess.ErrorMessage)
        };
        Assert.Equal(1, rejected);
        Assert.Contains("invalid-value", diagnostics);
    }

    [Theory]
    [InlineData(false, "available")]
    [InlineData(true, "invalid-value")]
    public async Task LogBodySelection_RequiresStructureRatherThanParsingJsonText(bool text, string expected)
    {
        using var directory = new TestRegistryFile();
        var binding = BindingFor("logs");
        binding = binding with { Source = binding.Source with { ValueFrom = "body", ValueKey = null, BodyPath = ["pressure"] } };
        await using var factory = new OtlpFactory(directory.Path, binding);
        using var client = factory.CreateClient();
        var payload = ExportLogsServiceRequest.Parser.ParseFrom(Payload("logs"));
        payload.ResourceLogs[0].ScopeLogs[0].LogRecords[0].Body = text
            ? new() { StringValue = "{\"pressure\":0.25}" }
            : new()
            {
                KvlistValue = new()
                {
                    Values = { new KeyValue { Key = "pressure", Value = new() { DoubleValue = 0.25 } } }
                }
            };
        using var response = await SendAsync(client, "logs", payload.ToByteArray());
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var exported = ExportLogsServiceResponse.Parser.ParseFrom(await response.Content.ReadAsByteArrayAsync());
        Assert.Equal(text ? 1 : 0, exported.PartialSuccess?.RejectedLogRecords ?? 0);
        Assert.Equal(expected, (await ReadAsync(factory.Services.GetRequiredService<IInputEvidenceReader>(), binding)).Status);
    }

    [Theory]
    [InlineData("storage", 503)]
    [InlineData("capacity", 429)]
    public async Task PublicationFailure_IsRetryableAndNeverAcknowledged(string failure, int status)
    {
        using var directory = new TestRegistryFile();
        var store = new MemoryEvidenceStore { Failure = failure == "storage"
            ? new IOException("Disk unavailable.") : new InputEvidenceCapacityException("Full.") };
        await using var factory = new OtlpFactory(directory.Path, Binding(), store: store);
        using var client = factory.CreateClient();
        using var response = await SendAsync(client, "metrics", Payload("metrics"));
        Assert.Equal(status, (int)response.StatusCode);
        Assert.Equal("1", response.Headers.RetryAfter!.ToString());
        Assert.Equal(status == 503 ? 14 : 8, Google.Rpc.Status.Parser.ParseFrom(await response.Content.ReadAsByteArrayAsync()).Code);
        Assert.Equal(0, store.Publications);
    }

    [Fact]
    public async Task KeyOnlyEvidenceDecision_ReplayRetainsResolvedValuesAuditAndExplicitExposure()
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        using var missing = await DecideAsync(client, "evidence-decision");
        Assert.Equal(HttpStatusCode.ServiceUnavailable, missing.StatusCode);
        var unavailable = await missing.Content.ReadFromJsonAsync<JsonElement>();
        Assert.False(unavailable.GetProperty("clientFallback").GetProperty("eligible").GetBoolean());
        Assert.False(missing.Headers.Contains("Idempotency-Key-Expires-At"));

        using var telemetry = await SendAsync(client, "metrics", Payload("metrics"));
        Assert.Equal(HttpStatusCode.OK, telemetry.StatusCode);
        using var first = await DecideAsync(client, "evidence-decision");
        Assert.Equal(HttpStatusCode.OK, first.StatusCode);
        var decision = await first.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(850, decision.GetProperty("value").GetDouble());
        Assert.Equal(JsonValueKind.Null, decision.GetProperty("confidence").ValueKind);
        var audit = factory.Services.GetRequiredService<InMemoryAuditSink>();
        var record = Assert.Single(audit.Records);
        Assert.Empty(record.RequestInputs!);
        Assert.Equal(0.75, record.Inputs["pressure"].GetDouble());
        Assert.Equal("evidence", record.InputProvenance!["pressure"].Source);
        Assert.Equal(new("global", "global", "server-derived"), record.InputProvenance["pressure"].TargetResolution);
        var exposures = factory.Services.GetRequiredService<InMemoryExposureStore>();
        Assert.Null(exposures.Find(record.DecisionId)!.Confirmation);
        Assert.Equal(record.InputProvenance, exposures.Find(record.DecisionId)!.Snapshot.InputProvenance);

        factory.Clock.Now += TimeSpan.FromSeconds(1);
        using var newer = await SendAsync(client, "metrics", MetricPayloadAt(Timestamp + 1_000_000_000, 0.1).ToByteArray());
        Assert.Equal(HttpStatusCode.OK, newer.StatusCode);
        using var replay = await DecideAsync(client, "evidence-decision");
        Assert.Equal(await first.Content.ReadAsStringAsync(), await replay.Content.ReadAsStringAsync());
        Assert.Single(audit.Records);
        using var next = await DecideAsync(client, "new-evidence-decision");
        Assert.Equal(750, (await next.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("value").GetDouble());
        Assert.Equal(2, audit.Records.Count);

        client.DefaultRequestHeaders.Authorization = new("Bearer", "decide");
        var confirm = new { confirmToken = decision.GetProperty("exposure").GetProperty("confirmToken").GetString() };
        using var confirmation = await client.PostAsJsonAsync($"/v1/exposures/{record.DecisionId}:confirm", confirm);
        using var confirmationReplay = await client.PostAsJsonAsync($"/v1/exposures/{record.DecisionId}:confirm", confirm);
        Assert.Equal(HttpStatusCode.OK, confirmation.StatusCode);
        Assert.Equal(await confirmation.Content.ReadAsStringAsync(), await confirmationReplay.Content.ReadAsStringAsync());
        Assert.Single(audit.ExposureRecords);
    }

    [Fact]
    public async Task CohortEvidence_RetainsAuthoritativeTargetInDurableAuditAndExposureOutsideStateResolution()
    {
        using var directory = new TestRegistryFile();
        var binding = Binding() with { TargetType = "cohort", TargetIdAttribute = new("attributes", "cohort.id") };
        var definition = Definition(binding) with
        {
            RuntimeContext = [new("sessionId", "string", true, "session"), new("cohortId", "string", true, "cohort")],
            TargetHierarchy = ["session", "cohort", "global"], InferenceTarget = "session", FallbackOrder = ["global"]
        };
        await using var factory = new OtlpFactory(directory.Path, binding, definition: definition,
            targetResolver: new DefaultTargetResolver(new Dictionary<string, string> { ["client-claim"] = "actual-cohort" }));
        using var client = factory.CreateClient();
        var payload = MetricPayload();
        payload.ResourceMetrics[0].ScopeMetrics[0].Metrics[0].Gauge.DataPoints.Add(new NumberDataPoint
        {
            TimeUnixNano = Timestamp, AsDouble = 0.75,
            Attributes = { new KeyValue { Key = "cohort.id", Value = new() { StringValue = "actual-cohort" } } }
        });
        using var telemetry = await SendAsync(client, "metrics", payload.ToByteArray());
        Assert.Equal(HttpStatusCode.OK, telemetry.StatusCode);
        using var response = await DecideAsync(client, "cohort-input", runtimeContext: new { sessionId = "session-1", cohortId = "client-claim" });
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var result = await response.Content.ReadFromJsonAsync<ServerDecisionResult>();
        Assert.Equal(850, result!.Value.GetDouble());
        Assert.DoesNotContain(result.TargetProvenance, target => target.TargetType == "cohort");
        var record = Assert.Single(factory.Services.GetRequiredService<InMemoryAuditSink>().Records);
        var expected = new TargetResolutionProvenance("cohort", "actual-cohort", "server-replaced", "client-claim");
        Assert.Equal(expected, record.InputProvenance!["pressure"].TargetResolution);
        var exposure = factory.Services.GetRequiredService<InMemoryExposureStore>().Find(record.DecisionId)!;
        Assert.Equal(expected, exposure.Snapshot.InputProvenance!["pressure"].TargetResolution);
        var auditPath = Path.Combine(Path.GetDirectoryName(directory.Path)!, "audit");
        using (var sink = new LocalFileAuditSink(new(auditPath)))
            await sink.RecordDecisionAsync(record, CancellationToken.None);
        using (var reopened = new LocalFileAuditSink(new(auditPath)))
            Assert.True(await reopened.IsAvailableAsync(CancellationToken.None));
        var segment = Assert.Single(Directory.GetFiles(Path.Combine(auditPath + ".d", "segments")));
        var persisted = (await File.ReadAllLinesAsync(segment))
            .Select(line => JsonSerializer.Deserialize<JsonElement>(line))
            .Single(line => line.GetProperty("kind").GetString() == "decision");
        var target = persisted.GetProperty("record").GetProperty("inputProvenance")
            .GetProperty("pressure").GetProperty("targetResolution");
        Assert.Equal("actual-cohort", target.GetProperty("resolvedId").GetString());
        Assert.Equal("client-claim", target.GetProperty("claimedId").GetString());
        Assert.Equal("server-replaced", target.GetProperty("source").GetString());
    }

    [Fact]
    public async Task EvidenceOwnedInputOverride_IsRejectedEvenWhenFrameIsAvailable()
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        using var telemetry = await SendAsync(client, "metrics", Payload("metrics"));
        using var response = await DecideAsync(client, "override", new { pressure = 0.1 });
        Assert.Equal(HttpStatusCode.UnprocessableEntity, response.StatusCode);
        var error = await response.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal("input-source-conflict", error.GetProperty("issues")[0].GetProperty("code").GetString());
        Assert.Empty(factory.Services.GetRequiredService<InMemoryAuditSink>().Records);
    }

    [Fact]
    public async Task AuthenticatedTenant_IsolatesEvidenceReplayAndExposureConfirmation()
    {
        using var directory = new TestRegistryFile();
        await using var factory = new OtlpFactory(directory.Path, Binding());
        using var client = factory.CreateClient();
        using var telemetry = await SendAsync(client, "metrics", Payload("metrics"));
        Assert.Equal(HttpStatusCode.OK, telemetry.StatusCode);
        using var first = await DecideAsync(client, "shared-key");
        Assert.Equal(HttpStatusCode.OK, first.StatusCode);
        var decision = await first.Content.ReadFromJsonAsync<JsonElement>();
        using var foreignMissing = await DecideAsync(client, "shared-key", token: "foreign-decide");
        Assert.Equal(HttpStatusCode.ServiceUnavailable, foreignMissing.StatusCode);

        using var foreignTelemetry = await SendAsync(client, "metrics", Payload("metrics", 0.25), token: "foreign-ingest");
        Assert.Equal(HttpStatusCode.OK, foreignTelemetry.StatusCode);
        using var foreignDecision = await DecideAsync(client, "shared-key", token: "foreign-decide");
        Assert.Equal(750, (await foreignDecision.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("value").GetDouble());
        using var replay = await DecideAsync(client, "shared-key");
        Assert.Equal(await first.Content.ReadAsStringAsync(), await replay.Content.ReadAsStringAsync());
        var audit = factory.Services.GetRequiredService<InMemoryAuditSink>();
        Assert.Equal(new[] { "tenant-a", "tenant-b" }, audit.Records.Select(record => record.TenantId));

        var confirmUrl = $"/v1/exposures/{decision.GetProperty("decisionId").GetString()}:confirm";
        var confirm = new { confirmToken = decision.GetProperty("exposure").GetProperty("confirmToken").GetString() };
        client.DefaultRequestHeaders.Authorization = new("Bearer", "foreign-decide");
        using var foreignConfirmation = await client.PostAsJsonAsync(confirmUrl, confirm);
        Assert.Equal(HttpStatusCode.NotFound, foreignConfirmation.StatusCode);
        Assert.Empty(audit.ExposureRecords);
        client.DefaultRequestHeaders.Authorization = new("Bearer", "decide");
        using var confirmation = await client.PostAsJsonAsync(confirmUrl, confirm);
        Assert.Equal(HttpStatusCode.OK, confirmation.StatusCode);
        client.DefaultRequestHeaders.Authorization = new("Bearer", "foreign-decide");
        using var foreignReplay = await client.PostAsJsonAsync(confirmUrl, confirm);
        Assert.Equal(HttpStatusCode.NotFound, foreignReplay.StatusCode);
        Assert.Single(audit.ExposureRecords);
    }

    private static Task<HttpResponseMessage> DecideAsync(HttpClient client, string key, object? inputs = null, string token = "decide",
        object? runtimeContext = null)
    {
        var request = new HttpRequestMessage(HttpMethod.Post, "/v1/decisions/worker.limit:decide");
        request.Headers.Authorization = new("Bearer", token);
        request.Headers.Add("Idempotency-Key", key);
        request.Content = JsonContent.Create(new
        {
            expectedContract = new { Identity.DefinitionId, Identity.ContractDigest, Identity.Revision },
            runtimeContext = runtimeContext ?? new { }, inputs = inputs ?? new { },
            client = new { appId = Scope.AppId, environment = Scope.Environment }
        });
        return SendAndDisposeAsync(client, request);
    }

    private static Task<HttpResponseMessage> SendAsync(HttpClient client, string signal, byte[] payload,
        string token = "ingest", bool gzip = false, string app = "worker", string environment = "dev")
    {
        var request = new HttpRequestMessage(HttpMethod.Post, $"/otlp/{app}/{environment}/v1/{signal}");
        request.Headers.Authorization = new("Bearer", token);
        request.Content = new ByteArrayContent(gzip ? Compress(payload) : payload);
        request.Content.Headers.ContentType = new("application/x-protobuf");
        if (gzip) request.Content.Headers.ContentEncoding.Add("gzip");
        return SendAndDisposeAsync(client, request);
    }

    private static async Task<HttpResponseMessage> SendAndDisposeAsync(HttpClient client, HttpRequestMessage request)
    {
        using (request) return await client.SendAsync(request);
    }

    private static byte[] Compress(byte[] bytes)
    {
        using var output = new MemoryStream();
        using (var gzip = new GZipStream(output, CompressionLevel.SmallestSize, leaveOpen: true)) gzip.Write(bytes);
        return output.ToArray();
    }

    private static RegisteredEvidenceBinding BindingFor(string signal) => signal == "metrics" ? Binding() :
        Binding() with { Source = Binding().Source with
        {
            Kind = signal == "traces" ? "span" : "log",
            Name = signal == "traces" ? "work.pressure" : null,
            EventName = signal == "logs" ? "work.completed" : null,
            ValueFrom = "attributes", ValueKey = "pressure"
        }};

    private static byte[] Payload(string signal, double value = 0.75)
    {
        if (signal == "metrics") return MetricPayload(value).ToByteArray();
        if (signal == "traces")
        {
            var request = new ExportTraceServiceRequest();
            var scope = new ScopeSpans { Scope = new() { Name = "worker", Version = "1" } };
            scope.Spans.Add(new Span
            {
                Name = "work.pressure", StartTimeUnixNano = Timestamp - 1000, EndTimeUnixNano = Timestamp,
                TraceId = ByteString.CopyFrom(new byte[16].Select(_ => (byte)1).ToArray()),
                SpanId = ByteString.CopyFrom(new byte[8].Select(_ => (byte)2).ToArray()),
                Flags = 1, Attributes = { new KeyValue { Key = "pressure", Value = new() { DoubleValue = value } } }
            });
            request.ResourceSpans.Add(new ResourceSpans { ScopeSpans = { scope } });
            return request.ToByteArray();
        }
        var logs = new ExportLogsServiceRequest();
        var scopeLogs = new ScopeLogs { Scope = new() { Name = "worker", Version = "1" } };
        scopeLogs.LogRecords.Add(new LogRecord
        {
            TimeUnixNano = Timestamp, EventName = "work.completed",
            Body = new() { StringValue = "Application-owned outcome" },
            Attributes = { new KeyValue { Key = "pressure", Value = new() { DoubleValue = value } } }
        });
        logs.ResourceLogs.Add(new ResourceLogs { ScopeLogs = { scopeLogs } });
        return logs.ToByteArray();
    }

    private static ExportMetricsServiceRequest MetricPayload(params double[] values) => MetricPayloadAt(Timestamp, values);

    private static ExportMetricsServiceRequest MetricPayloadAt(ulong timestamp, params double[] values)
    {
        var request = new ExportMetricsServiceRequest();
        var metric = new Metric { Name = "work.pressure", Unit = "1", Gauge = new Gauge() };
        foreach (var value in values) metric.Gauge.DataPoints.Add(new NumberDataPoint { TimeUnixNano = timestamp, AsDouble = value });
        request.ResourceMetrics.Add(new ResourceMetrics
        {
            Resource = new() { Attributes = { new KeyValue { Key = "app.id", Value = new() { StringValue = "foreign" } } } },
            ScopeMetrics = { new ScopeMetrics { Scope = new() { Name = "worker", Version = "1" }, Metrics = { metric } } }
        });
        return request;
    }

    private sealed class OtlpFactory(string registryPath, RegisteredEvidenceBinding binding,
        int maximumBytes = 4096, int maximumRecords = 100, IInputEvidenceSnapshotStore? store = null,
        RuntimeDecisionDefinition? definition = null, ITargetResolver? targetResolver = null)
        : WebApplicationFactory<DataPlaneAssemblyMarker>
    {
        public EvidenceClock Clock { get; } = new();
        public string TelemetryPath { get; } = Path.Combine(Path.GetDirectoryName(registryPath)!, "telemetry", "current.commit.json");
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting("Flaggo:Authentication:LocalDevelopmentBypass", "true");
            builder.UseSetting("Flaggo:Registry:LocalFilePath", registryPath);
            builder.UseSetting("Flaggo:Telemetry:CommitDescriptorPath", TelemetryPath);
            builder.UseSetting("Flaggo:Telemetry:MaximumBodyBytes", maximumBytes.ToString());
            builder.UseSetting("Flaggo:Telemetry:MaximumRecords", maximumRecords.ToString());
            builder.ConfigureTestServices(services =>
            {
                var registry = new InMemoryDefinitionRegistry([definition ?? Definition(binding)]);
                services.RemoveAll<IRuntimeDefinitionReader>();
                services.RemoveAll<IEvidenceBindingReader>();
                services.RemoveAll<IRegistryHealth>();
                services.AddSingleton<IRuntimeDefinitionReader>(registry);
                services.AddSingleton<IEvidenceBindingReader>(registry);
                services.AddSingleton<IRegistryHealth>(registry);
                services.RemoveAll<TimeProvider>();
                services.AddSingleton<TimeProvider>(Clock);
                if (targetResolver is not null)
                {
                    services.RemoveAll<ITargetResolver>();
                    services.AddSingleton(targetResolver);
                }
                services.RemoveAll<IStateStore>();
                services.AddSingleton<IStateStore>(new InMemoryStateStore([("worker.limit", new(
                    Identity.DefinitionId, Identity.Revision, Identity.ContractDigest, JsonSerializer.SerializeToElement(800),
                    Global, "strategy", "pressure-rule", new NumericRuleStrategy("pressure", 0.5, 850, 750)))]));
                var audit = new InMemoryAuditSink();
                services.RemoveAll<IAuditSink>();
                services.RemoveAll<IExposureAuditSink>();
                services.AddSingleton(audit);
                services.AddSingleton<IAuditSink>(audit);
                services.AddSingleton<IExposureAuditSink>(audit);
                if (store is not null)
                {
                    services.RemoveAll<IInputEvidenceSnapshotStore>();
                    services.AddSingleton(store);
                }
                services.AddAuthentication("OtlpTest")
                    .AddScheme<AuthenticationSchemeOptions, OtlpAuthentication>("OtlpTest", _ => { });
            });
        }
    }

    private sealed class OtlpAuthentication(IOptionsMonitor<AuthenticationSchemeOptions> options,
        ILoggerFactory logger, UrlEncoder encoder) : AuthenticationHandler<AuthenticationSchemeOptions>(options, logger, encoder)
    {
        protected override Task<AuthenticateResult> HandleAuthenticateAsync()
        {
            var token = Request.Headers.Authorization.ToString();
            if (token is not ("Bearer ingest" or "Bearer decide" or "Bearer foreign-ingest" or "Bearer foreign-decide" or "Bearer no-tenant-ingest"))
                return Task.FromResult(AuthenticateResult.NoResult());
            var claims = new[]
            {
                new Claim("sub", "otel-test"),
                new Claim("polari_tenant_id", token.Contains("no-tenant", StringComparison.Ordinal) ? "" :
                    token.Contains("foreign", StringComparison.Ordinal) ? "tenant-b" : "tenant-a"),
                new Claim("scope", token.EndsWith("ingest", StringComparison.Ordinal) ? "polari.telemetry:ingest" : "polari.decisions:decide polari.exposures:confirm"),
                new Claim("polari_app_id", "worker"), new Claim("polari_environment", "dev")
            };
            return Task.FromResult(AuthenticateResult.Success(new(
                new ClaimsPrincipal(new ClaimsIdentity(claims, Scheme.Name)), Scheme.Name)));
        }
    }
}
