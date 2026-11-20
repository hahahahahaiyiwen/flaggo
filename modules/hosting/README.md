# Service Hosting

Shared HTTP mechanics for the Rust core services. Superseded .NET hosting
sources have been removed; no runtime compatibility layer is retained.

The Rust crate owns bounded strict JSON request decoding, Problem Details,
fallback status pages, validated request-correlation IDs, W3C trace-parent
extraction, HTTP server spans and duration metrics. Applications own routes,
domain error mapping, health dependencies and domain instrumentation.

Route patterns supplied at composition preserve the reference's case-insensitive
literal matching and optional trailing slash. Captured resource identities are
never case-normalized. HTTP metrics use only the bounded method, status and
registered route template; contract names, digests, correlation IDs and query
strings never become dimensions.

Request bodies accept `application/json`, including parameters, and are limited
to exactly 262,144 bytes even without Content-Length. Only native JSON faults
become invalid-body problems. Invalid Unicode, missing polymorphic discriminators,
serialization and transport failures retain their distinct typed error paths for
the application mapper. The HTTP comparison currently asserts native parser
diagnostic categories and prefixes, not byte-identical diagnostic suffixes.

The process initializes `flaggo-service-observability` before composing a router.
Core startup rejects legacy connection-string/listen environment settings unless
their explicit Rust replacements are provided, avoiding an unintended default
database or port during cutover. It never interprets legacy values or logs them.
Both shared HTTP and domain instruments use the emitting application's registered
scope and package version. No new exporter or telemetry platform is introduced.

## Authorized request admission

The shared Contract and Decision HTTP routers call
`RequestSecurity::admit_authorized(&Principal, &ResourceScope, Option<&str>, Operation)`
exactly once after authentication and successful resource/action authorization,
before invoking the handler. The trait uses `async_trait`; `RequestAdmission`
and `RequestAdmissionError` are exported from `flaggo_service_hosting`.
The callback receives the same authenticated principal, decoded routed scope,
optional contract name (`None` for catalog), and operation as authorization.
Body declarations never select the charged resource.

The default returns `RequestAdmission::Accepted` without metering, preserving
existing implementations and explicit `TrustedLocal` composition. A metered
adapter charges **one unit per accepted authorized request attempt**, not per
successful response or logical SDK operation. Later validation errors, missing
resources, conflicts, dependency failures and cancellation do not refund a
charge. Retrying, including a retry with identical input, is a new attempt.
HEAD and conditional catalog requests are attempts even if the handler would
return an empty body or 304.

| Callback result | Shared HTTP result |
| --- | --- |
| `Accepted` | Continue to the existing handler, retaining its success/error behavior |
| `Limited { retry_after_seconds: NonZeroU32 }` | 429 `request-quota-exceeded`, with positive delay-seconds `Retry-After` |
| `Err(RequestAdmissionError::Unavailable)` | Sanitized 503 `request-admission-unavailable`, without an invented `Retry-After` |

Both rejection outcomes stop before the handler and use the existing RFC 9457
`Problem` response and correlation header. Quota exhaustion can therefore take
precedence over a handler's 400/404/409/415/422. Authentication failures (401 with
challenge), concealed resources (404), forbidden operations (403), and identity
or authorization authority failures (503) never invoke admission.

This is **not** a promise that quota precedes all request failures. Route matching
happens first. After authentication, the middleware decodes all route captures,
validates the scope and resolves an operation before authorization/admission.
Unmatched paths, non-UTF-8 captures (including a digest), invalid scope identifiers
and unrecognized operations never invoke admission. Handler-level digest, name,
query, media-type and body validation comes later. Existing method inference is
unchanged: for example DELETE on a management resource fails operation resolution
with 400, whereas PUT on the catalog resolves `DeployContract`, is authorized and
admitted, then returns 405 if accepted (or 429 if limited).

Health routes bypass identity, authorization and admission. Hosted provisioning
and usage endpoints must remain outside `with_resource_security`; this hook is
not global HTTP middleware. OTLP's separate whole-batch authorization path is
not routed through this single-resource hook.

## OTLP batch admission

`RequestSecurity::admit_authorized_otlp(&Principal, &[ResourceScope])` uses the
same async trait and `Result<RequestAdmission, RequestAdmissionError>` exports.
The [OTLP receiver](../otlp-http/README.md) calls it once **after** complete
transport/scope validation and successful authorization of every scope, before
appending the unsplit batch. The slice contains distinct scopes, not a single
representative scope per tenant. Authorization is non-metering.

A metered adapter atomically charges one unit per **distinct tenant**, all or
none if any tenant lacks quota. An empty slice charges nobody and does not infer
a tenant from the principal; existing empty exports are still accepted/appended.
The default accepts without metering. Append failures/cancellation do not refund;
retries are new attempts. The adapter owns atomicity and cancellation safety.

OTLP keeps its own `google.rpc.Status` wire responses: Limited maps to 429/code 8
with the supplied positive `Retry-After`; Unavailable maps to sanitized 503/code
14 without an invented retry delay. Neither appends. This is separate from the
single-resource RFC 9457 mapping above. Empty-batch row/metadata capacity must be
bounded before hosted enablement; payload-byte quota alone is not sufficient.

Adapters own atomic budget decisions and cancellation-safe accounting. Dropping
the request or admission future does not imply a pending commit rolled back.
There is deliberately no refund/settlement callback, persistence dependency,
numeric budget or production quota policy in this seam. Public HTTP qualification
does not establish distributed accounting guarantees for an adapter.

Verify from the repository root:

```powershell
cargo test --locked -p flaggo-service-hosting
```
