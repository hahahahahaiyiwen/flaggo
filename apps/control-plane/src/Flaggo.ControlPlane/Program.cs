using System.Text.Json;
using Flaggo.ControlPlane;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
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
        "ValidateDefinitions",
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context =>
                RuntimeHttp.HasScope(context.User, "polari.definitions:validate")));
    options.AddPolicy(
        "ApplyDefinitions",
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context =>
                RuntimeHttp.HasScope(context.User, "polari.definitions:apply")));
    options.AddPolicy(
        "ApproveDefinitions",
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context =>
                RuntimeHttp.HasScope(context.User, "polari.definitions:approve")));
});

builder.Services.AddSingleton(
    LocalRegistryHosting.CreateDefinitionRegistry(
        builder.Configuration,
        builder.Environment.ContentRootPath));
builder.Services.AddSingleton<IDefinitionBundleManager>(
    provider => provider.GetRequiredService<LocalFileDefinitionRegistry>());
builder.Services.AddSingleton<IDefinitionApprovalManager>(
    provider => provider.GetRequiredService<LocalFileDefinitionRegistry>());

var app = builder.Build();

app.Use(async (context, next) =>
{
    var correlationId = context.Request.Headers["X-Flaggo-Correlation-Id"].FirstOrDefault();
    RuntimeHttp.SetCorrelationId(
        context,
        string.IsNullOrWhiteSpace(correlationId)
            ? context.TraceIdentifier
            : correlationId);
    await next(context);
});
app.UseExceptionHandler(errorApp => errorApp.Run(async context =>
{
    RuntimeHttp.RestoreCorrelationId(context);
    var exception = context.Features
        .Get<Microsoft.AspNetCore.Diagnostics.IExceptionHandlerFeature>()
        ?.Error;
    var availabilityFailure = exception is IOException or TimeoutException;
    var status = availabilityFailure ? 503 : 500;
    var code = availabilityFailure ? "service-unavailable" : "internal-error";
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
        availabilityFailure
            ? "The control plane is temporarily unavailable."
            : "The control plane encountered an unexpected error.",
        retryAfterSeconds: availabilityFailure ? 1 : null);
    await context.Response.WriteAsync(
        JsonSerializer.Serialize(problem, RuntimeHttp.JsonOptions));
}));
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
        "/v1/definition-bundles:validate",
        async (
            HttpContext context,
            IDefinitionBundleManager manager,
            CancellationToken cancellationToken) =>
        {
            var (bundle, _, error) =
                await RuntimeHttp.ReadJsonAsync<JsonElement>(context, cancellationToken);
            if (error is not null)
            {
                return RuntimeHttp.ProblemResult(context, error);
            }

            var authorizationError = ManagementHttp.AuthorizeBundle(context, bundle);
            if (authorizationError is not null)
            {
                return RuntimeHttp.ProblemResult(context, authorizationError);
            }

            var result = await manager.ValidateAsync(bundle, cancellationToken);
            return Results.Json(result, RuntimeHttp.JsonOptions);
        })
    .RequireAuthorization("ValidateDefinitions");

app.MapPost(
        "/v1/definition-bundles:apply",
        async (
            HttpContext context,
            IDefinitionBundleManager manager,
            CancellationToken cancellationToken) =>
        {
            var (bundle, _, error) =
                await RuntimeHttp.ReadJsonAsync<JsonElement>(context, cancellationToken);
            if (error is not null)
            {
                return RuntimeHttp.ProblemResult(context, error);
            }

            var authorizationError = ManagementHttp.AuthorizeBundle(context, bundle);
            if (authorizationError is not null)
            {
                return RuntimeHttp.ProblemResult(context, authorizationError);
            }

            var idempotencyKey = context.Request.Headers["Idempotency-Key"].FirstOrDefault();
            if (string.IsNullOrWhiteSpace(idempotencyKey))
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        400,
                        "idempotency-key-required",
                        "Idempotency-Key is required."));
            }

            try
            {
                var outcome = await manager.ApplyAsync(
                    idempotencyKey,
                    bundle,
                    cancellationToken);
                return Results.Json(
                    outcome.Body,
                    RuntimeHttp.JsonOptions,
                    statusCode: outcome.StatusCode);
            }
            catch (DefinitionLifecycleException exception)
            {
                return ManagementHttp.Problem(context, exception);
            }
        })
    .RequireAuthorization("ApplyDefinitions");

app.MapGet(
        "/v1/definition-bundle-approvals/{approvalRequestId}",
        async (
            string approvalRequestId,
            HttpContext context,
            IDefinitionApprovalManager manager,
            CancellationToken cancellationToken) =>
        {
            var result = await manager.GetApprovalAsync(
                approvalRequestId,
                cancellationToken);
            if (result is null)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "approval-not-found",
                        "The approval request does not exist."));
            }

            var authorizationError = ManagementHttp.AuthorizeScope(
                context,
                result.Application,
                result.Environment);
            return authorizationError is null
                ? Results.Json(result, RuntimeHttp.JsonOptions)
                : RuntimeHttp.ProblemResult(context, authorizationError);
        })
    .RequireAuthorization("ApproveDefinitions");

app.MapGet(
        "/v1/definition-bundle-approvals/{approvalRequestId}/bundle",
        async (
            string approvalRequestId,
            HttpContext context,
            IDefinitionApprovalManager manager,
            CancellationToken cancellationToken) =>
        {
            var approval = await manager.GetApprovalAsync(
                approvalRequestId,
                cancellationToken);
            if (approval is null)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "approval-not-found",
                        "The approval request does not exist."));
            }

            var authorizationError = ManagementHttp.AuthorizeScope(
                context,
                approval.Application,
                approval.Environment);
            if (authorizationError is not null)
            {
                return RuntimeHttp.ProblemResult(context, authorizationError);
            }

            var snapshot = await manager.GetSnapshotAsync(
                approvalRequestId,
                cancellationToken);
            if (snapshot is null)
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        404,
                        "approval-not-found",
                        "The approval request does not exist."));
            }

            context.Response.Headers.ETag = $"\"{snapshot.BundleDigest}\"";
            context.Response.Headers["Content-Digest"] =
                ManagementHttp.ContentDigest(snapshot.BundleDigest);
            return Results.Bytes(snapshot.CanonicalBytes, "application/json");
        })
    .RequireAuthorization("ApproveDefinitions");

app.MapPost(
        "/v1/definition-bundle-approvals/{approvalRequestId}:approve",
        async (
            string approvalRequestId,
            HttpContext context,
            IDefinitionApprovalManager manager,
            CancellationToken cancellationToken) =>
        {
            var (request, _, error) =
                await RuntimeHttp.ReadJsonAsync<ApproveDefinitionBundleRequest>(
                    context,
                    cancellationToken);
            if (error is not null)
            {
                return RuntimeHttp.ProblemResult(context, error);
            }

            if (request is null ||
                !RuntimeHttp.IsSha256Digest(request.ExpectedBundleDigest))
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        422,
                        "invalid-approval",
                        "expectedBundleDigest must be a canonical sha256 digest."));
            }

            try
            {
                var pending = await manager.GetApprovalAsync(
                    approvalRequestId,
                    cancellationToken);
                if (pending is null)
                {
                    throw new DefinitionLifecycleException(
                        404,
                        "approval-not-found",
                        "The approval request does not exist.");
                }

                var authorizationError = ManagementHttp.AuthorizeScope(
                    context,
                    pending.Application,
                    pending.Environment);
                if (authorizationError is not null)
                {
                    return RuntimeHttp.ProblemResult(context, authorizationError);
                }

                var result = await manager.ApproveAsync(
                    approvalRequestId,
                    request.ExpectedBundleDigest,
                    ManagementHttp.Actor(context),
                    request.Comment,
                    cancellationToken);
                return Results.Json(result, RuntimeHttp.JsonOptions);
            }
            catch (DefinitionLifecycleException exception)
            {
                return ManagementHttp.Problem(context, exception);
            }
        })
    .RequireAuthorization("ApproveDefinitions");

app.MapPost(
        "/v1/definition-bundle-approvals/{approvalRequestId}:reject",
        async (
            string approvalRequestId,
            HttpContext context,
            IDefinitionApprovalManager manager,
            CancellationToken cancellationToken) =>
        {
            var (request, _, error) =
                await RuntimeHttp.ReadJsonAsync<RejectDefinitionBundleRequest>(
                    context,
                    cancellationToken);
            if (error is not null)
            {
                return RuntimeHttp.ProblemResult(context, error);
            }

            if (request is null ||
                !RuntimeHttp.IsSha256Digest(request.ExpectedBundleDigest) ||
                string.IsNullOrWhiteSpace(request.ReasonCode))
            {
                return RuntimeHttp.ProblemResult(
                    context,
                    RuntimeHttp.Problem(
                        context,
                        422,
                        "invalid-rejection",
                        "expectedBundleDigest and reasonCode are required."));
            }

            try
            {
                var pending = await manager.GetApprovalAsync(
                    approvalRequestId,
                    cancellationToken);
                if (pending is null)
                {
                    throw new DefinitionLifecycleException(
                        404,
                        "approval-not-found",
                        "The approval request does not exist.");
                }

                var authorizationError = ManagementHttp.AuthorizeScope(
                    context,
                    pending.Application,
                    pending.Environment);
                if (authorizationError is not null)
                {
                    return RuntimeHttp.ProblemResult(context, authorizationError);
                }

                var result = await manager.RejectAsync(
                    approvalRequestId,
                    request.ExpectedBundleDigest,
                    ManagementHttp.Actor(context),
                    request.ReasonCode,
                    request.Comment,
                    cancellationToken);
                return Results.Json(result, RuntimeHttp.JsonOptions);
            }
            catch (DefinitionLifecycleException exception)
            {
                return ManagementHttp.Problem(context, exception);
            }
        })
    .RequireAuthorization("ApproveDefinitions");

app.Run();
