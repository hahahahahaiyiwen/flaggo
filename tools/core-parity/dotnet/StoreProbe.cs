using System.Globalization;
using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

internal static class StoreProbe
{
    internal static async Task<object> EvaluateAsync(
        JsonElement request, FlaggoExpressionCompiler compiler)
    {
        var database = request.GetProperty("database").GetString()!;
        var connectionString = new SqliteConnectionStringBuilder { DataSource = database }.ToString();
        var expressions = request.GetProperty("withExpressions").GetBoolean() ? compiler : null;
        var store = new SqliteContractVersionStore(connectionString, expressions);
        var versions = new Dictionary<string, AcceptedContractVersion>();
        var cursors = new Dictionary<string, string?>();
        var replies = new List<object>();
        foreach (var operation in request.GetProperty("operations").EnumerateArray())
        {
            var id = operation.GetProperty("id").GetString()!;
            using var cancellation = new CancellationTokenSource();
            if (operation.TryGetProperty("cancelled", out var cancelled) && cancelled.GetBoolean())
                cancellation.Cancel();
            try
            {
                string Name() => operation.TryGetProperty("use", out var use)
                    ? versions[use.GetString()!].Contract.Name
                    : operation.GetProperty("contractName").GetString()!;
                string Digest() => operation.TryGetProperty("contractDigest", out var digest)
                    ? digest.GetString()! : versions[operation.GetProperty("use").GetString()!].ContractDigest;
                object? value = null;
                var action = operation.GetProperty("action").GetString();
                switch (action)
                {
                    case "initialize":
                        await store.InitializeAsync(cancellation.Token);
                        break;
                    case "initializeExecutable":
                        await new SqliteExecutableStore(connectionString, expressions).InitializeAsync(cancellation.Token);
                        break;
                    case "put":
                        var contract = StrictJson.Deserialize<DecisionContract>(
                            Encoding.UTF8.GetBytes(operation.GetProperty("document").GetString()!));
                        var version = new AcceptedContractVersion(
                            operation.TryGetProperty("contractDigest", out var specified) ? specified.GetString()!
                                : ContractDigests.ComputeContractDigest(contract, expressions),
                            DateTimeOffset.ParseExact(operation.GetProperty("acceptedAt").GetString()!,
                                "O", CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind), contract);
                        versions[id] = version;
                        value = (await store.PutAsync(version, cancellation.Token)).ToString().ToLowerInvariant();
                        break;
                    case "get": value = await store.GetAsync(Name(), Digest(), cancellation.Token); break;
                    case "current": value = await store.GetCurrentAsync(Name(), cancellation.Token); break;
                    case "catalog": value = await store.ListAllCurrentAsync(cancellation.Token); break;
                    case "setCurrent": await store.SetCurrentAsync(Name(), Digest(), cancellation.Token); break;
                    case "list":
                        var cursor = operation.TryGetProperty("cursorFrom", out var cursorFrom)
                            ? cursors[cursorFrom.GetString()!]
                            : operation.TryGetProperty("cursor", out var supplied) ? supplied.GetString() : null;
                        var page = await store.ListAsync(Name(), operation.GetProperty("pageSize").GetInt32(),
                            cursor, cancellation.Token);
                        cursors[id] = page.NextCursor;
                        value = page;
                        break;
                    case "health": value = await store.IsAvailableAsync(cancellation.Token); break;
                    case "sql":
                        await using (var connection = new SqliteConnection(connectionString))
                        {
                            await connection.OpenAsync(cancellation.Token);
                            await using var command = connection.CreateCommand();
                            command.CommandText = operation.GetProperty("query").GetString()!;
                            await command.ExecuteNonQueryAsync(cancellation.Token);
                        }
                        break;
                    default: throw new InvalidOperationException("Unknown store action.");
                }
                replies.Add(new Dictionary<string, object?> {
                    ["id"] = id, ["outcome"] = "ok", ["value"] = value
                });
            }
            catch (Exception error) when (error is ContractNameAuthorityConflictException
                or ContractVersionNotFoundException or InvalidDataException or SqliteException
                or ArgumentException or FormatException or JsonException or ExpressionCompilationException
                or OperationCanceledException or InvalidOperationException or NotSupportedException)
            {
                var detail = new Dictionary<string, object?> {
                    ["id"] = id, ["outcome"] = "error",
                    ["exceptionType"] = error is OperationCanceledException
                        ? typeof(OperationCanceledException).FullName
                        : error is JsonException ? typeof(JsonException).FullName : error.GetType().FullName
                };
                if (error is ArgumentException argument) detail["parameter"] = argument.ParamName;
                if (error is ContractNameAuthorityConflictException conflict)
                {
                    detail["contractName"] = conflict.ContractName;
                    detail["existingAuthority"] = conflict.ExistingAuthority;
                    detail["requestedAuthority"] = conflict.RequestedAuthority;
                }
                if (error is ContractVersionNotFoundException or InvalidDataException
                    or ExpressionCompilationException) detail["message"] = error.Message;
                if (error is SqliteException sqlite) detail["sqliteCode"] = sqlite.SqliteErrorCode;
                replies.Add(detail);
            }
        }
        return new { id = request.GetProperty("id").GetString(), status = "value", value = replies };
    }
}
