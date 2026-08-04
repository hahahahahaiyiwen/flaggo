# Shared Contract Packages

This boundary contains language-specific data contracts generated or verified
from the language-neutral artifacts in `contracts/`.

It contains DTOs and value types, not service interfaces or infrastructure
implementations. Generated output must be reproducible and must pass fixture
conformance before publication.

Update this document when generation, compatibility, or package-versioning
rules change.

## Current implementation

`src/Flaggo.Shared.Contracts` contains strict .NET runtime request, response,
Problem Details, and exposure confirmation DTOs. Unknown JSON members are
rejected, absent optional members are omitted, and required nullable contract
members remain present on the wire. It also provides the shared evidence
snapshot and RFC 8785 canonical JSON implementation used to compute bundle and
semantic contract digests. Canonical behavior is regression-tested against
every frozen canonicalization and semantic-digest vector.
