using System.Diagnostics.Metrics;
using System.IO.Compression;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Google.Protobuf;
using Microsoft.Net.Http.Headers;
using OpenTelemetry.Proto.Collector.Logs.V1;
using OpenTelemetry.Proto.Collector.Metrics.V1;
using OpenTelemetry.Proto.Collector.Trace.V1;

namespace Flaggo.DataPlane.Telemetry;

internal sealed record OtlpLimits(int MaximumBodyBytes, int MaximumRecords);

internal static class OtlpHttp
{
    private const string ContentType = "application/x-protobuf";

    public static void AddInputEvidence(IServiceCollection services, IConfiguration configuration, string contentRoot)
    {
        var maxBody = configuration.GetValue("Flaggo:Telemetry:MaximumBodyBytes", 4 * 1024 * 1024);
        var maxRecords = configuration.GetValue("Flaggo:Telemetry:MaximumRecords", 1_000);
        var maxFrames = configuration.GetValue("Flaggo:Telemetry:MaximumFrames", 10_000);
        var maxBindings = configuration.GetValue("Flaggo:Telemetry:MaximumBindings", 1_000);
        var maxSnapshot = configuration.GetValue("Flaggo:Telemetry:MaximumSnapshotBytes", 16 * 1024 * 1024);
        if (maxBody <= 0 || maxRecords <= 0 || maxFrames <= 0 || maxBindings <= 0 || maxSnapshot <= 0)
            throw new InvalidOperationException("Telemetry limits must be positive finite integers.");
        var path = configuration["Flaggo:Telemetry:CommitDescriptorPath"]
            ?? Path.Combine("data", "telemetry", "inputs.commit.json");
        var descriptor = Path.GetFullPath(path, contentRoot);
        services.AddSingleton(new OtlpLimits(maxBody, maxRecords));
        services.AddSingleton<IEvidenceBindingReader>(provider => provider.GetRequiredService<LocalFileDefinitionRegistry>());
        services.AddSingleton<IConfirmedExposureReader>(provider => provider.GetRequiredService<InMemoryExposureStore>());
        services.AddSingleton<IInputEvidenceSnapshotStore>(
            _ => new LocalInputEvidenceStore(new(descriptor, maxFrames, maxSnapshot)));
        services.AddSingleton(provider => new InputEvidenceMaterializer(
            provider.GetRequiredService<IEvidenceBindingReader>(),
            provider.GetRequiredService<IConfirmedExposureReader>(),
            provider.GetRequiredService<IInputEvidenceSnapshotStore>(),
            provider.GetRequiredService<TimeProvider>(),
            new(maxRecords, maxFrames, maxBindings)));
        services.AddSingleton<IInputTelemetrySink>(provider => provider.GetRequiredService<InputEvidenceMaterializer>());
        services.AddSingleton<IInputEvidenceReader>(provider => provider.GetRequiredService<InputEvidenceMaterializer>());
        services.AddSingleton<OtlpCounters>();
        services.AddHostedService<InputEvidenceStartup>();
    }

    public static void MapRoutes(WebApplication app)
    {
        foreach (var signal in new[] { "metrics", "traces", "logs" })
        {
            app.MapPost($"/otlp/{{appId}}/{{environment}}/v1/{signal}",
                async (string appId, string environment, HttpContext context, IInputTelemetrySink sink,
                    OtlpLimits limits, OtlpCounters counters, ILogger<InputEvidenceMaterializer> logger) =>
                {
                    var tenantId = context.User.FindFirst("polari_tenant_id")?.Value;
                    if (string.IsNullOrWhiteSpace(tenantId) || !RuntimeHttp.HasClientScope(context.User, appId, environment))
                    {
                        await WriteErrorAsync(context, 403, "Token is not authorized for this application/environment.");
                        return;
                    }
                    if (!MediaTypeHeaderValue.TryParse(context.Request.ContentType, out var mediaType) ||
                        !string.Equals(mediaType.MediaType.Value, ContentType, StringComparison.OrdinalIgnoreCase))
                    {
                        await WriteErrorAsync(context, 415, "Only binary OTLP/HTTP Protobuf is supported.");
                        return;
                    }
                    var encoding = context.Request.Headers.ContentEncoding.ToString();
                    if (encoding.Length > 0 && !encoding.Equals("gzip", StringComparison.OrdinalIgnoreCase)
                        && !encoding.Equals("identity", StringComparison.OrdinalIgnoreCase))
                    {
                        await WriteErrorAsync(context, 415, "Supported content encodings are gzip and uncompressed.");
                        return;
                    }
                    try
                    {
                        var bytes = await ReadBodyAsync(context, limits.MaximumBodyBytes, encoding);
                        var observations = OtlpDecoder.Decode(signal, bytes, limits.MaximumRecords);
                        var result = await sink.IngestAsync(new ApplicationScope(appId, environment, tenantId), observations, context.RequestAborted);
                        counters.Record(signal, result);
                        var message = string.Join("; ", result.Diagnostics);
                        if (message.Length > 2_048) message = message[..2_048];
                        if (result.Diagnostics.Count > 0)
                            logger.LogWarning("OTLP {Signal} binding diagnostics: {Diagnostics}", signal, message);
                        var partial = result.RejectedRecords > 0 || message.Length > 0;
                        IMessage response = signal switch
                        {
                            "metrics" => new ExportMetricsServiceResponse
                            {
                                PartialSuccess = partial ? new ExportMetricsPartialSuccess
                                { RejectedDataPoints = result.RejectedRecords, ErrorMessage = message } : null
                            },
                            "traces" => new ExportTraceServiceResponse
                            {
                                PartialSuccess = partial ? new ExportTracePartialSuccess
                                { RejectedSpans = result.RejectedRecords, ErrorMessage = message } : null
                            },
                            _ => new ExportLogsServiceResponse
                            {
                                PartialSuccess = partial ? new ExportLogsPartialSuccess
                                { RejectedLogRecords = result.RejectedRecords, ErrorMessage = message } : null
                            }
                        };
                        await WriteAsync(context, 200, response);
                    }
                    catch (OtlpRequestTooLargeException)
                    {
                        await WriteErrorAsync(context, 413, "The OTLP request exceeds its encoded/decompressed byte or work limit.");
                    }
                    catch (Exception error) when (error is InvalidProtocolBufferException or InvalidDataException or System.Text.Json.JsonException)
                    {
                        logger.LogWarning(error, "Rejected malformed OTLP {Signal} data", signal);
                        await WriteErrorAsync(context, 400, "The OTLP payload is malformed.");
                    }
                    catch (InputEvidenceCapacityException error)
                    {
                        logger.LogWarning(error, "Input evidence capacity exceeded");
                        context.Response.Headers.RetryAfter = "1";
                        await WriteErrorAsync(context, 429, "Input evidence capacity is unavailable.");
                    }
                    catch (InputEvidenceUnavailableException error)
                    {
                        logger.LogWarning(error, "Input evidence publication is unavailable");
                        context.Response.Headers.RetryAfter = "1";
                        await WriteErrorAsync(context, 503, "Durable input evidence materialization is unavailable.");
                    }
                })
                .RequireAuthorization("TelemetryIngest")
                .WithMetadata(new DataPlaneOperationMetadata(DataPlaneOperation.TelemetryIngest));
        }
    }

    public static bool IsTelemetry(HttpContext context) => context.Request.Path.StartsWithSegments("/otlp");

    public static Task WriteErrorAsync(HttpContext context, int status, string message) =>
        WriteAsync(context, status, new Google.Rpc.Status
        {
            Code = status switch
            {
                401 => 16,
                403 => 7,
                413 or 429 => 8,
                502 or 503 or 504 => 14,
                >= 500 => 13,
                _ => 3
            },
            Message = message
        });

    private static async Task WriteAsync(HttpContext context, int status, IMessage message)
    {
        var bytes = message.ToByteArray();
        context.Response.StatusCode = status;
        context.Response.ContentType = ContentType;
        context.Response.ContentLength = bytes.Length;
        await context.Response.Body.WriteAsync(bytes, context.RequestAborted);
    }

    private static async Task<byte[]> ReadBodyAsync(HttpContext context, int maximumBytes, string encoding)
    {
        if (context.Request.ContentLength > maximumBytes) throw new OtlpRequestTooLargeException();
        var encoded = await ReadBoundedAsync(context.Request.Body, maximumBytes, context.RequestAborted);
        if (!encoding.Equals("gzip", StringComparison.OrdinalIgnoreCase)) return encoded;
        using var input = new MemoryStream(encoded, writable: false);
        using var gzip = new GZipStream(input, CompressionMode.Decompress);
        return await ReadBoundedAsync(gzip, maximumBytes, context.RequestAborted);
    }

    private static async Task<byte[]> ReadBoundedAsync(Stream source, int maximumBytes, CancellationToken cancellationToken)
    {
        using var output = new MemoryStream();
        var buffer = new byte[16 * 1024];
        int count;
        while ((count = await source.ReadAsync(buffer, cancellationToken)) > 0)
        {
            if (output.Length > maximumBytes - count) throw new OtlpRequestTooLargeException();
            await output.WriteAsync(buffer.AsMemory(0, count), cancellationToken);
        }
        return output.ToArray();
    }

    private sealed class InputEvidenceStartup(
        InputEvidenceMaterializer materializer, ILogger<InputEvidenceStartup> logger) : IHostedService
    {
        public async Task StartAsync(CancellationToken cancellationToken)
        {
            try { await materializer.InitializeAsync(cancellationToken); }
            catch (Exception error) when (error is InputEvidenceUnavailableException or InputEvidenceCapacityException)
            {
                logger.LogError(error, "Input evidence is unavailable; evidence-dependent operations will fail closed.");
            }
        }

        public Task StopAsync(CancellationToken cancellationToken) =>
            materializer.DisposeAsync().AsTask();
    }
}

internal sealed class OtlpCounters : IDisposable
{
    private readonly Meter _meter = new("Flaggo.DataPlane.Telemetry");
    private readonly Counter<long> _accepted;
    private readonly Counter<long> _rejected;
    private readonly Counter<long> _unmatched;

    public OtlpCounters()
    {
        _accepted = _meter.CreateCounter<long>("flaggo.telemetry.accepted");
        _rejected = _meter.CreateCounter<long>("flaggo.telemetry.rejected");
        _unmatched = _meter.CreateCounter<long>("flaggo.telemetry.unmatched");
    }

    public void Record(string signal, TelemetryIngestResult result)
    {
        var tag = KeyValuePair.Create<string, object?>("signal", signal);
        _accepted.Add(result.AcceptedRecords, tag);
        _rejected.Add(result.RejectedRecords, tag);
        _unmatched.Add(result.UnmatchedRecords, tag);
    }

    public void Dispose() => _meter.Dispose();
}
