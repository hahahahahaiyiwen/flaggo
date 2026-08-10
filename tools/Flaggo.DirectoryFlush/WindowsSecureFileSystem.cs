using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

internal sealed class WindowsSecureFileSystem : IDisposable
{
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint Delete = 0x00010000;
    private const uint Synchronize = 0x00100000;
    private const uint FileListDirectory = 0x00000001;
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
    private const int FileRenameInformationEx = 65;
    private const int FileDispositionInfo = 4;
    private const uint VolumeNameDos = 0;
    private const uint ObjectCaseInsensitive = 0x00000040;
    private const uint FileOpen = 1;
    private const uint FileCreate = 2;
    private const uint FileWriteThrough = 0x00000002;
    private const uint FileSequentialOnly = 0x00000004;
    private const uint FileDirectoryFile = 0x00000001;
    private const uint FileSynchronousIoNonAlert = 0x00000020;
    private const uint FileNonDirectoryFile = 0x00000040;
    private const uint FileOpenForBackupIntent = 0x00004000;
    private const uint FileOpenReparsePoint = 0x00200000;
    private const uint FileRenameReplaceIfExists = 0x00000001;

    private readonly List<OpenedPath> _opened = [];
    private readonly Dictionary<string, OpenedPath> _directories =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly Action<string>? _afterOpen;
    private readonly ulong _volumeSerialNumber;
    private bool _disposed;

    private WindowsSecureFileSystem(
        string rootPath,
        bool create,
        Action<string>? afterOpen)
    {
        _afterOpen = afterOpen;
        RootPath = NormalizePath(rootPath);
        var volumePath = Path.GetPathRoot(RootPath)
            ?? throw new HelperException(
                "invalid_path",
                $"Path '{RootPath}' has no volume root.");
        SafeFileHandle? volumeHandle = null;
        try
        {
            volumeHandle = OpenAbsoluteDirectory(
                volumePath,
                FileListDirectory | FileReadAttributes);
            _afterOpen?.Invoke(volumePath);
            var volume = Capture(
                volumeHandle,
                volumePath,
                expectedPath: null,
                PathKind.Directory,
                expectedVolume: null,
                parent: null,
                name: string.Empty);
            volumeHandle = null;
            Add(volume);
            _volumeSerialNumber = volume.Identity.VolumeSerialNumber;

            var current = volume;
            foreach (var component in ComponentsFrom(volumePath, RootPath))
            {
                var expectedPath = Path.Combine(
                    current.ExpectedFinalPath,
                    component);
                var next = TryOpenRelative(
                    current,
                    component,
                    FileListDirectory | FileReadAttributes | Synchronize,
                    DirectoryOpenOptions,
                    FileOpen,
                    PathKind.Directory,
                    expectedPath);
                if (next is null)
                {
                    if (!create)
                    {
                        throw new HelperException(
                            "not_found",
                            $"Directory '{RootPath}' was not found.");
                    }

                    var created = true;
                    try
                    {
                        next = OpenRelative(
                            current,
                            component,
                            FileListDirectory | FileReadAttributes | Synchronize,
                            DirectoryOpenOptions,
                            FileCreate,
                            PathKind.Directory,
                            expectedPath);
                    }
                    catch (HelperException error) when (
                        error.Code == "already_exists")
                    {
                        created = false;
                        next = OpenRelative(
                            current,
                            component,
                            FileListDirectory | FileReadAttributes | Synchronize,
                            DirectoryOpenOptions,
                            FileOpen,
                            PathKind.Directory,
                            expectedPath);
                    }
                    Add(next);
                    if (created)
                    {
                        Flush(current);
                        Flush(next);
                    }
                }
                else
                {
                    Add(next);
                }
                current = next;
                ValidatePinnedPaths();
            }

            Root = current;
            _directories[string.Empty] = Root;
        }
        catch
        {
            Dispose();
            throw;
        }
        finally
        {
            volumeHandle?.Dispose();
        }
    }

    private static uint DirectoryOpenOptions =>
        FileDirectoryFile |
        FileSynchronousIoNonAlert |
        FileOpenForBackupIntent |
        FileOpenReparsePoint;

    private static uint FileCreateOptions =>
        FileNonDirectoryFile |
        FileSynchronousIoNonAlert |
        FileOpenReparsePoint;

    private static uint FileReadOptions =>
        FileSynchronousIoNonAlert |
        FileOpenForBackupIntent |
        FileOpenReparsePoint;

    public string RootPath { get; }

    public string RootFinalPath => Root.ExpectedFinalPath;

    private OpenedPath Root { get; }

    public static WindowsSecureFileSystem OpenRoot(
        string rootPath,
        bool create,
        Action<string>? afterOpen = null) =>
        new(rootPath, create, afterOpen);

    public string EnsureDirectory(string relativePath)
    {
        var components = ValidateRelativePath(relativePath, allowEmpty: true);
        var current = Root;
        var currentRelative = string.Empty;
        foreach (var component in components)
        {
            currentRelative = CombineRelative(currentRelative, component);
            if (_directories.TryGetValue(currentRelative, out var existing))
            {
                current = existing;
                continue;
            }

            var expectedPath = Path.Combine(
                current.ExpectedFinalPath,
                component);
            var next = TryOpenRelative(
                current,
                component,
                FileListDirectory | FileReadAttributes | Synchronize,
                DirectoryOpenOptions,
                FileOpen,
                PathKind.Directory,
                expectedPath);
            if (next is null)
            {
                var created = true;
                try
                {
                    next = OpenRelative(
                        current,
                        component,
                        FileListDirectory | FileReadAttributes | Synchronize,
                        DirectoryOpenOptions,
                        FileCreate,
                        PathKind.Directory,
                        expectedPath);
                }
                catch (HelperException error) when (
                    error.Code == "already_exists")
                {
                    created = false;
                    next = OpenRelative(
                        current,
                        component,
                        FileListDirectory | FileReadAttributes | Synchronize,
                        DirectoryOpenOptions,
                        FileOpen,
                        PathKind.Directory,
                        expectedPath);
                }
                Add(next);
                _directories[currentRelative] = next;
                if (created)
                {
                    Flush(current);
                    Flush(next);
                }
            }
            else
            {
                Add(next);
                _directories[currentRelative] = next;
            }
            current = next;
            ValidatePinnedPaths();
        }

        return AbsolutePath(relativePath);
    }

    public OpenedPath CreateNewFile(
        string relativePath,
        ReadOnlySpan<byte> bytes)
    {
        var (parent, name) = ResolveParent(relativePath, create: true);
        var expectedPath = Path.Combine(parent.ExpectedFinalPath, name);
        var file = OpenRelative(
            parent,
            name,
            GenericRead |
            GenericWrite |
            Delete |
            FileReadAttributes |
            Synchronize,
            FileCreateOptions | FileWriteThrough | FileSequentialOnly,
            FileCreate,
            PathKind.File,
            expectedPath);
        Add(file);
        try
        {
            RandomAccess.Write(file.Handle, bytes, 0);
            if (!FlushFileBuffers(file.Handle))
            {
                throw NativeError(
                    "flush_file",
                    $"Failed to synchronize '{AbsolutePath(relativePath)}'.");
            }
            ValidatePinnedPaths();
            return file;
        }
        catch
        {
            TryDelete(file);
            throw;
        }
    }

    public byte[] ReadFile(string relativePath, int maximumBytes)
    {
        var (parent, name) = ResolveParent(relativePath, create: false);
        var expectedPath = Path.Combine(parent.ExpectedFinalPath, name);
        var file = OpenRelative(
            parent,
            name,
            GenericRead | FileReadAttributes | Synchronize,
            FileReadOptions | FileSequentialOnly,
            FileOpen,
            PathKind.File,
            expectedPath);
        Add(file);
        ValidatePinnedPaths();
        var length = RandomAccess.GetLength(file.Handle);
        if (length < 0 || length > maximumBytes || length > int.MaxValue)
        {
            throw new HelperException(
                "file_too_large",
                $"File '{AbsolutePath(relativePath)}' exceeds its byte limit.");
        }

        var bytes = GC.AllocateUninitializedArray<byte>((int)length);
        var offset = 0;
        while (offset < bytes.Length)
        {
            var read = RandomAccess.Read(
                file.Handle,
                bytes.AsSpan(offset),
                offset);
            if (read == 0)
            {
                throw new HelperException(
                    "unexpected_eof",
                    $"File '{AbsolutePath(relativePath)}' ended during read.");
            }
            offset += read;
        }
        if (RandomAccess.GetLength(file.Handle) != length)
        {
            throw new HelperException(
                "file_changed",
                $"File '{AbsolutePath(relativePath)}' changed during read.");
        }
        ValidatePinnedPaths();
        return bytes;
    }

    public void Rename(
        OpenedPath source,
        string destinationRelativePath,
        bool replace,
        Action onCommitted)
    {
        ArgumentNullException.ThrowIfNull(onCommitted);
        ValidatePinnedPaths();
        var (destinationParent, destinationName) = ResolveParent(
            destinationRelativePath,
            create: true);
        RejectExistingReparse(destinationParent, destinationName);
        var destinationExpectedPath = Path.Combine(
            destinationParent.ExpectedFinalPath,
            destinationName);
        var destinationDisplayPath = AbsolutePath(destinationRelativePath);

        var fileNameBytes = checked(destinationName.Length * sizeof(char));
        var rootOffset = IntPtr.Size == 8 ? 8 : 4;
        var lengthOffset = rootOffset + IntPtr.Size;
        var nameOffset = lengthOffset + sizeof(uint);
        var buffer = Marshal.AllocHGlobal(nameOffset + fileNameBytes);
        try
        {
            for (var index = 0; index < nameOffset + fileNameBytes; index++)
            {
                Marshal.WriteByte(buffer, index, 0);
            }
            Marshal.WriteInt32(
                buffer,
                0,
                replace ? (int)FileRenameReplaceIfExists : 0);
            Marshal.WriteIntPtr(
                buffer,
                rootOffset,
                destinationParent.Handle.DangerousGetHandle());
            Marshal.WriteInt32(buffer, lengthOffset, fileNameBytes);
            Marshal.Copy(
                destinationName.ToCharArray(),
                0,
                buffer + nameOffset,
                destinationName.Length);
            var status = NtSetInformationFile(
                source.Handle,
                out _,
                buffer,
                (uint)(nameOffset + fileNameBytes),
                FileRenameInformationEx);
            if (status < 0)
            {
                var error = unchecked((int)RtlNtStatusToDosError(status));
                throw new HelperException(
                    "rename_failed",
                    $"Failed to atomically publish " +
                    $"'{AbsolutePath(destinationRelativePath)}' " +
                    $"(Win32 error {error}).",
                    new Win32Exception(error));
            }
            onCommitted();
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }

        source.ExpectedFinalPath = destinationExpectedPath;
        source.DisplayPath = destinationDisplayPath;
    }

    public void FlushDirectory(string relativePath)
    {
        var directory = ResolveDirectory(relativePath, create: false);
        Flush(directory);
        ValidatePinnedPaths();
    }

    public string OpenDirectory(string relativePath)
    {
        _ = ResolveDirectory(relativePath, create: false);
        ValidatePinnedPaths();
        return AbsolutePath(relativePath);
    }

    public void DeleteSubtree(string relativePath)
    {
        var absolute = AbsolutePath(relativePath);
        var prefix = $"{Path.TrimEndingDirectorySeparator(absolute)}" +
            Path.DirectorySeparatorChar;
        var entries = _opened
            .Where(entry =>
                !entry.Disposed &&
                (PathEquals(entry.ExpectedFinalPath, absolute) ||
                 entry.ExpectedFinalPath.StartsWith(
                     prefix,
                     StringComparison.OrdinalIgnoreCase)))
            .OrderBy(entry => entry.Kind == PathKind.File ? 0 : 1)
            .ThenByDescending(entry => entry.ExpectedFinalPath.Length)
            .ToArray();
        foreach (var entry in entries)
        {
            TryDelete(entry);
            entry.Dispose();
        }
    }

    public void TryDelete(OpenedPath path)
    {
        if (path.Disposed)
        {
            return;
        }
        if (path.Kind == PathKind.Directory)
        {
            if (path.Parent is null)
            {
                throw new HelperException(
                    "cleanup_failed",
                    $"Refusing to delete volume root '{path.DisplayPath}'.");
            }

            using var deleteHandle = OpenRelative(
                path.Parent,
                path.Name,
                Delete |
                FileListDirectory |
                FileReadAttributes |
                Synchronize,
                DirectoryOpenOptions,
                FileOpen,
                PathKind.Directory,
                path.ExpectedFinalPath);
            EnsureSameIdentity(path, deleteHandle);
            SetDeleteDisposition(deleteHandle);
            return;
        }

        SetDeleteDisposition(path);
    }

    private static void SetDeleteDisposition(OpenedPath path)
    {
        var disposition = new FileDispositionInformation { DeleteFile = true };
        var buffer = Marshal.AllocHGlobal(
            Marshal.SizeOf<FileDispositionInformation>());
        try
        {
            Marshal.StructureToPtr(disposition, buffer, fDeleteOld: false);
            if (!SetFileInformationByHandle(
                    path.Handle,
                    FileDispositionInfo,
                    buffer,
                    (uint)Marshal.SizeOf<FileDispositionInformation>()))
            {
                throw NativeError(
                    "cleanup_failed",
                    $"Failed to remove unpublished path " +
                    $"'{path.DisplayPath}'.");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    public string AbsolutePath(string relativePath)
    {
        var components = ValidateRelativePath(relativePath, allowEmpty: true);
        var combined = components.Aggregate(
            RootPath,
            Path.Combine);
        var fullPath = Path.GetFullPath(combined);
        EnsureContained(RootPath, fullPath);
        return fullPath;
    }

    public void ValidatePinnedPaths()
    {
        foreach (var entry in _opened.Where(entry => !entry.Disposed))
        {
            Validate(entry);
        }
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        for (var index = _opened.Count - 1; index >= 0; index--)
        {
            _opened[index].Dispose();
        }
    }

    private void Add(OpenedPath path)
    {
        try
        {
            _opened.Add(path);
        }
        catch
        {
            path.Dispose();
            throw;
        }
    }

    private OpenedPath ResolveDirectory(string relativePath, bool create)
    {
        var normalized = NormalizeRelative(relativePath);
        if (_directories.TryGetValue(normalized, out var existing))
        {
            return existing;
        }
        if (create)
        {
            EnsureDirectory(normalized);
            return _directories[normalized];
        }

        var components = ValidateRelativePath(normalized, allowEmpty: true);
        var current = Root;
        var currentRelative = string.Empty;
        foreach (var component in components)
        {
            currentRelative = CombineRelative(currentRelative, component);
            var expectedPath = Path.Combine(
                current.ExpectedFinalPath,
                component);
            var next = OpenRelative(
                current,
                component,
                FileListDirectory | FileReadAttributes | Synchronize,
                DirectoryOpenOptions,
                FileOpen,
                PathKind.Directory,
                expectedPath);
            Add(next);
            _directories[currentRelative] = next;
            current = next;
            ValidatePinnedPaths();
        }
        return current;
    }

    private (OpenedPath Parent, string Name) ResolveParent(
        string relativePath,
        bool create)
    {
        var components = ValidateRelativePath(relativePath, allowEmpty: false);
        var name = components[^1];
        var parentRelative = components.Length == 1
            ? string.Empty
            : Path.Combine(components[..^1]);
        return (ResolveDirectory(parentRelative, create), name);
    }

    private void RejectExistingReparse(
        OpenedPath parent,
        string name)
    {
        var expectedPath = Path.Combine(parent.ExpectedFinalPath, name);
        var existing = TryOpenRelative(
            parent,
            name,
            FileReadAttributes | Synchronize,
            FileSynchronousIoNonAlert |
            FileOpenForBackupIntent |
            FileOpenReparsePoint,
            FileOpen,
            PathKind.Any,
            expectedPath);
        if (existing is null)
        {
            return;
        }

        using (existing)
        {
            Validate(existing);
        }
    }

    private void Flush(OpenedPath directory)
    {
        using var writable = OpenFlushHandle(directory);
        if (!FlushFileBuffers(writable.Handle))
        {
            throw NativeError(
                "flush_directory",
                $"Failed to synchronize directory '{directory.DisplayPath}'.");
        }
    }

    private OpenedPath OpenFlushHandle(OpenedPath directory)
    {
        if (directory.Parent is null)
        {
            SafeFileHandle? handle = OpenAbsoluteDirectory(
                directory.DisplayPath,
                GenericWrite | FileListDirectory | FileReadAttributes);
            try
            {
                var opened = Capture(
                    handle,
                    directory.DisplayPath,
                    directory.ExpectedFinalPath,
                    PathKind.Directory,
                    _volumeSerialNumber,
                    parent: null,
                    directory.Name);
                handle = null;
                try
                {
                    EnsureSameIdentity(directory, opened);
                    return opened;
                }
                catch
                {
                    opened.Dispose();
                    throw;
                }
            }
            finally
            {
                handle?.Dispose();
            }
        }

        var reopened = OpenRelative(
            directory.Parent,
            directory.Name,
            GenericWrite |
            FileListDirectory |
            FileReadAttributes |
            Synchronize,
            DirectoryOpenOptions,
            FileOpen,
            PathKind.Directory,
            directory.ExpectedFinalPath);
        try
        {
            EnsureSameIdentity(directory, reopened);
            return reopened;
        }
        catch
        {
            reopened.Dispose();
            throw;
        }
    }

    private void EnsureSameIdentity(OpenedPath expected, OpenedPath actual)
    {
        if (expected.Identity.VolumeSerialNumber !=
                actual.Identity.VolumeSerialNumber ||
            expected.Identity.FileId != actual.Identity.FileId)
        {
            throw new HelperException(
                "path_changed",
                $"Directory '{expected.DisplayPath}' changed identity.");
        }
    }

    private OpenedPath? TryOpenRelative(
        OpenedPath parent,
        string name,
        uint desiredAccess,
        uint createOptions,
        uint disposition,
        PathKind kind,
        string expectedPath)
    {
        try
        {
            return OpenRelative(
                parent,
                name,
                desiredAccess,
                createOptions,
                disposition,
                kind,
                expectedPath);
        }
        catch (HelperException error) when (error.Code == "not_found")
        {
            return null;
        }
    }

    private OpenedPath OpenRelative(
        OpenedPath parent,
        string name,
        uint desiredAccess,
        uint createOptions,
        uint disposition,
        PathKind kind,
        string expectedPath)
    {
        ValidateComponent(name);
        var nameBuffer = Marshal.StringToHGlobalUni(name);
        var unicodeString = new UnicodeString
        {
            Length = checked((ushort)(name.Length * sizeof(char))),
            MaximumLength = checked((ushort)((name.Length + 1) * sizeof(char))),
            Buffer = nameBuffer
        };
        var unicodeBuffer = Marshal.AllocHGlobal(
            Marshal.SizeOf<UnicodeString>());
        var attributesBuffer = Marshal.AllocHGlobal(
            Marshal.SizeOf<ObjectAttributes>());
        try
        {
            Marshal.StructureToPtr(
                unicodeString,
                unicodeBuffer,
                fDeleteOld: false);
            var attributes = new ObjectAttributes
            {
                Length = (uint)Marshal.SizeOf<ObjectAttributes>(),
                RootDirectory = parent.Handle.DangerousGetHandle(),
                ObjectName = unicodeBuffer,
                Attributes = ObjectCaseInsensitive
            };
            Marshal.StructureToPtr(
                attributes,
                attributesBuffer,
                fDeleteOld: false);
            var status = NtCreateFile(
                out var rawHandle,
                desiredAccess,
                attributesBuffer,
                out _,
                IntPtr.Zero,
                0,
                FileShareRead | FileShareWrite | FileShareDelete,
                disposition,
                createOptions,
                IntPtr.Zero,
                0);
            if (status < 0)
            {
                throw NtError(status, expectedPath);
            }

            var handle = new SafeFileHandle(rawHandle, ownsHandle: true);
            try
            {
                _afterOpen?.Invoke(expectedPath);
                return Capture(
                    handle,
                    expectedPath,
                    expectedPath,
                    kind,
                    _volumeSerialNumber,
                    parent,
                    name);
            }
            catch
            {
                handle.Dispose();
                throw;
            }
        }
        finally
        {
            Marshal.FreeHGlobal(attributesBuffer);
            Marshal.FreeHGlobal(unicodeBuffer);
            Marshal.FreeHGlobal(nameBuffer);
        }
    }

    private static SafeFileHandle OpenAbsoluteDirectory(
        string path,
        uint desiredAccess)
    {
        var handle = CreateFile(
            ExtendedPath(path),
            desiredAccess,
            FileShareRead | FileShareWrite | FileShareDelete,
            IntPtr.Zero,
            OpenExisting,
            FileFlagBackupSemantics | FileFlagOpenReparsePoint,
            IntPtr.Zero);
        if (!handle.IsInvalid)
        {
            return handle;
        }

        var error = Marshal.GetLastPInvokeError();
        handle.Dispose();
        throw Win32Error(error, path);
    }

    private static OpenedPath Capture(
        SafeFileHandle handle,
        string displayPath,
        string? expectedPath,
        PathKind kind,
        ulong? expectedVolume,
        OpenedPath? parent,
        string name)
    {
        var attributes = AttributeInfo(handle, displayPath);
        if ((attributes.FileAttributes & FileAttributeReparsePoint) != 0)
        {
            throw new HelperException(
                "reparse_point",
                $"Path '{displayPath}' cannot be a symbolic link or reparse point.");
        }
        if (kind == PathKind.Directory &&
            (attributes.FileAttributes & FileAttributeDirectory) == 0)
        {
            throw new HelperException(
                "wrong_type",
                $"Path '{displayPath}' is not a directory.");
        }
        if (kind == PathKind.File &&
            ((attributes.FileAttributes & FileAttributeDirectory) != 0 ||
             GetFileType(handle) != FileTypeDisk))
        {
            throw new HelperException(
                "wrong_type",
                $"Path '{displayPath}' is not a regular file.");
        }

        var finalPath = FinalPath(handle);
        var normalizedExpected = expectedPath is null
            ? finalPath
            : NormalizePath(expectedPath);
        if (expectedPath is not null &&
            !PathEquals(finalPath, normalizedExpected))
        {
            throw new HelperException(
                "outside_root",
                $"Opened path '{finalPath}' is not exact component " +
                $"'{normalizedExpected}'.");
        }

        var identity = IdentityInfo(handle, displayPath);
        if (expectedVolume.HasValue &&
            identity.VolumeSerialNumber != expectedVolume.Value)
        {
            throw new HelperException(
                "outside_root",
                $"Path '{displayPath}' crossed filesystem volumes.");
        }

        return new OpenedPath(
            handle,
            normalizedExpected,
            identity,
            kind,
            displayPath,
            parent,
            name);
    }

    private static void Validate(OpenedPath path)
    {
        var attributes = AttributeInfo(path.Handle, path.DisplayPath);
        if ((attributes.FileAttributes & FileAttributeReparsePoint) != 0)
        {
            throw new HelperException(
                "reparse_point",
                $"Path '{path.DisplayPath}' became a reparse point.");
        }
        if (path.Kind == PathKind.Directory &&
            (attributes.FileAttributes & FileAttributeDirectory) == 0)
        {
            throw new HelperException(
                "path_changed",
                $"Path '{path.DisplayPath}' changed file type.");
        }
        if (path.Kind == PathKind.File &&
            ((attributes.FileAttributes & FileAttributeDirectory) != 0 ||
             GetFileType(path.Handle) != FileTypeDisk))
        {
            throw new HelperException(
                "path_changed",
                $"Path '{path.DisplayPath}' changed file type.");
        }

        var finalPath = FinalPath(path.Handle);
        var identity = IdentityInfo(path.Handle, path.DisplayPath);
        if (!PathEquals(finalPath, path.ExpectedFinalPath) ||
            identity.VolumeSerialNumber !=
                path.Identity.VolumeSerialNumber ||
            identity.FileId != path.Identity.FileId)
        {
            throw new HelperException(
                "path_changed",
                $"Path '{path.DisplayPath}' changed exact opened path or identity.");
        }
    }

    private static FileAttributeTagInformation AttributeInfo(
        SafeFileHandle handle,
        string path)
    {
        if (GetFileAttributeInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                out FileAttributeTagInformation info,
                (uint)Marshal.SizeOf<FileAttributeTagInformation>()))
        {
            return info;
        }
        throw NativeError(
            "inspect_path",
            $"Failed to inspect opened path '{path}'.");
    }

    private static FileIdInformation IdentityInfo(
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
        throw NativeError(
            "inspect_identity",
            $"Failed to inspect opened path identity '{path}'.");
    }

    private static string FinalPath(SafeFileHandle handle)
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
                throw NativeError(
                    "resolve_path",
                    "Failed to resolve an opened path.");
            }
            if (length < buffer.Length)
            {
                return NormalizePath(
                    new string(buffer, 0, checked((int)length)));
            }
            capacity = checked((int)length + 1);
        }
    }

    private static string NormalizePath(string path)
    {
        const string uncPrefix = @"\\?\UNC\";
        const string devicePrefix = @"\\?\";
        var normalized = path.StartsWith(
            uncPrefix,
            StringComparison.OrdinalIgnoreCase)
            ? $@"\\{path[uncPrefix.Length..]}"
            : path.StartsWith(
                devicePrefix,
                StringComparison.OrdinalIgnoreCase)
                ? path[devicePrefix.Length..]
                : path;
        var fullPath = Path.GetFullPath(normalized);
        var root = Path.GetPathRoot(fullPath);
        return string.Equals(
            fullPath,
            root,
            StringComparison.OrdinalIgnoreCase)
            ? fullPath
            : Path.TrimEndingDirectorySeparator(fullPath);
    }

    private static string ExtendedPath(string path)
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

    private static string[] ComponentsFrom(string root, string target)
    {
        if (PathEquals(root, target))
        {
            return [];
        }
        var relative = Path.GetRelativePath(root, target);
        return ValidateRelativePath(relative, allowEmpty: true);
    }

    private static string[] ValidateRelativePath(
        string relativePath,
        bool allowEmpty)
    {
        if (Path.IsPathRooted(relativePath))
        {
            throw new HelperException(
                "outside_root",
                $"Relative path '{relativePath}' must not be rooted.");
        }
        var components = relativePath.Split(
            [Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar],
            StringSplitOptions.RemoveEmptyEntries);
        if (!allowEmpty && components.Length == 0)
        {
            throw new HelperException(
                "invalid_path",
                "A non-empty relative path is required.");
        }
        foreach (var component in components)
        {
            ValidateComponent(component);
        }
        return components;
    }

    private static void ValidateComponent(string component)
    {
        if (component.Length == 0 ||
            component is "." or ".." ||
            component.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
        {
            throw new HelperException(
                "invalid_path",
                $"Path component '{component}' is invalid.");
        }
    }

    private static string NormalizeRelative(string relativePath)
    {
        var components = ValidateRelativePath(
            relativePath,
            allowEmpty: true);
        return components.Length == 0
            ? string.Empty
            : Path.Combine(components);
    }

    private static string CombineRelative(string left, string right) =>
        left.Length == 0 ? right : Path.Combine(left, right);

    internal static void EnsureContained(string root, string path)
    {
        var fullRoot = Path.GetFullPath(root);
        var fullPath = Path.GetFullPath(path);
        var relative = Path.GetRelativePath(fullRoot, fullPath);
        if (relative == ".." ||
            relative.StartsWith(
                $"..{Path.DirectorySeparatorChar}",
                StringComparison.Ordinal) ||
            Path.IsPathRooted(relative))
        {
            throw new HelperException(
                "outside_root",
                $"Path '{fullPath}' escapes configured root '{fullRoot}'.");
        }
    }

    private static bool PathEquals(string left, string right) =>
        string.Equals(
            NormalizePath(left),
            NormalizePath(right),
            StringComparison.OrdinalIgnoreCase);

    private static HelperException NtError(int status, string path)
    {
        var error = unchecked((int)RtlNtStatusToDosError(status));
        return Win32Error(error, path);
    }

    private static HelperException Win32Error(int error, string path) =>
        error switch
        {
            2 or 3 => new HelperException(
                "not_found",
                $"Path '{path}' was not found.",
                new Win32Exception(error)),
            80 or 183 => new HelperException(
                "already_exists",
                $"Path '{path}' already exists.",
                new Win32Exception(error)),
            5 => new HelperException(
                "access_denied",
                $"Access to path '{path}' was denied.",
                new Win32Exception(error)),
            _ => new HelperException(
                "native_io",
                $"Native Windows operation failed for '{path}'.",
                new Win32Exception(error))
        };

    private static HelperException NativeError(
        string code,
        string message,
        int? error = null) =>
        new(
            code,
            message,
            new Win32Exception(error ?? Marshal.GetLastPInvokeError()));

    internal sealed class OpenedPath(
        SafeFileHandle handle,
        string expectedFinalPath,
        FileIdInformation identity,
        PathKind kind,
        string displayPath,
        OpenedPath? parent,
        string name) : IDisposable
    {
        public SafeFileHandle Handle { get; } = handle;
        public string ExpectedFinalPath { get; set; } = expectedFinalPath;
        public FileIdInformation Identity { get; } = identity;
        public PathKind Kind { get; } = kind;
        public string DisplayPath { get; set; } = displayPath;
        public OpenedPath? Parent { get; } = parent;
        public string Name { get; } = name;
        public bool Disposed { get; private set; }

        public void Dispose()
        {
            if (Disposed)
            {
                return;
            }
            Disposed = true;
            Handle.Dispose();
        }
    }

    internal enum PathKind
    {
        Any,
        Directory,
        File
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct FileIdInformation
    {
        public ulong VolumeSerialNumber;
        public Guid FileId;
    }

    [StructLayout(LayoutKind.Sequential)]
    private readonly struct FileAttributeTagInformation
    {
        public readonly uint FileAttributes;
        public readonly uint ReparseTag;
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

    [StructLayout(LayoutKind.Sequential)]
    private struct FileDispositionInformation
    {
        [MarshalAs(UnmanagedType.Bool)]
        public bool DeleteFile;
    }

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

    [DllImport("ntdll.dll")]
    private static extern int NtSetInformationFile(
        SafeFileHandle fileHandle,
        out IoStatusBlock ioStatusBlock,
        IntPtr fileInformation,
        uint length,
        int fileInformationClass);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool FlushFileBuffers(SafeFileHandle handle);

    [DllImport(
        "kernel32.dll",
        EntryPoint = "GetFileInformationByHandleEx",
        SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileAttributeInformationByHandleEx(
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
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetFileInformationByHandle(
        SafeFileHandle handle,
        int fileInformationClass,
        IntPtr fileInformation,
        uint bufferSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint GetFileType(SafeFileHandle handle);
}

internal sealed class HelperException(
    string code,
    string message,
    Exception? innerException = null) : IOException(message, innerException)
{
    public string Code { get; } = code;
}
