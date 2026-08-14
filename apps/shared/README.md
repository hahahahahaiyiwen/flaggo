# Application Hosting

Owns strict HTTP contract parsing, Problem Details rendering, correlation
identity, scope helpers, and canonical request fingerprinting shared by the
control-plane and data-plane composition roots.
Strict digest checks use absolute whole-string matching and reject encoded
leading or trailing whitespace and control characters.

This project contains hosting infrastructure only. It does not own endpoints,
business behavior, authentication policy, or module ports. Each executable host
defines its own routes and operation-specific authorization scopes.
It owns the shared parsing and principal construction for the Development-only
resource identity used by both hosts. Each executable host still supplies its
own operation scope when constructing the principal.
`Flaggo:Authentication:LocalDevelopmentAppId` and
`Flaggo:Authentication:LocalDevelopmentEnvironment` default to the existing
`tetris-demo`/`dev` resource and must remain non-empty when overridden.

Update this document when cross-host HTTP invariants change.
