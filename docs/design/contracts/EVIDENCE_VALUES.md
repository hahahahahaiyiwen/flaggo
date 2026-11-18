# Canonical evidence values v1

Status: scalar schema and public Rust codec implemented for #94. The maintainer
approved **"Approve tagged canonical cells (Recommended)"**. This is not yet a
negotiated query-result profile: current SQLite query results and saved analysis
artifacts still use their existing raw JSON representation.

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

## Remaining integration gate

Before switching local query output, define and implement the containing
versioned capability/result envelope, explicit profile selection, canonical
column metadata, scope/source/read metadata, limits, saved-artifact identity
and qualified projected-ID provenance. Update SQLite conversion, Python/agent
consumers, deterministic examples and required C11 fixtures together. Reject
unsupported provider values; never relabel provider type strings or silently
coerce legacy results into this format.

These cells do not change HTTP v3, contract/executable digests, database schema
versions or existing saved artifacts. They are not a C11 portability claim.
