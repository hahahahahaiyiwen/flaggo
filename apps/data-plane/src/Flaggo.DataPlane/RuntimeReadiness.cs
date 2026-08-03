using Flaggo.Audit;
using Flaggo.Registry;
using Flaggo.State;

namespace Flaggo.DataPlane;

public sealed record RuntimeDependencyCheck(
    string Name,
    string Status,
    bool Required);

public sealed record RuntimeReadinessResult(
    string Status,
    string ObservedAt,
    IReadOnlyList<RuntimeDependencyCheck> Checks);

public interface IRuntimeReadinessProbe
{
    Task<IReadOnlyList<RuntimeDependencyCheck>> CheckAsync(
        CancellationToken cancellationToken);
}

public sealed class RuntimeReadinessProbe(
    IRegistryHealth registry,
    IStateHealth state,
    IAuditHealth audit,
    IConfiguration configuration)
    : IRuntimeReadinessProbe
{
    public async Task<IReadOnlyList<RuntimeDependencyCheck>> CheckAsync(
        CancellationToken cancellationToken)
    {
        var registryAvailable = await registry.IsAvailableAsync(cancellationToken);
        var stateAvailable = await state.IsAvailableAsync(cancellationToken);
        var auditAvailable = await audit.IsAvailableAsync(cancellationToken);
        var evidenceAvailable = !string.Equals(
            configuration["Flaggo:Health:Evidence"],
            "down",
            StringComparison.OrdinalIgnoreCase);
        return
        [
            Check("contract-registry", registryAvailable, true),
            Check("decision-state", stateAvailable, true),
            Check("policy", true, true),
            Check("audit", auditAvailable, true),
            Check("evidence", evidenceAvailable, false)
        ];
    }

    private static RuntimeDependencyCheck Check(
        string name,
        bool available,
        bool required) =>
        new(name, available ? "up" : "down", required);
}
