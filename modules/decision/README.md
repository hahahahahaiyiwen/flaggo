# Decision domain and runtime

`flaggo-decision` is the Rust-owned decision evaluator and request runtime for
issue #82. It composes `flaggo-contract` and `flaggo-expression` alongside the
unchanged .NET implementation; no service or launcher uses it by default yet.

`DecisionRuntime::decide` validates the requested identity, resolves the accepted
contract, validates the complete `RuntimeInput`, then reads the current active
executable on every request. Missing versions/activation, store integrity faults
and cross-contract bindings remain explicit errors. The asynchronous
`ContractVersionReader` and `ActiveExecutableReader` ports take immutable,
owned snapshots and a cancellation handle. Implementations must validate stored
records before returning them and classify integrity/dependency/timeout/
cancellation failures while preserving native argument/JSON faults.
`flaggo-contract-store` and `flaggo-executable-store` now supply both real SQLite
readers. Store integration tests exercise persisted checked-program evaluation
and native/integrity error routing; actual HTTP/OTel composition remains pending.

The cache keys the executable digest and ordered checked-rule identities,
materializes each entry once across concurrent callers, and bounds successful cache population to
1,024 entries with reference FIFO eviction. Cache hits never replace
the per-request active-executable read. Invalid checked representations are
evicted so repaired data can retry; other native faults preserve the reference's
faulted-cache behavior and distinct category.
Compiled executables expose only read-only references and own their normalized
documents, literal values and executable identity.

The evaluator uses first-match rule order. A rule is skipped if any attribute
referenced by either its predicate or expression return is absent, including
references in unreachable branches. Expression failures do not return the
default. Selected results and the no-match default are validated at `/result`.
Decisions retain both digests and `rule`/`default` provenance. Cancellation is
checked at each rule boundary, preserving the reference's empty-rule default
behavior and the expression engine's loop-free cancellation semantics.

`DecisionError::telemetry_classification` preserves the existing service's
outcome/category pairs; `EvaluationProvenance::source` preserves its source
dimension. This verifies classification, not actual spans, metrics, HTTP
status/envelopes or service identity. Those remain service-composition gates.

```powershell
cargo test --locked -p flaggo-decision
cargo clippy --locked -p flaggo-decision --all-targets -- -D warnings
python tools\core-parity\run.py --profile --report .flaggo\core-parity\cel-report.json
```

Focused tests cover activation replacement, validation/read ordering,
cross-contract rejection, immutable compiled state, missing-attribute skipping,
dynamic/default result validation, cancellation, failed-cache retry, checked
identity changes, concurrent materialization and the exact cache bound.
The differential harness also materializes complete executable artifacts in all
four .NET/Rust producer/consumer combinations and rejects corrupted artifacts.
No .NET fallback, persisted-state reset or service cutover is introduced.
