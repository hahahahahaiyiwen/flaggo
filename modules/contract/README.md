# Contract domain

The `flaggo-contract` Rust crate owns typed contract, schema,
learning/evidence, authored/executable-rule, Candidate submission/provenance,
activation request, runtime-input and runtime-decision models, strict JSON
reading and writing, normalized SHA-256 identities and contract/schema/value/runtime-input/
executable validation. Both Rust HTTP services compose this module.

Runtime decisions preserve contract/executable digests, literal result data and
strict `rule`/`default` evaluation provenance. Provenance rejects unknown fields,
unknown sources and missing/empty rule names while accepting either member
order, matching the reference converter.

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
Literal numbers use a local `JsonNumber` representation that preserves raw JSON
lexemes, including valid JSON numbers outside binary64's finite range.
`strict_json::serialize` writes those tokens losslessly. Numeric access retains
the reference distinction: integer `-0` is integer zero, while `-0.0` and
`-0e0` are floating-point and do not satisfy an integer schema. An overflowing
raw number is parsed successfully, then fails finite-number value validation
with the normal issue rather than becoming a JSON deserialization error.
No global `serde_json` arbitrary-precision feature is enabled.

Typed optional schema doubles also accept binary64 overflow during reading,
matching the reference; serializing a non-finite typed double instead returns
`ContractError::Serialization`. Use the owned `strict_json::serialize` and
`strict_json::to_value` seams to preserve serialization-fault versus JSON-error
categories. The latter explicitly converts serialized JSON into finite
canonicalization input; out-of-range raw numbers fail there as JSON errors.
Digest and unique-item canonicalization use that same conversion seam.

Missing or late evidence-source discriminators return the distinct
`ContractError::MissingTypeDiscriminator` native fault. Unknown/invalid
discriminator values and unsupported reserved metadata remain JSON errors.
Scoped deserializer tracking preserves first-failure order in nested models,
without matching error strings, introducing global state, or treating literal
objects containing a `source` member as polymorphic models. Global token
validation (including duplicate/Unicode checks) still precedes model decoding.
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
through `ExpressionCanonicalizer`, implemented by `flaggo-expression`.
Omitting the adapter intentionally matches the existing
digest API's no-canonicalizer overload, not full service admission.
Executable digests preserve null rule entries and canonicalize only non-null
rules, matching the reference hashing API. Hashing does not validate an
executable; null rules still fault during executable validation.

Run `cargo test --locked -p flaggo-contract` and
`cargo clippy --locked -p flaggo-contract --all-targets -- -D warnings`.
The differential harness in `tools\core-parity` also calls this real crate
against the pinned historical .NET models and digest functions through an
explicit external checkout; see [core parity](../../tools/core-parity/README.md).

The differential harness gates raw-number parsing/serialization, typed numeric
bounds, defaults, literal returns, input validation, unique-item conversion and
polymorphic discriminator/metadata failures, including native exception
families and failure ordering. These close the previously recorded library
number/discriminator gaps. Side-by-side Rust service tests now verify HTTP
status/envelope mapping against actual .NET hosts and exported OTLP behavior;
see the Contract and Decision Service READMEs for opt-in live gates and limits.
The owned `flaggo-decision`
runtime now validates admission inputs before resolving active executables.
`flaggo-contract-store` now supplies SQLite contract-version persistence and its
runtime reader. `flaggo-executable-store` supplies executable-v5 persistence,
v4 migration and lifecycle operations. Rust services are the default after
the HTTP/OTel and unchanged-client full-loop gates passed. No .NET sources,
runtime fallback or persisted-state reset are retained.
