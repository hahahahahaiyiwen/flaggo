using System.Security.Claims;
using System.Text.Encodings.Web;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Authorization.Policy;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Flaggo.ServiceHosting;

public static class FlaggoAuthentication
{
    public const string LocalDevelopmentScheme = "FlaggoLocalDevelopment";

    public static IServiceCollection AddFlaggoAuthorizationProblemResults(
        this IServiceCollection services)
    {
        ArgumentNullException.ThrowIfNull(services);
        services.AddSingleton<
            IAuthorizationMiddlewareResultHandler,
            FlaggoAuthorizationResultHandler>();
        return services;
    }

    public static AuthenticationBuilder AddFlaggoAuthentication(
        this IServiceCollection services,
        IConfiguration configuration,
        IHostEnvironment environment,
        params string[] localDevelopmentScopes)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);
        ArgumentNullException.ThrowIfNull(environment);

        var localBypass = configuration.GetValue<bool>(
            "Flaggo:Authentication:LocalDevelopmentBypass");
        if (localBypass && !environment.IsDevelopment())
        {
            throw new InvalidOperationException(
                "Local-development authentication bypass is allowed only in Development.");
        }

        if (localBypass)
        {
            services.Configure<LocalDevelopmentIdentityOptions>(
                configuration.GetSection("Flaggo:Authentication"));
            services.AddSingleton(new LocalDevelopmentScopes(localDevelopmentScopes));
            return services
                .AddAuthentication(LocalDevelopmentScheme)
                .AddScheme<AuthenticationSchemeOptions, LocalDevelopmentAuthenticationHandler>(
                    LocalDevelopmentScheme,
                    _ => { });
        }

        var authority = configuration["Flaggo:Authentication:Authority"];
        var audience = configuration["Flaggo:Authentication:Audience"];
        if (string.IsNullOrWhiteSpace(authority) || string.IsNullOrWhiteSpace(audience))
        {
            throw new InvalidOperationException(
                "OAuth authority and audience are required unless the explicit "
                + "local-development bypass is enabled.");
        }

        return services
            .AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
            .AddJwtBearer(options =>
            {
                options.Authority = authority;
                options.Audience = audience;
                options.RequireHttpsMetadata = configuration.GetValue(
                    "Flaggo:Authentication:RequireHttpsMetadata",
                    true);
            });
    }
}

public sealed class FlaggoAuthorizationResultHandler : IAuthorizationMiddlewareResultHandler
{
    public async Task HandleAsync(
        RequestDelegate next,
        HttpContext context,
        AuthorizationPolicy policy,
        PolicyAuthorizationResult authorizeResult)
    {
        if (authorizeResult.Challenged)
        {
            await ProblemResults.Create(
                context,
                StatusCodes.Status401Unauthorized,
                ProblemTypes.AuthenticationRequired,
                "Authentication required",
                "A valid bearer credential is required.")
                .ExecuteAsync(context);
            return;
        }

        if (authorizeResult.Forbidden)
        {
            await ProblemResults.Create(
                context,
                StatusCodes.Status403Forbidden,
                ProblemTypes.InsufficientScope,
                "Insufficient scope",
                "The credential does not grant the required operation.")
                .ExecuteAsync(context);
            return;
        }

        await next(context);
    }
}

public sealed class LocalDevelopmentIdentityOptions
{
    public string Application { get; set; } = "local-application";

    public string Environment { get; set; } = "development";
}

internal sealed record LocalDevelopmentScopes(IReadOnlyList<string> Values);

internal sealed class LocalDevelopmentAuthenticationHandler(
    IOptionsMonitor<AuthenticationSchemeOptions> options,
    ILoggerFactory logger,
    UrlEncoder encoder,
    IOptions<LocalDevelopmentIdentityOptions> identityOptions,
    LocalDevelopmentScopes scopes)
    : AuthenticationHandler<AuthenticationSchemeOptions>(options, logger, encoder)
{
    protected override Task<AuthenticateResult> HandleAuthenticateAsync()
    {
        var identity = identityOptions.Value;
        if (string.IsNullOrWhiteSpace(identity.Application)
            || string.IsNullOrWhiteSpace(identity.Environment))
        {
            return Task.FromResult(AuthenticateResult.Fail(
                "Local-development identity requires non-empty Application and Environment."));
        }

        var claims = new List<Claim>
        {
            new(ClaimTypes.NameIdentifier, "local-development"),
            new(FlaggoClaimTypes.Application, identity.Application),
            new(FlaggoClaimTypes.Environment, identity.Environment)
        };
        if (scopes.Values.Count > 0)
        {
            claims.Add(new Claim("scope", string.Join(' ', scopes.Values)));
        }

        var principal = new ClaimsPrincipal(new ClaimsIdentity(claims, Scheme.Name));
        return Task.FromResult(AuthenticateResult.Success(
            new AuthenticationTicket(principal, Scheme.Name)));
    }
}
