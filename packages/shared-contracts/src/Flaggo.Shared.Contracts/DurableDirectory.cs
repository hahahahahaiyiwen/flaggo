using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Flaggo.Shared.Contracts;

public static class DurableDirectory
{
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareRead = 0x00000001;
    private const uint FileShareWrite = 0x00000002;
    private const uint FileShareDelete = 0x00000004;
    private const uint OpenExisting = 3;
    private const uint FileFlagBackupSemantics = 0x02000000;

    public static void Create(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        Create(Path.GetFullPath(path), FileSystemDirectoryOperations.Instance);
    }

    public static void Flush(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        if (OperatingSystem.IsWindows())
        {
            FlushWindows(path);
            return;
        }

        FlushUnix(path);
    }

    internal static bool IsUnsupportedUnixError(int error) =>
        error is 22 or 45 or 95;

    internal static bool IsUnsupportedWindowsError(int error) =>
        error is 1 or 50 or 87;

    internal static void Create(
        string path,
        IDurableDirectoryOperations operations)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        ArgumentNullException.ThrowIfNull(operations);

        var fullPath = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
        var missing = new Stack<string>();
        var cursor = fullPath;
        while (!operations.Exists(cursor))
        {
            var parent = Path.GetDirectoryName(cursor);
            if (string.IsNullOrEmpty(parent) || parent == cursor)
            {
                throw new DirectoryNotFoundException(
                    $"No existing ancestor was found for '{fullPath}'.");
            }

            missing.Push(cursor);
            cursor = parent;
        }

        if (missing.Count > 0)
        {
            FlushBoundary(cursor, operations);
        }

        var durableParent = cursor;
        while (missing.TryPop(out var directory))
        {
            try
            {
                operations.Create(directory);
            }
            catch (IOException) when (operations.Exists(directory))
            {
                // A concurrent creator won the race.
            }

            operations.Flush(durableParent);
            operations.Flush(directory);
            durableParent = directory;
        }

        FlushBoundary(fullPath, operations);
    }

    private static void FlushBoundary(
        string path,
        IDurableDirectoryOperations operations)
    {
        var immediateParent = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(immediateParent) &&
            !string.Equals(immediateParent, path, StringComparison.Ordinal))
        {
            operations.Flush(immediateParent);
        }

        operations.Flush(path);
    }

    private static void FlushUnix(string path)
    {
        var descriptor = Open(path, 0);
        if (descriptor < 0)
        {
            throw NativeIOException(
                "open the directory for durable synchronization",
                path,
                Marshal.GetLastPInvokeError());
        }

        using var handle = new SafeFileHandle((IntPtr)descriptor, ownsHandle: true);
        if (Fsync(descriptor) == 0)
        {
            return;
        }

        var error = Marshal.GetLastPInvokeError();
        if (IsUnsupportedUnixError(error))
        {
            return;
        }

        throw NativeIOException(
            "synchronize the directory",
            path,
            error);
    }

    private static void FlushWindows(string path)
    {
        using var handle = CreateFile(
            path,
            GenericWrite,
            FileShareRead | FileShareWrite | FileShareDelete,
            IntPtr.Zero,
            OpenExisting,
            FileFlagBackupSemantics,
            IntPtr.Zero);
        if (handle.IsInvalid)
        {
            throw NativeIOException(
                "open the directory for durable synchronization",
                path,
                Marshal.GetLastPInvokeError());
        }

        if (FlushFileBuffers(handle))
        {
            return;
        }

        var error = Marshal.GetLastPInvokeError();
        if (IsUnsupportedWindowsError(error))
        {
            return;
        }

        throw NativeIOException(
            "synchronize the directory",
            path,
            error);
    }

    private static IOException NativeIOException(
        string operation,
        string path,
        int error) =>
        new(
            $"Failed to {operation} '{path}'.",
            new Win32Exception(error));

    [DllImport("libc", EntryPoint = "open", SetLastError = true)]
    private static extern int Open(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        int flags);

    [DllImport("libc", EntryPoint = "fsync", SetLastError = true)]
    private static extern int Fsync(int descriptor);

    [DllImport(
        "kernel32.dll",
        EntryPoint = "CreateFileW",
        SetLastError = true,
        CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool FlushFileBuffers(SafeFileHandle handle);

    private sealed class FileSystemDirectoryOperations :
        IDurableDirectoryOperations
    {
        public static FileSystemDirectoryOperations Instance { get; } = new();

        public bool Exists(string path) => Directory.Exists(path);

        public void Create(string path) => Directory.CreateDirectory(path);

        public void Flush(string path) => DurableDirectory.Flush(path);
    }
}

internal interface IDurableDirectoryOperations
{
    bool Exists(string path);

    void Create(string path);

    void Flush(string path);
}
