using System.Diagnostics;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;

namespace Flaggo.ServiceHosting;

public static class CorrelationIds
{
    public const string HeaderName = "X-Flaggo-Correlation-Id";
    private const int MaximumLength = 128;
    private static readonly object ItemKey = new();

    public static IApplicationBuilder UseFlaggoCorrelationIds(this IApplicationBuilder app)
    {
        ArgumentNullException.ThrowIfNull(app);

        return app.Use(async (context, next) =>
        {
            var supplied = context.Request.Headers[HeaderName].FirstOrDefault();
            var correlationId = IsValid(supplied)
                ? supplied!
                : Activity.Current?.TraceId.ToString() ?? Guid.NewGuid().ToString("N");
            context.Items[ItemKey] = correlationId;
            Activity.Current?.SetTag(
                "flaggo.request.correlation_id",
                correlationId);
            context.Response.OnStarting(() =>
            {
                context.Response.Headers[HeaderName] = correlationId;
                return Task.CompletedTask;
            });
            await next(context);
        });
    }

    public static string Get(HttpContext context)
    {
        ArgumentNullException.ThrowIfNull(context);
        return context.Items.TryGetValue(ItemKey, out var value) && value is string correlationId
            ? correlationId
            : Activity.Current?.TraceId.ToString() ?? Guid.NewGuid().ToString("N");
    }

    private static bool IsValid(string? value) =>
        value is { Length: > 0 and <= MaximumLength }
        && value.All(character => character is >= (char)0x21 and <= (char)0x7e);
}
