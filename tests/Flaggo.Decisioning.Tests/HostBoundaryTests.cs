using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.ControlPlane;
using Flaggo.DataPlane;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;

namespace Flaggo.Decisioning.Tests;

public sealed class HostBoundaryTests
{
    [Fact]
    public async Task DataPlane_DoesNotHostManagementEndpoints()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        using var client = factory.CreateClient();

        using var response = await client.PostAsync(
            "/v1/definition-bundles:validate",
            new StringContent("{}", Encoding.UTF8, "application/json"));

        Assert.Equal(HttpStatusCode.NotFound, response.StatusCode);
    }

    [Fact]
    public async Task ControlPlane_DoesNotHostRuntimeEndpoints()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory =
            new LocalHostFactory<ControlPlaneAssemblyMarker>(registryFile.Path);
        using var client = factory.CreateClient();

        using var response = await client.PostAsync(
            "/v1/decisions/tetris.dropInterval:decide",
            new StringContent("{}", Encoding.UTF8, "application/json"));

        Assert.Equal(HttpStatusCode.NotFound, response.StatusCode);
    }

    [Fact]
    public async Task ApprovedControlPlaneBundle_BecomesDecidableInSeparateDataPlaneHost()
    {
        using var registryFile = new TestRegistryFile();
        await using var dataFactory =
            new LocalHostFactory<DataPlaneAssemblyMarker>(registryFile.Path);
        await using var controlFactory =
            new LocalHostFactory<ControlPlaneAssemblyMarker>(registryFile.Path);
        using var dataClient = dataFactory.CreateClient();
        using var controlClient = controlFactory.CreateClient();
        using var initialReadiness = await dataClient.GetAsync("/health/ready");
        Assert.Equal(HttpStatusCode.OK, initialReadiness.StatusCode);

        var fixturePath = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            "definition-bundle",
            "04-apply-approved-receipt.json");
        using var fixture = JsonDocument.Parse(File.ReadAllBytes(fixturePath));
        var bundle = JsonNode.Parse(
            fixture.RootElement
                .GetProperty("request")
                .GetProperty("body")
                .GetRawText())!.AsObject();
        bundle["definitions"]![0]!["fallback"]!["value"] = 850;
        bundle["definitions"]![0]!["actionSpace"]!["default"] = 850;
        using var applyRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/definition-bundles:apply")
        {
            Content = new StringContent(
                bundle.ToJsonString(),
                Encoding.UTF8,
                "application/json")
        };
        applyRequest.Headers.Add("Idempotency-Key", "cross-host-approval");

        using var applyResponse = await controlClient.SendAsync(applyRequest);
        Assert.Equal(HttpStatusCode.Accepted, applyResponse.StatusCode);
        var pending = await applyResponse.Content.ReadFromJsonAsync<JsonElement>();
        var approvalRequestId = pending.GetProperty("approvalRequestId").GetString()!;
        var bundleDigest = pending.GetProperty("bundleDigest").GetString()!;
        using var approvalResponse = await controlClient.PostAsJsonAsync(
            $"/v1/definition-bundle-approvals/{approvalRequestId}:approve",
            new
            {
                expectedBundleDigest = bundleDigest,
                comment = "Cross-host visibility test."
            });
        Assert.Equal(HttpStatusCode.OK, approvalResponse.StatusCode);
        var approval = await approvalResponse.Content.ReadFromJsonAsync<JsonElement>();
        var accepted = approval.GetProperty("receipt")
            .GetProperty("acceptedDefinitions")
            .GetProperty("tetris.dropInterval");
        var decideRequest = new
        {
            expectedContract = new
            {
                definitionId = accepted.GetProperty("definitionId").GetString(),
                contractDigest = accepted.GetProperty("contractDigest").GetString(),
                revision = accepted.GetProperty("revision").GetString(),
                bundleDigest
            },
            runtimeContext = new { },
            client = new
            {
                appId = "tetris-demo",
                environment = "dev"
            }
        };

        using var decideResponse = await dataClient.PostAsJsonAsync(
            "/v1/decisions/tetris.dropInterval:decide",
            decideRequest);

        Assert.Equal(HttpStatusCode.OK, decideResponse.StatusCode);
        var decision = await decideResponse.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal(850, decision.GetProperty("value").GetInt32());
        Assert.Equal(
            accepted.GetProperty("revision").GetString(),
            decision.GetProperty("definition").GetProperty("revision").GetString());
        Assert.Equal(
            accepted.GetProperty("contractDigest").GetString(),
            decision.GetProperty("definitionStatus")
                .GetProperty("contractDigest")
                .GetString());
    }

    private sealed class LocalHostFactory<TEntryPoint>(string registryPath)
        : WebApplicationFactory<TEntryPoint>
        where TEntryPoint : class
    {
        protected override void ConfigureWebHost(IWebHostBuilder builder)
        {
            builder.UseEnvironment("Development");
            builder.UseSetting(
                "Flaggo:Authentication:LocalDevelopmentBypass",
                "true");
            builder.UseSetting(
                "Flaggo:Registry:LocalFilePath",
                registryPath);
        }
    }
}
