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

Verify from the repository root:

```powershell
cargo test --locked -p flaggo-service-hosting
```
