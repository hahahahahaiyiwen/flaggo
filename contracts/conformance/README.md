# Contract Conformance

`validate.py` performs the offline executable-contract gate:

- validates all three Draft 2020-12 schemas;
- parses both OpenAPI 3.1 documents and resolves local references;
- requires every fixture to be indexed exactly once;
- checks fixture methods, paths, statuses, and secured-operation coverage;
- validates positive request/response bodies and rejects negative samples;
- rejects duplicate object keys, non-finite JSON numbers, and malformed
  RFC 3339 values;
- verifies RFC 8785 canonicalization vectors;
- requires correlation headers on HTTP fixtures; and
- verifies consistent contract and executable identities in v3 fixtures.

Install and run:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

`fixture-manifest-v1.json` is the stable case index. Implementations may
generate language-specific models from the OpenAPI documents and schemas, but
generated output is not committed until an implementation track selects its
generator.

The .NET host suites dispatch the same management and runtime fixtures through
real Contract Service and Decision Service hosts. The TypeScript SDK additionally
checks the exact operations, scopes, headers, and schemas it consumes.
