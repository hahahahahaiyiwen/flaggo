using System.Text.Json;
using Flaggo.Audit;
using Flaggo.DataPlane;
using Flaggo.Decisioning;
using Flaggo.Evidence;
using Flaggo.Hosting;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Authentication.JwtBearer;

var builder = WebApplication.CreateBuilder(args);
var localBypass = builder.Configuration.GetValue<bool>(
    "Flaggo:Authentication:LocalDevelopmentBypass");

builder.Services.ConfigureHttpJsonOptions(options =>
{
    options.SerializerOptions.DefaultIgnoreCondition =
        RuntimeHttp.JsonOptions.DefaultIgnoreCondition;
    options.SerializerOptions.UnmappedMemberHandling =
        RuntimeHttp.JsonOptions.UnmappedMemberHandling;
});

if (localBypass && !builder.Environment.IsDevelopment())
{
    throw new InvalidOperationException(
        "The local-development authentication bypass can only run in the Development environment.");
}

if (localBypass)
{
    builder.Services
        .AddAuthentication("LocalDevelopment")
        .AddScheme<AuthenticationSchemeOptions, LocalDevelopmentAuthenticationHandler>(
            "LocalDevelopment",
            _ => { });
}
else
{
    var authority = builder.Configuration["Flaggo:Authentication:Authority"];
    var audience = builder.Configuration["Flaggo:Authentication:Audience"];
    if (string.IsNullOrWhiteSpace(authority) || string.IsNullOrWhiteSpace(audience))
    {
        throw new InvalidOperationException(
            "OAuth authority and audience are required unless the explicit local-development bypass is enabled.");
    }

    builder.Services
        .AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
        .AddJwtBearer(options =>
        {
            options.Authority = authority;
            options.Audience = audience;
            options.RequireHttpsMetadata = true;
        });
}

builder.Services.AddAuthorization(options =>
{
    options.AddPolicy(
        "Decide",
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context =>
                RuntimeHttp.HasScope(context.User, "polari.decisions:decide")));
    options.AddPolicy(
        "ConfirmExposure",
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context =>
                RuntimeHttp.HasScope(context.User, "polari.exposures:confirm")));
});

var contractIdentity = LocalRegistryHosting.DefaultDefinitions()
    .Single(definition => definition.LifecycleStatus == "active")
    .Identity;

builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton(
    LocalRegistryHosting.CreateDefinitionRegistry(
        builder.Configuration,
        builder.Environment.ContentRootPath,
        TimeProvider.System));
builder.Services.AddSingleton<IRuntimeDefinitionReader>(
    provider => provider.GetRequiredService<LocalFileDefinitionRegistry>());
builder.Services.AddSingleton<IRegistryHealth>(
    provider => provider.GetRequiredService<LocalFileDefinitionRegistry>());
LocalRuntimeAdapterHosting.AddDecisionSnapshotScope(
    builder.Services,
    builder.Configuration);
LocalRuntimeAdapterHosting.AddStateAdapter(
    builder.Services,
    builder.Configuration,
    contractIdentity);
LocalRuntimeAdapterHosting.AddAuditAdapter(
    builder.Services,
    builder.Configuration);
LocalRuntimeAdapterHosting.AddEvidenceAdapter(
    builder.Services,
    builder.Configuration);
builder.Services.AddSingleton<IRuntimeIdGenerator, GuidRuntimeIdGenerator>();
builder.Services.AddSingleton<IDecideIdempotencyStore>(provider =>
    new InMemoryDecideIdempotencyStore(provider.GetRequiredService<TimeProvider>()));
builder.Services.AddSingleton<IExposureStore>(provider =>
{
    var ids = provider.GetRequiredService<IRuntimeIdGenerator>();
    return new InMemoryExposureStore(
        provider.GetRequiredService<TimeProvider>(),
        ids.CreateExposureId);
});
builder.Services.AddSingleton<IPostAuditExposureCommitPolicy>(provider =>
    new BoundedPostAuditExposureCommitPolicy(
        TimeSpan.FromSeconds(5),
        provider.GetRequiredService<TimeProvider>(),
        provider.GetRequiredService<IHostApplicationLifetime>()
            .ApplicationStopping,
        provider.GetRequiredService<
            ILogger<BoundedPostAuditExposureCommitPolicy>>()));
builder.Services.AddSingleton<
    IExposureConfirmationService,
    ExposureConfirmationService>();
builder.Services.AddSingleton<ITargetResolver>(
    new DefaultTargetResolver(
        LocalTargetingHosting.CreateAuthoritativeCohorts(
            builder.Configuration)));
builder.Services.AddSingleton<IStrategyExecutor, DeterministicStrategyExecutor>();
builder.Services.AddSingleton<IPolicyEvaluator>(provider =>
    new DefaultPolicyEvaluator(provider.GetRequiredService<TimeProvider>()));
builder.Services.AddScoped<DecisionService>();
builder.Services.AddScoped<IRuntimeReadinessProbe, RuntimeReadinessProbe>();

var app = builder.Build();

app.Use(async (context, next) =>
{
    var correlationId = context.Request.Headers["X-Flaggo-Correlation-Id"].FirstOrDefault();
    correlationId = string.IsNullOrWhiteSpace(correlationId)
        ? context.TraceIdentifier
        : correlationId;
    RuntimeHttp.SetCorrelationId(context, correlationId);
    await next(context);
});
app.UseExceptionHandler(errorApp => errorApp.Run(async context =>
{
    RuntimeHttp.RestoreCorrelationId(context);
    var exception = context.Features
        .Get<Microsoft.AspNetCore.Diagnostics.IExceptionHandlerFeature>()
        ?.Error;
    var operation = DataPlaneOperationMetadata.Get(context);
    var availabilityFailure = exception is IOException or TimeoutException;
    var clientFallbackEligible =
        operation == DataPlaneOperation.Decide &&
        exception is TimeoutException;
    var status = availabilityFailure ? 503 : 500;
    var code = availabilityFailure ? "service-unavailable" : "internal-error";
    var detail = availabilityFailure
        ? "The data plane is temporarily unavailable."
        : "The data plane encountered an unexpected error.";
    context.Response.StatusCode = status;
    context.Response.ContentType = "application/problem+json";
    if (availabilityFailure)
    {
        context.Response.Headers.RetryAfter = "1";
    }

    var problem = RuntimeHttp.Problem(
        context,
        status,
        code,
        detail,
        retryAfterSeconds: availabilityFailure ? 1 : null,
        clientFallback: availabilityFailure
            ? new ClientFallbackEligibility(
                clientFallbackEligible,
                exception is TimeoutException
                    ? "service-unavailable"
                    : "unclassified-io-failure")
            : null);
    await context.Response.WriteAsync(
        JsonSerializer.Serialize(problem, RuntimeHttp.JsonOptions));
}));
app.UseRouting();
app.Use(async (context, next) =>
{
    DataPlaneOperationMetadata.Capture(context);
    await next(context);
});
app.UseStatusCodePages(async statusContext =>
{
    var context = statusContext.HttpContext;
    var (code, detail) = context.Response.StatusCode switch
    {
        401 => ("authentication-required", "A valid bearer token is required."),
        403 => ("insufficient-scope", "The caller lacks the required operation scope."),
        _ => ("http-error", "The request could not be completed.")
    };
    context.Response.ContentType = "application/problem+json";
    await context.Response.WriteAsync(
        JsonSerializer.Serialize(
            RuntimeHttp.Problem(context, context.Response.StatusCode, code, detail),
            RuntimeHttp.JsonOptions));
});
app.UseAuthentication();
app.UseAuthorization();

app.MapPost(
        "/v1/decisions/{decisionKey}:decide",
        async (
            string decisionKey,
            HttpContext context,
            DecisionService decisionService,
            IDecideIdempotencyStore idempotencyStore,
            CancellationToken cancellationToken) =>
        {
            var parsed = await RuntimeHttp.ReadJsonAsync<DecideRequest>(
                context,
                cancellationToken);
            if (parsed.Error is not null)
            {
                return RuntimeHttp.ProblemResult(context, parsed.Error);
            }

            var request = parsed.Value;
            if (request is null)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        400,
                        "malformed-json",
                        "Request body must be a JSON object."));
            }

            if (request.Client is not null &&
                !string.IsNullOrWhiteSpace(request.Client.AppId) &&
                !string.IsNullOrWhiteSpace(request.Client.Environment) &&
                !RuntimeHttp.HasClientScope(
                    context.User,
                    request.Client.AppId,
                    request.Client.Environment))
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        403,
                        "scope-mismatch",
                        "Token is not authorized for the declared application/environment."));
            }

            try
            {
                var idempotencyKey = context.Request.Headers["Idempotency-Key"].FirstOrDefault();
                if (string.IsNullOrWhiteSpace(idempotencyKey))
                {
                    var outcome = await EvaluateDecisionAsync(
                        decisionKey,
                        request,
                        decisionService,
                        cancellationToken);
                    return RenderOutcome(context, outcome);
                }

                var tenantId = context.User.FindFirst("polari_tenant_id")?.Value;
                if (string.IsNullOrWhiteSpace(tenantId))
                {
                    return RuntimeHttp.ProblemResult(
                        context,
                        RuntimeHttp.Problem(
                            context,
                            403,
                            "scope-mismatch",
                            "Token is not authorized for a tenant resource scope."));
                }

                var namespaceApp = request.Client?.AppId ?? "invalid";
                var namespaceEnvironment = request.Client?.Environment ?? "invalid";
                var idempotencyNamespace =
                    $"{tenantId}/{namespaceApp}/{namespaceEnvironment}/decide/{decisionKey}";
                var retained = await idempotencyStore.ExecuteAsync(
                    idempotencyNamespace,
                    idempotencyKey,
                    RuntimeHttp.Fingerprint(parsed.Body, decisionKey),
                    token => EvaluateDecisionAsync(decisionKey, request, decisionService, token),
                    cancellationToken);
                if (retained.ExpiresAt is { } expiresAt)
                {
                    context.Response.Headers["Idempotency-Key-Expires-At"] =
                        expiresAt.UtcDateTime.ToString("O");
                }

                return RenderOutcome(context, retained.Outcome);
            }
            catch (IdempotencyConflictException)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        409,
                        "idempotency-conflict",
                        "The idempotency key was reused with a different canonical request."));
            }
            catch (IdempotencyInProgressException)
            {
                context.Response.Headers.RetryAfter = "1";
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        409,
                        "idempotency-in-progress",
                        "A matching request is still executing; retry the same key.",
                        retryAfterSeconds: 1));
            }
            catch (JsonException)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        422,
                        "invalid-runtime-context",
                        "Numeric request values must be finite IEEE-754 values."));
            }
        })
    .RequireAuthorization("Decide")
    .WithMetadata(
        new DataPlaneOperationMetadata(DataPlaneOperation.Decide));

app.MapPost(
        "/v1/exposures/{decisionId}:confirm",
        async (
            string decisionId,
            HttpContext context,
            IExposureStore exposureStore,
            IExposureConfirmationService exposureConfirmationService,
            TimeProvider timeProvider,
            CancellationToken cancellationToken) =>
        {
            var parsed = await RuntimeHttp.ReadJsonAsync<ExposureConfirmationRequest>(
                context,
                cancellationToken);
            if (parsed.Error is not null)
            {
                return RuntimeHttp.ProblemResult(context, parsed.Error);
            }

            var request = parsed.Value;
            if (request is null || string.IsNullOrWhiteSpace(request.ConfirmToken))
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "exposure-not-found",
                        "The decision or confirmation capability was not found."));
            }

            var appIds = context.User.FindAll("polari_app_id")
                .Select(claim => claim.Value)
                .ToHashSet(StringComparer.Ordinal);
            var environments = context.User.FindAll("polari_environment")
                .Select(claim => claim.Value)
                .ToHashSet(StringComparer.Ordinal);
            if (appIds.Count == 0 || environments.Count == 0)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "exposure-not-found",
                        "The decision or confirmation capability was not found."));
            }

            try
            {
                var replay = await exposureStore.FindReplayAsync(
                    decisionId,
                    request,
                    appIds,
                    environments,
                    cancellationToken);
                if (replay is null && request.AppliedAt is not null)
                {
                    var now = timeProvider.GetUtcNow();
                    var validTimestamp =
                        System.Text.RegularExpressions.Regex.IsMatch(
                            request.AppliedAt,
                            "\\A\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d{1,9})?Z\\z",
                            System.Text.RegularExpressions.RegexOptions.CultureInvariant) &&
                        DateTimeOffset.TryParse(
                            request.AppliedAt,
                            System.Globalization.CultureInfo.InvariantCulture,
                            System.Globalization.DateTimeStyles.AssumeUniversal |
                            System.Globalization.DateTimeStyles.AdjustToUniversal,
                            out var appliedAt) &&
                        appliedAt >= now.Subtract(TimeSpan.FromMinutes(5)) &&
                        appliedAt <= now.Add(TimeSpan.FromMinutes(5));
                    if (!validTimestamp)
                    {
                        return RuntimeHttp.ProblemResult(
                            context,
                            RuntimeHttp.Problem(
                                context,
                                422,
                                "invalid-applied-at",
                                "appliedAt is outside accepted clock-skew bounds."));
                    }
                }

                var outcome = await exposureConfirmationService.ConfirmAsync(
                    decisionId,
                    request,
                    appIds,
                    environments,
                    cancellationToken);
                return Results.Json(outcome.Result, RuntimeHttp.JsonOptions);
            }
            catch (ExposureNotFoundException)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "exposure-not-found",
                        "The decision or confirmation capability was not found."));
            }
            catch (ExposureConfirmationConflictException)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        409,
                        "exposure-confirmation-conflict",
                        "The decision was already confirmed with a different observation."));
            }
            catch (ExposureAuditConflictException)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        409,
                        "exposure-confirmation-conflict",
                        "The decision was already confirmed with a different observation."));
            }
        })
    .RequireAuthorization("ConfirmExposure")
    .WithMetadata(
        new DataPlaneOperationMetadata(
            DataPlaneOperation.ExposureConfirmation));

app.MapGet(
    "/health/live",
    (TimeProvider timeProvider) => Results.Json(
        new
        {
            status = "live",
            service = "flaggo",
            version = "0.1.0",
            observedAt = timeProvider.GetUtcNow().UtcDateTime.ToString("O")
        },
        RuntimeHttp.JsonOptions));

app.MapGet(
    "/health/ready",
    async (
        TimeProvider timeProvider,
        IRuntimeReadinessProbe readiness,
        CancellationToken cancellationToken) =>
    {
        var checks = await readiness.CheckAsync(cancellationToken);
        var status = checks.Any(check => check.Required && check.Status != "up")
            ? "not-ready"
            : checks.Any(check => check.Status != "up")
                ? "degraded"
                : "ready";
        return Results.Json(
            new RuntimeReadinessResult(
                status,
                timeProvider.GetUtcNow().UtcDateTime.ToString("O"),
                checks),
            RuntimeHttp.JsonOptions,
            statusCode: status == "not-ready" ? 503 : 200);
    });

app.Run();

static async Task<DecideTerminalOutcome> EvaluateDecisionAsync(
    string decisionKey,
    DecideRequest request,
    DecisionService decisionService,
    CancellationToken cancellationToken)
{
    var validationFailure = ValidateDecideRequest(request);
    if (validationFailure is not null)
    {
        return DecideTerminalOutcome.Rejected(validationFailure);
    }

    try
    {
        return DecideTerminalOutcome.Success(
            await decisionService.DecideAsync(decisionKey, request, cancellationToken));
    }
    catch (DecisionContractException error)
    {
        return DecideTerminalOutcome.Rejected(
            new DecisionFailure(
                error.Status,
                error.Code,
                error.Message,
                error.Issues,
                error.ClientFallback,
                error.RetryAfterSeconds));
    }
}

static DecisionFailure? ValidateDecideRequest(DecideRequest request)
{
    if (request.Client is null ||
        string.IsNullOrWhiteSpace(request.Client.AppId) ||
        string.IsNullOrWhiteSpace(request.Client.Environment))
    {
        return new DecisionFailure(
            422,
            "invalid-runtime-context",
            "client appId and environment are required.");
    }

    if (request.ExpectedContract is null ||
        string.IsNullOrWhiteSpace(request.ExpectedContract.DefinitionId) ||
        string.IsNullOrWhiteSpace(request.ExpectedContract.Revision) ||
        string.IsNullOrWhiteSpace(request.ExpectedContract.ContractDigest))
    {
        return new DecisionFailure(
            400,
            "missing-contract-identity",
            "expectedContract with definitionId, revision, and contractDigest is required.");
    }

    if (!RuntimeHttp.IsSha256Digest(request.ExpectedContract.ContractDigest) ||
        (request.ExpectedContract.BundleDigest is not null &&
         !RuntimeHttp.IsSha256Digest(request.ExpectedContract.BundleDigest)))
    {
        return new DecisionFailure(
            400,
            "malformed-json",
            "Contract digests must use sha256:<lowercase-hex> form.");
    }

    if (request.RuntimeContext is null ||
        request.RuntimeContext.Values.Any(value => !RuntimeHttp.IsRuntimeContextValue(value)) ||
        (request.RuntimeTarget is not null &&
         (string.IsNullOrWhiteSpace(request.RuntimeTarget.Type) ||
          string.IsNullOrWhiteSpace(request.RuntimeTarget.Id))) ||
        (request.Inputs?.Any(input =>
            !RuntimeHttp.IsValidSignalInput(input)) ?? false))
    {
        return new DecisionFailure(
            422,
            "invalid-runtime-context",
            "runtimeContext, runtimeTarget, or inputs do not match the runtime contract.");
    }

    return null;
}

static IResult RenderOutcome(HttpContext context, DecideTerminalOutcome outcome)
{
    if (outcome.Result is not null)
    {
        return Results.Json(outcome.Result, RuntimeHttp.JsonOptions);
    }

    var failure = outcome.Failure ??
                  new DecisionFailure(500, "service-unavailable", "The request failed.");
    if (failure.RetryAfterSeconds is int retryAfterSeconds)
    {
        context.Response.Headers.RetryAfter = retryAfterSeconds.ToString(
            System.Globalization.CultureInfo.InvariantCulture);
    }

    return RuntimeHttp.ProblemResult(
        context,
        RuntimeHttp.Problem(
            context,
            failure.Status,
            failure.Code,
            failure.Detail,
            failure.Issues,
            failure.RetryAfterSeconds,
            clientFallback: failure.ClientFallback));
}

public partial class Program
{
}

internal enum DataPlaneOperation
{
    Decide,
    ExposureConfirmation
}

internal sealed record DataPlaneOperationMetadata(DataPlaneOperation Operation)
{
    private static readonly object ItemKey = new();

    public static void Capture(HttpContext context)
    {
        var metadata = context.GetEndpoint()?
            .Metadata.GetMetadata<DataPlaneOperationMetadata>();
        if (metadata is not null)
        {
            context.Items[ItemKey] = metadata.Operation;
        }
    }

    public static DataPlaneOperation? Get(HttpContext context) =>
        context.Items.TryGetValue(ItemKey, out var value) &&
        value is DataPlaneOperation operation
            ? operation
            : null;
}
