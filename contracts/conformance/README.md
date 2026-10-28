# Contract Conformance

`validate.py` performs the offline executable-contract gate:

- validates all seven Draft 2020-12 schemas;
- parses both OpenAPI 3.1 documents and resolves local references;
- requires every fixture to be indexed exactly once;
- checks fixture methods, paths, statuses, and secured-operation coverage;
- validates positive request/response bodies and rejects negative samples;
- validates Flaggo event JSON, result hashes, and evaluation provenance;
- validates the OpenTelemetry Schema File, Flaggo OTLP/HTTP profile, and
  Flaggo service-observability profile;
- verifies service Resources, scopes, signal references, bounded metric
  dimensions, contract correlation, export isolation, redaction, and health;
- verifies the analysis-action start/terminal contract, bounded kinds/outcomes,
  exact correlation and exclusion of action metadata from metric dimensions;
- checks OTLP JSON and gzip-compressed Protobuf fixtures against the profile;
- rejects duplicate object keys, non-finite JSON numbers, and malformed
  RFC 3339 values;
- verifies RFC 8785 canonicalization vectors;
- requires correlation headers on HTTP fixtures; and
- verifies consistent contract and executable identities in v3 fixtures.

Install and run:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python -m unittest discover -s contracts\conformance -p "test_*.py"
python contracts\conformance\validate.py
```

Canonicalization vectors default to `numberModel: "native"`, retaining the
strict JSON loader's numeric types and the Python canonicalizer's safe-integer
checks. The `ieee754-number-boundaries` vector explicitly selects
`numberModel: "binary64"`:
only that vector's input is interpreted as IEEE-754 binary64 before exact
expected-byte comparison. Its original integer tokens remain in the shared
file for the Rust/.NET parity probes. This opt-in does not change fixture
parsing, contract identity calculation, or the global canonicalization helper.
Unknown number models and non-finite values are rejected.

`fixture-manifest-v1.json` is the stable case index. Implementations may
generate language-specific models from the OpenAPI documents and schemas, but
generated output is not committed until an implementation track selects its
generator.

The OTLP checks validate executable contract artifacts and fixture envelopes.
They do not yet dispatch every profile fixture through a real ingestion host.

The .NET host suites dispatch the same management and runtime fixtures through
real Contract Service and Decision Service hosts. The TypeScript SDK additionally
checks the exact operations, scopes, headers, and schemas it consumes.
