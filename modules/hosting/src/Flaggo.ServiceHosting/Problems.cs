using Flaggo.Contract;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;

namespace Flaggo.ServiceHosting;

public static class ProblemTypes
{
    private const string Root = "https://flaggo.dev/problems/";

    public const string InvalidRequest = Root + "invalid-request";
    public const string ResourceNotFound = Root + "resource-not-found";
    public const string MethodNotAllowed = Root + "method-not-allowed";
    public const string ContractVersionNotFound = Root + "contract-version-not-found";
    public const string ContractNameMismatch = Root + "contract-name-mismatch";
    public const string ContractNameAuthorityConflict = Root + "contract-name-authority-conflict";
    public const string InvalidDecisionContract = Root + "invalid-decision-contract";
    public const string InvalidDecisionExecutable = Root + "invalid-decision-executable";
    public const string StaleContractDigest = Root + "stale-contract-digest";
    public const string CandidateAdmissionConflict = Root + "candidate-admission-conflict";
    public const string ActivationConflict = Root + "activation-conflict";
    public const string UnsupportedMediaType = Root + "unsupported-media-type";
    public const string InvalidRuntimeInput = Root + "invalid-runtime-input";
    public const string RateLimited = Root + "rate-limited";
    public const string ExecutableNotActive = Root + "executable-not-active";
    public const string DependencyUnavailable = Root + "dependency-unavailable";
    public const string ExecutableIntegrityFailure = Root + "executable-integrity-failure";
    public const string EvaluationFailed = Root + "evaluation-failed";
    public const string InvalidRuntimeResult = Root + "invalid-runtime-result";
    public const string InternalError = Root + "internal-error";
}

public static class ProblemResults
{
    public static IResult Create(
        HttpContext context,
        int status,
        string type,
        string title,
        string detail,
        int? retryAfterSeconds = null)
    {
        ArgumentNullException.ThrowIfNull(context);
        if (retryAfterSeconds is not null)
        {
            context.Response.Headers.RetryAfter = retryAfterSeconds.Value.ToString(
                System.Globalization.CultureInfo.InvariantCulture);
        }

        context.Response.Headers[CorrelationIds.HeaderName] = CorrelationIds.Get(context);
        return Results.Json(
            new ProblemDetailsDocument
            {
                Type = type,
                Title = title,
                Status = status,
                Detail = detail,
                Instance = context.Request.Path
            },
            StrictJson.Options,
            contentType: "application/problem+json",
            statusCode: status);
    }

    public static IResult FromException(HttpContext context, HttpContractException exception) =>
        Create(context, exception.Status, exception.Type, exception.Title, exception.Message);
}

public static class ProblemStatusPages
{
    public static IApplicationBuilder UseFlaggoProblemStatusPages(
        this IApplicationBuilder application)
    {
        ArgumentNullException.ThrowIfNull(application);
        return application.UseStatusCodePages(async statusCodeContext =>
        {
            var context = statusCodeContext.HttpContext;
            var problem = context.Response.StatusCode switch
            {
                StatusCodes.Status400BadRequest => (
                    ProblemTypes.InvalidRequest,
                    "Invalid request",
                    "The request could not be processed."),
                StatusCodes.Status404NotFound => (
                    ProblemTypes.ResourceNotFound,
                    "Resource not found",
                    "The requested resource does not exist."),
                StatusCodes.Status405MethodNotAllowed => (
                    ProblemTypes.MethodNotAllowed,
                    "Method not allowed",
                    "The requested method is not supported for this resource."),
                StatusCodes.Status415UnsupportedMediaType => (
                    ProblemTypes.UnsupportedMediaType,
                    "Unsupported media type",
                    "The request media type is not supported."),
                StatusCodes.Status429TooManyRequests => (
                    ProblemTypes.RateLimited,
                    "Rate limited",
                    "The request was rate limited."),
                StatusCodes.Status502BadGateway
                    or StatusCodes.Status503ServiceUnavailable
                    or StatusCodes.Status504GatewayTimeout => (
                        ProblemTypes.DependencyUnavailable,
                        "Dependency unavailable",
                        "A required dependency is unavailable."),
                _ => (
                    ProblemTypes.InternalError,
                    "Request failed",
                    "The request failed.")
            };
            await ProblemResults.Create(
                    context,
                    context.Response.StatusCode,
                    problem.Item1,
                    problem.Item2,
                    problem.Item3)
                .ExecuteAsync(context);
        });
    }
}
