using Flaggo.Audit;
using Flaggo.Evidence;
using Flaggo.Lifecycle;
using Flaggo.Policy;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Flaggo.ControlPlane;

public static class LocalLifecycleHosting
{
    public static void AddServices(IServiceCollection services, IConfiguration configuration)
    {
        services.TryAddSingleton(TimeProvider.System);
        services.AddHttpContextAccessor();
        services.AddScoped<ILifecycleActorProvider, HttpLifecycleActorProvider>();
        services.AddSingleton<ILifecyclePolicyEvaluator, DefaultLifecyclePolicyEvaluator>();
        services.AddSingleton<ILifecyclePolicyContextProvider>(_ =>
            new ConfigurationPolicyContextProvider(configuration));
        services.AddSingleton<IProposalEvidenceReader>(_ =>
        {
            var path = configuration["Flaggo:Lifecycle:EvidencePath"];
            return string.IsNullOrWhiteSpace(path)
                ? new InMemoryProposalEvidenceReader([])
                : new LocalFileProposalEvidenceReader(path);
        });
        services.AddSingleton(provider =>
        {
            var path = configuration["Flaggo:State:LocalFilePath"];
            if (string.IsNullOrWhiteSpace(path))
            {
                throw new InvalidOperationException(
                    "Flaggo:State:LocalFilePath must be configured before resolving lifecycle governance.");
            }
            if (!string.IsNullOrWhiteSpace(configuration["Flaggo:Bootstrap:LocalGenerationPath"]))
            {
                throw new InvalidOperationException(
                    "Lifecycle governance requires direct state composition, not a bootstrap generation.");
            }
            return new LocalFileGovernedStateLifecycleStore(
                new LocalFileGovernedStateLifecycleStoreOptions(path),
                provider.GetRequiredService<TimeProvider>());
        });
        services.AddSingleton<IGovernedStateLifecycleStore>(
            provider => provider.GetRequiredService<LocalFileGovernedStateLifecycleStore>());
        services.AddSingleton<ILifecycleAuditReader>(
            provider => provider.GetRequiredService<LocalFileGovernedStateLifecycleStore>());
        services.AddSingleton(provider => new LifecycleCommitOptions(
            configuration.GetValue("Flaggo:Lifecycle:CommitTimeout", TimeSpan.FromSeconds(5)),
            provider.GetRequiredService<IHostApplicationLifetime>().ApplicationStopping));
        services.AddScoped<IProposalGovernance, ProposalGovernance>();
    }

    private sealed class ConfigurationPolicyContextProvider(IConfiguration configuration)
        : ILifecyclePolicyContextProvider
    {
        public Task<LifecyclePolicyContext?> GetAsync(
            GovernedDefinitionIdentity definition, CancellationToken cancellationToken) =>
            new ConfiguredLifecyclePolicyContextProvider(
                configuration.GetSection("Flaggo:Lifecycle:Policies")
                    .Get<LifecyclePolicyContext[]>(options => options.ErrorOnUnknownConfiguration = true) ?? [])
                .GetAsync(definition, cancellationToken);
    }
}
