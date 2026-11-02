# Flaggo expressions

`flaggo-expression` is the Rust-owned bounded CEL compiler, checked-expression
materializer and evaluator. It implements the
contract module's `ExpressionCanonicalizer` and `ContractExpressionValidator`
ports. Both core services use it; the optional parity oracle is a pinned
historical checkout, not a second implementation in this tree.

`ExpressionCompiler::compile` accepts typed attribute/result schemas and an
expression purpose. It always checks the Flaggo profile, result type and static
cost, then emits deterministic checked protobuf and its SHA-256 identity.
Nullable standard overloads preserve ambiguous intermediate `dyn` types.
The owned adapter checks calls bottom-up so those types reach parent calls,
and explicitly persists `dyn` entries omitted by the upstream serializer.
Dynamic intermediate values do not relax root result schemas or profile bounds.
String-addition checked references use the reference's `add_string` overload ID,
which its restart-time cost estimator requires to propagate operand sizes.
Conditional generic results retain the reference's first-branch substitution,
including nested container parameters; incompatible branches still reject.
`materialize` takes only persisted `CheckedExpression` metadata, not source text;
it validates the byte/checksum/reference/type/canonical/profile/cost bounds and
does not parse source or rerun the source type checker. `CompiledExpression`
exposes read-only source, purpose and checked metadata, and evaluates typed
contract JSON values.

`ExecutableCompiler` composes those expression APIs into production-owned rule
compilation and restart materialization. Compilation validates the whole
contract/executable, normalizes expressions before hashing, and emits the
reference checked-rule JSON shape. Materialization verifies stored binding,
kind/count/digest/names/literal results, canonical expression correspondence
and every checked program without compiling source. Compiled rules own their
literal returns and union predicate/return attribute references; their public
API is read-only.

The limits match the reference: 4,096 source bytes, 32 AST levels, 256 AST nodes,
1,000 static cost and 65,536 checked-protobuf bytes. The reference's 10,000
runtime-iteration budget and cancellation polling apply to comprehension
iterations; Flaggo forbids comprehensions. Consequently a pre-cancelled token
does not interrupt a permitted loop-free expression, in either runtime. The
Rust API deliberately preserves that observed behavior rather than introducing
new cancellation points.

Syntax-only canonicalization is deliberately distinct from executable profile
validation: guardrails may reference opaque evidence names, unknown functions,
lists, maps and message literals. The owned lexer and heap-backed grammar
continuations leave standard macro names as ordinary calls, matching the
reference's macro-disabled parser without rewriting source or invoking the
upstream recursive parsers. Parser nesting is limited to 250 expression entries
and syntax AST depth to 1,000; these limits are distinct from executable bounds.
An iterative printer avoids recursive unparsing of long syntax-only chains.
Native diagnostics preserve one-based UTF-16 locations, sibling type errors
without cascading parent errors, parameterized type names, and the reference's
profile/declaration/type/result/static-cost failure ordering.

## Verification

```powershell
cargo test --locked -p flaggo-expression -p flaggo-contract
```

The parity probe calls this crate directly, including compiler-backed contract
and executable validation, expression-normalized identities, syntax-only
canonicalization and cancellation. It no longer owns a separate profile or
checked-expression implementation.

The differential corpus includes 373 syntax-only cases and 97 compiler-backed
validation/identity cases. Syntax failures compare exact diagnostic text, not
just rejection; contract adapters compare full validation issue arrays.
Nullable regressions cover guarded and failing operators, nested dynamic calls,
root-type rejection and both producers in all four restart directions.
An exhaustive scalar/null/Dyn/container conditional matrix exercises both branch
orders at direct roots and inside equality, Boolean conversion and size calls.
Actual decision cases assert successful reference outcomes as well as rejection,
so invalid contract fixtures cannot substitute for expression-admission proof.
Boolean ordering covers all four relational truth tables, and nullable string
addition covers both operand orders through actual cross-runtime restarts.
Focused regressions also exercise accepted 249-level nesting, rejected 250-level
nesting and the 1,000/1,001 syntax AST boundary on a 512 KiB thread stack.

The [historical differential harness](../../tools/core-parity/README.md)
provides library-level parity, not complete production-service proof. The
crate does not fall back to .NET. `flaggo-decision` composes the owned
executable compiler/evaluator through immutable store-reader snapshots. SQLite
store adapters are covered by cross-runtime persistence/lifecycle gates.
Service/API/OTel and unchanged-client full-loop tests separately verify the
composition, including native JSON fault mapping at the HTTP boundary.
