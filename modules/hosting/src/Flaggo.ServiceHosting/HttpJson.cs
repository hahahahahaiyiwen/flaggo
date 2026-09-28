using System.Text.Json;
using Flaggo.Contract;
using Microsoft.AspNetCore.Http;
using Microsoft.Net.Http.Headers;

namespace Flaggo.ServiceHosting;

public static class HttpJson
{
    public static async ValueTask<T> ReadAsync<T>(
        HttpRequest request,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(request);

        if (!MediaTypeHeaderValue.TryParse(request.ContentType, out var contentType)
            || !string.Equals(
                contentType.MediaType.Value,
                "application/json",
                StringComparison.OrdinalIgnoreCase))
        {
            throw new HttpContractException(
                StatusCodes.Status415UnsupportedMediaType,
                ProblemTypes.UnsupportedMediaType,
                "Unsupported media type",
                "Content-Type must be application/json.");
        }

        if (request.ContentLength > StrictJson.MaximumDocumentBytes)
        {
            throw InvalidRequest("Request body exceeds the maximum size.");
        }

        await using var body = new MemoryStream();
        var buffer = new byte[16_384];
        while (true)
        {
            var read = await request.Body.ReadAsync(buffer, cancellationToken);
            if (read == 0)
            {
                break;
            }

            if (body.Length + read > StrictJson.MaximumDocumentBytes)
            {
                throw InvalidRequest("Request body exceeds the maximum size.");
            }

            body.Write(buffer, 0, read);
        }

        try
        {
            return StrictJson.Deserialize<T>(body.ToArray());
        }
        catch (JsonException exception)
        {
            throw InvalidRequest($"Request body is invalid: {exception.Message}", exception);
        }
    }

    private static HttpContractException InvalidRequest(
        string detail,
        Exception? innerException = null) =>
        new(
            StatusCodes.Status400BadRequest,
            ProblemTypes.InvalidRequest,
            "Invalid request",
            detail,
            innerException);
}

public sealed class HttpContractException : Exception
{
    public HttpContractException(
        int status,
        string type,
        string title,
        string detail,
        Exception? innerException = null)
        : base(detail, innerException)
    {
        Status = status;
        Type = type;
        Title = title;
    }

    public int Status { get; }

    public string Type { get; }

    public string Title { get; }
}
