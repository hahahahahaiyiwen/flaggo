# Shared contract application

`flaggo-contract-application` owns the scoped **v4** management HTTP routes,
validation, default/authored compilation, Candidate admission policy, management
projections, activation worker and observability. It depends on public domain
types and `ContractAuthority`, not SQLx, SQLite, host paths or runnable apps.

Construct `ContractLifecycle::new(Arc<dyn ContractAuthority>)` and pass it to
`router` (with an independent `RequestSecurity`) and `worker::ActivationWorker`.
One adapter owns the whole authority transaction domain.
The shared [authorized request-admission hook](../hosting/README.md#authorized-request-admission)
runs before handler validation or lookup, including HEAD and conditional catalog
reads. It is separate from Candidate admission: metered adapters charge accepted
authorized attempts even when the handler later returns an error. Existing
security adapters remain non-metered by default; health is excluded.
`publish_ready` commits full-resource ownership, immutable acceptance,
bootstrap activation and current selection together; `admit_current` checks
currentness and current workspace/cycle ownership with immutable admission.
A process mutex or preflight-only ownership check is insufficient.
Validation and compilation happen before these semantic commands, not inside
provider-specific copies of service rules.

The local app composes `SqliteContractAuthority` with the local workspace's
admission fence. V4 deliberately replaces global v3 lookup with full scope.
Explicit migrations preserve existing digests, timestamps and sealed provenance.
Local `TrustedLocal` authentication is not hosted identity qualification.
See the [extraction record](../../docs/design/architecture/CORE_EXTRACTION.md).
