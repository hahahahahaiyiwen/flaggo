using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.Decision;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;

namespace Flaggo.Core.Tests;

public sealed class DecisionTests
{
    private static readonly AuthorityScope Scope = new("local", "checkout", "production");

    [Fact]
    public async Task SkipsRulesMissingPredicateOrReturnAttributes()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [
                new ContractAttribute
                {
                    Name = "x",
                    Schema = new ValueSchema { Type = "integer" }
                },
                new ContractAttribute
                {
                    Name = "y",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            new ValueSchema { Type = "integer" },
            99);
        var executable = CreateExecutable(
            contract,
            Rule(
                "needs-return-attribute",
                "attributes.x > 0",
                new ExpressionReturn("attributes.y")),
            Rule(
                "eligible",
                "attributes.x > 0",
                new LiteralReturn(JsonSerializer.SerializeToElement(7))));
        var runtime = await CreateRuntimeAsync(database, contract, executable);

        var decision = await runtime.DecideAsync(
            contract.Name,
            executable.Executable.ContractDigest,
            Input(("_random", 0.5), ("x", 1L)));

        Assert.Equal(7, decision.Result.GetInt32());
        Assert.Equal("eligible", Assert.IsType<RuleEvaluation>(decision.Evaluation).Rule);
    }

    [Fact]
    public async Task NoMatchingRuleReturnsContractDefault()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [
                new ContractAttribute
                {
                    Name = "x",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            new ValueSchema { Type = "integer" },
            99);
        var executable = CreateExecutable(
            contract,
            Rule(
                "positive",
                "attributes.x > 0",
                new LiteralReturn(JsonSerializer.SerializeToElement(7))));
        var runtime = await CreateRuntimeAsync(database, contract, executable);

        var decision = await runtime.DecideAsync(
            contract.Name,
            executable.Executable.ContractDigest,
            Input(("_random", 0.5), ("x", -1L)));

        Assert.Equal(99, decision.Result.GetInt32());
        Assert.IsType<DefaultEvaluation>(decision.Evaluation);
    }

    [Fact]
    public async Task LaterRequestObservesReplacementActivation()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [],
            new ValueSchema { Type = "integer" },
            0);
        var first = CreateExecutable(
            contract,
            Rule(
                "first",
                "true",
                new LiteralReturn(JsonSerializer.SerializeToElement(1))));
        var second = CreateExecutable(
            contract,
            Rule(
                "second",
                "true",
                new LiteralReturn(JsonSerializer.SerializeToElement(2))));
        var (runtime, executableStore) = await CreateRuntimeWithStoreAsync(
            database,
            contract,
            first,
            second);
        await executableStore.ActivateAsync(
            first.Executable.ContractDigest,
            first.ExecutableDigest);

        var firstDecision = await runtime.DecideAsync(
            contract.Name,
            first.Executable.ContractDigest,
            Input(("_random", 0.25)));
        await executableStore.ActivateAsync(
            first.Executable.ContractDigest,
            second.ExecutableDigest,
            first.ExecutableDigest);
        var secondDecision = await runtime.DecideAsync(
            contract.Name,
            first.Executable.ContractDigest,
            Input(("_random", 0.25)));

        Assert.Equal(first.ExecutableDigest, firstDecision.ExecutableDigest);
        Assert.Equal(1, firstDecision.Result.GetInt32());
        Assert.Equal(second.ExecutableDigest, secondDecision.ExecutableDigest);
        Assert.Equal(2, secondDecision.Result.GetInt32());
    }

    [Fact]
    public async Task RejectsInvalidInputBeforeResolvingAnExecutable()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [],
            new ValueSchema { Type = "integer" },
            0);
        var runtime = await CreateRuntimeAsync(
            database,
            contract,
            CreateExecutable(contract));

        var exception = await Assert.ThrowsAsync<RuntimeInputValidationException>(
            () => runtime.DecideAsync(
                contract.Name,
                ContractDigests.ComputeContractDigest(
                    contract,
                    new FlaggoExpressionCompiler()),
                Input(("undeclared", 1L))));

        Assert.Contains(
            exception.Issues,
            issue => issue.Code == "invalid-internal-attribute");
        Assert.Contains(
            exception.Issues,
            issue => issue.Code == "undeclared-attribute");
    }

    [Fact]
    public async Task ReportsUnknownContractAndMissingActivationExplicitly()
    {
        using var database = new TemporaryDatabase();
        var compiler = new FlaggoExpressionCompiler();
        var contractStore = new SqliteContractVersionStore(
            database.ConnectionString,
            compiler);
        var executableStore = new SqliteExecutableStore(
            database.ConnectionString,
            compiler);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var contract = CreateContract(
            [],
            new ValueSchema { Type = "integer" },
            0);
        var contractDigest = ContractDigests.ComputeContractDigest(contract, compiler);
        await contractStore.PutAsync(new AcceptedContractVersion(
            contractDigest,
            DateTimeOffset.UtcNow,
            contract));
        var runtime = new DecisionRuntime(
            contractStore,
            executableStore,
            new FlaggoExecutableCompiler(compiler),
            new DecisionEvaluator());

        await Assert.ThrowsAsync<DecisionContractVersionNotFoundException>(
            () => runtime.DecideAsync(
                "other.name",
                contractDigest,
                Input(("_random", 0.5))));
        await Assert.ThrowsAsync<ActiveExecutableNotFoundException>(
            () => runtime.DecideAsync(
                contract.Name,
                contractDigest,
                Input(("_random", 0.5))));
    }

    [Fact]
    public async Task ExpressionFailureDoesNotReturnDefault()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [
                new ContractAttribute
                {
                    Name = "divisor",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            new ValueSchema { Type = "integer" },
            99);
        var executable = CreateExecutable(
            contract,
            Rule(
                "divide",
                "true",
                new ExpressionReturn("1 / attributes.divisor")));
        var runtime = await CreateRuntimeAsync(database, contract, executable);

        var exception = await Assert.ThrowsAsync<DecisionEvaluationException>(
            () => runtime.DecideAsync(
                contract.Name,
                executable.Executable.ContractDigest,
                Input(("_random", 0.5), ("divisor", 0L))));

        Assert.Equal("divide", exception.RuleName);
        Assert.Equal("return", exception.ExpressionRole);
    }

    [Fact]
    public async Task RejectsDynamicallyInvalidResult()
    {
        using var database = new TemporaryDatabase();
        var contract = CreateContract(
            [
                new ContractAttribute
                {
                    Name = "value",
                    Schema = new ValueSchema { Type = "integer" }
                }
            ],
            new ValueSchema { Type = "integer", Minimum = 0 },
            0);
        var executable = CreateExecutable(
            contract,
            Rule(
                "dynamic",
                "true",
                new ExpressionReturn("attributes.value")));
        var runtime = await CreateRuntimeAsync(database, contract, executable);

        var exception = await Assert.ThrowsAsync<DecisionResultValidationException>(
            () => runtime.DecideAsync(
                contract.Name,
                executable.Executable.ContractDigest,
                Input(("_random", 0.5), ("value", -1L))));

        Assert.Equal("dynamic", exception.RuleName);
        Assert.Contains(exception.Issues, issue => issue.Code == "number-out-of-range");
    }

    private static async Task<DecisionRuntime> CreateRuntimeAsync(
        TemporaryDatabase database,
        DecisionContract contract,
        params ExecutableCompilation[] executables)
    {
        var (runtime, executableStore) = await CreateRuntimeWithStoreAsync(
            database,
            contract,
            executables);
        if (executables.Length > 0)
        {
            await executableStore.ActivateAsync(
                executables[0].Executable.ContractDigest,
                executables[0].ExecutableDigest);
        }

        return runtime;
    }

    private static async Task<(DecisionRuntime Runtime, IExecutableStore ExecutableStore)>
        CreateRuntimeWithStoreAsync(
            TemporaryDatabase database,
            DecisionContract contract,
            params ExecutableCompilation[] executables)
    {
        var expressionCompiler = new FlaggoExpressionCompiler();
        var contractStore = new SqliteContractVersionStore(
            database.ConnectionString,
            expressionCompiler);
        var executableStore = new SqliteExecutableStore(
            database.ConnectionString,
            expressionCompiler);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var contractDigest = ContractDigests.ComputeContractDigest(
            contract,
            expressionCompiler);
        await contractStore.PutAsync(new AcceptedContractVersion(
            contractDigest,
            new DateTimeOffset(2026, 3, 4, 10, 0, 0, TimeSpan.Zero),
            contract));
        foreach (var executable in executables)
        {
            await executableStore.PutCandidateAsync(new StoredExecutable(
                executable.ExecutableDigest,
                executable.Executable,
                executable.CheckedExecutable,
                ExecutableLifecycleState.Candidate,
                StateVersion: 0,
                CreatedAt: new DateTimeOffset(2026, 3, 4, 10, 1, 0, TimeSpan.Zero),
                ActivatedAt: null));
        }

        return (
            new DecisionRuntime(
                contractStore,
                executableStore,
                new FlaggoExecutableCompiler(expressionCompiler),
                new DecisionEvaluator()),
            executableStore);
    }

    private static DecisionContract CreateContract(
        IReadOnlyList<ContractAttribute> attributes,
        ValueSchema resultSchema,
        object defaultValue) =>
        new()
        {
            Authority = Scope,
            Name = "checkout.delay",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes = attributes,
            Result = new ContractResult
            {
                Schema = resultSchema,
                Default = JsonSerializer.SerializeToElement(defaultValue)
            }
        };

    private static ExecutableCompilation CreateExecutable(
        DecisionContract contract,
        params ExecutableRule[] rules)
    {
        var expressionCompiler = new FlaggoExpressionCompiler();
        var executable = new DecisionExecutable
        {
            ContractDigest = ContractDigests.ComputeContractDigest(
                contract,
                expressionCompiler),
            Rules = rules
        };
        return new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
    }

    private static ExecutableRule Rule(
        string name,
        string predicate,
        RuleReturn result) =>
        new()
        {
            Name = name,
            When = new ExpressionWhen(predicate),
            Return = result
        };

    private static RuntimeInput Input(params (string Name, object Value)[] attributes) =>
        new()
        {
            Attributes = attributes.ToDictionary(
                attribute => attribute.Name,
                attribute => JsonSerializer.SerializeToElement(attribute.Value),
                StringComparer.Ordinal)
        };

    private sealed class TemporaryDatabase : IDisposable
    {
        private readonly string _path =
            Path.Combine(Path.GetTempPath(), $"flaggo-decision-{Guid.NewGuid():N}.db");

        public string ConnectionString => $"Data Source={_path};Pooling=False";

        public void Dispose()
        {
            DeleteIfExists(_path);
            DeleteIfExists($"{_path}-shm");
            DeleteIfExists($"{_path}-wal");
        }

        private static void DeleteIfExists(string path)
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
    }
}
