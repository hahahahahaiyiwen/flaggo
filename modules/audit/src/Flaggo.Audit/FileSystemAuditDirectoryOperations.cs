using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

internal sealed class FileSystemAuditDirectoryOperations :
    IDurableDirectoryOperations
{
    public static FileSystemAuditDirectoryOperations Instance { get; } = new();

    public bool Exists(string path) => Directory.Exists(path);

    public void Create(string path) => Directory.CreateDirectory(path);

    public void Flush(string path) => DurableDirectory.Flush(path);
}
