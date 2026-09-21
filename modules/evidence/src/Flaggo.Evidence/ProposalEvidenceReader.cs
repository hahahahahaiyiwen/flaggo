using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Evidence;

public sealed record ProposalEvidenceRequest(
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef ControlTarget,
    IReadOnlyList<string> References);

public interface IProposalEvidenceReader
{
    Task<IReadOnlyList<ProposalEvidenceSnapshot>> GetAsync(
        ProposalEvidenceRequest request,
        CancellationToken cancellationToken);
}

public sealed record ProposalEvidenceDocument(
    int Version,
    IReadOnlyList<ProposalEvidenceSnapshot> Snapshots);

public sealed class InMemoryProposalEvidenceReader : IProposalEvidenceReader
{
    private readonly IReadOnlyDictionary<string, ProposalEvidenceSnapshot> _snapshots;

    public InMemoryProposalEvidenceReader(IReadOnlyList<ProposalEvidenceSnapshot> snapshots)
    {
        ArgumentNullException.ThrowIfNull(snapshots);
        foreach (var snapshot in snapshots)
        {
            ProposalEvidenceValidation.Validate(snapshot);
        }
        if (snapshots.Select(snapshot => snapshot.Reference).Distinct(StringComparer.Ordinal).Count() != snapshots.Count)
        {
            throw new InvalidDataException("Proposal evidence references must be unique and immutable.");
        }
        _snapshots = LifecycleJson.Copy(snapshots.ToArray())
            .ToDictionary(snapshot => snapshot.Reference, StringComparer.Ordinal);
    }

    public Task<IReadOnlyList<ProposalEvidenceSnapshot>> GetAsync(
        ProposalEvidenceRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ArgumentNullException.ThrowIfNull(request);
        var resolved = new List<ProposalEvidenceSnapshot>();
        foreach (var reference in request.References.Distinct(StringComparer.Ordinal).Order(StringComparer.Ordinal))
        {
            if (!_snapshots.TryGetValue(reference, out var snapshot))
            {
                continue;
            }
            if (!snapshot.Definition.HasSameDefinition(request.Definition) || snapshot.ControlTarget != request.ControlTarget)
            {
                throw new InvalidDataException("The referenced evidence belongs to another definition or control target.");
            }
            resolved.Add(LifecycleJson.Copy(snapshot));
        }
        return Task.FromResult<IReadOnlyList<ProposalEvidenceSnapshot>>(resolved);
    }
}

public sealed class LocalFileProposalEvidenceReader : IProposalEvidenceReader
{
    private readonly CommittedFileSnapshotSource _source;

    public LocalFileProposalEvidenceReader(string commitDescriptorPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(commitDescriptorPath);
        _source = CommittedFileSnapshotSource.FromDescriptor(commitDescriptorPath);
    }

    public async Task<IReadOnlyList<ProposalEvidenceSnapshot>> GetAsync(
        ProposalEvidenceRequest request,
        CancellationToken cancellationToken)
    {
        try
        {
            var bytes = await CommittedFileSnapshot.ReadAsync(_source, null, cancellationToken);
            var document = LifecycleJson.Read<ProposalEvidenceDocument>(bytes);
            if (document.Version != 1 || document.Snapshots is null)
            {
                throw new InvalidDataException("The proposal evidence catalog must use version 1.");
            }
            return await new InMemoryProposalEvidenceReader(document.Snapshots)
                .GetAsync(request, cancellationToken);
        }
        catch (Exception error) when (error is IOException or InvalidDataException or
            UnauthorizedAccessException or JsonException)
        {
            throw new EvidenceUnavailableException("The configured proposal evidence catalog is unavailable.", error);
        }
    }
}
