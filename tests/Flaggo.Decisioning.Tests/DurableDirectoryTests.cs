using Flaggo.Audit;

namespace Flaggo.Decisioning.Tests;

public sealed class DurableDirectoryTests
{
    [Fact]
    public void Flush_SynchronizesDirectoryOnCurrentPlatform()
    {
        var path = ArtifactPath("durable-directory");
        try
        {
            Directory.CreateDirectory(path);
            File.WriteAllText(Path.Combine(path, "record.json"), "{}");

            DurableDirectory.Flush(path);
        }
        finally
        {
            Directory.Delete(path, recursive: true);
        }
    }

    [Theory]
    [InlineData(22, true)]
    [InlineData(45, true)]
    [InlineData(95, true)]
    [InlineData(13, false)]
    public void UnixUnsupportedClassification_DoesNotHidePermissionFailures(
        int error,
        bool expected)
    {
        Assert.Equal(expected, DurableDirectory.IsUnsupportedUnixError(error));
    }

    [Theory]
    [InlineData(1, true)]
    [InlineData(50, true)]
    [InlineData(87, true)]
    [InlineData(5, false)]
    public void WindowsUnsupportedClassification_DoesNotHidePermissionFailures(
        int error,
        bool expected)
    {
        Assert.Equal(expected, DurableDirectory.IsUnsupportedWindowsError(error));
    }

    private static string ArtifactPath(string prefix) =>
        Path.Combine(
            Directory.GetCurrentDirectory(),
            ".flaggo",
            "test-artifacts",
            $"{prefix}-{Guid.NewGuid():N}");
}
