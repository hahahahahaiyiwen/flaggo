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
every frozen canonicalization and semantic-digest vector. Numbers that would
change mathematical value when converted to the RFC 8785 IEEE-754 domain are
rejected, preventing distinct inputs from collapsing to one digest.

The package also owns the strict JSON structural validator shared by HTTP and
local persistence adapters. It requires exactly one complete root value,
permits only JSON whitespace after that value, rejects malformed trailing data, and
case-sensitive duplicate member names independently within every object before
typed deserialization.

The frozen v1 cooldown contract remains any finite nonnegative number.
Overflow safety belongs to elapsed-time policy evaluation rather than a new
shared contract ceiling or persisted-state timestamp bound.
