# Contract Conformance

`validate.py` performs the offline Phase 1 contract gate:

- validates all four Draft 2020-12 schemas,
- parses both OpenAPI 3.1 documents and resolves every local reference,
- requires each fixture to be indexed exactly once,
- requires fixture-declared coverage of all 43 proposal scenarios,
- checks fixture methods, paths, and statuses against OpenAPI operations,
- requires an insufficient-scope case for every secured operation,
- validates positive fixture bodies against their declared schemas,
- confirms every schema-negative sample and result-mode mutation is rejected,
- rejects non-finite JSON numbers and malformed RFC 3339 values,
- correlates validation issues with the offending request content,
- verifies RFC 8785 canonical bundle and snapshot bytes against numeric and
  Unicode vectors, including `ETag` and `Content-Digest` values,
- executes concurrent startup convergence plus typed invalid-bundle and
  approval-pending SDK startup rejection through an in-memory registration
  harness.

Install and run:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

`fixture-manifest-v1.json` is the stable case index consumed by conformance
tests. Implementations may generate language-specific models from the OpenAPI
and schemas, but generated output is not committed until an implementation
track selects its generator.
