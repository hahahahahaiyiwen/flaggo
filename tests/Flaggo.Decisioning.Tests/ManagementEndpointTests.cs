using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using Flaggo.ControlPlane;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Flaggo.Decisioning.Tests;

public sealed class ManagementEndpointTests
{
    [Fact]
    public async Task ValidateAndApply_ExposeSdkStartupContract()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory = new FlaggoApplicationFactory(registryFile.Path);
        using var client = factory.CreateClient();
        var fixture = Fixture("definition-bundle", "04-apply-approved-receipt.json");
        var body = fixture.GetProperty("request").GetProperty("body");

        using var validation = await client.PostAsync(
            "/v1/definition-bundles:validate",
            Json(body));
        Assert.Equal(HttpStatusCode.OK, validation.StatusCode);
        var validationBody = await validation.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal("valid", validationBody.GetProperty("status").GetString());
        Assert.Equal("metadata-only", validationBody.GetProperty("compatibility").GetString());

        using var applyRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/definition-bundles:apply")
        {
            Content = Json(body)
        };
        applyRequest.Headers.Add("Idempotency-Key", "sdk-startup-test");
        using var apply = await client.SendAsync(applyRequest);

        Assert.Equal(HttpStatusCode.OK, apply.StatusCode);
        var receipt = await apply.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal("approved", receipt.GetProperty("status").GetString());
        Assert.Equal(
            "sha256:6eadd7bd76b36ae06e89376d57107da83fdcabf07ff58c528ae97fddb7f08ee9",
            receipt.GetProperty("acceptedDefinitions")
                .GetProperty("tetris.dropInterval")
                .GetProperty("contractDigest")
                .GetString());
    }

    [Fact]
    public async Task SemanticApply_ReplaysApprovedReceiptForSdkStartup()
    {
        using var registryFile = new TestRegistryFile();
        await using var factory = new FlaggoApplicationFactory(
            registryFile.Path,
            usePreviousRevision: true);
        using var client = factory.CreateClient();
        var fixture = Fixture("definition-bundle", "06-apply-requires-approval.json");
        var body = fixture.GetProperty("request").GetProperty("body");
        using var request = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/definition-bundles:apply")
        {
            Content = Json(body)
        };
        request.Headers.Add("Idempotency-Key", "semantic-http-test");

        using var apply = await client.SendAsync(request);

        Assert.Equal(HttpStatusCode.Accepted, apply.StatusCode);
        var pending = await apply.Content.ReadFromJsonAsync<JsonElement>();
        var approvalRequestId = pending.GetProperty("approvalRequestId").GetString()!;
        using var snapshot = await client.GetAsync(
            $"/v1/definition-bundle-approvals/{approvalRequestId}/bundle");
        Assert.Equal(HttpStatusCode.OK, snapshot.StatusCode);
        Assert.Equal(
            $"\"{pending.GetProperty("bundleDigest").GetString()}\"",
            snapshot.Headers.ETag!.ToString());
        Assert.True(snapshot.Headers.Contains("Content-Digest"));
        var snapshotBytes = await snapshot.Content.ReadAsByteArrayAsync();
        Assert.Equal(
            CanonicalJson.NormalizeBundleBytes(body),
            snapshotBytes);

        using var approve = await client.PostAsJsonAsync(
            $"/v1/definition-bundle-approvals/{approvalRequestId}:approve",
            new
            {
                expectedBundleDigest =
                    pending.GetProperty("bundleDigest").GetString(),
                comment = "Approved for startup retry."
            });
        Assert.Equal(HttpStatusCode.OK, approve.StatusCode);

        using var replayRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "/v1/definition-bundles:apply")
        {
            Content = Json(body)
        };
        replayRequest.Headers.Add("Idempotency-Key", "semantic-http-test");
        using var replay = await client.SendAsync(replayRequest);

        Assert.Equal(HttpStatusCode.OK, replay.StatusCode);
        var receipt = await replay.Content.ReadFromJsonAsync<JsonElement>();
        Assert.Equal("approved", receipt.GetProperty("status").GetString());
        Assert.Equal(
            pending.GetProperty("bundleDigest").GetString(),
            receipt.GetProperty("bundleDigest").GetString());
    }

    private static StringContent Json(JsonElement body) =>
        new(body.GetRawText(), Encoding.UTF8, "application/json");

    private static JsonElement Fixture(string group, string file)
    {
        var path = Path.GetFullPath(
            Path.Combine(
                AppContext.BaseDirectory,
                "..",
                "..",
                "..",
                "..",
                "..",
                "contracts",
                "fixtures",
                "management",
                group,
                file));
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.Clone();
    }

    private sealed class FlaggoApplicationFactory(
        string registryPath,
        bool usePreviousRevision = false)
        : WebApplicationFactory<ControlPlaneAssemblyMarker>
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
            if (!usePreviousRevision)
            {
                return;
            }

            builder.ConfigureServices(services =>
            {
                services.RemoveAll<LocalFileDefinitionRegistry>();
                services.RemoveAll<IDefinitionBundleManager>();
                services.RemoveAll<IDefinitionApprovalManager>();
                services.RemoveAll<InMemoryDefinitionRegistry>();
                services.AddSingleton(
                    new InMemoryDefinitionRegistry(
                    [
                        new RegisteredDecisionDefinition(
                            "tetris-demo",
                            "dev",
                            "tetris.dropInterval",
                            new RuntimeContractIdentity(
                                "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
                                "sha256:313cf567ee322f3f7028a48095cd4da6016d9d99759761d5651ac4ce84f3ff4e",
                                "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K",
                                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                            "number",
                            JsonSerializer.SerializeToElement(800),
                            "safe_default_drop_interval",
                            [],
                            [])
                    ],
                    new SequenceDefinitionIdentityGenerator()));
                services.AddSingleton<IDefinitionBundleManager>(
                    provider => provider.GetRequiredService<InMemoryDefinitionRegistry>());
                services.AddSingleton<IDefinitionApprovalManager>(
                    provider => provider.GetRequiredService<InMemoryDefinitionRegistry>());
            });
        }
    }
}
