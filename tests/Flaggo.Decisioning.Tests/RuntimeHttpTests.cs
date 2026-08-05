using System.Text;
using Flaggo.Hosting;
using Flaggo.Shared.Contracts;
using Microsoft.AspNetCore.Http;

namespace Flaggo.Decisioning.Tests;

public sealed class RuntimeHttpTests
{
    [Fact]
    public async Task ReadJsonAsync_RejectsIncorrectPropertyCasing()
    {
        var context = CreateJsonContext(
            """
            {
              "expectedContract": {
                "definitionId": "definition",
                "contractDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "revision": "revision"
              },
              "runtimeContext": {},
              "Client": { "appId": "app", "environment": "dev" }
            }
            """);

        var parsed = await RuntimeHttp.ReadJsonAsync<DecideRequest>(
            context,
            CancellationToken.None);

        Assert.Equal("malformed-json", parsed.Error!.Code);
    }

    [Fact]
    public void IsValidSignalInput_RejectsNullEntry()
    {
        Assert.False(RuntimeHttp.IsValidSignalInput(null));
    }

    [Fact]
    public async Task ReadJsonAsync_RejectsExplicitNullOptionalMember()
    {
        var context = CreateJsonContext(
            """
            {
              "expectedContract": {
                "definitionId": "definition",
                "contractDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "revision": "revision",
                "bundleDigest": null
              },
              "runtimeContext": {},
              "client": { "appId": "app", "environment": "dev" }
            }
            """);

        var parsed = await RuntimeHttp.ReadJsonAsync<DecideRequest>(
            context,
            CancellationToken.None);

        Assert.Equal("malformed-json", parsed.Error!.Code);
    }

    [Fact]
    public async Task ReadJsonAsync_RejectsNullRequiredRuntimeContext()
    {
        var context = CreateJsonContext(
            """
            {
              "expectedContract": {
                "definitionId": "definition",
                "contractDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "revision": "revision"
              },
              "runtimeContext": null,
              "client": { "appId": "app", "environment": "dev" }
            }
            """);

        var parsed = await RuntimeHttp.ReadJsonAsync<DecideRequest>(
            context,
            CancellationToken.None);

        Assert.Equal("malformed-json", parsed.Error!.Code);
    }

    [Fact]
    public void IsRuntimeContextValue_RejectsNumberOutsideIeee754Range()
    {
        using var document = System.Text.Json.JsonDocument.Parse("1e400");

        Assert.False(RuntimeHttp.IsRuntimeContextValue(document.RootElement));
    }

    [Fact]
    public void RestoreCorrelationId_ReappliesHeaderAndProblemIdentity()
    {
        var context = new DefaultHttpContext();
        RuntimeHttp.SetCorrelationId(context, "correlation-1");
        context.Response.Headers.Clear();

        RuntimeHttp.RestoreCorrelationId(context);
        var problem = RuntimeHttp.Problem(context, 500, "internal-error", "failed");

        Assert.Equal("correlation-1", context.Response.Headers["X-Flaggo-Correlation-Id"]);
        Assert.Equal("correlation-1", problem.CorrelationId);
    }

    private static DefaultHttpContext CreateJsonContext(string json)
    {
        var context = new DefaultHttpContext();
        context.Request.ContentType = "application/json";
        context.Request.Body = new MemoryStream(Encoding.UTF8.GetBytes(json));
        RuntimeHttp.SetCorrelationId(context, "test-correlation");
        return context;
    }
}
