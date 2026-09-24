using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class DefinitionProjectionTests
{
    private const string Key = "tetris.dropInterval";

    [Fact]
    public void SeededTetrisDefinition_ComesFromApprovedManifestWithoutImplicitQualityEvidence()
    {
        var runtime = LocalRegistryHosting.DefaultDefinitions()
            .Single(definition => definition.LifecycleStatus == "active");
        var intelligence = Assert.Single(LocalRegistryHosting.DefaultIntelligenceDefinitions([runtime]));

        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(CanonicalJson.ContractDigest(Key, FixtureBody().GetProperty("decisions").GetProperty(Key)),
            runtime.Identity.ContractDigest);
        Assert.Equal(200, runtime.NumberActionSpace!.Minimum);
        Assert.Equal(1500, runtime.NumberActionSpace.Maximum);
        Assert.Equal(50, runtime.NumberActionSpace.Step);
        Assert.False(runtime.Policy!.RequiresEvidence);
        Assert.Null(intelligence.Objectives.Primary);
        Assert.NotEmpty(intelligence.Objectives.NaturalLanguage!);
        Assert.Empty(intelligence.Evidence);
        Assert.All(runtime.Inputs, input => Assert.Equal("request", input.Source));
    }

    [Fact]
    public async Task ApprovedManifest_ProjectsTypedInputsEvidenceAndObjectives()
    {
        var registry = new InMemoryDefinitionRegistry([], new SequenceDefinitionIdentityGenerator());
        var runtime = await ApproveAsync(registry, EvidenceBundle());
        var intelligence = Assert.IsType<IntelligenceLifecycleDefinitionSnapshot>(
            (await registry.ResolveIntelligenceAsync(runtime.AppId, runtime.Environment, Key,
                runtime.Identity.DefinitionId, runtime.Identity.Revision, CancellationToken.None)).Definition);

        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(["session", "user", "cohort", "global"], runtime.TargetHierarchy);
        Assert.Equal("session", runtime.InferenceTarget);
        Assert.Equal(["cohort", "global"], runtime.FallbackOrder);
        Assert.True(Assert.Single(runtime.RuntimeContext, field => field.Key == "sessionId").Required);
        var input = Assert.Single(runtime.Inputs, input => input.Key == "duration");
        Assert.Equal("evidence", input.Source);
        Assert.Equal("placement", input.Binding);
        Assert.Equal("number", input.ValueType);
        Assert.Equal("ms", input.Unit);
        Assert.Equal(0, input.Minimum);
        Assert.Equal(5000, input.Maximum);
        var binding = Assert.Single(runtime.Evidence);
        Assert.Equal(input.Meaning, binding.Meaning);
        Assert.Equal(binding, Assert.Single(intelligence.Evidence));
        Assert.Equal("game.place", binding.Source.Name);
        Assert.Equal("game", binding.Source.ScopeName);
        Assert.Equal("tetris-demo", binding.Source.ResourceAttributes["service.name"].GetString());
        Assert.Equal("duration", binding.Source.ValueFrom);
        Assert.Equal(new TelemetryAttributeSelector("attributes", "session.id"), binding.TargetIdAttribute);
        Assert.Equal(30, binding.MaxAgeSeconds);
        Assert.Equal(new DecisionObjective("placement", "minimize"), intelligence.Objectives.Primary);
        Assert.Equal(800, intelligence.ActionSpace.DefaultValue.GetDouble());
        Assert.Equal(20, intelligence.SafetyEnvelope.CooldownSeconds);
        Assert.Equal(50, intelligence.SafetyEnvelope.MaximumDelta);
        Assert.False(intelligence.SafetyEnvelope.RequiresEvidence);

        var projection = Assert.Single(await registry.ReadBindingsAsync(new("tetris-demo", "dev", "local-development"), CancellationToken.None));
        Assert.Equal(runtime.Identity, projection.Identity);
        Assert.Equal(binding, Assert.Single(projection.Bindings));
        Assert.Empty(await registry.ReadBindingsAsync(new("other-app", "dev", "local-development"), CancellationToken.None));
        Assert.Empty(await registry.ReadBindingsAsync(new("tetris-demo", "prod", "local-development"), CancellationToken.None));
    }

    [Fact]
    public async Task LocalRegistry_RestartPreservesBothProjectionsAndScopedBindings()
    {
        using var file = new TestRegistryFile();
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(options, [], new SequenceDefinitionIdentityGenerator());
        var pending = Assert.IsType<RequiresApprovalResult>((await first.ApplyAsync("projection-reload", EvidenceBundle())).Body);
        var approved = await first.ApproveAsync(pending.ApprovalRequestId, pending.BundleDigest,
            new ApprovalActor("projection-test"), "Approve persisted projections.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        var restarted = new LocalFileDefinitionRegistry(options, []);
        var runtime = Assert.IsType<RuntimeDecisionDefinition>(
            (await restarted.ResolveRuntimeAsync("tetris-demo", "dev", Key, accepted.DefinitionId,
                accepted.Revision, CancellationToken.None)).Definition);
        var intelligence = Assert.IsType<IntelligenceLifecycleDefinitionSnapshot>(
            (await restarted.ResolveIntelligenceAsync("tetris-demo", "dev", Key, accepted.DefinitionId,
                accepted.Revision, CancellationToken.None)).Definition);

        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(["cohort", "global"], runtime.FallbackOrder);
        Assert.Equal("placement", intelligence.Objectives.Primary!.EvidenceKey);
        var binding = Assert.Single((await restarted.ReadBindingsAsync(new("tetris-demo", "dev", "local-development"), CancellationToken.None))
            .Single().Bindings);
        Assert.Equal("duration", binding.Source.ValueFrom);
        Assert.Equal(30, binding.MaxAgeSeconds);
        Assert.Equal("ms", Assert.Single(runtime.Inputs, input => input.Source == "evidence").Unit);
    }

    [Fact]
    public async Task LocalRegistry_RejectsOldFormatWithoutMigratingOrOverwritingIt()
    {
        using var file = new TestRegistryFile();
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(options, LocalRegistryHosting.DefaultDefinitions());
        Assert.True(await first.IsAvailableAsync(CancellationToken.None));
        var persisted = JsonNode.Parse(await File.ReadAllTextAsync(file.Path))!.AsObject();
        persisted["version"] = 1;
        var oldContents = persisted.ToJsonString();
        await File.WriteAllTextAsync(file.Path, oldContents);

        var restarted = new LocalFileDefinitionRegistry(options, LocalRegistryHosting.DefaultDefinitions());
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(() =>
            restarted.ReadBindingsAsync(new("tetris-demo", "dev", "local-development"), CancellationToken.None));
        Assert.Equal(oldContents, await File.ReadAllTextAsync(file.Path));
    }

    [Theory]
    [InlineData("hierarchy")]
    [InlineData("primary")]
    [InlineData("fallbackOrder")]
    public void RuntimeProjection_RequiresExplicitTargeting(string missing)
    {
        Assert.Throws<ArgumentException>(() => new RuntimeDecisionDefinition(
            "app", "dev", "decision",
            new("definition", $"sha256:{new string('a', 64)}", "revision"),
            "number", JsonSerializer.SerializeToElement(1), "safe-default", [], [],
            TargetHierarchy: missing == "hierarchy" ? null : ["global"],
            InferenceTarget: missing == "primary" ? null : "global",
            FallbackOrder: missing == "fallbackOrder" ? null : []));
    }

    [Fact]
    public async Task ContextMapOrder_DoesNotChangeIdentityOrExplicitTargeting()
    {
        var first = JsonNode.Parse(EvidenceBundle().GetRawText())!.AsObject();
        var second = first.DeepClone().AsObject();
        var definition = second["decisions"]![Key]!;
        var context = definition["context"]!.AsObject();
        definition["context"] = new JsonObject(context.Reverse()
            .Select(field => KeyValuePair.Create<string, JsonNode?>(field.Key, field.Value!.DeepClone())));

        var left = JsonSerializer.SerializeToElement(first);
        var right = JsonSerializer.SerializeToElement(second);
        Assert.Equal(CanonicalJson.BundleDigest(left), CanonicalJson.BundleDigest(right));
        var a = await ApproveAsync(new([], new SequenceDefinitionIdentityGenerator()), left);
        var b = await ApproveAsync(new([], new SequenceDefinitionIdentityGenerator()), right);
        Assert.Equal(a.Identity.ContractDigest, b.Identity.ContractDigest);
        Assert.Equal(a.TargetHierarchy, b.TargetHierarchy);
        Assert.Equal("session", b.InferenceTarget);
    }

    [Theory]
    [InlineData("missing default")]
    [InlineData("out-of-bounds default")]
    [InlineData("unaligned default")]
    [InlineData("missing targeting")]
    [InlineData("missing fallback order")]
    [InlineData("unknown target")]
    [InlineData("duplicate target")]
    [InlineData("target outside hierarchy")]
    [InlineData("unknown binding")]
    [InlineData("optional target context")]
    [InlineData("unknown objective")]
    [InlineData("nonnumeric objective")]
    [InlineData("missing intent text")]
    [InlineData("obsolete strategy")]
    [InlineData("authored identity")]
    [InlineData("zero freshness")]
    [InlineData("overflow freshness")]
    [InlineData("unsupported projection")]
    [InlineData("wrong attribute namespace")]
    [InlineData("duplicate allowed strings")]
    [InlineData("invalid string default")]
    public async Task InvalidManifest_IsRejectedBeforePublication(string mutation)
    {
        var node = JsonNode.Parse(EvidenceBundle().GetRawText())!.AsObject();
        var definition = node["decisions"]![Key]!.AsObject();
        var binding = definition["evidence"]!["placement"]!;
        switch (mutation)
        {
            case "missing default": definition["result"]!.AsObject().Remove("default"); break;
            case "out-of-bounds default": definition["result"]!["default"] = 5000; break;
            case "unaligned default": definition["result"]!["default"] = 825; break;
            case "missing targeting": definition.Remove("targeting"); break;
            case "missing fallback order": definition["targeting"]!.AsObject().Remove("fallbackOrder"); break;
            case "unknown target": definition["targeting"]!["primary"] = "unknown"; break;
            case "duplicate target": definition["targeting"]!["hierarchy"]!.AsArray().Add("session"); break;
            case "target outside hierarchy":
                definition["targeting"]!["hierarchy"] = new JsonArray("cohort", "global");
                definition["targeting"]!["primary"] = "cohort";
                definition["targeting"]!["fallbackOrder"] = new JsonArray("global");
                break;
            case "unknown binding": definition["inputs"]!["duration"]!["binding"] = "unknown"; break;
            case "optional target context": definition["context"]!["sessionId"]!["required"] = false; break;
            case "unknown objective": definition["intent"]!["primary"]!["evidence"] = "unknown"; break;
            case "nonnumeric objective":
                binding["type"] = "boolean";
                binding.AsObject().Remove("range");
                binding.AsObject().Remove("unit");
                binding["source"]!["value"] = JsonNode.Parse("""{"from":"attributes","key":"enabled"}""");
                break;
            case "missing intent text": definition["intent"] = JsonNode.Parse("""{"type":"natural-language"}"""); break;
            case "obsolete strategy": definition["onlineStrategy"] = JsonNode.Parse("""{"mode":"approved-strategy"}"""); break;
            case "authored identity": definition["definitionId"] = "authored"; break;
            case "zero freshness": binding["freshness"]!["maxAgeSeconds"] = 0; break;
            case "overflow freshness": binding["freshness"]!["maxAgeSeconds"] = 922337203686L; break;
            case "unsupported projection": binding["projection"]!["kind"] = "average"; break;
            case "wrong attribute namespace": binding["target"]!["idAttribute"]!["from"] = "eventAttributes"; break;
            case "duplicate allowed strings":
                definition["result"] = JsonNode.Parse("""{"type":"string","default":"a","allowedValues":["a","a"]}""");
                break;
            case "invalid string default":
                definition["result"] = JsonNode.Parse("""{"type":"string","default":"z","allowedValues":["a","b"]}""");
                break;
            default: throw new ArgumentOutOfRangeException(nameof(mutation));
        }

        var registry = new InMemoryDefinitionRegistry([]);
        var bundle = JsonSerializer.SerializeToElement(node);
        var result = await registry.ValidateAsync(bundle);
        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue =>
            issue.Code == "invalid-definition" && issue.Path.StartsWith("/decisions/", StringComparison.Ordinal));
        await Assert.ThrowsAsync<DefinitionLifecycleException>(() => registry.ApplyAsync("invalid", bundle));
        Assert.Empty(await registry.ReadBindingsAsync(new("tetris-demo", "dev", "local-development"), CancellationToken.None));
    }

    private static JsonElement FixtureBody()
    {
        using var document = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(TestPaths.RepositoryRoot,
            "contracts", "fixtures", "management", "definition-bundle", "04-apply-approved-receipt.json")));
        return document.RootElement.GetProperty("request").GetProperty("body").Clone();
    }

    private static JsonElement EvidenceBundle()
    {
        var node = JsonNode.Parse(FixtureBody().GetRawText())!.AsObject();
        var definition = node["decisions"]![Key]!;
        definition["context"]!["sessionId"]!["required"] = true;
        definition["evidence"] = JsonNode.Parse("""
            {
              "placement": {
                "meaning": "Latest received completed placement duration.",
                "type": "number", "unit": "ms", "range": [0, 5000],
                "source": {
                  "kind": "span", "name": "game.place", "scope": { "name": "game" },
                  "resourceAttributes": { "service.name": "tetris-demo" },
                  "value": { "from": "duration" }
                },
                "projection": { "kind": "latest" },
                "target": { "type": "session", "idAttribute": { "from": "attributes", "key": "session.id" } },
                "freshness": { "maxAgeSeconds": 30 },
                "sampling": { "accept": "observed" },
                "attribution": { "kind": "none" }
              }
            }
            """);
        definition["inputs"]!["duration"] = JsonNode.Parse("""{"source":"evidence","binding":"placement"}""");
        definition["intent"] = JsonNode.Parse("""
            {"type":"numeric-objective","primary":{"evidence":"placement","direction":"minimize"}}
            """);
        return JsonSerializer.SerializeToElement(node);
    }

    private static async Task<RuntimeDecisionDefinition> ApproveAsync(
        InMemoryDefinitionRegistry registry, JsonElement bundle)
    {
        var validation = await registry.ValidateAsync(bundle);
        Assert.True(validation.Status == "valid", JsonSerializer.Serialize(validation.Issues));
        var pending = Assert.IsType<RequiresApprovalResult>((await registry.ApplyAsync("projection", bundle)).Body);
        var approved = await registry.ApproveAsync(pending.ApprovalRequestId, pending.BundleDigest,
            new ApprovalActor("projection-test"), "Approve manifest projection.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        return Assert.IsType<RuntimeDecisionDefinition>((await registry.ResolveRuntimeAsync(
            "tetris-demo", "dev", Key, accepted.DefinitionId, accepted.Revision, CancellationToken.None)).Definition);
    }
}
