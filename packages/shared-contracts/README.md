# Shared Contract Packages

This boundary contains language-specific data contracts generated or verified
from the language-neutral artifacts in `contracts/`, plus narrowly scoped
byte/JSON safety primitives shared by persistence adapters.

It contains DTOs, value types, and deterministic safety utilities, not service
interfaces or application-specific storage implementations. Generated output
must be reproducible and must pass fixture conformance before publication.

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

Runtime inputs are primitive maps, not producer-reference arrays.
`ApplicationScope` carries verified tenant/application/environment identity;
telemetry attributes and client JSON cannot set the tenant. `InputProvenance`
preserves source ownership, exact binding/generation, nanosecond observation
time as a decimal string, observed-only coverage, and available correlation.
Manifest v2 hashes `{ key, contract: normalizedDefinition }`; optional empty
maps and context requiredness normalize consistently in TypeScript, .NET, and
Python. The manifest has no signal declarations, producer digest, or duplicate
default. Native OTLP generated wire types belong to the host adapter, not this
package.

The package also owns the strict JSON structural validator shared by HTTP and
local persistence adapters. It requires exactly one complete root value,
permits only JSON whitespace after that value, rejects malformed trailing data, and
case-sensitive duplicate member names independently within every object before
typed deserialization.

Local persistence adapters share an enforceable cooperative commit protocol.
A direct commit descriptor uses strict JSON with exact
`flaggo.committed-artifact` format/version, a safe immutable sibling filename,
byte length, and a whole-string lowercase sha256 digest. A generation manifest
uses `flaggo.committed-generation` and pins every required artifact with the
same length/digest tuple. Paths are constrained to the descriptor sibling or
the exact immutable generation directory; traversal, sibling redirects,
symbolic links, and reparse points fail closed. Linux readers traverse from a
directory handle with `openat` plus `O_NOFOLLOW` and validate opened inode
types. Windows readers open every component with
handle-relative `NtCreateFile`, reject reparse attributes, and validate every
opened handle's exact `GetFinalPathNameByHandle` result plus volume/file
identity against the pinned volume-root path. All ancestor handles remain
alive until descriptor or artifact reading completes, preventing path-entry
replacement where Windows sharing semantics protect it. Descriptor and
artifact bytes are read from those same validated handles. Each native handle
transfers into the pinned set only after successful capture; rejected roots or
components are disposed immediately so repeated fail-closed traversal remains
handle-bounded. Platforms without Linux `openat`/`O_NOFOLLOW` or the Windows
handle-relative implementation fail closed rather than using a pathname
check/open/check sequence that could follow a raced link.

Readers load the commit document, read the referenced artifact with a 16 MiB
default bound and permissive sharing, re-read the commit document, then verify
exact length and sha256 before strict JSON and typed adapter validation.
In-place writes cannot pass unless the exact bytes still match the committed
digest. A commit switch during the read receives at most two bounded retries;
a stable read performs one artifact read with no timer delay. Stable corruption
is never retried or accepted.

`CommittedFileSnapshotWriter` is the reusable trusted-writer entry point for
direct descriptors. It writes and durably flushes a new immutable artifact,
synchronizes the artifact parent directory, writes and flushes a descriptor
temporary file, atomically renames the descriptor last, and synchronizes the
directory again. Old artifacts remain valid for readers that opened the
previous descriptor and can be reclaimed only by separate retention tooling.
If the descriptor directory does not exist, the writer finds the nearest
existing ancestor and creates each missing component downward. After each
`mkdir`, it synchronizes that component's immediate parent before synchronizing
the new directory and using it. Before creating descendants, it also
synchronizes the nearest existing ancestor's immediate parent and then the
ancestor, covering an intermediate directory made visible by a concurrent
creator paused before its parent barrier. Every successful ensure also
synchronizes the requested directory's immediate parent and the requested
directory before returning. Filesystem and volume roots safely synchronize
only the root itself. Concurrent creators converge, and a failed creation or
final boundary barrier cannot publish the descriptor.
Bootstrap tooling uses the generation manifest as the single commit authority
rather than adding redundant sidecars.
Reader and writer share one exact artifact-filename rule: 1-128 ASCII
characters, an alphanumeric first character, and only alphanumeric, dot,
underscore, or dash thereafter. The writer validates the complete generated
filename, including UUID and extension, before creating or publishing files;
invalid stems, separators, Unicode, and excessive final names leave no
descriptor, artifact, or staging file.

The package also owns `DurableDirectory`, the native cross-platform directory
metadata synchronization helper shared by committed snapshots and audit
persistence. Its durable create operation stops at the nearest pre-existing
ancestor rather than walking to and unnecessarily synchronizing the filesystem
root while still applying the final requested parent-and-directory boundary
barrier on every call. Native unsupported-directory-sync classifications
remain unchanged on Windows and Unix.
The injectable `IDurableDirectoryOperations` boundary lets persistence modules
reuse the parent-chain algorithm while testing barrier order and failure.

The frozen v1 cooldown contract remains any finite nonnegative number.
Overflow safety belongs to elapsed-time policy evaluation rather than a new
shared contract ceiling or persisted-state timestamp bound.
