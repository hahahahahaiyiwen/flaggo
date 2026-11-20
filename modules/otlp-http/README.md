# Portable OTLP HTTP receiver

`flaggo-otlp-http` owns bounded JSON/protobuf decoding, gzip decompression,
whole-batch authorization/admission and append-before-acknowledgement responses.
It depends on inbox/security ports, not SQLx or a runnable application.

`router_with_private_health(inbox: Arc<dyn RawOtlpInbox>,
receiver: OtlpReceiverConfig, security: Arc<dyn RequestSecurity>) -> Router`
composes the existing receiver without requiring, defaulting or reporting a
retention policy. Readiness still calls `RawOtlpInbox::inspect`; its response
contains only service, status and version, even if the receiver's local detail
flag is enabled. Dependency failure remains 503 `not_ready`. Inbox metrics
remain available to the application's configured observability pipeline.

Existing `router` and `instrumented_router` retain `RawOtlpInboxLimits` and
explicit `with_local_health_details()` behavior for local composition.
Retention scheduling and limits remain adapter/application responsibilities.

After authentication, transport validation, bounded decompression and validation
of **every** Resource scope, the receiver authorizes all scopes. It then validates
the producer envelope and invokes `RequestSecurity::admit_authorized_otlp`
exactly once with the principal and complete distinct-scope slice before one
atomic inbox append. Authorization must never charge quota. No resource splitting
or single-resource `admit_authorized` call is introduced.

Metered adapters atomically charge **one unit per distinct tenant**: multiple
applications/environments in one tenant count once, and insufficient quota for
any tenant means no tenant is charged. Empty exports invoke the callback with
an empty slice, charge no tenant and preserve the existing empty-batch append;
there is no inferred caller tenant. Acceptance is not refunded on append failure
or cancellation; a retry is a new attempt. The default callback accepts without
metering, preserving legacy implementers and `TrustedLocal`.

Admission `Limited { retry_after_seconds: NonZeroU32 }` returns OTLP
`google.rpc.Status` code 8 with HTTP 429 and the exact positive `Retry-After`.
`RequestAdmissionError::Unavailable` returns sanitized code 14 / HTTP 503 with
no invented retry delay. JSON and protobuf retain their wire format/content type.
Neither outcome appends; validation/authentication/authorization denials do not
invoke admission. Existing inbox capacity/unavailability responses are unchanged.

The public seam does not implement a distributed ledger or certify cancellation
safety of a private adapter. Empty batches still consume rows/metadata:
**payload-byte limits alone are insufficient**. Hosted enablement requires
separate row/metadata capacity protections, including for empty exports.
No numeric production budget or retention policy is selected here.

```powershell
cargo test --locked -p flaggo-otlp-http
```
