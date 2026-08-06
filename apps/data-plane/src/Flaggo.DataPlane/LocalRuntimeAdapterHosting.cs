using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Evidence;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.DataPlane;

public static class LocalRuntimeAdapterHosting
{
    public static void AddStateAdapter(
        IServiceCollection services,
        IConfiguration configuration,
        RuntimeContractIdentity defaultIdentity,
        BootstrapGenerationPaths? bootstrapGeneration = null)
    {
        var statePath = bootstrapGeneration?.StatePath ??
            configuration["Flaggo:State:LocalFilePath"];
        if (!string.IsNullOrWhiteSpace(statePath))
        {
            services.AddSingleton(
                new LocalFileStateStore(
                    new LocalFileStateStoreOptions(statePath)));
            services.AddSingleton<IStateStore>(
                provider => provider.GetRequiredService<LocalFileStateStore>());
            services.AddSingleton<IStateHealth>(
                provider => provider.GetRequiredService<LocalFileStateStore>());
            return;
        }

        services.AddSingleton<IStateStore>(
            new InMemoryStateStore(
            [
                (
                    "tetris.dropInterval",
                    new GovernedDecisionState(
                        defaultIdentity.DefinitionId,
                        defaultIdentity.Revision,
                        defaultIdentity.ContractDigest,
                        JsonSerializer.SerializeToElement(800),
                        new DecisionTargetRef("cohort", "new_players")))
            ]));
        services.AddSingleton<IStateHealth>(
            provider => (IStateHealth)provider.GetRequiredService<IStateStore>());
    }

    public static void AddAuditAdapter(
        IServiceCollection services,
        IConfiguration configuration)
    {
        var auditPath = configuration["Flaggo:Audit:LocalFilePath"];
        if (!string.IsNullOrWhiteSpace(auditPath))
        {
            services.AddSingleton(
                new LocalFileAuditSink(
                    new LocalFileAuditSinkOptions(auditPath)));
            services.AddSingleton<IAuditSink>(
                provider => provider.GetRequiredService<LocalFileAuditSink>());
            services.AddSingleton<IExposureAuditSink>(
                provider => provider.GetRequiredService<LocalFileAuditSink>());
            services.AddSingleton<IAuditHealth>(
                provider => provider.GetRequiredService<LocalFileAuditSink>());
            return;
        }

        services.AddSingleton<InMemoryAuditSink>();
        services.AddSingleton<IAuditSink>(
            provider => provider.GetRequiredService<InMemoryAuditSink>());
        services.AddSingleton<IExposureAuditSink>(
            provider => provider.GetRequiredService<InMemoryAuditSink>());
        services.AddSingleton<IAuditHealth>(
            provider => provider.GetRequiredService<InMemoryAuditSink>());
    }

    public static void AddEvidenceAdapter(
        IServiceCollection services,
        IConfiguration configuration,
        BootstrapGenerationPaths? bootstrapGeneration = null)
    {
        var evidencePath = bootstrapGeneration?.EvidencePath ??
            configuration["Flaggo:Evidence:LocalFilePath"];
        if (!string.IsNullOrWhiteSpace(evidencePath))
        {
            services.AddSingleton(
                new LocalFileEvidenceProvider(
                    new LocalFileEvidenceProviderOptions(evidencePath)));
            services.AddSingleton<IEvidenceProvider>(
                provider => provider.GetRequiredService<LocalFileEvidenceProvider>());
            services.AddSingleton<IEvidenceHealth>(
                provider => provider.GetRequiredService<LocalFileEvidenceProvider>());
            return;
        }

        services.AddSingleton<InMemoryEvidenceProvider>();
        services.AddSingleton<IEvidenceProvider>(
            provider => provider.GetRequiredService<InMemoryEvidenceProvider>());
        services.AddSingleton<IEvidenceHealth>(
            provider => provider.GetRequiredService<InMemoryEvidenceProvider>());
    }
}
