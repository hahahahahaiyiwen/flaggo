# Shared contract application

`flaggo-contract-application` owns the existing **v3** management HTTP routes,
validation, default/authored compilation, Candidate admission policy, management
projections, activation worker and observability. It depends on public domain
types and `ContractAuthority`, not SQLx, SQLite, host paths or runnable apps.

Construct `ContractLifecycle::new(Arc<dyn ContractAuthority>)` and pass it to
`router` and `worker::ActivationWorker`. One adapter owns the whole authority
transaction domain. `publish_ready` commits name ownership, immutable acceptance,
bootstrap activation and current selection together; `admit_current` checks
currentness with immutable admission. A process mutex is not part of correctness.
Validation and compilation happen before these semantic commands, not inside
provider-specific copies of service rules.

The local app composes `SqliteContractAuthority`. It preserves global v3 name
ownership, existing digests, acceptance timestamps and v3 retry/error ordering.
No scoped API, authentication boundary or hosted qualification is implied.
See the [extraction record](../../docs/design/architecture/CORE_EXTRACTION.md).
