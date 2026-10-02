using Flaggo.Contract;
using Flaggo.ContractService;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Diagnostics;
using Microsoft.Data.Sqlite;

const string validatePolicy = "ValidateContract";
const string acceptPolicy = "AcceptContract";
const string readPolicy = "ReadContract";
const string validateScope = "flaggo.contracts:validate";
const string acceptScope = "flaggo.contracts:accept";
const string readScope = "flaggo.contracts:read";
const string serviceName = "flaggo-contract-service";

var builder = WebApplication.CreateBuilder(args);
var connectionString = builder.Configuration.GetConnectionString("Flaggo")
    ?? "Data Source=flaggo.db";

builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton<FlaggoExpressionCompiler>();
builder.Services.AddSingleton<FlaggoExecutableCompiler>();
builder.Services.AddSingleton<IContractVersionStore>(provider =>
    new SqliteContractVersionStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>()));
builder.Services.AddSingleton<IExecutableStore>(provider =>
    new SqliteExecutableStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>(),
        provider.GetRequiredService<TimeProvider>()));
builder.Services.AddSingleton<IContractLifecycle, ContractLifecycle>();
builder.Services.AddFlaggoAuthentication(
    builder.Configuration,
    builder.Environment,
    validateScope,
    acceptScope,
    readScope);
builder.Services.AddAuthorization(options =>
{
    AddPolicy(options, validatePolicy, validateScope);
    AddPolicy(options, acceptPolicy, acceptScope);
    AddPolicy(options, readPolicy, readScope);
});
builder.Services.AddFlaggoAuthorizationProblemResults();

var app = builder.Build();

app.UseFlaggoCorrelationIds();
app.UseExceptionHandler(new ExceptionHandlerOptions
{
    AllowStatusCode404Response = true,
    ExceptionHandler = WriteExceptionAsync
});
app.UseFlaggoProblemStatusPages();
app.UseAuthentication();
app.UseAuthorization();

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
        })
    .RequireAuthorization(validatePolicy);

app.MapPut(
        "/v3/decision-contracts/{contractName}",
        async (
            string contractName,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var scope = AuthorityScope(context);
            var contract = await HttpJson.ReadAsync<DecisionContract>(
                context.Request,
                cancellationToken);
            var deployment = await lifecycle.DeployAsync(
                scope,
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
        })
    .RequireAuthorization(acceptPolicy);

app.MapGet(
        "/v3/decision-contracts/{contractName}",
        async (
            string contractName,
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var version = await lifecycle.GetCurrentAsync(
                AuthorityScope(context),
                contractName,
                cancellationToken);
            return version is null
                ? NotFound(context)
                : Results.Json(version, StrictJson.Options);
        })
    .RequireAuthorization(readPolicy);

app.MapGet(
        "/v3/decision-contract-snapshots/current",
        async (
            HttpContext context,
            IContractLifecycle lifecycle,
            CancellationToken cancellationToken) =>
        {
            var snapshot = await lifecycle.GetCurrentSnapshotAsync(
                AuthorityScope(context),
                cancellationToken);
            var etag = $"\"{snapshot.SnapshotDigest}\"";
            context.Response.Headers.ETag = etag;
            return MatchesEtag(context.Request.Headers.IfNoneMatch, etag)
                ? Results.StatusCode(StatusCodes.Status304NotModified)
                : Results.Json(snapshot, StrictJson.Options);
        })
    .RequireAuthorization(readPolicy);

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
                AuthorityScope(context),
                contractName,
                limit ?? 50,
                cursor,
                cancellationToken);
            return page is null
                ? NotFound(context)
                : Results.Json(page, StrictJson.Options);
        })
    .RequireAuthorization(readPolicy);

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
                AuthorityScope(context),
                contractName,
                contractDigest,
                cancellationToken);
            return version is null
                ? NotFound(context)
                : Results.Json(version, StrictJson.Options);
        })
    .RequireAuthorization(readPolicy);

app.MapGet(
    "/health/live",
    (TimeProvider timeProvider) => Results.Json(
        new LivenessResult
        {
            Service = serviceName,
            Version = typeof(Program).Assembly.GetName().Version?.ToString() ?? "0.0.0",
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

static void AddPolicy(
    Microsoft.AspNetCore.Authorization.AuthorizationOptions options,
    string name,
    string scope)
{
    options.AddPolicy(
        name,
        policy => policy
            .RequireAuthenticatedUser()
            .RequireAssertion(context => FlaggoClaims.HasScope(context.User, scope)));
}

static DecisionScope AuthorityScope(HttpContext context)
{
    if (FlaggoClaims.TryGetAuthorityScope(context.User, out var scope))
    {
        return scope;
    }

    throw new UnauthorizedAccessException(
        "The credential must identify exactly one application and environment.");
}

static IResult NotFound(HttpContext context) =>
    ProblemResults.Create(
        context,
        StatusCodes.Status404NotFound,
        ProblemTypes.ContractVersionNotFound,
        "Decision contract not found",
        "The requested DecisionContract resource does not exist in this scope.");

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
        UnauthorizedAccessException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status401Unauthorized,
                ProblemTypes.AuthenticationRequired,
                "Authentication required",
                "The credential must identify exactly one application and environment."),
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
