using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.Decision;
using Flaggo.DecisionService;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Diagnostics;
using Microsoft.Data.Sqlite;

const string serviceName = "flaggo-decision-service";

var builder = WebApplication.CreateBuilder(args);
var connectionString = builder.Configuration.GetConnectionString("Flaggo")
    ?? "Data Source=flaggo.db";
var serviceVersion = typeof(Program).Assembly.GetName().Version?.ToString() ?? "0.0.0";

builder.AddFlaggoServiceObservability(
    serviceName,
    "flaggo.decision-service",
    serviceVersion);
builder.Services.AddSingleton(TimeProvider.System);
builder.Services.AddSingleton<FlaggoExpressionCompiler>();
builder.Services.AddSingleton<FlaggoExecutableCompiler>();
builder.Services.AddSingleton<DecisionServiceObservability>();
builder.Services.AddSingleton<IContractVersionStore>(provider =>
    new SqliteContractVersionStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>()));
builder.Services.AddSingleton<IExecutableStore>(provider =>
    new SqliteExecutableStore(
        connectionString,
        provider.GetRequiredService<FlaggoExpressionCompiler>(),
        provider.GetRequiredService<TimeProvider>()));
builder.Services.AddSingleton<DecisionEvaluator>();
builder.Services.AddSingleton<IDecisionRuntime, DecisionRuntime>();
var app = builder.Build();

app.UseFlaggoCorrelationIds();
app.UseExceptionHandler(new ExceptionHandlerOptions
{
    AllowStatusCode404Response = true,
    ExceptionHandler = WriteExceptionAsync
});
app.UseFlaggoProblemStatusPages();
app.MapPost(
        "/v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions",
        async (
            HttpContext context,
            string contractName,
            string contractDigest,
            IDecisionRuntime runtime,
            DecisionServiceObservability observability,
            CancellationToken cancellationToken) =>
        {
            using var operation = observability.StartEvaluation(
                contractName,
                contractDigest);
            try
            {
                var input = await HttpJson.ReadAsync<RuntimeInput>(
                    context.Request,
                    cancellationToken);
                var decision = await runtime.DecideAsync(
                    contractName,
                    contractDigest,
                    input,
                    cancellationToken);
                var source = DecisionServiceObservability.EvaluationSource(decision);
                operation.Activity?.SetTag(
                    "flaggo.executable.digest",
                    decision.ExecutableDigest);
                operation.Complete(
                    "success",
                    attributes:
                    [
                        new(
                            "flaggo.evaluation.source",
                            source)
                    ]);
                return Results.Json(decision, StrictJson.Options);
            }
            catch (Exception exception)
            {
                var (outcome, category) =
                    DecisionServiceObservability.Classify(exception);
                operation.Fail(exception, outcome, category);
                throw;
            }
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
        ArgumentException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status400BadRequest,
                ProblemTypes.InvalidRequest,
                "Invalid request",
                "The request path or parameters are invalid."),
        DecisionContractVersionNotFoundException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status404NotFound,
                ProblemTypes.ContractVersionNotFound,
                "Contract version not found",
                "The requested contract version does not exist."),
        RuntimeInputValidationException inputException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status422UnprocessableEntity,
                ProblemTypes.InvalidRuntimeInput,
                "Invalid runtime input",
                string.Join(
                    " ",
                    inputException.Issues.Select(issue =>
                        $"{issue.Path}: {issue.Message}"))),
        ActiveExecutableNotFoundException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status503ServiceUnavailable,
                ProblemTypes.ExecutableNotActive,
                "Executable not active",
                "No executable is active for the requested contract version.",
                retryAfterSeconds: 1),
        RuntimeIntegrityException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.ExecutableIntegrityFailure,
                "Executable integrity failure",
                "Stored runtime authority failed integrity validation."),
        DecisionEvaluationException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.EvaluationFailed,
                "Evaluation failed",
                "The active executable could not evaluate the runtime input."),
        DecisionResultValidationException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.InvalidRuntimeResult,
                "Invalid runtime result",
                "The active executable produced a result outside the contract schema."),
        SqliteException or IOException or TimeoutException =>
            ProblemResults.Create(
                context,
                StatusCodes.Status503ServiceUnavailable,
                ProblemTypes.DependencyUnavailable,
                "Dependency unavailable",
                "A required runtime dependency is unavailable.",
                retryAfterSeconds: 1),
        _ =>
            ProblemResults.Create(
                context,
                StatusCodes.Status500InternalServerError,
                ProblemTypes.InternalError,
                "Internal error",
                "The Decision Service encountered an unexpected error.")
    };

    if (exception is RuntimeIntegrityException
        or DecisionEvaluationException
        or DecisionResultValidationException)
    {
        logger.LogError(exception, "Decision request failed.");
    }
    else if (exception is not null
        and not HttpContractException
        and not ArgumentException
        and not DecisionContractVersionNotFoundException
        and not RuntimeInputValidationException
        and not ActiveExecutableNotFoundException)
    {
        logger.LogError(exception, "Unhandled Decision Service failure.");
    }

    await result.ExecuteAsync(context);
}

public partial class Program
{
}
