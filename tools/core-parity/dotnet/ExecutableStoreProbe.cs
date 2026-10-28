using System.Globalization;
using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.Decision;
using Flaggo.DecisionService;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

internal static class ExecutableStoreProbe
{
    internal static async Task<object> EvaluateAsync(JsonElement request, FlaggoExpressionCompiler expressions)
    {
        var connectionString = new SqliteConnectionStringBuilder {
            DataSource = request.GetProperty("database").GetString(), Pooling = false
        }.ToString();
        var canonicalizer = request.GetProperty("withExpressions").GetBoolean() ? expressions : null;
        var store = new SqliteExecutableStore(connectionString, canonicalizer,
            new Clock(ParseTime(request.GetProperty("now").GetString()!)));
        var contracts = new SqliteContractVersionStore(connectionString, canonicalizer);
        var versions = new Dictionary<string, AcceptedContractVersion>();
        var records = new Dictionary<string, StoredExecutable>();
        var admissions = new Dictionary<string, AnalysisCandidateAdmission>();
        var pages = new Dictionary<string, AnalysisCandidateActivationPage>();
        var cursors = new Dictionary<string, string?>();
        var replies = new List<object>();
        foreach (var op in request.GetProperty("operations").EnumerateArray())
        {
            var id = op.GetProperty("id").GetString()!;
            var action = op.GetProperty("action").GetString()!;
            using var cancel = new CancellationTokenSource();
            if (op.TryGetProperty("cancelled", out var cancelled) && cancelled.GetBoolean()) cancel.Cancel();
            try
            {
                string Alias() => op.GetProperty("use").GetString()!;
                string Digest() => Text(op, "executableDigest") ?? records[Alias()].ExecutableDigest;
                string ContractDigest() => Text(op, "contractDigest") ?? records[Alias()].Executable.ContractDigest;
                string? ExpectedDigest() => Text(op, "expected") ??
                    (Text(op, "expectedFrom") is { } from ? records[from].ExecutableDigest : null);
                ExecutableLifecycleToken? ExpectedToken() => Text(op, "expectedFrom") is { } from
                    ? new ExecutableLifecycleToken(records[from].ExecutableDigest,
                        op.TryGetProperty("activeVersion", out var av) ? av.GetInt64() : 1)
                    : op.TryGetProperty("expectedActive", out var tokenValue)
                        ? JsonSerializer.Deserialize<ExecutableLifecycleToken?>(tokenValue.GetRawText(), StrictJson.Options) : null;
                object? value = null;
                switch (action)
                {
                    case "initialize": await store.InitializeAsync(cancel.Token); break;
                    case "initializeContract": await contracts.InitializeAsync(cancel.Token); break;
                    case "accept":
                        var contract = Read<DecisionContract>(op.GetProperty("document").GetString()!);
                        var version = new AcceptedContractVersion(
                            ContractDigests.ComputeContractDigest(contract, canonicalizer),
                            ParseTime(op.GetProperty("createdAt").GetString()!), contract);
                        versions[id] = version;
                        await contracts.PutAsync(version, cancel.Token);
                        await contracts.SetCurrentAsync(contract.Name, version.ContractDigest, cancel.Token);
                        break;
                    case "put":
                    case "admit":
                        var accepted = versions[op.GetProperty("contract").GetString()!];
                        var authored = Read<DecisionExecutable>(op.GetProperty("document").GetString()!) with {
                            ContractDigest = accepted.ContractDigest
                        };
                        var compiled = new FlaggoExecutableCompiler(expressions).Compile(accepted.Contract, authored);
                        var executable = op.TryGetProperty("raw", out var raw) && raw.GetBoolean() ? authored : compiled.Executable;
                        var stored = new StoredExecutable(
                            Text(op, "executableDigest") ?? compiled.ExecutableDigest, executable,
                            op.TryGetProperty("checked", out var check) ? Read<CheckedDecisionExecutable>(check.GetRawText()) : compiled.CheckedExecutable,
                            (ExecutableLifecycleState)(op.TryGetProperty("state", out var state) ? state.GetInt32() : 0),
                            op.TryGetProperty("stateVersion", out var sv) ? sv.GetInt64() : 0,
                            ParseTime(op.GetProperty("createdAt").GetString()!),
                            Text(op, "activatedAt") is { } time ? ParseTime(time) : null,
                            Text(op, "provenance") is { } provenance ? Read<JsonElement>(provenance) : null);
                        records[id] = stored;
                        if (action == "put")
                            value = (await store.PutCandidateAsync(stored, cancel.Token)).ToString().ToLowerInvariant();
                        else
                        {
                            var admission = new AnalysisCandidateAdmission(
                                Text(op, "workspaceId") ?? "workspace", Text(op, "cycleId") ?? id,
                                Text(op, "attemptId") ?? "attempt", Text(op, "contractName") ?? accepted.Contract.Name,
                                Text(op, "admissionContractDigest") ?? stored.Executable.ContractDigest,
                                Text(op, "admissionExecutableDigest") ?? stored.ExecutableDigest,
                                Text(op, "manifestDigest") ?? "sha256:" + new string('a', 64),
                                Text(op, "admissionTime") is { } at ? ParseTime(at) : stored.CreatedAt);
                            admissions[id] = admission;
                            value = await store.PutAnalysisCandidateAsync(stored, admission, cancel.Token);
                        }
                        break;
                    case "get": value = await store.GetAsync(Digest(), cancel.Token); break;
                    case "active": value = await store.GetActiveAsync(ContractDigest(), cancel.Token); break;
                    case "activate":
                        value = await store.ActivateAsync(ContractDigest(), Digest(), ExpectedDigest(), cancel.Token); break;
                    case "activateIfNone":
                        value = await store.ActivateIfNoneAsync(ContractDigest(), Digest(), cancel.Token); break;
                    case "activateCandidate":
                        var token = ExpectedToken()!;
                        value = await store.ActivateCandidateAsync(new CandidateActivationCommand(
                            Text(op, "contractName") ?? versions[op.GetProperty("contract").GetString()!].Contract.Name,
                            ContractDigest(), Digest(), op.GetProperty("expectedVersion").GetInt64(), token), cancel.Token);
                        break;
                    case "list":
                    case "candidates":
                        var cursor = Text(op, "cursorFrom") is { } from ? cursors[from] : Text(op, "cursor");
                        var size = op.GetProperty("pageSize").GetInt32();
                        var page = action == "list"
                            ? await store.ListExecutablesAsync(ContractDigest(), size, cursor, cancel.Token)
                            : await store.ListCandidatesAsync(ContractDigest(), size, cursor, cancel.Token);
                        cursors[id] = page.NextCursor;
                        value = page;
                        break;
                    case "pending":
                        var pending = await store.ListPendingAsync(op.GetProperty("pageSize").GetInt32(),
                            Text(op, "afterContractName"), cancel.Token);
                        pages[id] = pending;
                        value = pending;
                        break;
                    case "resolve":
                        var snapshot = pages[op.GetProperty("snapshotFrom").GetString()!].Candidates[
                            op.TryGetProperty("index", out var index) ? index.GetInt32() : 0];
                        var resolvedAdmission = snapshot.Admission with {
                            CreatedAt = Text(op, "admissionTime") is { } admittedAt ? ParseTime(admittedAt) : snapshot.Admission.CreatedAt
                        };
                        var expected = op.TryGetProperty("expectedActive", out _) || op.TryGetProperty("expectedFrom", out _)
                            ? ExpectedToken() : snapshot.ActiveExecutable;
                        value = await store.ResolveAsync(new AnalysisCandidateResolutionCommand(
                            resolvedAdmission, op.TryGetProperty("expectedVersion", out var expectedVersion)
                                ? expectedVersion.GetInt64() : snapshot.CandidateStateVersion,
                            expected, (AnalysisCandidateResolutionAction)op.GetProperty("resolution").GetInt32()), cancel.Token);
                        break;
                    case "runtime":
                        var runtimeVersion = versions[op.GetProperty("contract").GetString()!];
                        value = await new DecisionRuntime(contracts, store,
                            new FlaggoExecutableCompiler(expressions), new DecisionEvaluator()).DecideAsync(
                                runtimeVersion.Contract.Name, runtimeVersion.ContractDigest,
                                Read<RuntimeInput>(op.GetProperty("input").GetString()!), cancel.Token);
                        break;
                    case "health": value = await store.IsAvailableAsync(cancel.Token); break;
                    case "sql":
                        await using (var connection = new SqliteConnection(connectionString))
                        {
                            await connection.OpenAsync(cancel.Token);
                            await using var command = connection.CreateCommand();
                            command.CommandText = op.GetProperty("query").GetString()!;
                            await command.ExecuteNonQueryAsync(cancel.Token);
                        }
                        break;
                    default: throw new InvalidOperationException("Unknown executable-store probe action.");
                }
                replies.Add(new Dictionary<string, object?> { ["id"] = id, ["outcome"] = "ok", ["value"] = value });
            }
            catch (Exception error) when (error is ArgumentException or IOException or InvalidDataException or SqliteException
                or FormatException or JsonException or InvalidOperationException or NullReferenceException
                or NotSupportedException or OperationCanceledException or ExecutableNotFoundException
                or ExecutableContractMismatchException or ActivationConflictException
                or CandidateActivationConflictException or ActivationContractConflictException
                or CandidateAdmissionConflictException or CandidateLifecycleConflictException
                or ExecutableCompilationException or ActiveExecutableNotFoundException or RuntimeIntegrityException)
            {
                var detail = new Dictionary<string, object?> {
                    ["id"] = id, ["outcome"] = "error", ["exceptionType"] = error is JsonException
                        ? typeof(JsonException).FullName : error is OperationCanceledException
                        ? typeof(OperationCanceledException).FullName : error.GetType().FullName
                };
                if (error is ArgumentException argument) detail["parameter"] = argument.ParamName;
                if (error is SqliteException sqlite) detail["sqliteCode"] = sqlite.SqliteErrorCode;
                if (error is ExecutableNotFoundException or ExecutableContractMismatchException
                    or ActivationConflictException or CandidateActivationConflictException
                    or ActivationContractConflictException or CandidateAdmissionConflictException
                    or CandidateLifecycleConflictException or InvalidDataException or RuntimeIntegrityException)
                    detail["message"] = error.Message;
                if (action == "runtime")
                {
                    var classification = DecisionServiceObservability.Classify(error);
                    detail["telemetry"] = new { outcome = classification.Outcome, category = classification.Category };
                }
                replies.Add(detail);
            }
        }
        return new { id = request.GetProperty("id").GetString(), status = "value", value = replies };
    }

    private static string? Text(JsonElement op, string key) =>
        op.TryGetProperty(key, out var value) && value.ValueKind != JsonValueKind.Null ? value.GetString() : null;
    private static T Read<T>(string text) => StrictJson.Deserialize<T>(Encoding.UTF8.GetBytes(text));
    private static DateTimeOffset ParseTime(string time) =>
        DateTimeOffset.ParseExact(time, "O", CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
    private sealed class Clock(DateTimeOffset now) : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => now;
    }
}
