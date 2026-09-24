# Decision API design

## Purpose and boundaries

The Decision API evaluates an exact accepted definition against runtime data
and existing governed state. It owns bounded orchestration, not instrumentation,
contract publication, authority generation, or an online AI loop.

The [API baseline](../API_CONTRACT_PROPOSAL.md), [executable contracts](../../../contracts/README.md),
and [runtime architecture](../../architecture/RUNTIME_EXECUTION.md) define the
shared behavior. Initial-authority manifest publication/activation-readiness
remains #40; the final bundle-approved integration remains #41.

## Request flow

```text
authenticated tenant + authorized application/environment
  -> exact definitionId/revision/contractDigest
  -> validate caller context and request-owned input map
  -> resolve authorized targets
  -> resolve evidence-owned operands from one immutable input generation
  -> select compatible governed state
  -> active value / bounded numeric rule / governed fallback
  -> explicit policy (separate quality evidence only when required)
  -> durable audit and immutable pending exposure snapshot
  -> return server result
```

The host supplies verified `ApplicationScope`; tenant identity never comes
from the JSON body or OTel attributes. Registry read ports provide validated
projections instead of leaking manifest/persistence parsing into runtime.

## Wire contract

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

Decide supplies the exact expected identity, client application/environment,
optional target/context, and a primitive `inputs` object. For Tetris:

```json
{
  "boardPressure": 0.82,
  "recentPlacementTimeMs": 1420,
  "recoveryFailures": 2,
  "currentLevel": 3
}
```

Only request-owned inputs are submitted. There is no inline definition,
producer handle, source selector, snapshot ID or evidence override.
Duplicate JSON properties and old arrays fail strict parsing. Unknown,
missing, wrong-type, non-finite and out-of-range values fail with an explicit
input path rather than silently defaulting. Context fields and target bindings
receive equivalent validation.

The SDK's catalog supplies identity/type information; REST callers obey the
same schema. A key-only SDK call is valid only when the definition requires no
caller-owned operands or context.

## Input resolution

`DecisionInputResolver` owns the source boundary. Each declared operand is
required and has either `request` or `evidence` ownership. Evidence inputs
inherit their binding's scalar contract. A read pins one generation and one
evaluation time under the exact scope/definition/binding/target.

Missing, stale, future, ambiguous, invalid, or unavailable required evidence
returns 503 `required-evidence-unavailable` with
`clientFallback.eligible: false`. No last-good value is invented, no missing
measurement becomes zero, and evaluation never waits for Collector export or
queries raw telemetry.

A request-only decision without an evidence-dependent policy accesses neither
input materialization nor policy-quality evidence. Those are separate ports:
fresh observed telemetry is not evidence quality, model uncertainty, or an
expected-outcome estimate.

## Target and authority resolution

Manifest `targeting.primary` and explicit `fallbackOrder` determine the
ordered chain. Hierarchy authorizes target kinds; it does not insert implicit
levels. Required string target context must be present and consistent.
Client cohort claims are verified/replaced through the configured resolver;
unverified claims do not authorize evidence targets.

`IStateStore` receives only ordered exact targets and exact definition
identity. Well-formed incompatible state may be skipped; corrupt state is an
error. Existing fixed/numeric authority is consumed read-only. New authority,
CAS, activation and approval belong to their control-plane/state boundaries.

The current local fixtures use existing state adapters. They preserve exact
Tetris `850ms`/`750ms` branches and an audited `800ms` fallback without claiming
the future activation-ready receipt or complete #41 authority-audit shape.

## Numeric execution and policy

`INumericRuleExecutor` receives only `RuntimeDecisionDefinition`,
`NumericRuleStrategy` and resolved primitive inputs. Rule operands use
definition-local `inputKey`. It returns a candidate/reason or explicit failure;
it cannot query evidence or apply fallback. Existing normalized weighted
scoring is preserved. Every deterministic numeric result has null learned
confidence.

Orchestration resolves active values directly and applies output and runtime
policy after candidate selection. Invalid inputs are not policy fallback.
Conditional policy-quality evidence remains independently explicit. No
telemetry coverage flag changes numeric confidence or bypasses policy.

## Results and errors

Server success carries the exact registered identity, value/type, decision
mode, target provenance, policy result, fallback classification, reason,
decision/audit IDs, and explicit exposure confirmation state. Detailed raw
telemetry is never copied into the response.

| Result | Meaning |
| --- | --- |
| Approved server decision | Audited fixed or numeric authority result |
| Governed server fallback | Completed audited evaluation; distinct from availability |
| SDK-local fallback | Explicitly enabled eligible outage response; no server IDs, policy success, or exposure |
| Contract/readiness/integrity error | Actionable failure, not an older revision or default value |
| Required input evidence unavailable | Explicit operand/binding failure, never SDK fallback |

The default SDK receipt is intentionally smaller than its detailed result.
Numeric execution does not fabricate confidence from received observations.
The schema retains a nullable confidence contract for independently supported
evidence claims; an empty or inconsistent report is invalid.

## Retry, audit and confirmation

The [retry namespace and retention](../API_CONTRACT_PROPOSAL.md#correlation-and-retries)
cover the authenticated scope and canonical caller request. The fingerprint
does not include current input generation or tracing-only metadata. A retained
successful retry returns the original result and provenance even after new
telemetry arrives; a failed dependency evaluation can be retried after recovery.

Audit records preserve caller inputs separately from resolved values and
per-input source/binding/generation/source-time/correlation. Durable decision
audit must complete before success. The exposure snapshot copies those values;
confirmation cannot replace them.

Only applying/rendering a returned server value permits explicit confirmation
with its capability. Preparation reserves one exposure; durable exposure audit
precedes bounded post-audit commit. Request cancellation cannot silently undo
the audit-first transition. Retry preserves the exposure identity.
Capabilities are omitted from audit output.

`IConfirmedExposureReader` exposes only completed confirmations under exact
tenant/application/environment. A raw decision ID, pending receipt or merely
appended audit record cannot authorize attributed telemetry. New references to
lost in-memory confirmations after restart fail closed.

## Hosting and native telemetry

Production decide/confirm require OAuth/OIDC and operation-specific
`polari.decisions:decide` / `polari.exposures:confirm` scopes. The explicit
local-development bypass cannot run outside Development.

The same data-plane host composes separate native routes:

```text
/otlp/{appId}/{environment}/v1/metrics
/otlp/{appId}/{environment}/v1/traces
/otlp/{appId}/{environment}/v1/logs
```

These require `polari.telemetry:ingest`, binary Protobuf and bounded
uncompressed/gzip input. Native responses remain OTLP; the host adapter
decodes data and the evidence module owns projection/publication. Ordinary
runtime credentials do not grant ingest permission.

Health exposes coarse dependency state, not internal files, credentials or
exception data. Per-request evidence failure does not rewrite global readiness
or make live-input decisions depend on a Collector.

Management/approval endpoints remain in the separate control-plane host.
Public raw telemetry/audit queries, batch decisions, arbitrary query languages,
and request-time AI are not part of this API.
