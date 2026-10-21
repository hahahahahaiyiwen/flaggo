# Core-server parity migration

Issue #82 temporarily keeps .NET as the reference while developing Rust
replacements. Preserve current observable behavior; do not switch launchers,
delete .NET, or reset persisted state until service, storage, HTTP, telemetry,
and deterministic full-loop parity pass.

## Run the feasibility checks

From the repository root:

```powershell
python -m unittest discover -s tools\core-parity -p "test_*.py"
cargo test --locked --manifest-path tools\core-parity\rust\Cargo.toml
cargo test --locked -p flaggo-contract
python tools\core-parity\run.py --profile --report .flaggo\core-parity\cel-report.json
```

The runner builds both probes, sends identical JSON-lines requests, validates
response IDs and shapes, and writes a machine-readable report. Mismatches exit
1; build/probe failures exit 2 and preserve a failure report. `--no-build` reuses
existing binaries. CEL dependencies remain in an isolated Cargo
workspace/lockfile. The probe also depends on the new production-owned
`modules\contract` Rust crate; its workspace member and dependencies are
tracked in the root manifest/lockfile.

The .NET oracle calls the existing compiler and materializer unchanged. Both
directions explicitly use UTF-8, including redirected Windows streams.
Values compare structurally, with JSON numeric equivalence but booleans distinct
from numbers. Error stages compare, not library-specific exception wording;
unhandled non-finite serialization faults remain distinct from evaluation errors.
Successful compilation/evaluation also requires exactly matching canonical CEL.

## Rust prototype

The bounded prototype combines pinned pure-Rust libraries:

- `cel` 0.15.0 supplies the Pratt parser and evaluator, with explicit Flaggo
  profile/type/cost checks and canonical printing.
- `cel-core`/`cel-core-proto` 0.5.1 check the bounded AST and produce standard CEL
  checked protobuf. They do not parse source a second time. The adapter preserves
  qualified attribute types, standard unary-negation names, and deterministic
  map serialization for repeatable checked-AST bytes/checksums.
- `serde_jcs` 0.2.0 is checked against the shared RFC 8785 golden vectors,
  including ECMAScript number formatting and UTF-16 property ordering.

JSON inputs follow the existing signed-int64-then-double mapping. String
conversion retains Celly's Go-style float formatting. Source/AST/static-cost
bounds and Celly's 250-level parser-recursion limit are explicit, including
grouping that the Pratt parser otherwise optimizes away.

The original ANTLR parser and `cel-core` source parser overflowed their Windows
process stacks on bounded inputs. The prototype instead parses once with Pratt,
checks Flaggo's AST bounds before further recursive processing, and transfers
that AST to the checked compiler through CEL protobuf.

Restart checks deliberately replace source text with invalid syntax, then load
only the stored protobuf. They validate checksums, references, canonical form,
stored types, profile/cost/size limits, and evaluate without reparsing source
or invoking the source type checker. Both .NET-produced and Rust-produced
artifacts must reload in both processes.

## Current coverage and remaining gates

The profile corpus covers 106 expressions, including exact 4096-byte source,
32-level AST, 256-node AST and 1000-cost boundaries; parser nesting; Unicode;
canonical precedence/escapes; conversions; overflow and error absorption.
All 63 accepted expressions also undergo four cross-runtime restart checks.
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

These are feasibility results, not a production compiler or whole-service
replacement. Full schema/profile/cost conformance, cancellation, normalized
canonicalizer-backed contract/executable digests and full compiler-backed
validation, complete strict JSON behavior,
persisted SQLite compatibility,
admission/activation concurrency, HTTP/OTel behavior and the deterministic
end-to-end loop remain migration gates. Existing services and default launchers
still use .NET.

In particular, out-of-range `JsonElement` numbers and missing/out-of-order
polymorphic discriminator exception/HTTP behavior remain unverified. Rust
currently rejects the latter but has not reproduced their legacy native-fault
category. Domain baseline parity does not remove those gates.

Omitting `--profile` measures the unadapted `cel` library. That comparison is
expected to fail and can abort on parser stress cases; it is diagnostic only.
The earlier 44-case baseline matched 27 cases. Its apparent Unicode size/order
differences were Windows probe-encoding bugs, not production semantics. UTF-8
corrected those findings; do not preserve the encoding artifacts.
