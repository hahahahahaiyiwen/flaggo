# Contract domain

The existing .NET implementation remains the production authority during issue
#82. The parallel `flaggo-contract` Rust crate owns typed contract, schema,
learning/evidence, authored/executable-rule and runtime-input models, strict JSON
reading, normalized SHA-256 identities and contract/schema/value/runtime-input/
executable validation. It does not replace either HTTP service.

Strict reading rejects unknown/case-mismatched model members, duplicate decoded
property names, comments and trailing JSON. Limits match .NET: 262144 document
bytes, 16 container levels and 16384 decoded UTF-8 bytes per string/property.
Invalid Unicode is a distinct error because the current .NET reader raises a
native fault rather than `JsonException`. Missing constructor fields, explicit
nulls, nullable collection entries and concrete executable `when` shapes retain
the reference distinctions.

Validation preserves issue codes, severities, paths, messages and ordering.
Schema dictionaries, correlation maps, runtime attributes and literal JSON
objects retain source insertion order without enabling `serde_json`'s global
ordered-map feature in other services. `JsonValue` owns ordered literal data.
Literal parsing retains numeric-token distinctions: integer `-0` normalizes to
integer zero, while `-0.0` and `-0e0` remain floating-point values and do not
satisfy an integer schema.
Invalid model states that fault in .NET (such as a null executable rule or a
null required-property entry during value validation) return an explicit
`ContractError::InvalidModel`, not an ordinary validation issue.

Expression checking is supplied by `ContractExpressionValidator`, keeping typed
predicate/result validation distinct from guardrail syntax validation.
Without an adapter, validation intentionally matches the .NET validator's
no-expression-validator overload. This is not full service admission.
Reference quirks, including final-LF identifier matching, UTF-16 duration digit
classification and inapplicable string keywords on collection schemas, are
preserved rather than silently corrected.

Identity normalization removes descriptions only at model metadata locations,
preserves literal/default values and rule/ordinary-array order, sorts named
collections and set-like strings with UTF-16 ordinal comparison, then uses
RFC 8785 canonical JSON and SHA-256. Expression canonicalization is injected
through `ExpressionCanonicalizer`; the Rust production compiler adapter is not
yet implemented. Omitting the adapter intentionally matches the existing
digest API's no-canonicalizer overload, not full service admission.
Executable digests preserve null rule entries and canonicalize only non-null
rules, matching the reference hashing API. Hashing does not validate an
executable; null rules still fault during executable validation.

Run `cargo test --locked -p flaggo-contract` and
`cargo clippy --locked -p flaggo-contract --all-targets -- -D warnings`.
The differential harness in `tools\core-parity` also calls this real crate
against the unchanged .NET models and digest functions.

Remaining compatibility gates include production expression-adapter integration,
full compiler-backed validation and identity conformance,
`JsonElement` numbers beyond finite double range, and native
exception/HTTP behavior for missing or out-of-order polymorphic discriminators.
The Rust models reject those malformed discriminators, but do not yet expose
the equivalent legacy native-fault category. Do not cut services over based on
library baseline parity alone.
