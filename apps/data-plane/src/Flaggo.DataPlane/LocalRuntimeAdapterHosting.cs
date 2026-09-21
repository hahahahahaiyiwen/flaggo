using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Evidence;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.DataPlane;

public static class LocalRuntimeAdapterHosting
{
    public static void AddDecisionSnapshotScope(
        IServiceCollection services,
        IConfiguration configuration)
    {
        var generationPath =
            configuration["Flaggo:Bootstrap:LocalGenerationPath"];
        if (!string.IsNullOrWhiteSpace(generationPath))
        {
            services.AddScoped(
                _ => new BootstrapGenerationResolver(generationPath));
            services.AddScoped<IStateSnapshotProvider>(
                provider => provider.GetRequiredService<
                    BootstrapGenerationResolver>());
            services.AddScoped<IEvidenceSnapshotProvider>(
                provider => provider.GetRequiredService<
                    BootstrapGenerationResolver>());
            return;
        }

        var stateDescriptorPath =
            configuration["Flaggo:State:LocalFilePath"];
        var evidenceDescriptorPath =
            configuration["Flaggo:Evidence:LocalFilePath"];
        if (string.IsNullOrWhiteSpace(stateDescriptorPath) &&
            string.IsNullOrWhiteSpace(evidenceDescriptorPath))
        {
            return;
        }

        services.AddScoped(
            _ => new DirectCommittedSnapshotResolver(
                stateDescriptorPath,
                evidenceDescriptorPath));
        if (!string.IsNullOrWhiteSpace(stateDescriptorPath))
        {
            services.AddScoped<IStateSnapshotProvider>(
                provider => provider.GetRequiredService<
                    DirectCommittedSnapshotResolver>());
        }

        if (!string.IsNullOrWhiteSpace(evidenceDescriptorPath))
        {
            services.AddScoped<IEvidenceSnapshotProvider>(
                provider => provider.GetRequiredService<
                    DirectCommittedSnapshotResolver>());
        }
    }

    public static void AddStateAdapter(
        IServiceCollection services,
        IConfiguration configuration,
        RuntimeContractIdentity defaultIdentity)
    {
        var directCommitPath = configuration["Flaggo:State:LocalFilePath"];
        var generationPath =
            configuration["Flaggo:Bootstrap:LocalGenerationPath"];
        if (!string.IsNullOrWhiteSpace(generationPath) ||
            !string.IsNullOrWhiteSpace(directCommitPath))
        {
            services.AddSingleton<LocalFileStateSnapshotCache>();
            services.AddScoped<LocalFileStateStore>(
                provider => new LocalFileStateStore(
                    provider.GetRequiredService<IStateSnapshotProvider>(),
                    provider.GetRequiredService<LocalFileStateSnapshotCache>()));
            services.AddScoped<IStateStore>(
                provider => provider.GetRequiredService<LocalFileStateStore>());
            services.AddScoped<IStateHealth>(
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
        IConfiguration configuration)
    {
        var directCommitPath =
            configuration["Flaggo:Evidence:LocalFilePath"];
        var generationPath =
            configuration["Flaggo:Bootstrap:LocalGenerationPath"];
        if (!string.IsNullOrWhiteSpace(generationPath) ||
            !string.IsNullOrWhiteSpace(directCommitPath))
        {
            services.AddScoped<LocalFileEvidenceProvider>(
                provider => new LocalFileEvidenceProvider(
                    provider.GetRequiredService<IEvidenceSnapshotProvider>()));
            services.AddScoped<IEvidenceProvider>(
                provider => provider.GetRequiredService<LocalFileEvidenceProvider>());
            services.AddScoped<IEvidenceHealth>(
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
