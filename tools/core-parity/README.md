# Historical core-server parity

The core services and all normal launchers/CI now use Rust. Superseded C#
sources, projects, solution, NuGet configuration and .NET prerequisites have
been removed after library, storage, HTTP, telemetry and unchanged-client
deterministic full-loop acceptance. Existing SQLite state is preserved.

The migration differential gates remain opt-in against a **separate historical
checkout**, not a second maintained implementation or runtime fallback.
Only these diagnostic commands require .NET SDK 10. Normal Cargo, npm and
conformance commands do not build, discover or launch .NET.

## Prepare the explicit historical oracle

From the current repository, create a detached checkout of the frozen
pre-cutover revision (fetch the issue branch/PR containing it first if needed):

```powershell
git worktree add --detach ..\flaggo-dotnet-oracle c4532a103d68a6659ca8ea94e9a9595c7265d14e
```

Every differential invocation requires `--reference-root`. The runner rejects
the current tree, missing projects, a different revision or modified/untracked
files before building either probe. Ignored build outputs are allowed; no sources are
copied back into the current tree. The report records the resolved reference
root and exact revision. The pinned checkout is historical test input, never
part of default CI or service composition.

## Run the feasibility checks

From the repository root:

```powershell
python -m unittest discover -s tools\core-parity -p "test_*.py"
cargo test --locked --manifest-path tools\core-parity\rust\Cargo.toml
cargo test --locked -p flaggo-contract
python tools\core-parity\run.py --reference-root ..\flaggo-dotnet-oracle --profile --report .flaggo\core-parity\cel-report.json
```

The runner builds the historical .NET probe and current Rust probe, sends identical JSON-lines requests, validates
response IDs and shapes, and writes a machine-readable report. Mismatches exit
1; build/probe failures exit 2 and preserve a failure report. `--no-build` reuses
existing probe binaries. The diagnostic probe retains an isolated Cargo workspace/lockfile, but its
bounded profile calls the production-owned `modules\expression` and
`modules\contract` Rust crates. Their workspace members and dependencies are
tracked in the root manifest/lockfile.

Decision requests additionally call `flaggo-decision` and the owned executable
compiler. The historical .NET probe references the unchanged Decision service
assembly so it exercises the real runtime, evaluator and telemetry classifier;
explicit read-only fake stores isolate this gate from SQLite. Unused store
operations fail explicitly rather than acting as fallback implementations.

The .NET oracle calls the existing compiler and materializer unchanged. Both
directions explicitly use UTF-8, including redirected Windows streams.
Values compare structurally, with JSON numeric equivalence but booleans distinct
from numbers. Ordinary evaluation error stages compare without requiring
library-specific exception wording; syntax errors and compiler-backed
validation issues require exact native diagnostic text, including source positions;
unhandled non-finite serialization faults remain distinct from evaluation errors.
Successful compilation/evaluation also requires exactly matching canonical CEL.
The `jsonFaults` gate additionally compares native exception families, not just
generic rejection status. Its `rawRoundtrip` action returns serialized JSON as
text, preserving numeric lexemes without overflowing Python's transport floats.

## Opt-in live HTTP acceptance

Run the owned library/runtime/store corpus and both actual-host HTTP gates in
one acceptance command:

```powershell
python tools\core-parity\run.py --reference-root ..\flaggo-dotnet-oracle --profile --http --report .flaggo\core-parity\acceptance.json
```

`--http` requires `--profile`: the unadapted upstream library is a diagnostic
probe, not a service candidate. In addition to building the existing probes,
the runner explicitly builds both unchanged .NET reference applications in
Release **in the external historical checkout** and invokes the current Rust
Contract and Decision HTTP gates together. It supplies
`FLAGGO_DOTNET_REFERENCE_ROOT` only to that Cargo child; ignored gates fail
explicitly if launched directly without a reference root.
Cargo builds their Rust services and test binaries as needed. This is
historical differential acceptance; no .NET sources remain in the current tree.

The JSON report adds `httpServices` with status, total/matched comparisons,
per-service/per-producer counts and the path to the sibling
`acceptance.http.log`. That log records the reference build commands,
test command, stdout and stderr. The current gates prove 78 management and
58 runtime comparisons over both .NET- and Rust-produced databases, including
automatic activation of differing .NET checked artifacts without rewriting.
Their existing assertions/normalization policies are unchanged.

Both gates emit versioned `FLAGGO_HTTP_PARITY` JSON records only after each
producer's assertions complete. Acceptance requires successful process exit,
exactly one record for each service/producer pair and at least the established
39 Contract / 29 Decision comparisons per producer. Zero/ignored tests,
missing/duplicate/malformed records, reduced counts and nonzero test/build exits
cannot produce a passing report. Command/protocol failures exit 2 and replace
the report with `probe-failure` and HTTP status `not-completed`; a log path is
reported even if an earlier prerequisite failure prevented its creation.
Library mismatches still exit 1.

Without `--http`, the report explicitly says `httpServices.status` is
`not-requested`; no additional reference host builds or HTTP tests run. Normal
Rust test runs still leave the .NET-dependent gates ignored. For repeated
acceptance, `--no-build --profile --http` skips probe and reference builds but
still executes `cargo test`, which may rebuild stale/missing Rust artifacts.
It never skips the live assertions or substitutes an old success report.

Emitted-OTLP router/process tests and the unchanged-TypeScript-client full loop
are separate gates, not silently credited by the live HTTP comparison report:

```powershell
cargo test --locked -p flaggo-contract-service -p flaggo-decision-service --test http
npm run test:tetris-full-loop
```

The deterministic real Rust full loop verifies reviewed Candidate admission,
automatic activation, SDK/REST behavior and exact authority/provenance
persistence after restart without .NET or model credentials. See its
[instructions and limits](../../examples/tetris-integration/README.md#rust-full-loop-acceptance).
These local commands do not establish remote CI execution.
The Contracts workflow runs the lightweight Python harness regressions, which
mock command failures and require no reference hosts. It does not silently
enable the opt-in live HTTP gates.

## Rust-owned expression module

The bounded module combines pinned pure-Rust libraries:

- `cel` 0.15.0 supplies the evaluator and AST/token types. The owned frontend
  implements macro-disabled grammar and lexical behavior; explicit Flaggo
  profile/type/cost checks and canonical printing preserve the reference seams.
- `cel-core`/`cel-core-proto` 0.5.1 check the bounded AST and produce standard CEL
  checked protobuf. They do not parse source a second time. The adapter preserves
  qualified attribute types, standard unary-negation names, and deterministic
  map serialization for repeatable checked-AST bytes/checksums.
- `serde_jcs` 0.2.0 is checked against the shared RFC 8785 golden vectors,
  including ECMAScript number formatting and UTF-16 property ordering.

JSON inputs follow the existing signed-int64-then-double mapping. String
conversion retains Celly's Go-style float formatting. Source/AST/static-cost
bounds and Celly's 250-level parser-recursion limit are explicit, including
grouping that is absent from the resulting AST.

The compiler always produces checked artifacts. Contract adapters also exercise
syntax-only canonicalization for opaque guardrail expressions, normalized
contract/executable identities, and compiler-backed validation. The 371 syntax
cases, 91 adapter cases and three pre-cancelled evaluation cases extend
the original differential baseline. The reference only polls cancellation when
iterating comprehensions, which Flaggo forbids; the Rust evaluator preserves
its loop-free behavior rather than adding new cancellation semantics.

The original ANTLR parser and `cel-core` source parser overflowed their Windows
process stacks on bounded inputs. The owned frontend instead uses heap-backed
grammar continuations, checks Flaggo's AST bounds before further recursive
processing, and transfers that AST to the checked compiler through CEL protobuf.
Low-stack regressions verify 249/250-level nesting and 1,000/1,001 syntax AST
depth without increasing the production thread stack.

Restart checks deliberately replace source text with invalid syntax, then load
only the stored protobuf. They validate checksums, references, canonical form,
stored types, profile/cost/size limits, and evaluate without reparsing source
or invoking the source type checker. Both .NET-produced and Rust-produced
artifacts must reload in both processes.

## Current coverage and remaining gates

The original profile corpus covered 106 expressions, including exact 4096-byte source,
32-level AST, 256-node AST and 1000-cost boundaries; parser nesting; Unicode;
canonical precedence/escapes; conversions; overflow and error absorption.
Later review regressions expand that corpus; the report records the current
measured counts. All accepted expressions undergo four cross-runtime restart checks.
Eight corruption cases cover checksum, canonical text, references, malformed
protobuf, empty bytes, result type and undeclared attributes. Separate checks
exercise the exact 65536-byte checked-protobuf limit using valid unknown-field
padding, then reject 65537 bytes.

Domain cases call the real Rust contract crate and unchanged .NET models/digest
functions. They compare strict deserialization, typed round trips and normalized
contract/executable digests without expression canonicalizers. They include the
published management-fixture digest, UTF-16 named/set-like ordering, description
metadata, literal values, constructor missing/null distinctions, rule unions,
all four evidence-source variants and exact JSON byte/depth/string limits.
Unicode decoding faults remain distinct from ordinary JSON errors; the oracle
captures native deserialization faults without reclassifying them.

Validation cases also invoke the Rust production module and unchanged .NET
contract, schema, value, runtime-input and executable validators, using their
no-expression-validator overloads. Results compare issue codes, severities,
JSON-pointer paths, messages and array ordering exactly. Coverage includes
keyword applicability, bounds, lexical int64 and Unicode rune rules, JCS array
uniqueness, learning durations/evidence/correlations, collection limits,
`_random`, exposure byte limits, executable contract binding and native faults.
Literal/context JSON stays text in the protocol, preserving original property
order. Separate Rust tests exercise the expression-validator port and zero-based
schema/value depth boundaries.

Without `--http`, these are library-level migration results, not a whole-service
replacement. HTTP comparisons and separately executed emitted-OTel/full-loop
gates extend the library/store proof; none alone authorizes cutover.
All default launchers use Rust. The pinned historical oracle is used only
when explicitly requested.

The owned frontend closes the previous standard-macro expansion and native
diagnostic mismatches. Regression cases cover ordinary macro-shaped calls,
lexical/syntactic failures, optional syntax, UTF-16 locations, multiple sibling
type errors without cascading parent errors, container type names, and
profile/declaration/type/result/static-cost ordering. Syntax comparison requires
exact native error messages. These results do not establish whole-service HTTP
fault or storage/runtime compatibility.

The separate 170-case `jsonFaults` gate closes the previously recorded
out-of-range `JsonElement` number and polymorphic-discriminator library gaps.
It checks lossless raw-number round trips, normal finite-number validation
issues, defaults/literal returns, canonicalization rejection, typed infinite
schema bounds, compiler-backed admission, native serialization failures and
missing/late versus unknown/invalid discriminator handling. Reserved metadata,
global duplicate validation and first model-failure ordering are covered too.
The owned APIs retain typed native-fault variants rather than matching error
strings or routing all failures to ordinary JSON rejection. No global
arbitrary-precision setting changes other services' persisted bytes.

Actual HTTP status/envelope behavior for these categories remains part of the
service-composition gate. Library error-category parity is not HTTP/full-loop
acceptance.

The decision gate covers request identity and lookup/validation ordering,
complete inputs, `_random`/exposure limits, rule ordering, missing
predicate/return attributes (including unreachable branches), no-match defaults,
literal/dynamic result validation, expression failures, cancellation,
store fault routing and exact telemetry outcome/category/source dimensions.
It compares complete decision identities/provenance and public error details,
not library-specific inner CEL exception wording. Reference outcome assertions
ensure success/error/limit vectors really reach the intended paths.

Successful decisions undergo four executable-level restart combinations.
The original admission document is replaced with invalid JSON; the persisted
normalized executable and checked rules are the only executable inputs.
Corruption gates cover binding, content and checked-AST digests, kind, rule
count/name, canonical source correspondence, references and unexpected checked
returns. All decision/restart/corruption reports participate in exit status.
Contract-model coverage also checks strict decision-provenance decoding.
Rust tests independently verify activation replacement, immutable compilation,
same-key failed-materialization retry, checked-identity cache misses, concurrent
single-entry materialization and exact 1,024-entry FIFO eviction.

These decision-library results alone do not establish service parity.
HTTP responses, emitted OTel signals and unchanged-client full-loop execution
have separate service-level gates. SQLite/lifecycle proof is also a separate gate.

The `contractStore` gate calls the real .NET and Rust SQLite contract stores.
Contract schema v3 is separate from executable schema v5. It covers immutable
versions, authority ownership, current selection, history/cursors, catalog,
health, cancellation, corruption and native failures, with and without expression
canonicalization. Native argument subtypes/parameters, integrity details and
primary SQLite codes compare explicitly. JSON failures compare the public
`JsonException` family, not the private CLR reader subclass.

Both producer databases are reinitialized and read by both runtimes. Snapshots
require exact preservation of stored BLOBs/timestamps, resources, current pointers
and schema metadata. Reference-success assertions reject all-failed pseudo-parity.
The gate also bootstraps the actual .NET executable-v5 schema, adds/selects a
Rust contract version and verifies coexistence. All operation comparisons
participate in exit status. This is contract persistence proof, not executable
admission/activation, v4 migration or whole-service acceptance.

The separate `executableStore` gate now verifies executable-v5 persistence,
v4 migration, candidate immutability, admission idempotency, queries/pagination,
current-contract preference, manual activation and all automatic-resolution
outcomes. Native/integrity errors, cancellation, lifecycle tokens and real
SQLite-backed runtime decisions participate in comparisons.

Each store receives identical prepared checked artifacts from each compiler.
Standard protobuf source metadata and hence checked-byte digests may legitimately
differ between compilers; the storage gate does not require compiler byte equality
or remove checked fields from read comparisons. It exercises both artifact
producers, both database producers and both database consumers.
Restart requires byte-for-byte unchanged persisted records/schema. Cross-runtime
activation/rejection on cloned producer databases must produce exactly the same
persisted state. Successful v4 migration preserves payload/provenance/lifecycle
and admission identities; foreign-key copy failures must roll back DDL/version/data.
Reference assertions reject all-failed or wrong-path pseudo-parity, and missing
or reordered operation responses fail closed.

Focused Rust tests additionally verify concurrent cycle admissions, one-winner
basic/manual activation, automatic-resolution idempotency, active-token ABA,
rollback with pending work preserved and concurrent initialization/migration.
These close the library/store increment, not HTTP/OTel/full-loop or cutover.

Omitting `--profile` measures the unadapted `cel` library. That comparison is
expected to fail and can abort on parser stress cases; it is diagnostic only.
The earlier 44-case baseline matched 27 cases. Its apparent Unicode size/order
differences were Windows probe-encoding bugs, not production semantics. UTF-8
corrected those findings; do not preserve the encoding artifacts.
