# Canonical evidence values v1

Status: scalar schema, public Rust codec and explicit local query-profile
integration implemented for #94. The maintainer approved
**"Approve tagged canonical cells (Recommended)"**. New query results use these
cells. Existing saved artifacts are preserved; they are not silently rewritten.

The scalar format is `flaggo.evidence.value/v1`, defined by
[`evidence-value-v1.schema.json`](../../../contracts/schemas/evidence-value-v1.schema.json).
`flaggo_evidence_store::EvidenceValue` implements its encoding and decoding
without requiring SQLite. Rust and JavaScript schema tests share
[`evidence-value-v1.json`](../../../contracts/fixtures/evidence-value-v1.json).
The new type is not an implicit conversion or fallback for legacy results.

## Wire representation

| Kind | Shape | Semantics |
| --- | --- | --- |
| Null | `{"kind":"null"}` | SQL/typed null, not the string `"null"` |
| Signed integer | `{"kind":"int64","value":"9007199254740993"}` | Canonical signed 64-bit decimal text; no sign prefix `+`, leading zeros or negative zero |
| Double | `{"kind":"float64","bits":"3ff0000000000000"}` | Exact binary64 bits, sixteen lowercase hexadecimal digits, most significant digit first |
| Text | `{"kind":"text","value":"123"}` | Text remains text even when it resembles a number |
| Boolean | `{"kind":"boolean","value":true}` | Distinct from integer `1` |
| JSON | `{"kind":"json","value":"{\"n\":9007199254740993}"}` | Exact JSON text in a string, never implicitly parsed into JavaScript numbers |

Unknown kinds, extra fields, invalid integer ranges and malformed double bits
are errors. The float representation preserves signed zero, infinities and
NaN payloads, not just finite numeric equality. This does not imply that every
query provider supports non-finite arithmetic or can recover bits it already
discarded.

JSON text must be syntactically valid. The Rust codec validates syntax both
when encoding and decoding, without converting embedded numeric lexemes.
JSON Schema's `contentMediaType` is only an annotation: a shape-only validator
is insufficient to validate JSON text. A consumer that interprets the text
must choose a lossless parser for its own data needs; blindly using
`JSON.parse` can still lose numeric precision. Whitespace, member ordering and
numeric spelling inside the text are preserved, not semantically normalized.

Column metadata is separate from cells. An unknown/variant expression type,
including an empty result's column type, does not justify guessing a canonical
kind from the first row. No generic "unknown" cell or stringification fallback
is provided. Binary cells are not supported by this revision.

## Query profile and saved artifacts

The local adapter advertises `flaggo.evidence.sqlite/v1`, the result format
`flaggo.evidence.query-result/v1` and this scalar format. Requests must select
the advertised profile explicitly. Unsupported profiles fail; Async Analysis
also rejects incompatible result/value formats and mismatched response profiles.
There is no arbitrary-SQL translation or legacy fallback.

[`evidence-query-result-v1.schema.json`](../../../contracts/schemas/evidence-query-result-v1.schema.json)
defines saved results: query identity, normalized SQL, explicit authorized
authority/source/exact-contract scope, configured bounds, start/completion times,
live-read consistency, ordered column metadata, cells and truncation. Returned
metadata and the persisted query-history entry carry the SHA-256 digest of the
exact saved bytes. The file does not recursively contain its own digest.
Failed-query history records the same profile/scope/limits and failure.

`providerType` is diagnostic provider metadata. `kind` is canonical, and
`variant` plus unknown (`null`) nullability remain representable for empty
expression projections. Neither is inferred from sample rows. Row maps require
unique output names; duplicate aliases fail before execution.

`maxRowBytes` bounds the sum of compact serialized row-object bytes (not commas,
array framing or artifact metadata). Row excess truncates with one-row
lookahead; byte excess and deadline expiry fail. `resultBytes` reports the
entire saved file size. Local deadlines interrupt SQLite work and allow pooled
connections to be reused. These local configurable bounds are not hosted
capacity policy.

`projectedObservationIds` contains only digest-shaped text values explicitly
projected in the `observation_id` output field. It is not proof of identity,
existence or complete aggregate lineage. Saved results are reproducibility
artifacts, not promises of future replay against the same live dataset.

The SQLite profile deliberately retains SQLite arithmetic, overflow, NULL,
collation and JSON extraction semantics, not cross-dialect parity. The
`observed_at_unix_nano` relation column is now unsigned decimal **text**, avoiding
the old saturating int64 cast for valid OTLP u64 timestamps. Order numerically
by `length(observed_at_unix_nano), observed_at_unix_nano`; cast only after bounding
to int64. SQLite booleans remain int64 query values, and JSON objects/arrays
remain text rather than guessed JSON cells. Binary output is rejected. Original
OTLP non-finite double bits remain available in `payload_json`; the adapter
does not claim to recover NaNs SQLite itself has converted to NULL.

Local `code_interpreter` exposes the shared `flaggo_evidence` decoder module;
`load_rows(path)` rejects unversioned results and preserves Python integers,
binary64 bits and JSON decimals. The source ships in the public evidence-store
crate for other execution adapters. Agent guidance and deterministic Tetris
consumers use this explicit boundary. No schema/database migration is needed
for prior immutable files; external consumers must select the new format and
must not reinterpret old files as this revision.

This query cutover does not change management/runtime HTTP versions or
contract/executable digests. Cross-adapter qualification still requires the
shared C11 outcomes, not simply advertising this format.
