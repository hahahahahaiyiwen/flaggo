using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Flaggo.Shared.Contracts;

internal static class NoFollowFile
{
    private const int UnixReadOnly = 0;
    private const int LinuxCloseOnExec = 0x80000;
    private const int LinuxDirectory = 0x10000;
    private const int LinuxNoFollow = 0x20000;
    private const int AtEmptyPath = 0x1000;
    private const int AtSymlinkNoFollow = 0x100;
    private const uint StatxType = 0x00000001;
    private const int StatxModeOffset = 28;
    private const ushort FileTypeMask = 0xF000;
    private const ushort RegularFileType = 0x8000;
    private const ushort DirectoryFileType = 0x4000;

    private const uint GenericRead = 0x80000000;
    private const uint FileReadAttributes = 0x00000080;
    private const uint FileShareRead = 0x00000001;
    private const uint FileShareWrite = 0x00000002;
    private const uint FileShareDelete = 0x00000004;
    private const uint OpenExisting = 3;
    private const uint FileAttributeDirectory = 0x00000010;
    private const uint FileAttributeReparsePoint = 0x00000400;
    private const uint FileFlagBackupSemantics = 0x02000000;
    private const uint FileFlagOpenReparsePoint = 0x00200000;
    private const uint FileTypeDisk = 0x0001;
    private const int FileAttributeTagInfo = 9;
    private const int FileIdInfo = 18;
    private const uint VolumeNameDos = 0;
    private const uint Synchronize = 0x00100000;
    private const uint ObjectCaseInsensitive = 0x00000040;
    private const uint FileOpen = 1;
    private const uint FileDirectoryFile = 0x00000001;
    private const uint FileSynchronousIoNonAlert = 0x00000020;
    private const uint FileOpenForBackupIntent = 0x00004000;
    private const uint FileOpenReparsePoint = 0x00200000;

    public static NoFollowFileHandle OpenRead(
        string path,
        Action<WindowsPathOpenObservation>? windowsObserver = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        var fullPath = Path.GetFullPath(path);
        if (OperatingSystem.IsLinux())
        {
            return OpenLinux(fullPath);
        }

        if (OperatingSystem.IsWindows())
        {
            return OpenWindows(fullPath, windowsObserver);
        }

        throw new PlatformNotSupportedException(
            "Secure committed-file traversal is supported only on Linux " +
            "and Windows.");
    }

    private static NoFollowFileHandle OpenLinux(string fullPath)
    {
        var root = Path.GetPathRoot(fullPath)
            ?? throw new IOException($"Path '{fullPath}' has no filesystem root.");
        var relative = Path.GetRelativePath(root, fullPath);
        var components = relative.Split(
            Path.DirectorySeparatorChar,
            StringSplitOptions.RemoveEmptyEntries);
        if (components.Length == 0)
        {
            throw new InvalidDataException(
                $"Committed path '{fullPath}' is not a regular file.");
        }

        SafeFileHandle? current = null;
        try
        {
            current = OpenLinuxPath(
                root,
                UnixReadOnly | LinuxDirectory | LinuxNoFollow | LinuxCloseOnExec,
                fullPath);
            RequireLinuxType(current, DirectoryFileType, fullPath);
            for (var index = 0; index < components.Length - 1; index++)
            {
                var next = OpenLinuxAt(
                    current,
                    components[index],
                    UnixReadOnly |
                    LinuxDirectory |
                    LinuxNoFollow |
                    LinuxCloseOnExec,
                    fullPath);
                try
                {
                    RequireLinuxType(next, DirectoryFileType, fullPath);
                }
                catch
                {
                    next.Dispose();
                    throw;
                }
                current.Dispose();
                current = next;
            }

            var file = OpenLinuxAt(
                current,
                components[^1],
                UnixReadOnly | LinuxNoFollow | LinuxCloseOnExec,
                fullPath);
            try
            {
                RequireLinuxType(file, RegularFileType, fullPath);
                return new NoFollowFileHandle(file);
            }
            catch
            {
                file.Dispose();
                throw;
            }
        }
        finally
        {
            current?.Dispose();
        }
    }

    private static NoFollowFileHandle OpenWindows(
        string fullPath,
        Action<WindowsPathOpenObservation>? observer)
    {
        var root = Path.GetPathRoot(fullPath)
            ?? throw new IOException($"Path '{fullPath}' has no filesystem root.");
        var relative = Path.GetRelativePath(root, fullPath);
        var components = relative.Split(
            Path.DirectorySeparatorChar,
            StringSplitOptions.RemoveEmptyEntries);
        if (components.Length == 0)
        {
            throw new InvalidDataException(
                $"Committed path '{fullPath}' is not a regular file.");
        }

        var pinned = new List<WindowsPinnedPath>();
        try
        {
            var rootHandle = OpenWindowsPath(
                root,
                FileReadAttributes,
                FileFlagBackupSemantics | FileFlagOpenReparsePoint,
                fullPath);
            try
            {
                observer?.Invoke(new WindowsPathOpenObservation(
                    root,
                    WindowsPathOpenKind.Root));
            }
            catch
            {
                rootHandle.Dispose();
                throw;
            }
            WindowsPinnedPath rootPath;
            try
            {
                rootPath = CaptureWindowsPinnedPath(
                    rootHandle,
                    root,
                    expectedPath: null,
                    WindowsPathOpenKind.Root,
                    expectedVolume: null);
            }
            catch
            {
                rootHandle.Dispose();
                throw;
            }
            AddWindowsPinnedPath(pinned, rootPath);

            for (var index = 0; index < components.Length - 1; index++)
            {
                var expectedPath = Path.Combine(
                    pinned[^1].ExpectedFinalPath,
                    components[index]);
                var directoryHandle = OpenWindowsRelative(
                    pinned[^1].Handle,
                    components[index],
                    FileReadAttributes | Synchronize,
                    FileDirectoryFile |
                    FileSynchronousIoNonAlert |
                    FileOpenForBackupIntent |
                    FileOpenReparsePoint,
                    fullPath);
                try
                {
                    observer?.Invoke(new WindowsPathOpenObservation(
                        expectedPath,
                        WindowsPathOpenKind.Directory));
                }
                catch
                {
                    directoryHandle.Dispose();
                    throw;
                }
                WindowsPinnedPath directoryPath;
                try
                {
                    directoryPath = CaptureWindowsPinnedPath(
                        directoryHandle,
                        expectedPath,
                        expectedPath,
                        WindowsPathOpenKind.Directory,
                        rootPath.Identity.VolumeSerialNumber);
                }
                catch
                {
                    directoryHandle.Dispose();
                    throw;
                }
                AddWindowsPinnedPath(pinned, directoryPath);
                ValidateWindowsPinnedPaths(pinned);
            }

            var expectedFilePath = Path.Combine(
                pinned[^1].ExpectedFinalPath,
                components[^1]);
            var file = OpenWindowsRelative(
                pinned[^1].Handle,
                components[^1],
                GenericRead | FileReadAttributes | Synchronize,
                FileSynchronousIoNonAlert |
                FileOpenForBackupIntent |
                FileOpenReparsePoint,
                fullPath);
            try
            {
                observer?.Invoke(new WindowsPathOpenObservation(
                    expectedFilePath,
                    WindowsPathOpenKind.File));
                var pinnedFile = CaptureWindowsPinnedPath(
                    file,
                    fullPath,
                    expectedFilePath,
                    WindowsPathOpenKind.File,
                    rootPath.Identity.VolumeSerialNumber);
                ValidateWindowsPinnedPaths(pinned);
                ValidateWindowsPinnedPath(pinnedFile);
                var ancestors = pinned
                    .Select(entry => entry.Handle)
                    .ToArray();
                pinned.Clear();
                return new NoFollowFileHandle(file, ancestors);
            }
            catch
            {
                file.Dispose();
                throw;
            }
        }
        finally
        {
            foreach (var path in pinned)
            {
                path.Handle.Dispose();
            }
        }
    }

    private static void AddWindowsPinnedPath(
        List<WindowsPinnedPath> pinned,
        WindowsPinnedPath path)
    {
        try
        {
            pinned.Add(path);
        }
        catch
        {
            path.Handle.Dispose();
            throw;
        }
    }

    private static SafeFileHandle OpenLinuxPath(
        string path,
        int flags,
        string displayPath)
    {
        var descriptor = Open(path, flags);
        if (descriptor < 0)
        {
            throw UnixOpenError(displayPath, Marshal.GetLastPInvokeError());
        }

        return new SafeFileHandle((IntPtr)descriptor, ownsHandle: true);
    }

    private static SafeFileHandle OpenLinuxAt(
        SafeFileHandle directory,
        string component,
        int flags,
        string displayPath)
    {
        var descriptor = OpenAt(
            directory.DangerousGetHandle().ToInt32(),
            component,
            flags);
        if (descriptor < 0)
        {
            throw UnixOpenError(displayPath, Marshal.GetLastPInvokeError());
        }

        return new SafeFileHandle((IntPtr)descriptor, ownsHandle: true);
    }

    private static Exception UnixOpenError(string path, int error) =>
        error switch
        {
            2 => new FileNotFoundException(
                $"Committed file '{path}' was not found.",
                path),
            13 => new UnauthorizedAccessException(
                $"Access to committed file '{path}' was denied.",
                NativeIOException(
                    "open without following links",
                    path,
                    error)),
            20 or 40 => LinkError(path),
            _ => NativeIOException(
                "open without following links",
                path,
                error)
        };

    private static void RequireLinuxType(
        SafeFileHandle handle,
        ushort expectedType,
        string path)
    {
        var buffer = Marshal.AllocHGlobal(256);
        try
        {
            if (Statx(
                    handle.DangerousGetHandle().ToInt32(),
                    string.Empty,
                    AtEmptyPath | AtSymlinkNoFollow,
                    StatxType,
                    buffer) != 0)
            {
                throw NativeIOException(
                    "inspect the opened path",
                    path,
                    Marshal.GetLastPInvokeError());
            }

            var mode = unchecked((ushort)Marshal.ReadInt16(
                buffer,
                StatxModeOffset));
            if ((mode & FileTypeMask) != expectedType)
            {
                throw new InvalidDataException(
                    $"Committed path '{path}' is not the required " +
                    $"{(expectedType == RegularFileType ? "regular file" : "directory")}.");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private static SafeFileHandle OpenWindowsPath(
        string path,
        uint desiredAccess,
        uint flags,
        string displayPath)
    {
        var handle = CreateFile(
            ToWindowsExtendedPath(path),
            desiredAccess,
            FileShareRead | FileShareWrite | FileShareDelete,
            IntPtr.Zero,
            OpenExisting,
            flags,
            IntPtr.Zero);
        if (handle.IsInvalid)
        {
            var error = Marshal.GetLastPInvokeError();
            handle.Dispose();
            if (error == 2)
            {
                throw new FileNotFoundException(
                    $"Committed file '{displayPath}' was not found.",
                    displayPath);
            }
            if (error == 3)
            {
                throw new DirectoryNotFoundException(
                    $"A parent directory for committed file '{displayPath}' " +
                    "was not found.");
            }
            if (error is 267 or 4390)
            {
                throw LinkError(displayPath);
            }
            throw NativeIOException(
                "open without following reparse points",
                displayPath,
                error);
        }

        return handle;
    }

    private static SafeFileHandle OpenWindowsRelative(
        SafeFileHandle directory,
        string component,
        uint desiredAccess,
        uint createOptions,
        string displayPath)
    {
        if (component.Length == 0 ||
            component is "." or ".." ||
            component.IndexOfAny(
                [Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar])
            >= 0)
        {
            throw new InvalidDataException(
                $"Committed path '{displayPath}' contains an invalid component.");
        }

        var nameBuffer = Marshal.StringToHGlobalUni(component);
        var unicodeString = new UnicodeString
        {
            Length = checked((ushort)(component.Length * sizeof(char))),
            MaximumLength = checked((ushort)(
                (component.Length + 1) * sizeof(char))),
            Buffer = nameBuffer
        };
        var unicodeStringBuffer = Marshal.AllocHGlobal(
            Marshal.SizeOf<UnicodeString>());
        var objectAttributesBuffer = Marshal.AllocHGlobal(
            Marshal.SizeOf<ObjectAttributes>());
        try
        {
            Marshal.StructureToPtr(
                unicodeString,
                unicodeStringBuffer,
                fDeleteOld: false);
            var objectAttributes = new ObjectAttributes
            {
                Length = (uint)Marshal.SizeOf<ObjectAttributes>(),
                RootDirectory = directory.DangerousGetHandle(),
                ObjectName = unicodeStringBuffer,
                Attributes = ObjectCaseInsensitive
            };
            Marshal.StructureToPtr(
                objectAttributes,
                objectAttributesBuffer,
                fDeleteOld: false);
            var status = NtCreateFile(
                out var rawHandle,
                desiredAccess,
                objectAttributesBuffer,
                out _,
                IntPtr.Zero,
                0,
                FileShareRead | FileShareWrite | FileShareDelete,
                FileOpen,
                createOptions,
                IntPtr.Zero,
                0);
            if (status >= 0)
            {
                return new SafeFileHandle(rawHandle, ownsHandle: true);
            }

            var error = unchecked((int)RtlNtStatusToDosError(status));
            if (error == 2)
            {
                throw new FileNotFoundException(
                    $"Committed file '{displayPath}' was not found.",
                    displayPath);
            }
            if (error == 3)
            {
                throw new DirectoryNotFoundException(
                    $"A parent directory for committed file '{displayPath}' " +
                    "was not found.");
            }
            throw NativeIOException(
                "open relative to the validated parent handle",
                displayPath,
                error);
        }
        finally
        {
            Marshal.FreeHGlobal(objectAttributesBuffer);
            Marshal.FreeHGlobal(unicodeStringBuffer);
            Marshal.FreeHGlobal(nameBuffer);
        }
    }

    private static WindowsPinnedPath CaptureWindowsPinnedPath(
        SafeFileHandle handle,
        string displayPath,
        string? expectedPath,
        WindowsPathOpenKind kind,
        ulong? expectedVolume)
    {
        var info = GetWindowsAttributeInfo(handle, displayPath);
        if ((info.FileAttributes & FileAttributeReparsePoint) != 0)
        {
            throw LinkError(displayPath);
        }
        if (kind is WindowsPathOpenKind.Root or WindowsPathOpenKind.Directory)
        {
            if ((info.FileAttributes & FileAttributeDirectory) == 0)
            {
                throw new InvalidDataException(
                    $"Committed path '{displayPath}' is not a directory.");
            }
        }
        else if ((info.FileAttributes & FileAttributeDirectory) != 0 ||
                 GetFileType(handle) != FileTypeDisk)
        {
            throw new InvalidDataException(
                $"Committed path '{displayPath}' is not a regular file.");
        }

        var finalPath = FinalWindowsPath(handle);
        var normalizedExpected = expectedPath is null
            ? finalPath
            : NormalizeComparableWindowsPath(expectedPath);
        if (expectedPath is not null &&
            !PathEquals(finalPath, normalizedExpected))
        {
            throw new InvalidDataException(
                $"Committed path '{displayPath}' resolved to '{finalPath}', " +
                $"outside the exact opened component '{normalizedExpected}'.");
        }

        var identity = GetWindowsIdentity(handle, displayPath);
        if (expectedVolume.HasValue &&
            identity.VolumeSerialNumber != expectedVolume.Value)
        {
            throw new InvalidDataException(
                $"Committed path '{displayPath}' crossed filesystem volumes.");
        }

        return new WindowsPinnedPath(
            handle,
            normalizedExpected,
            identity,
            kind,
            displayPath);
    }

    private static void ValidateWindowsPinnedPaths(
        IEnumerable<WindowsPinnedPath> paths)
    {
        foreach (var path in paths)
        {
            ValidateWindowsPinnedPath(path);
        }
    }

    private static void ValidateWindowsPinnedPath(WindowsPinnedPath path)
    {
        var info = GetWindowsAttributeInfo(path.Handle, path.DisplayPath);
        if ((info.FileAttributes & FileAttributeReparsePoint) != 0)
        {
            throw LinkError(path.DisplayPath);
        }
        if (path.Kind is WindowsPathOpenKind.Root or
            WindowsPathOpenKind.Directory)
        {
            if ((info.FileAttributes & FileAttributeDirectory) == 0)
            {
                throw new InvalidDataException(
                    $"Committed path '{path.DisplayPath}' changed file type.");
            }
        }
        else if ((info.FileAttributes & FileAttributeDirectory) != 0 ||
                 GetFileType(path.Handle) != FileTypeDisk)
        {
            throw new InvalidDataException(
                $"Committed path '{path.DisplayPath}' changed file type.");
        }

        var finalPath = FinalWindowsPath(path.Handle);
        var identity = GetWindowsIdentity(path.Handle, path.DisplayPath);
        if (!PathEquals(finalPath, path.ExpectedFinalPath) ||
            identity.VolumeSerialNumber !=
            path.Identity.VolumeSerialNumber ||
            identity.FileId != path.Identity.FileId)
        {
            throw new InvalidDataException(
                $"Committed path '{path.DisplayPath}' changed exact opened " +
                "path or volume/file identity.");
        }
    }

    private static FileAttributeTagInformation GetWindowsAttributeInfo(
        SafeFileHandle handle,
        string path)
    {
        if (GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                out FileAttributeTagInformation info,
                (uint)Marshal.SizeOf<FileAttributeTagInformation>()))
        {
            return info;
        }

        throw NativeIOException(
            "inspect the opened path",
            path,
            Marshal.GetLastPInvokeError());
    }

    private static FileIdInformation GetWindowsIdentity(
        SafeFileHandle handle,
        string path)
    {
        if (GetFileIdentityInformationByHandleEx(
                handle,
                FileIdInfo,
                out FileIdInformation info,
                (uint)Marshal.SizeOf<FileIdInformation>()))
        {
            return info;
        }

        throw NativeIOException(
            "inspect the opened path identity",
            path,
            Marshal.GetLastPInvokeError());
    }

    private static string FinalWindowsPath(SafeFileHandle handle)
    {
        var capacity = 512;
        while (true)
        {
            var buffer = new char[capacity];
            var length = GetFinalPathNameByHandle(
                handle,
                buffer,
                (uint)buffer.Length,
                VolumeNameDos);
            if (length == 0)
            {
                throw NativeIOException(
                    "resolve the opened path",
                    "<handle>",
                    Marshal.GetLastPInvokeError());
            }
            if (length < buffer.Length)
            {
                return NormalizeWindowsFinalPath(
                    new string(buffer, 0, checked((int)length)));
            }

            capacity = checked((int)length + 1);
        }
    }

    private static string NormalizeWindowsFinalPath(string path)
    {
        const string uncPrefix = @"\\?\UNC\";
        const string devicePrefix = @"\\?\";
        if (path.StartsWith(uncPrefix, StringComparison.OrdinalIgnoreCase))
        {
            return $@"\\{path[uncPrefix.Length..]}";
        }
        return path.StartsWith(devicePrefix, StringComparison.OrdinalIgnoreCase)
            ? path[devicePrefix.Length..]
            : path;
    }

    private static string NormalizeComparableWindowsPath(string path) =>
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));

    private static string ToWindowsExtendedPath(string path)
    {
        if (path.StartsWith(@"\\?\", StringComparison.Ordinal))
        {
            return path;
        }
        if (path.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return $@"\\?\UNC\{path[2..]}";
        }
        return $@"\\?\{path}";
    }

    private static bool PathEquals(string left, string right) =>
        string.Equals(
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(left)),
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(right)),
            OperatingSystem.IsWindows()
                ? StringComparison.OrdinalIgnoreCase
                : StringComparison.Ordinal);

    private static InvalidDataException LinkError(string path) =>
        new(
            $"Committed path '{path}' cannot be a symbolic link or reparse point.");

    private static IOException NativeIOException(
        string operation,
        string path,
        int error) =>
        new(
            $"Failed to {operation} '{path}'.",
            new Win32Exception(error));

    [StructLayout(LayoutKind.Sequential)]
    private readonly struct FileAttributeTagInformation
    {
        public readonly uint FileAttributes;
        public readonly uint ReparseTag;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FileIdInformation
    {
        public ulong VolumeSerialNumber;
        public Guid FileId;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct UnicodeString
    {
        public ushort Length;
        public ushort MaximumLength;
        public IntPtr Buffer;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ObjectAttributes
    {
        public uint Length;
        public IntPtr RootDirectory;
        public IntPtr ObjectName;
        public uint Attributes;
        public IntPtr SecurityDescriptor;
        public IntPtr SecurityQualityOfService;
    }

    [StructLayout(LayoutKind.Sequential)]
    private readonly struct IoStatusBlock
    {
        public readonly IntPtr Status;
        public readonly UIntPtr Information;
    }

    private sealed record WindowsPinnedPath(
        SafeFileHandle Handle,
        string ExpectedFinalPath,
        FileIdInformation Identity,
        WindowsPathOpenKind Kind,
        string DisplayPath);

    [DllImport("libc", EntryPoint = "open", SetLastError = true)]
    private static extern int Open(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        int flags);

    [DllImport("libc", EntryPoint = "openat", SetLastError = true)]
    private static extern int OpenAt(
        int directoryDescriptor,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        int flags);

    [DllImport("libc", EntryPoint = "statx", SetLastError = true)]
    private static extern int Statx(
        int directoryDescriptor,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
        int flags,
        uint mask,
        IntPtr buffer);

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

    [DllImport("ntdll.dll")]
    private static extern int NtCreateFile(
        out IntPtr fileHandle,
        uint desiredAccess,
        IntPtr objectAttributes,
        out IoStatusBlock ioStatusBlock,
        IntPtr allocationSize,
        uint fileAttributes,
        uint shareAccess,
        uint createDisposition,
        uint createOptions,
        IntPtr eaBuffer,
        uint eaLength);

    [DllImport("ntdll.dll")]
    private static extern uint RtlNtStatusToDosError(int status);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandleEx(
        SafeFileHandle handle,
        int fileInformationClass,
        out FileAttributeTagInformation fileInformation,
        uint bufferSize);

    [DllImport(
        "kernel32.dll",
        EntryPoint = "GetFileInformationByHandleEx",
        SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileIdentityInformationByHandleEx(
        SafeFileHandle handle,
        int fileInformationClass,
        out FileIdInformation fileInformation,
        uint bufferSize);

    [DllImport(
        "kernel32.dll",
        EntryPoint = "GetFinalPathNameByHandleW",
        SetLastError = true,
        CharSet = CharSet.Unicode)]
    private static extern uint GetFinalPathNameByHandle(
        SafeFileHandle handle,
        [Out] char[] path,
        uint pathLength,
        uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint GetFileType(SafeFileHandle handle);
}

internal enum WindowsPathOpenKind
{
    Root,
    Directory,
    File
}

internal sealed record WindowsPathOpenObservation(
    string ExpectedPath,
    WindowsPathOpenKind Kind);

internal sealed class NoFollowFileHandle : IDisposable
{
    private readonly IReadOnlyList<SafeFileHandle> _ancestors;
    private bool _disposed;

    public NoFollowFileHandle(
        SafeFileHandle handle,
        IReadOnlyList<SafeFileHandle>? ancestors = null)
    {
        Handle = handle;
        _ancestors = ancestors ?? [];
    }

    public SafeFileHandle Handle { get; }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        Handle.Dispose();
        foreach (var ancestor in _ancestors)
        {
            ancestor.Dispose();
        }
    }
}
