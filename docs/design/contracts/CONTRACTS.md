# Decision contracts and executables

## Status

This document defines the two core primitives in the target Flaggo model:
`DecisionContract` and `DecisionExecutable`.

The Management API v3 schema projects the decision name as a versioned
`DecisionContract` resource and each `contractDigest` as one immutable accepted
version. It exposes dry-run validation, create-or-update by name, current
version lookup, and paginated version history. Runtime APIs that still use
definition IDs, server revisions, state generations, or other identities
describe current wire behavior until their issue-scoped migrations replace
them.

Lifecycle behavior belongs in [Decision contract lifecycle](LIFECYCLE.md).

## Purpose

A `DecisionContract` is one atomic contract-as-code unit for a named decision.
It owns the runtime interface and may include authored behavior and
evidence-based generation declarations. All user-authored semantic content is
covered by one `contractDigest`.

A `DecisionExecutable` is an immutable runtime artifact generated under one
exact contract:

```text
DecisionContract {
  attributes,
  result,
  authoredExecutable?,
  learning?
}
  -> Executable Generation
  -> DecisionExecutable

DecisionExecutable + RuntimeInput
  -> RuntimeDecision
```

The primitives remain distinct even when the contract contains an
`authoredExecutable`. That field is source material for executable generation;
it is not active runtime authority. Evidence-based generation may later
produce other executables without mutating the accepted contract.

## Decision Contract

A `DecisionContract` defines the complete user-authored declaration for one
logical decision. Its conceptual shape is:

```yaml
name: tetris.dropInterval
expression_syntax: flaggo.cel/v1

attributes:
  - name: user_id
    schema:
      type: string
      description: Application-provided user identifier.

  - name: user_group
    schema:
      type: string
      description: Application-provided user grouping.

  - name: session_id
    schema:
      type: string
      description: Current game session.

  - name: board_pressure_mean_5s
    schema:
      type: number
      minimum: 0
      maximum: 1
      description: Time-weighted mean board pressure over the trailing five seconds.

  - name: board_pressure_max_5s
    schema:
      type: number
      minimum: 0
      maximum: 1
      description: Maximum board pressure over the trailing five seconds.

  - name: current_level
    schema:
      type: integer
      minimum: 0
      description: Current game level.

  - name: placement_time_mean_ms_5s
    schema:
      type: number
      minimum: 0
      description: Mean placement time for pieces locked in the trailing five seconds.

  - name: recovery_failures_5s
    schema:
      type: integer
      minimum: 0
      description: Failed recovery attempts for pieces locked in the trailing five seconds.

  - name: pieces_locked_5s
    schema:
      type: integer
      minimum: 0
      description: Pieces locked in the trailing five seconds.

result:
  schema:
    type: number
    minimum: 100
    maximum: 1000
  default: 500

authoredExecutable:
  rules:
    - name: high-pressure
      description: Slow placement when the occupied board creates high risk.
      when:
        expression: attributes.board_pressure_mean_5s >= 0.75
      return:
        value: 250

    - name: recovery
      description: Give the player more time after a failed recovery.
      when:
        condition: A recent recovery attempt failed.
      return:
        value: 700

    - name: level-adjustment
      description: Increase difficulty for experienced play.
      when:
        expression: attributes.current_level >= 10
      return:
        expression: 1000.0 - double(attributes.current_level) * 25.0

learning:
  policy:
    mode: auto-activation
    evaluate:
      interval: PT15M

  evidence:
    - name: placement_time
      description: Placement latency observed after a runtime decision.
      attribute: placement_time_mean_ms_5s
      correlateBy:
        - session_id
      source:
        kind: metric
        scope: tetris.engine
        name: tetris.placement_time
        metricKind: histogram
        unit: ms
        correlation:
          session_id:
            location: signal
            attribute: tetris.session.id

    - name: recovery_failure
      description: Recovery failures observed after a runtime decision.
      attribute: recovery_failures_5s
      correlateBy:
        - session_id
      source:
        kind: metric
        scope: tetris.engine
        name: tetris.recovery_failure
        metricKind: sum
        unit: "{failure}"
        correlation:
          session_id:
            location: signal
            attribute: tetris.session.id

  objective:
    primary:
      evidence: placement_time
      direction: minimize

    guardrails:
      - name: no-recovery-failures
        expression: sum(evidence.recovery_failure) <= 0
```

This is a conceptual shape, not the final wire schema. Both
`authoredExecutable` and `learning` are optional.

The contract does not contain an active executable, runtime attribute values,
collected evidence, activation state, or lifecycle history.
Deployment metadata such as repository, commit, build, owner, and timestamps
also remains outside its semantic content.

### Attributes

`attributes` is a list of named attribute declarations. Each declaration
contains an explicitly labeled JSON Schema:

```yaml
attributes:
  - name: board_pressure_mean_5s
    schema:
      type: number
      minimum: 0
      maximum: 1
      description: Time-weighted mean board pressure over the trailing five seconds.
```

Attribute names are unique. List order is not semantic; canonicalization orders
declarations by name. Each schema uses the bounded
`flaggo.value-schema/v1` profile of JSON Schema 2020-12.

The initial profile supports exactly:

| Value kind | Supported keywords |
| --- | --- |
| `null` | `type`, `description` |
| `boolean` | `type`, `description` |
| `integer`, `number` | `type`, `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`, `description` |
| `string` | `type`, `minLength`, `maxLength`, `description` |
| `array` | `type`, `items`, `minItems`, `maxItems`, `uniqueItems`, `description` |
| `object` | `type`, `properties`, `required`, `additionalProperties`, `minProperties`, `maxProperties`, `description` |

Every schema selects exactly one value kind. Union types, `$ref`, `$defs`,
`enum`, `const`, combinators, conditionals, pattern matching, dependent or
unevaluated keywords, and custom vocabularies are rejected. Object schemas
must declare `properties` and set `additionalProperties: false`; array schemas
must declare one recursive `items` schema. `description` is the only accepted
annotation and is excluded from semantic digest input.

The profile applies these implementation limits before evaluation:

- signed 64-bit JSON integers and finite IEEE 754 JSON numbers;
- at most 16 nested array/object levels;
- at most 256 items or properties in one collection;
- at most 16,384 UTF-8 bytes in one string; and
- at most 262,144 UTF-8 bytes in the complete runtime request body.

A schema may impose tighter limits. It cannot increase these profile limits.

Attributes are ordinary decision data. A user ID, principal ID, session ID,
request ID, or similar value may influence runtime evaluation or learning, but
never authenticates a caller, authorizes access, or selects application or
environment scope.

Attribute declarations do not assign authority or source bindings. Users bind
application activity to contract attributes through the SDK:

```text
application request or activity
  -> user SDK binding
  -> RuntimeInput.attributes
```

The initial model permits callers to supply any schema-valid subset of
attributes. Every user-declared attribute is optional at runtime.

Runtime handles supplied and missing values differently:

- an attribute not declared by the contract is rejected;
- a supplied value that violates its schema is rejected;
- a missing value is permitted;
- a rule is eligible only when every attribute referenced by its predicate and
  result expression is present;
- an ineligible rule is skipped; and
- if no eligible rule matches, runtime returns `result.default`.

Invalid supplied values, invalid executables, and evaluation failures are not
no-match conditions and must not be disguised as a default result.

### Internal Attributes

Flaggo reserves attribute names beginning with `_`. Users cannot declare,
bind, or override them. The SDK adds internal attributes when it constructs the
complete `RuntimeInput`; the runtime server validates but does not generate or
persist them.

The initial internal attribute is:

| Attribute | Schema | Meaning |
| --- | --- | --- |
| `_random` | Number in `[0, 1)` | Uniform value generated once for a new runtime decision, available to probability-based rules. |

For example:

```yaml
when:
  expression: attributes._random < 0.1
```

`_random` is part of the complete `RuntimeInput`. Evaluation remains
deterministic for the same active executable and complete runtime input. The
SDK generates it once for one logical evaluation and reuses it for transport
retries rather than drawing a new value.

The accepted contract format/version pins the available internal attributes
and their semantics. Adding or changing an internal attribute requires a
versioned contract-semantics change rather than an unannounced runtime
extension.

### Result

`result.schema` is an explicitly labeled JSON Schema for every decision value.
`result.default` is a required literal sibling of the schema. Runtime validates
authored constants, generated executables, evaluated values, and the default
against `result.schema`.

`result.default` is a required literal value used only when no executable rule
matches the supplied attributes. It must satisfy `result.schema`.

A dynamic calculation is executable behavior rather than a contract default.
Users express a dynamic baseline as a rule expression; if that rule is not
eligible or does not match, the literal contract default remains available.

Every accepted contract has a valid default, so Flaggo can generate a bounded
constant executable:

```text
DecisionContract.result.default
  -> DefaultExecutable
  -> ActiveExecutable
```

Default executable generation and activation are automatic. Contract
deployment reports runtime readiness only after this activation succeeds.
Authored or evidence-based candidates may later replace it, but their absence,
validation failure, or failed activation never makes the contract unable to
return a runtime decision.

The initial model does not add a generic `constraints` collection. Type and
value constraints belong in the attribute and result schemas. Rule
predicates and calculations belong in the executable. Activation eligibility,
pause state, and activation controls belong to governance and lifecycle
boundaries. New contract constraints require a concrete invariant that cannot
be represented by these existing owners.

### Authored Executable

`authoredExecutable` is an optional, user-authored source representation for
the initial executable. Its inclusion in `DecisionContract` binds the exact
bootstrap behavior to the same `contractDigest` as the runtime interface and
optional learning declaration.

Top-level `expression_syntax` identifies the expression language for the whole
contract. It applies consistently to authored `when.expression`,
`return.expression`, and learning guardrails. The initial syntax is a
versioned Flaggo profile of the Common Expression Language:

```text
expression_syntax = flaggo.cel/v1
```

Rule names are unique within the authored executable. `description` explains
the rule for people but is not semantic digest input.

Rule array order is the priority mechanism. The first listed eligible rule has
the highest precedence, so reordering rules changes contract and executable
identity. The contract does not add a separate `priority` property or define
tie-breaking rules for duplicate priorities.

`when` is an object with exactly one of two possible representations:

| Field | Meaning |
| --- | --- |
| `condition` | Natural-language condition used as generation intent. |
| `expression` | Deterministic CEL condition that can be compiled directly. |

`condition` and `expression` cannot coexist in one rule. A rule with neither is
invalid.

Rules with a deterministic expression are ordered. The first eligible rule
whose expression evaluates to `true` returns its literal value or result
expression. If no rule matches, runtime returns `result.default`.

A natural-language-only rule cannot enter runtime evaluation directly.
Executable Generation must turn it into deterministic candidate behavior,
record the generation provenance, and pass candidate validation before
activation. Runtime never evaluates natural language.

The profile uses CEL syntax and core semantics while restricting the available
types, functions, extensions, and evaluation cost. Contract Acceptance parses
and type-checks expressions against the attribute and result schemas, rejects
unsupported CEL features, and compiles accepted expressions before runtime.
Runtime never parses or type-checks expression source.

`flaggo.cel/v1` supports literals; `attributes.<name>` selection; grouping;
arithmetic, comparison, equality, boolean, and conditional operators; and the
standard `bool`, `double`, `int`, `string`, and `size` conversions/functions.
It rejects comprehensions and macros, regex, timestamps, durations, optionals,
reflection, dynamic function registration, and access outside the
`attributes` root. Predicates must check as `bool`; return expressions must
check against `result.schema`.

An expression is limited to 4,096 UTF-8 bytes, 32 AST levels, 256 AST nodes,
and a static cost of 1,000. Runtime evaluation has a cost budget of 10,000.
Compilation produces canonical expression text, a checked representation, and
the exact set of referenced attributes. Canonical expression text participates
in executable identity. The checked protobuf is derived storage data protected
by its own checksum; it does not independently change `executableDigest`.
Runtime receives only validated checked expressions, and a process restart may
materialize program objects from that stored representation without reparsing
or type-checking source.

Missing-attribute handling is Flaggo rule eligibility behavior, not CEL null or
unknown propagation. The initial profile does not allow expressions to branch
on attribute absence.

### Learning

`learning` is an optional declaration that enables evidence-based executable
generation for this contract digest. When it is absent, Flaggo performs no
evidence-based generation for the contract.

When present:

- `policy.mode` defines how evidence-generated candidates enter activation;
- `policy.evaluate.interval` defines the minimum interval between asynchronous
  analysis attempts;
- `evidence` declares one or more logical observed values;
- each evidence declaration maps one observed value to one contract attribute;
- `correlateBy` names additional contract attributes used for correlation;
- `source` selects exactly one application-owned OpenTelemetry metric, log,
  span, or span event and maps every `correlateBy` name to an exact OTel
  attribute location and key;
- `objective.primary` identifies the evidence value and optimization
  direction; and
- optional guardrails are expressions over collected evidence.

The contract does not name an observability backend table or database field.
Applications emit normal OpenTelemetry telemetry. Metric sources match exact
instrumentation scope, metric name, metric kind, and unit. Log and span sources
match exact instrumentation scope and signal name. Span-event sources also
match the exact parent span name. Source matching is case-sensitive.

One evidence declaration represents one logical observed value. If one
OpenTelemetry activity supplies multiple observed values, the contract
declares multiple evidence entries with distinct source selectors.

Returning a `RuntimeDecision` does not prove that the application used it.
After a successful response, the SDK emits the built-in
`flaggo.decision.received` observation through OpenTelemetry. That observation
carries the decision ID, contract and executable digests, result, evaluation
facts, and configured correlation attributes. It records receipt, not proof of
application.

Selected application telemetry may correlate through both:

1. the Flaggo decision ID, when the application includes it; and
2. every contract attribute listed by `correlateBy`, which associates and
   validates the surrounding application activity.

Evidence source and correlation semantics are decision data, not authentication
or authorization.

The initial policy supports one mode:

```yaml
policy:
  mode: auto-activation
  evaluate:
    interval: PT15M
```

`auto-activation` instructs the Contract Service to attempt atomic activation
when analysis produces a valid candidate for the current contract. It does not
bypass candidate validation, contract conformance, current-contract checks, or
activation conflict handling. It also does not make runtime search for the
latest generated executable. If validation or activation fails, the existing
active executable remains unchanged.

`evaluate.interval` is an ISO 8601 duration and represents a minimum delay, not
an execution-time guarantee. The first analysis attempt becomes eligible one
interval after the contract becomes accepted and ready. Only one analysis run
may be active for a contract digest. The next interval starts after the
previous attempt completes, whether it activates a candidate, produces no
candidate, or fails.

Analysis method selection is not part of the initial contract shape. Flaggo
selects the method automatically and records its exact name, version,
configuration, and evidence references in generation provenance. The generated
behavior is identified by the resulting `executableDigest`. An explicit method
contract may be added in a future contract syntax.

The contract does not prescribe aggregation windows, analysis populations, or
primary-objective aggregation. The learning service chooses those details and
records them in generation provenance.

Guardrails use the same expression model as other contract expressions, with
an evidence environment supplied by the learning service. For example:

```text
sum(evidence.recovery_failure) <= 0
```

The exact evidence collection functions available to guardrail expressions
belong to the versioned expression profile.

The presence of `learning` requires policy, evidence, and an objective. A
contract may contain:

| Authored executable | Learning | Meaning |
| --- | --- | --- |
| Present | Absent | Authored executable generation only. |
| Present | Present | Authored bootstrap followed by optional evidence-based replacements. |
| Absent | Present | The default executable serves runtime decisions while evidence-based generation learns a replacement. |
| Absent | Absent | The default executable serves all runtime decisions. |

### Identity Model

The decision name is the user-owned identity of the logical decision:

```text
decisionName = "tetris.dropInterval"
```

The contract digest identifies one exact named contract:

```text
contractDigest = digest(canonical({
  name,
  expression_syntax,
  attributes,
  result,
  authoredExecutable?,
  learning?
}))
```

The digest commits to all user-authored semantic content. Changing the decision
name, expression syntax, attribute or result schema, default, rule name,
`when` condition or expression, returned behavior, evidence declaration,
objective, guardrail, or learning policy produces a new digest.

Descriptions are non-semantic and do not affect the digest, including
`description` annotations nested inside JSON Schemas. Formatting, comments,
attribute/evidence list order, and deployment metadata also do not change the
digest. Canonicalization applies these collection rules:

- attribute, evidence, and guardrail declarations are unordered and sort by
  unique `name`;
- `correlateBy` and JSON Schema `required` are unordered sets and sort
  lexically, including `required` in nested value schemas;
- authored and generated executable rules preserve authored order because the
  first matching rule wins; and
- arrays used as literal values, defaults, or schema-valid application data
  preserve element order.

Ordered collections do not gain a second `priority` field. Their array
position is the single source of precedence.

Expression canonicalization must use the accepted CEL profile and canonical
expression text rather than raw author formatting. Its checked protobuf is
validated derived storage data, not an additional identity input. The v3
digest profile fixes the internal attribute semantics, including `_random`;
changing those semantics requires a versioned identity profile.

The identity model has two levels:

| Identity | Meaning |
| --- | --- |
| `decisionName` | Stable, user-facing identity of the logical decision. |
| `contractDigest` | Exact identity of one immutable atomic contract declaration. |

The server never resolves an implicit latest contract for a runtime request.
Application code carries the digest derived from the exact decision contract
with which it was built.

Flaggo does not infer compatibility between contract digests. An older digest
retains its existing activation for applications still carrying that digest;
the new digest begins its own executable-generation lifecycle.

## Decision Executable

A `DecisionExecutable` is immutable, bounded behavior that conforms to one
exact decision contract. It consumes the contract's `RuntimeInput` and
produces its `RuntimeDecision`.

Its conceptual shape is:

```yaml
contractDigest: sha256:...
kind: rules

rules:
  - name: high-pressure
    when:
      expression: attributes.board_pressure_mean_5s >= 0.75
    return:
      value: 250

  - name: level-adjustment
    when:
      expression: attributes.current_level >= 10
    return:
      expression: 1000.0 - double(attributes.current_level) * 25.0
```

```text
DecisionExecutable + RuntimeInput
  -> RuntimeDecision
```

A decision executable must:

- bind to exactly one `contractDigest`;
- use the `expression_syntax` pinned by that contract;
- accept only runtime input permitted by that contract;
- produce only results permitted by that contract;
- satisfy the contract's attribute and result schemas, default, and
  executable-kind semantics;
- execute within declared resource and capability bounds;
- avoid request-time evidence queries or asynchronous analysis; and
- produce the same runtime decision for the same executable and runtime input.

Any value that may influence the result, including time or caller state, must
be explicit in `RuntimeInput` or immutable executable content. It cannot come
from undeclared ambient state.

The initial executable kind is `rules`. Its rules are ordered, contain only
deterministic `when.expression` conditions, and return exactly one literal
`value` or deterministic `expression`. Natural-language `condition` fields
never appear in a `DecisionExecutable`; Executable Generation must resolve
them before producing a candidate.

A rule is eligible only when every attribute referenced by its `when`
expression and return expression is present. The first eligible matching rule
wins. If no rule matches, runtime returns the bound contract's
`result.default`.

The automatically generated default executable is the empty rule set:

```yaml
contractDigest: sha256:...
kind: rules
rules: []
```

It deterministically reaches the contract default for every runtime input.

The following terms describe representations or lifecycle roles of this
primitive rather than separate decision primitives:

| Term | Meaning |
| --- | --- |
| `AuthoredExecutable` | Optional source representation contained in `DecisionContract` and supplied to executable generation. It may differ from Flaggo's internal executable representation. |
| `DefaultExecutable` | Constant executable generated and activated automatically from `result.default` so every accepted contract is runtime-ready. |
| `CandidateExecutable` | Immutable generated executable that conforms to one contract but is not yet active. |
| `ActiveExecutable` | Candidate executable selected as runtime authority for its exact contract digest. |

Compilation may translate an `AuthoredExecutable` into a different internal
representation. Evidence-based analysis may generate that internal
representation directly. Once generated, both paths produce the same
`DecisionExecutable` primitive and follow the same validation and activation
boundary.

Authored, evidence-based, and default origin is generation provenance rather
than executable type. Origin does not change runtime evaluation semantics.

### Identity Model

The executable digest identifies one exact executable bound to one exact
contract:

```text
executableDigest = digest(canonical({
  contractDigest,
  kind,
  rules
}))
```

`contractDigest` already commits to `expression_syntax`, attribute and result
schemas, default behavior, and internal-attribute semantics. `kind` and
canonical ordered rules identify the remaining executable behavior.

Provenance such as default, authored, or analyzed origin; analysis method;
evidence references; timestamps; and activation attempts is recorded
alongside the executable but does not alter its semantic identity.

The identity model has two linked digests:

| Identity | Meaning |
| --- | --- |
| `contractDigest` | Exact contract to which the executable must conform. |
| `executableDigest` | Exact immutable executable semantics under that contract. |

Including `contractDigest` in executable identity prevents an executable from
being silently reused under another contract, even when its executable body is
byte-for-byte identical.

`CandidateExecutable` and `ActiveExecutable` retain the same
`executableDigest`. Activation changes the executable's lifecycle role, not
its identity or content.

Server-generated record IDs may identify deployment, generation, or
activation events, but they do not replace these semantic identities.

## Primitive invariants

1. `decisionName` identifies the user-owned logical decision.
2. `contractDigest` identifies one exact atomic decision contract, including
   optional authored and learning declarations.
3. `executableDigest` identifies one immutable executable bound to one exact
   contract digest.
4. A decision contract never contains active runtime authority.
5. A decision executable never widens or changes its decision contract.
6. Authored and evidence-based generation produce the same executable
   primitive.
7. Every generated executable contains deterministic behavior only and inherits
   one top-level contract `expression_syntax`.
8. Candidate and active are lifecycle roles of one executable identity.
9. Activation does not mutate executable content or identity.
10. Flaggo does not infer compatibility across contract digests.
11. Runtime behavior depends only on the active executable and complete
    runtime input, including SDK-generated internal attributes.
12. Every accepted contract has an automatically activated default executable
    derived from `result.default`.

## Deferred Design Issues

The conceptual boundary is established. The following issues are
explicitly deferred:

1. **Natural-language rules.** Define generation, validation, and activation
   requirements for condition-only rules.
2. **Internal randomness.** Define the random source, distribution tests, and
   whether future stable bucketing uses another internal attribute.
3. **Evidence interpretation.** Define analysis-owned metric temporality,
   distribution interpretation, rates, aggregation, and value derivation from
   complete materialized candidates.
4. **Evidence correlation failures.** Define behavior for a missing or unknown
   decision ID, missing correlation attributes, attribute mismatch, or
   ambiguous evidence.

Activation actor roles, retention, and operational scheduling remain lifecycle
or service concerns rather than additional fields in this conceptual contract
shape.

## Related documents

- [Decision contract lifecycle](LIFECYCLE.md)
- [Runtime evaluation model](RUNTIME_EVALUATION.md)
- [Contract clients and Contract Service](../architecture/CONTRACT_SERVICE.md)
- [Decision authority](../architecture/AUTHORITY.md)
- [Runtime client and Decision Service](../architecture/RUNTIME.md)
