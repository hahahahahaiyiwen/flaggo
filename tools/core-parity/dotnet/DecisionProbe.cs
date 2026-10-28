using System.Text;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.Decision;
using Flaggo.DecisionService;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;

internal static class DecisionProbe
{
    internal static async Task<object> EvaluateAsync(
        JsonElement request, FlaggoExpressionCompiler expressions)
    {
        var id = request.GetProperty("id").GetString();
        var calls = new List<string>();
        Artifact? artifact = null;
        try
        {
            var contract = Read<DecisionContract>(request.GetProperty("contract"));
            var digest = ContractDigests.ComputeContractDigest(contract, expressions);
            var compiler = new FlaggoExecutableCompiler(expressions);
            CompiledDecisionExecutable? compiled = null;
            if (request.TryGetProperty("artifact", out var stored))
            {
                artifact = StrictJson.Deserialize<Artifact>(Encoding.UTF8.GetBytes(stored.GetRawText()));
            }
            else
            {
                var executable = Read<DecisionExecutable>(request.GetProperty("executable")) with
                {
                    ContractDigest = digest
                };
                var compilation = compiler.Compile(contract, executable);
                compiled = compilation.CompiledExecutable;
                artifact = new Artifact(
                    compilation.Executable, compilation.ExecutableDigest, compilation.CheckedExecutable);
            }
            var action = request.GetProperty("decisionAction").GetString();
            if (action == "compile")
            {
                return new { id, status = "value", artifact, value = new {
                    outcome = "compiled", artifact.Executable, artifact.ExecutableDigest } };
            }
            var input = Read<RuntimeInput>(request.GetProperty("input"));
            using var cancellation = new CancellationTokenSource();
            if (request.TryGetProperty("cancelled", out var cancelled) && cancelled.GetBoolean())
                cancellation.Cancel();
            RuntimeDecision decision;
            if (action == "evaluate")
            {
                compiled ??= compiler.Materialize(
                    contract, digest, artifact.Executable,
                    artifact.ExecutableDigest, artifact.CheckedExecutable);
                var evaluationContract = request.TryGetProperty("evaluationContract", out var different)
                    ? Read<DecisionContract>(different) : contract;
                decision = new DecisionEvaluator().Evaluate(
                    evaluationContract, compiled, input, cancellation.Token);
            }
            else if (action == "runtime")
            {
                var contracts = new ContractReader(
                    new AcceptedContractVersion(digest, DateTimeOffset.UnixEpoch, contract),
                    Option(request, "contractFault"), calls);
                var active = new ExecutableReader(
                    new StoredExecutable(artifact.ExecutableDigest, artifact.Executable,
                        artifact.CheckedExecutable, ExecutableLifecycleState.Active,
                        1, DateTimeOffset.UnixEpoch, DateTimeOffset.UnixEpoch),
                    Option(request, "activeFault"), calls);
                decision = await new DecisionRuntime(
                    contracts, active, compiler, new DecisionEvaluator()).DecideAsync(
                        Option(request, "contractName") ?? contract.Name,
                        Option(request, "contractDigest") ?? digest, input, cancellation.Token);
            }
            else throw new InvalidOperationException("Unknown decision action.");
            return new { id, status = "value", artifact, value = new {
                outcome = "decision", decision, calls,
                source = DecisionServiceObservability.EvaluationSource(decision) } };
        }
        catch (ExecutableCompilationException error)
        {
            return new { id, status = "value", value = new {
                outcome = "compile-invalid", message = error.Message, issues = error.Issues } };
        }
        catch (ExecutableMaterializationException error)
        {
            return new { id, status = "value", artifact, value = new {
                outcome = "materialize-invalid", message = error.Message } };
        }
        catch (Exception error) when (error is RuntimeInputValidationException
            or DecisionResultValidationException or DecisionEvaluationException
            or DecisionContractVersionNotFoundException or ActiveExecutableNotFoundException
            or RuntimeIntegrityException or ArgumentException or OperationCanceledException
            or IOException or TimeoutException)
        {
            var classification = DecisionServiceObservability.Classify(error);
            var details = new Dictionary<string, object?> {
                ["outcome"] = "error",
                ["exceptionType"] = error is OperationCanceledException
                    ? typeof(OperationCanceledException).FullName : error.GetType().FullName,
                ["calls"] = calls,
                ["telemetry"] = new { outcome = classification.Outcome, category = classification.Category }
            };
            if (error is RuntimeInputValidationException input) details["issues"] = input.Issues;
            if (error is DecisionResultValidationException result)
            {
                details["rule"] = result.RuleName;
                details["issues"] = result.Issues;
            }
            if (error is DecisionEvaluationException evaluation)
            {
                details["rule"] = evaluation.RuleName;
                details["role"] = evaluation.ExpressionRole;
            }
            if (error is not ArgumentException && error is not OperationCanceledException)
                details["message"] = error.Message;
            return new { id, status = "value", artifact, value = details };
        }
        catch (JsonException error)
        {
            return new { id, status = "json-error", message = error.Message };
        }
        catch (Exception error) when (error is NullReferenceException
            or InvalidOperationException or NotSupportedException)
        {
            return new { id, status = "runtime-fault", exceptionType = error.GetType().FullName,
                message = error.Message };
        }
    }

    private static T Read<T>(JsonElement value) =>
        StrictJson.Deserialize<T>(Encoding.UTF8.GetBytes(value.GetString()!));

    private static string? Option(JsonElement request, string name) =>
        request.TryGetProperty(name, out var value) ? value.GetString() : null;

    internal sealed record Artifact(
        DecisionExecutable Executable, string ExecutableDigest,
        CheckedDecisionExecutable CheckedExecutable);

    private static void Fail(string? fault)
    {
        switch (fault)
        {
            case "integrity": throw new InvalidDataException("injected integrity failure");
            case "dependency": throw new IOException("injected dependency failure");
            case "timeout": throw new TimeoutException("injected timeout");
            case "cancelled": throw new OperationCanceledException();
            case null or "missing": return;
            default: throw new InvalidOperationException("Unknown injected store fault.");
        }
    }

    private sealed class ContractReader(
        AcceptedContractVersion version, string? fault, List<string> calls) : IContractVersionStore
    {
        public Task<AcceptedContractVersion?> GetAsync(
            string name, string digest, CancellationToken cancellationToken = default)
        {
            calls.Add("contract");
            Fail(fault);
            return Task.FromResult<AcceptedContractVersion?>(
                fault != "missing" && name == version.Contract.Name && digest == version.ContractDigest
                    ? version : null);
        }
        public Task InitializeAsync(CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ContractStoreWriteResult> PutAsync(AcceptedContractVersion value, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<AcceptedContractVersion?> GetCurrentAsync(string name, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<IReadOnlyList<AcceptedContractVersion>> ListAllCurrentAsync(CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ContractVersionPage> ListAsync(string name, int pageSize, string? cursor, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task SetCurrentAsync(string name, string digest, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default) => throw new NotSupportedException();
    }

    private sealed class ExecutableReader(
        StoredExecutable active, string? fault, List<string> calls) : IExecutableStore
    {
        public Task<StoredExecutable?> GetActiveAsync(
            string digest, CancellationToken cancellationToken = default)
        {
            calls.Add("active");
            Fail(fault);
            return Task.FromResult<StoredExecutable?>(fault == "missing" ? null : active);
        }
        public Task InitializeAsync(CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ExecutableStoreWriteResult> PutCandidateAsync(StoredExecutable value, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<AnalysisCandidateStoreResult> PutAnalysisCandidateAsync(StoredExecutable value, AnalysisCandidateAdmission admission, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<StoredExecutable?> GetAsync(string digest, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ExecutablePage> ListCandidatesAsync(string digest, int pageSize, string? cursor, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ExecutablePage> ListExecutablesAsync(string digest, int pageSize, string? cursor, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ActivationResult> ActivateAsync(string digest, string executable, string? expectedActiveExecutableDigest = null, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<ActivationResult> ActivateIfNoneAsync(string digest, string executable, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<CandidateActivationResult> ActivateCandidateAsync(CandidateActivationCommand command, CancellationToken cancellationToken = default) => throw new NotSupportedException();
        public Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default) => throw new NotSupportedException();
    }
}
