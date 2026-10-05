using Flaggo.ServiceHosting;
using Microsoft.AspNetCore.Http;

namespace Flaggo.Core.Tests;

public sealed class HostingTests
{
    [Fact]
    public async Task RejectsNonJsonRequestBodies()
    {
        var context = new DefaultHttpContext();
        context.Request.ContentType = "text/plain";

        var exception = await Assert.ThrowsAsync<HttpContractException>(
            async () => await HttpJson.ReadAsync<object>(context.Request, CancellationToken.None));

        Assert.Equal(StatusCodes.Status415UnsupportedMediaType, exception.Status);
        Assert.Equal(ProblemTypes.UnsupportedMediaType, exception.Type);
    }
}
