using Flaggo.Contract;
using Flaggo.ContractService;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Diagnostics;
using Microsoft.Data.Sqlite;

const string serviceName = "flaggo-contract-service";

var builder = WebApplication.CreateBuilder(args);
var connectionString = builder.Configuration.GetConnectionString("Flaggo")
    ?? "Data Source=flaggo.db";
var serviceVersion = typeof(Program).Assembly.GetName().Version?.ToString() ?? "0.0.0";
var candidateActivationOptions =
    CandidateActivationOptions.FromConfiguration(builder.Configuration);

builder.AddFlaggoServiceObservability(
    serviceName,
    "flaggo.contract-service",
    serviceVersion);
builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton<FlaggoExpressionCompiler>();
builder.Services.AddSingleton<FlaggoExecutableCompiler>();
builder.Services.AddSingleton<ContractServiceObservability>();
builder.Services.AddSingleton(candidateActivationOptions);
builder.Services.AddSingleton<IContractVersionStore>(provider =>
    new SqliteContractVersionStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>()));
builder.Services.AddSingleton(provider =>
    new SqliteExecutableStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>(),
        provider.GetRequiredService<TimeProvider>()));
builder.Services.AddSingleton<IExecutableStore>(provider =>
    provider.GetRequiredService<SqliteExecutableStore>());
builder.Services.AddSingleton<IAnalysisCandidateActivationStore>(provider =>
    provider.GetRequiredService<SqliteExecutableStore>());
builder.Services.AddSingleton<IContractLifecycle, ContractLifecycle>();
builder.Services.AddSingleton<AnalysisCandidateActivationWorker>();
builder.Services.AddHostedService(provider =>
    provider.GetRequiredService<AnalysisCandidateActivationWorker>());
var app = builder.Build();

app.UseFlaggoCorrelationIds();
app.UseExceptionHandler(new ExceptionHandlerOptions
{
    AllowStatusCode404Response = true,
    ExceptionHandler = WriteExceptionAsync
});
app.UseFlaggoProblemStatusPages();
app.MapPost(
        "/v3/decision-contracts/{contractName}/validate",
        async (
            string contractName,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var contract = await HttpJson.ReadAsync<DecisionContract>(
                context.Request,
                cancellationToken);
            return Results.Json(
                lifecycle.Validate(contractName, contract),
                StrictJson.Options);
        });

app.MapPut(
        "/v3/decision-contracts/{contractName}",
        async (
            string contractName,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var contract = await HttpJson.ReadAsync<DecisionContract>(
                context.Request,
                cancellationToken);
            var deployment = await lifecycle.DeployAsync(
                contractName,
                contract,
                cancellationToken);
            if (deployment.Created)
            {
                context.Response.Headers.Location =
                    $"/v3/decision-contracts/{Uri.EscapeDataString(contractName)}"
                    + $"/versions/{deployment.Version.ContractDigest}";
            }

            return Results.Json(
                deployment.Version,
                StrictJson.Options,
                statusCode: deployment.Created
                    ? StatusCodes.Status201Created
                    : StatusCodes.Status200OK);
        });

app.MapGet(
        "/v3/decision-contracts/{contractName}",
        async (
            string contractName,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var version = await lifecycle.GetCurrentAsync(
                contractName,
                cancellationToken);
            return version is null
                ? NotFound(context)
                : Results.Json(version, StrictJson.Options);
        });

app.MapGet(
        "/v3/decision-contract-catalog/current",
        async (
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var catalog = await lifecycle.GetCurrentCatalogAsync(cancellationToken);
            context.Response.Headers.ETag = catalog.Etag;
            return MatchesEtag(context.Request.Headers.IfNoneMatch, catalog.Etag)
                ? Results.StatusCode(StatusCodes.Status304NotModified)
                : Results.Json(catalog.Catalog, StrictJson.Options);
        });

app.MapGet(
        "/v3/decision-contracts/{contractName}/versions",
        async (
            string contractName,
            int? limit,
            string? cursor,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var page = await lifecycle.ListAsync(
                contractName,
                limit ?? 50,
                cursor,
                cancellationToken);
            return page is null
                ? NotFound(context)
                : Results.Json(page, StrictJson.Options);
        });

app.MapGet(
        "/v3/decision-contracts/{contractName}/versions/{contractDigest}",
        async (
            string contractName,
            string contractDigest,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var version = await lifecycle.GetAsync(
                contractName,
                contractDigest,
                cancellationToken);
            return version is null
                ? NotFound(context)
                : Results.Json(version, StrictJson.Options);
        });

app.MapPost(
        "/v3/decision-contracts/{contractName}/versions/{contractDigest}/candidates",
        async (
            string contractName,
            string contractDigest,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var submission = await HttpJson.ReadAsync<AnalysisCandidateSubmission>(
                context.Request,
                cancellationToken);
            var result = await lifecycle.SubmitAnalysisCandidateAsync(
                contractName,
                contractDigest,
                submission,
                cancellationToken);
            return Results.Json(
                result,
                StrictJson.Options,
                statusCode: result.Created
                    ? StatusCodes.Status201Created
                    : StatusCodes.Status200OK);
        });

app.MapGet(
    "/health/live",
    (TimeProvider timeProvider) => Results.Json(
        new LivenessResult
        {
            Service = serviceName,
            Version = serviceVersion,
            ObservedAt = timeProvider.GetUtcNow()
        },
        StrictJson.Options));

app.MapGet(
    "/health/ready",
    async (
        IContractVersionStore contractStore,
        IExecutableStore executableStore,
        TimeProvider timeProvider,
        CancellationToken cancellationToken) =>
    {
        var checks = new[]
        {
            new ReadinessCheck
            {
                Name = "contract-store",
                Status = await contractStore.IsAvailableAsync(cancellationToken)
                    ? "up"
                    : "down",
                Required = true
            },
            new ReadinessCheck
            {
                Name = "executable-store",
                Status = await executableStore.IsAvailableAsync(cancellationToken)
                    ? "up"
                    : "down",
                Required = true
            }
        };
        var ready = checks.All(check => check.Status == "up");
        return Results.Json(
            new ReadinessResult
            {
                Status = ready ? "ready" : "not-ready",
                ObservedAt = timeProvider.GetUtcNow(),
                Checks = checks
            },
            StrictJson.Options,
            statusCode: ready
                ? StatusCodes.Status200OK
                : StatusCodes.Status503ServiceUnavailable);
    });

await InitializeStoresAsync(app.Services);
app.Run();

static IResult NotFound(HttpContext context) =>
    ProblemResults.Create(
        context,
        StatusCodes.Status404NotFound,
        ProblemTypes.ContractVersionNotFound,
        "Decision contract not found",
        "The requested DecisionContract resource does not exist.");

static bool MatchesEtag(
    Microsoft.Extensions.Primitives.StringValues values,
    string expected) =>
    values
        .SelectMany(value => value?.Split(',') ?? [])
        .Select(value => value.Trim())
        .Any(value => value is "*" || string.Equals(value, expected, StringComparison.Ordinal));

static async Task InitializeStoresAsync(IServiceProvider services)
{
    await services.GetRequiredService<IContractVersionStore>().InitializeAsync();
    await services.GetRequiredService<IExecutableStore>().InitializeAsync();
}

static async Task WriteExceptionAsync(HttpContext context)
{
    var exception = context.Features.Get<IExceptionHandlerFeature>()?.Error;
    var logger = context.RequestServices.GetRequiredService<ILogger<Program>>();
    IResult result = exception switch
    {
        HttpContractException contractException =>
            ProblemResults.FromException(context, contractException),
        ContractNameAuthorityConflictException conflict =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.ContractNameAuthorityConflict,
                "Contract name authority conflict",
                $"Contract name '{conflict.ContractName}' is already owned by "
                + $"authority '{conflict.ExistingAuthority.Tenant}/"
                + $"{conflict.ExistingAuthority.Application}/"
                + $"{conflict.ExistingAuthority.Environment}'."),
        ContractNameMismatchException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.ContractNameMismatch,
                "Contract name mismatch",
                "The route contractName must equal the DecisionContract name."),
        InvalidDecisionContractException invalidContract =>
            ProblemResults.Create(
                context,
                StatusCodes.Status422UnprocessableEntity,
                ProblemTypes.InvalidDecisionContract,
                "Invalid decision contract",
                "The decision contract failed semantic validation."),
        ExecutableCompilationException invalidExecutable =>
            ProblemResults.Create(
                context,
                StatusCodes.Status422UnprocessableEntity,
                ProblemTypes.InvalidDecisionExecutable,
                "Invalid decision executable",
                string.Join(
                    "; ",
                    invalidExecutable.Issues.Select(issue =>
                        $"{issue.Code} at {issue.Path}: {issue.Message}"))),
        ContractVersionNotFoundException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status404NotFound,
                ProblemTypes.ContractVersionNotFound,
                "Decision contract not found",
                "The requested DecisionContract resource does not exist."),
        StaleContractDigestException stale =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.StaleContractDigest,
                "Stale contract digest",
                stale.Message),
        CandidateAdmissionConflictException conflict =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.CandidateAdmissionConflict,
                "Candidate admission conflict",
                conflict.Message),
        CandidateLifecycleConflictException conflict =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.CandidateAdmissionConflict,
                "Candidate lifecycle conflict",
                conflict.Message),
        ArgumentException or FormatException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid request",
                "The request path or query parameters are invalid."),
        ActivationConflictException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status409Conflict,
                ProblemTypes.ActivationConflict,
                "Activation conflict",
                "Runtime authority changed before the requested activation completed."),
        SqliteException or IOException or TimeoutException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status503ServiceUnavailable,
                ProblemTypes.DependencyUnavailable,
                "Dependency unavailable",
                "A required management dependency is unavailable.",
                retryAfterSeconds: 1),
        _ =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.InternalError,
                "Internal error",
                "The Contract Service encountered an unexpected error.")
    };

    if (exception is not null
        and not HttpContractException
        and not UnauthorizedAccessException
        and not ContractNameMismatchException
        and not InvalidDecisionContractException
        and not ExecutableCompilationException
        and not ContractVersionNotFoundException
        and not StaleContractDigestException
        and not CandidateAdmissionConflictException
        and not CandidateLifecycleConflictException
        and not ArgumentException
        and not FormatException
        and not ActivationConflictException)
    {
        logger.LogError(exception, "Unhandled Contract Service failure.");
    }

    await result.ExecuteAsync(context);
}

public partial class Program
{
}
