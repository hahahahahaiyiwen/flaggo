using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Hosting;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class DefinitionProjectionTests
{
    [Fact]
    public void SeededTetrisDefinition_HasCompleteIntelligenceProjection()
    {
        var runtime = LocalRegistryHosting.DefaultDefinitions()
            .Single(definition => definition.LifecycleStatus == "active");
        var intelligence = Assert.Single(
            LocalRegistryHosting.DefaultIntelligenceDefinitions([runtime]));

        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(200, runtime.NumberActionSpace!.Minimum);
        Assert.Equal(1500, runtime.NumberActionSpace.Maximum);
        Assert.Equal(50, runtime.NumberActionSpace.Step);
        Assert.Equal(0.7, runtime.Policy!.MinimumEvidenceQuality);
        Assert.Equal(
            "tetris.earlyLossRate24h",
            intelligence.Objectives.Primary!.SignalKey);
        Assert.Equal(
            "approved-strategy",
            intelligence.WorkflowPermissions.Mode);
        Assert.Equal(0.7, intelligence.SafetyEnvelope.MinimumEvidenceQuality);
        Assert.Equal(
            runtime.Inputs.Select(input => input.Key).Order(StringComparer.Ordinal),
            intelligence.WorkflowPermissions.LiveInputs.Order(StringComparer.Ordinal));
    }

    [Fact]
    public async Task ApprovedDefinition_ExposesTypedRuntimeAndIntelligenceProjections()
    {
        var registry = new InMemoryDefinitionRegistry(
            [],
            new SequenceDefinitionIdentityGenerator());
        var bundle = NewDefinitionBundle();
        var validation = await registry.ValidateAsync(bundle);
        Assert.True(
            validation.Status == "valid",
            JsonSerializer.Serialize(validation.Issues));

        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("projection-apply", bundle)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("projection-test"),
            "Approve projection test definition.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;

        var runtimeLookup = await registry.ResolveRuntimeAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None);
        var intelligenceLookup = await registry.ResolveIntelligenceAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None);

        var runtime = Assert.IsType<RuntimeDecisionDefinition>(
            runtimeLookup.Definition);
        var intelligence = Assert.IsType<IntelligenceLifecycleDefinitionSnapshot>(
            intelligenceLookup.Definition);
        Assert.Equal(accepted.ContractDigest, runtime.Identity.ContractDigest);
        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(
            ["session", "user", "cohort", "global"],
            runtime.TargetHierarchy);
        Assert.Equal("session", runtime.InferenceTarget);
        Assert.Equal(["cohort", "global"], runtime.FallbackOrder);
        var session = Assert.Single(
            runtime.RuntimeContext,
            field => field.Key == "sessionId");
        Assert.Equal("session", session.TargetType);
        Assert.True(session.Required);

        Assert.Equal(
            "tetris.earlyLossRate24h",
            intelligence.Objectives.Primary!.SignalKey);
        Assert.Equal("minimize", intelligence.Objectives.Primary.Direction);
        Assert.Contains(
            "tetris.boardPressure",
            intelligence.SignalRoles.Allowed);
        Assert.DoesNotContain(
            "tetris.piecePlaced",
            intelligence.SignalRoles.Allowed);
        Assert.DoesNotContain(
            "tetris.sessionEnded",
            intelligence.SignalRoles.Allowed);
        Assert.Contains(
            "tetris.earlyLossRate24h",
            intelligence.SignalRoles.Evidence);
        Assert.Contains(
            "tetris.hardDropRate24h",
            intelligence.SignalRoles.Guardrails);
        Assert.Equal(
            "approved-strategy",
            intelligence.WorkflowPermissions.Mode);
        Assert.Equal("number", intelligence.ActionSpace.ValueType);
        Assert.Equal(200, intelligence.ActionSpace.Minimum);
        Assert.Equal(1500, intelligence.ActionSpace.Maximum);
        Assert.Equal(50, intelligence.ActionSpace.Step);
        Assert.Equal(800, intelligence.ActionSpace.DefaultValue.GetDouble());
        Assert.Equal(20, intelligence.SafetyEnvelope.CooldownSeconds);
        Assert.Equal(50, intelligence.SafetyEnvelope.MaximumDelta);
        Assert.Equal(
            0.35,
            intelligence.SafetyEnvelope.MaximumModelUncertainty);
    }

    [Fact]
    public async Task LocalRegistry_ReloadsBothDefinitionProjections()
    {
        using var file = new TestRegistryFile();
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(
            options,
            [],
            new SequenceDefinitionIdentityGenerator(),
            new FixedTimeProvider());
        var bundle = NewDefinitionBundle();
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await first.ApplyAsync("projection-reload", bundle)).Body);
        var approved = await first.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("projection-test"),
            "Approve persisted projections.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;

        var restarted = new LocalFileDefinitionRegistry(
            options,
            [],
            new SequenceDefinitionIdentityGenerator(),
            new FixedTimeProvider());
        var runtime = (await restarted.ResolveRuntimeAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None)).Definition;
        var intelligence = (await restarted.ResolveIntelligenceAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None)).Definition;

        Assert.NotNull(runtime);
        Assert.NotNull(intelligence);
        Assert.Equal(runtime.Identity, intelligence.Identity);
        Assert.Equal(["cohort", "global"], runtime.FallbackOrder);
        Assert.Equal(
            "tetris.earlyLossRate24h",
            intelligence.Objectives.Primary!.SignalKey);
    }

    [Fact]
    public async Task LocalRegistry_LoadsLegacyRuntimeOnlyPersistence()
    {
        using var file = new TestRegistryFile();
        var runtime = LocalRegistryHosting.DefaultDefinitions()
            .Single(definition => definition.LifecycleStatus == "active");
        var intelligence = Assert.Single(
            LocalRegistryHosting.DefaultIntelligenceDefinitions([runtime]));
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(
            options,
            [runtime],
            seedIntelligenceDefinitions: [intelligence]);
        await first.ResolveRuntimeAsync(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            runtime.Identity.DefinitionId,
            runtime.Identity.Revision,
            CancellationToken.None);
        var persisted = JsonNode.Parse(await File.ReadAllTextAsync(file.Path))!
            .AsObject();
        persisted.Remove("intelligenceDefinitions");
        foreach (var definition in persisted["definitions"]!.AsArray())
        {
            var value = definition!.AsObject();
            value.Remove("numberActionSpace");
            value.Remove("policy");
            value.Remove("targetHierarchy");
            value.Remove("inferenceTarget");
            value.Remove("fallbackOrder");
            foreach (var field in value["runtimeContext"]!.AsArray())
            {
                field!.AsObject().Remove("required");
                field.AsObject().Remove("targetType");
            }
        }

        await File.WriteAllTextAsync(file.Path, persisted.ToJsonString());

        var restarted = new LocalFileDefinitionRegistry(
            options,
            [runtime],
            seedIntelligenceDefinitions: [intelligence]);
        var restoredRuntime = (await restarted.ResolveRuntimeAsync(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            runtime.Identity.DefinitionId,
            runtime.Identity.Revision,
            CancellationToken.None)).Definition;
        var restoredIntelligence = (await restarted.ResolveIntelligenceAsync(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            runtime.Identity.DefinitionId,
            runtime.Identity.Revision,
            CancellationToken.None)).Definition;

        Assert.NotNull(restoredRuntime);
        Assert.NotNull(restoredIntelligence);
        Assert.Equal(
            ["session", "user", "cohort", "global"],
            restoredRuntime.TargetHierarchy);
        Assert.Equal(
            ["user", "cohort", "global"],
            restoredRuntime.FallbackOrder);
        Assert.Equal(
            "session",
            restoredRuntime.RuntimeContext.Single(
                field => field.Key == "sessionId").TargetType);
        Assert.Equal(200, restoredRuntime.NumberActionSpace!.Minimum);
        Assert.Equal(0.7, restoredRuntime.Policy!.MinimumEvidenceQuality);
        Assert.Equal(restoredRuntime.Identity, restoredIntelligence.Identity);
        Assert.Equal(
            "tetris.earlyLossRate24h",
            restoredIntelligence.Objectives.Primary!.SignalKey);
        Assert.Equal(
            0.7,
            restoredIntelligence.SafetyEnvelope.MinimumEvidenceQuality);
    }

    [Fact]
    public async Task LocalRegistry_LegacyRuntimeWithoutMatchingIntelligenceFailsClosed()
    {
        using var file = new TestRegistryFile();
        var runtime = LocalRegistryHosting.DefaultDefinitions()
            .Single(definition => definition.LifecycleStatus == "active");
        var mismatchedIntelligence = Assert.Single(
            LocalRegistryHosting.DefaultIntelligenceDefinitions([runtime])) with
        {
            Identity = runtime.Identity with
            {
                ContractDigest = $"sha256:{new string('c', 64)}"
            }
        };
        var options = new LocalFileDefinitionRegistryOptions(file.Path);
        var first = new LocalFileDefinitionRegistry(options, [runtime]);
        await first.ResolveRuntimeAsync(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            runtime.Identity.DefinitionId,
            runtime.Identity.Revision,
            CancellationToken.None);
        var persisted = JsonNode.Parse(await File.ReadAllTextAsync(file.Path))!
            .AsObject();
        persisted.Remove("intelligenceDefinitions");
        await File.WriteAllTextAsync(file.Path, persisted.ToJsonString());

        var restarted = new LocalFileDefinitionRegistry(
            options,
            [runtime],
            seedIntelligenceDefinitions: [mismatchedIntelligence]);
        var restored = await restarted.ResolveIntelligenceAsync(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            runtime.Identity.DefinitionId,
            runtime.Identity.Revision,
            CancellationToken.None);

        Assert.True(restored.DecisionKeyExists);
        Assert.Null(restored.Definition);
    }

    [Fact]
    public void RuntimeDefinition_OmittedFallbackOrderRemainsEmpty()
    {
        var definition = new RuntimeDecisionDefinition(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            new RuntimeContractIdentity(
                "def-test",
                $"sha256:{new string('a', 64)}",
                "rev-test",
                $"sha256:{new string('b', 64)}"),
            "number",
            JsonSerializer.SerializeToElement(800),
            "safe-default",
            [],
            [],
            TargetHierarchy: ["session", "user", "global"],
            InferenceTarget: "session");

        Assert.Empty(definition.FallbackOrder);
    }

    [Fact]
    public async Task ApprovedDefinition_OmittedFallbackOrderRemainsEmpty()
    {
        var registry = new InMemoryDefinitionRegistry(
            [],
            new SequenceDefinitionIdentityGenerator());
        var node = JsonNode.Parse(
            NewDefinitionBundle().GetRawText())!.AsObject();
        node["definitions"]![0]!["inference"]!
            .AsObject()
            .Remove("fallbackOrder");
        using var document = JsonDocument.Parse(node.ToJsonString());
        var bundle = document.RootElement.Clone();
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync("omitted-fallback-order", bundle)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("projection-test"),
            "Approve explicit fallback omission.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;

        var runtime = (await registry.ResolveRuntimeAsync(
            "tetris-demo",
            "dev",
            "tetris.dropInterval",
            accepted.DefinitionId,
            accepted.Revision,
            CancellationToken.None)).Definition;

        Assert.NotNull(runtime);
        Assert.Empty(runtime.FallbackOrder);
    }

    [Fact]
    public async Task ContextPropertyOrder_DoesNotChangeDerivedInferenceTarget()
    {
        var firstBundle = DefinitionWithoutExplicitTargetPrecedence(
            "userId",
            "sessionId");
        var secondBundle = DefinitionWithoutExplicitTargetPrecedence(
            "sessionId",
            "userId");
        Assert.Equal(
            CanonicalJson.ContractDigest(
                firstBundle.GetProperty("definitions")[0]),
            CanonicalJson.ContractDigest(
                secondBundle.GetProperty("definitions")[0]));

        var first = await ApproveSingleDefinitionAsync(
            firstBundle,
            "ordered-context-first");
        var second = await ApproveSingleDefinitionAsync(
            secondBundle,
            "ordered-context-second");

        Assert.Equal("session", first.InferenceTarget);
        Assert.Equal(first.InferenceTarget, second.InferenceTarget);
        Assert.Equal(first.TargetHierarchy, second.TargetHierarchy);
    }

    [Fact]
    public async Task Validation_RejectsTargetBearingContextOutsideHierarchy()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["targetHierarchy"] = new JsonArray(
            "cohort",
            "global");
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-targeting");
    }

    [Fact]
    public async Task Validation_RejectsActionSpaceWithoutDefault()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["actionSpace"]!.AsObject().Remove("default");
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsSignalRoleWithoutKey()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["signals"]!["allowed"]![0]!
            .AsObject()
            .Remove("key");
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsFallbackOutsideNumericActionSpace()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["fallback"]!["value"] = 5000;
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsDefaultNotAlignedToNumericStep()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["actionSpace"]!["default"] = 825;
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsUnsupportedIntentType()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["intent"]!["type"] = "unsupported";
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-objective");
    }

    [Fact]
    public async Task Validation_RejectsUnsupportedWorkflowMode()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["onlineStrategy"]!["mode"] = "unsupported";
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-strategy");
    }

    [Fact]
    public async Task Validation_RejectsUndeclaredSignalRole()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["signals"]!["evidence"] = new JsonArray(
            new JsonObject
            {
                ["key"] = "tetris.unknown"
            });
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsStringFallbackOutsideAllowedValues()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        var definition = node["definitions"]![0]!.AsObject();
        definition["valueType"] = "string";
        definition["actionSpace"] = new JsonObject
        {
            ["type"] = "string",
            ["default"] = "balanced",
            ["allowedValues"] = new JsonArray("balanced", "fast")
        };
        definition["fallback"]!["value"] = "unsupported";
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsDuplicateStringAllowedValues()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        var definition = node["definitions"]![0]!.AsObject();
        definition["valueType"] = "string";
        definition["actionSpace"] = new JsonObject
        {
            ["type"] = "string",
            ["default"] = "balanced",
            ["allowedValues"] = new JsonArray("balanced", "balanced")
        };
        definition["fallback"]!["value"] = "balanced";
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-definition");
    }

    [Fact]
    public async Task Validation_RejectsNaturalLanguageIntentWithoutText()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["intent"] = new JsonObject
        {
            ["type"] = "natural-language"
        };
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-objective");
    }

    [Fact]
    public async Task Validation_RejectsNonnumericMetricObjective()
    {
        var registry = new InMemoryDefinitionRegistry([]);
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        node["definitions"]![0]!["intent"]!["primary"]!["signal"]!["key"] =
            "tetris.piecePlaced";
        using var document = JsonDocument.Parse(node.ToJsonString());

        var result = await registry.ValidateAsync(document.RootElement);

        Assert.Equal("invalid", result.Status);
        Assert.Contains(result.Issues, issue => issue.Code == "invalid-objective");
    }

    private static JsonElement FixtureBody(string file)
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot,
            "contracts",
            "fixtures",
            "management",
            "definition-bundle",
            file);
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        return document.RootElement.GetProperty("request").GetProperty("body").Clone();
    }

    private static JsonElement NewDefinitionBundle()
    {
        var node = JsonNode.Parse(
            FixtureBody("04-apply-approved-receipt.json").GetRawText())!.AsObject();
        var definition = node["definitions"]![0]!.AsObject();
        definition.Remove("definitionId");
        definition["runtimeContextSchema"]!["sessionId"]!["required"] = true;
        definition["signals"]!["evidence"] = new JsonArray(
            new JsonObject
            {
                ["key"] = "tetris.earlyLossRate24h"
            });
        definition["signals"]!["guardrails"] = new JsonArray(
            new JsonObject
            {
                ["key"] = "tetris.hardDropRate24h"
            });
        using var document = JsonDocument.Parse(node.ToJsonString());
        return document.RootElement.Clone();
    }

    private static JsonElement DefinitionWithoutExplicitTargetPrecedence(
        string firstContextKey,
        string secondContextKey)
    {
        var node = JsonNode.Parse(
            NewDefinitionBundle().GetRawText())!.AsObject();
        var definition = node["definitions"]![0]!.AsObject();
        definition.Remove("targetHierarchy");
        definition.Remove("inference");
        definition.Remove("onlineStrategy");
        var original = definition["runtimeContextSchema"]!.AsObject();
        definition["runtimeContextSchema"] = new JsonObject
        {
            [firstContextKey] = original[firstContextKey]!.DeepClone(),
            [secondContextKey] = original[secondContextKey]!.DeepClone(),
            ["cohort"] = original["cohort"]!.DeepClone(),
            ["deviceType"] = original["deviceType"]!.DeepClone()
        };
        using var document = JsonDocument.Parse(node.ToJsonString());
        return document.RootElement.Clone();
    }

    private static async Task<RuntimeDecisionDefinition>
        ApproveSingleDefinitionAsync(
            JsonElement bundle,
            string idempotencyKey)
    {
        var registry = new InMemoryDefinitionRegistry(
            [],
            new SequenceDefinitionIdentityGenerator());
        var pending = Assert.IsType<RequiresApprovalResult>(
            (await registry.ApplyAsync(idempotencyKey, bundle)).Body);
        var approved = await registry.ApproveAsync(
            pending.ApprovalRequestId,
            pending.BundleDigest,
            new ApprovalActor("projection-test"),
            "Approve deterministic target precedence.");
        var accepted = Assert.Single(approved.Receipt!.AcceptedDefinitions).Value;
        return Assert.IsType<RuntimeDecisionDefinition>(
            (await registry.ResolveRuntimeAsync(
                "tetris-demo",
                "dev",
                "tetris.dropInterval",
                accepted.DefinitionId,
                accepted.Revision,
                CancellationToken.None)).Definition);
    }

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 8, 18, 17, 0, 0, TimeSpan.Zero);
    }
}
