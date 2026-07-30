> Historical discussion record: examples using `decisionKey@number` predate the Phase 1 identity decision and are not current wire-contract examples. Current contracts use an opaque definition lineage ID plus an opaque registry revision and canonical contract digest.

Yes—that is a strong and much cleaner core model:

```text

Evidence

+ Runtime Decision Context

+ Decision Definition

→ decision process

```

I would make one addition: **governed decision state**. It is distinct from runtime context because it records what Flaggo has already decided or what operators have controlled.

## Core concepts

### 1. Evidence

**Evidence is durable, incremental, and reusable.**

It represents what has been observed or derived from observations over time.

```text

gameplay.piece_placed

gameplay.placement_time

gameplay.early_loss_rate

gameplay.recovery_success_rate

```

Evidence should be independent of a particular decision definition, target, policy, or application version.

An **evidence view** projects evidence into a decision process:

```text

gameplay.placement_time

  window: 2 minutes

  grouped by: session:game-456

```

```text

gameplay.early_loss_rate

  window: 1 hour

  grouped by: cohort:new_players

```

So:
> Evidence defines what was observed and what it means.  
> Evidence views define which slice of it is relevant now.

---

### 2. Runtime Decision Context

**Runtime context is the current fact snapshot supplied with a decision request.**

```text

current user: user-123

current session: game-456

current level: 3

board pressure: high

recent placement time: 1420ms

device type: mobile

```

It includes the identifiers from which Flaggo can resolve the **decision target**:

```text

tetris.dropInterval @ session:game-456

```

Runtime context is not durable evidence by default. It may be recorded in an audit trail or emitted as an observation when appropriate, but its primary meaning is:
> What is true right now, for this decision request?

It should also be the place where the application expresses the immediate target—explicitly or by providing keys such as `sessionId`, `userId`, or `tenantId`.

---

### 3. Decision Definition

**A decision definition is the versioned contract for a kind of decision.**

It answers:
> What are we deciding, for which kinds of targets, using what interpretation, toward what intent, within what limits?

For example:

```yaml

id: tetris.dropInterval

revision: 2

result:

  type: number

  range: [200, 1500]

  step: 50

  default: 800

targets:

  supported: [session, user, cohort, global]

  default: session

context:

  required:

    - boardPressure

    - recentPlacementTimeMs

evidence:

  - gameplay.placement_time@1

  - gameplay.early_loss_rate@1

  - gameplay.recovery_success_rate@1

intent:

  - keep gameplay challenging

  - reduce early losses

constraints:

  - maxDelta: 50

  - cooldown: 20s

  - minimumConfidence: 0.7

```

The definition does **not** own evidence. It references evidence definitions and describes how they matter to this decision.

When the decision evolves, create a new revision:

```text

tetris.dropInterval@1

tetris.dropInterval@2

```

Both revisions may reuse:

```text

gameplay.placement_time@1

gameplay.early_loss_rate@1

```

That preserves accumulated learning across decision evolution and across coexisting application versions.

---

### 4. Governed Decision State

This is the missing fourth concept I would include.

**State is the durable memory and operational authority for a decision.**

Examples:

```text

active strategy for cohort:new_players

last applied interval for session:game-456

cooldown until 12:01:20 UTC

operator override: 900ms

decision paused

active experiment assignment

previous known-safe value

```

It answers:
> What has already been approved, applied, overridden, or remembered?

This cannot be merged into runtime context because it is owned by Flaggo’s control plane, not supplied by the application. It also cannot be merged into evidence because it is not an observation about the external world; it is operational state about decision authority.

---

## Complete service-side model

```text

Evidence

  Durable observations and reusable measures.

Runtime Decision Context

  Facts about this request, including target identity.

Decision Definition

  Versioned contract for what to decide, relevant evidence,

  intent, result shape, and constraints.

Governed Decision State

  Active strategy/value, prior decisions, cooldowns,

  overrides, lifecycle, experiments, and rollout state.

```

Then:

```text

Decision Definition

+ Runtime Decision Context

+ Evidence Views

+ Governed Decision State

  → Decision Intelligence proposes a candidate

  → Governance approves, limits, blocks, or falls back

  → Governed runtime decision

```

## Important separation

| Concept | Owns | Does not own |

|---|---|---|

| Evidence | observations, semantic measures, provenance | decision-specific goals, scopes, policy |

| Runtime context | current facts and target identifiers | history, active strategy, durable evidence |

| Decision definition | decision meaning, intent, bounds, evidence references | raw evidence history, active runtime state |

| Governed state | authority and memory of active control | application-provided current facts |

So your summary is almost complete. I would phrase it as:
> **Flaggo has durable evidence, per-request runtime context, versioned decision definitions, and governed decision state.**  
> A decision definition projects reusable evidence into an evaluation for a target identified by runtime context; intelligence proposes an outcome, and governance turns it into an approved decision or fallback.
 
I like this service side model. How does it integrate with decision intelligence, in which we kinda define two paths: the async path and the online runtime path
 
The four-part service model becomes the shared foundation for **both** paths. The difference is not the concepts they use; it is **when they run, what they are allowed to produce, and whether they change governed state**.

```text

Shared inputs:

  Decision Definition

  + Runtime Decision Context

  + Evidence Views

  + Governed Decision State

```

## 1. Async intelligence: learns and proposes control changes

The async path operates over durable evidence and produces proposals that may change future runtime behavior.

```text

Evidence updates, schedule, drift signal, or operator request

  → select Decision Definition revision

  → inspect durable Evidence Views

  → inspect Governed Decision State

  → reason about intent and constraints

  → propose value, strategy, experiment, hold, or rollback

  → Governance reviews proposal

  → update Governed Decision State if approved

```

For Tetris:

```text

Definition:

  tetris.dropInterval@2

Evidence:

  gameplay.early_loss_rate@1 by cohort:new_players over 24h

  gameplay.placement_time@1 by player cohort over 24h

  gameplay.recovery_success_rate@1 by player cohort over 24h

State:

  active strategy: fixed 800ms

  current rollout: 10% of new-player sessions

Async intelligence proposes:

  activate a bounded strategy:

    base: 800ms

    range: 700–1000ms

    slow down 50ms under high pressure and slow placement

Governance:

  approves a limited rollout for cohort:new_players

Updated governed state:

  active strategy for tetris.dropInterval@2

  target/control boundary: cohort:new_players

  rollout: 10%

```

The async path should **not** directly change application behavior. It creates a proposal; governance turns an approved proposal into durable state.

Its job is:
> Learn from history and decide what runtime behavior is allowed to be active.

---

## 2. Online runtime: applies the currently governed behavior

The online path serves the application request. It should normally be fast, deterministic, and bounded.

```text

Application request

  → identify Decision Definition and target from Runtime Context

  → load applicable Governed Decision State

  → obtain only the Evidence Views required at decision time

  → execute active value / approved strategy / experiment assignment

  → Governance validates the concrete candidate

  → return value or fallback

  → record decision and later observe outcome

```

For Tetris:

```text

Request:

  tetris.dropInterval@2

  target: session:game-456

Runtime context:

  boardPressure: high

  recentPlacementTimeMs: 1420

  currentLevel: 3

Governed state:

  active strategy for cohort:new_players:

    base: 800ms

    adjust +50ms under pressure and slow placement

Online execution:

  candidate: 850ms

Runtime governance:

  - within 700–1000ms strategy range: yes

  - within 200–1500ms definition range: yes

  - max delta 50ms: yes

  - cooldown satisfied: yes

  - no operator override: yes

Response:

  850ms

```

Its job is:
> Apply already-authorized behavior to the current situation safely.

---

## 3. Evidence plays different roles in each path

This is where the evidence model is especially valuable.

| Path | Primary evidence use | Typical evidence view |

|---|---|---|

| **Async intelligence** | Learn, detect drift, compare outcomes, design or revise strategies | Cohort/global views over hours, days, or weeks |

| **Online runtime** | Adapt the active strategy to the current request or target | Session/user/request views over seconds or minutes |

| **Both** | Explain and audit why a result or proposal occurred | Immutable evidence snapshot references |

The definitions are reusable in both cases:

```text

gameplay.placement_time@1

```

But the views differ:

```text

Async:

  gameplay.placement_time@1

  grouped by: cohort:new_players

  window: 24h

Online:

  gameplay.placement_time@1

  grouped by: session:game-456

  window: 2m

```

Neither requires a new evidence resource.

---

## 4. Governed state is the handoff between the two paths

This is the most important integration point:

```text

Async intelligence

  → proposal

  → governance

  → Governed Decision State

  → online runtime

```

The online path should consume **governed state**, never raw agent output or unreviewed proposals.

```text

Bad:

  online request → invoke agent → agent invents a strategy → apply it

Good:

  async intelligence → proposes bounded strategy

  → governance approves it

  → state stores it as active

  → online request executes that approved strategy

```

This gives you:

- predictable latency;

- policy enforcement before behavior changes;

- stable rollout and rollback;

- auditable handoff;

- safety even if the intelligence system is unavailable.

---

## 5. Both paths use a Decision Definition, but differently

A versioned decision definition is the common contract.

```text

tetris.dropInterval@1

tetris.dropInterval@2

```

### Async path

Uses the definition to determine:

- what evidence may be considered;

- what outcomes mean success;

- which action space is valid;

- which target/control boundaries are permitted;

- what strategy forms may be proposed.

### Online path

Uses the definition to determine:

- whether the request context is valid;

- which target is being evaluated;

- the local fallback;

- whether a candidate fits the result type and action space;

- which active state revision is compatible.

This also supports coexisting service versions:

```text

Older client:

  requests tetris.dropInterval@1

Newer client:

  requests tetris.dropInterval@2

Registry:

  supports both definitions during migration

Evidence:

  gameplay.placement_time@1 is reusable by both

```

The async path can maintain or propose governed state separately per compatible definition revision, then migrate deliberately once the older version is retired.

---

## 6. Recommended refined model

```text

Evidence Layer

  Durable observations and reusable evidence definitions.

Decision Definition Layer

  Versioned declaration of decision meaning:

  result, targets, required context, evidence bindings,

  intent, action space, fallback, and constraints.

Async Intelligence Path

  Uses definitions + historical evidence + governed state

  to propose strategy/value/experiment/hold/rollback.

Governance Layer

  Reviews proposals and runtime candidates.

  Owns policy, authority, approval, lifecycle, overrides,

  rollout, fallback, and audit.

Governed Decision State

  Durable handoff from governance to runtime:

  active value, approved strategy, experiment, override,

  cooldown, previous safe value.

Online Runtime Path

  Uses definition + current context + applicable state +

  narrow evidence views to return a fast governed value.

Observation Flow

  Outcomes from runtime decisions become new durable evidence.

```

In one loop:

```text

Runtime outcome

  → Evidence

Evidence + Decision Definition + State

  → Async intelligence proposal

Proposal

  → Governance

  → Active governed state

Runtime context + Definition + Active state

  → Online execution

  → Governed value or fallback

Governed value + application outcome

  → Evidence

```

So **decision intelligence is not a separate competing abstraction**. It is the reasoning capability that operates over the durable evidence, versioned definitions, and governed state—primarily asynchronously, and only in bounded form online when the use case justifies it.
 
What would be a decision identity base on? Basically we need a new async intelligence path run for each versioned definition and thus one distinct governed decision state per versioned decision definition?
 
A decision needs **two related identities**:

1. **Definition identity** — *what decision contract is this?*

2. **Decision target/control identity** — *where does it apply or have authority?*

A versioned definition is necessary, but it is not sufficient by itself to identify a concrete governed decision.

## Recommended identity model

```text

Decision Definition Identity

  = app/environment + stable decision key + immutable revision

Decision Control Identity

  = definition identity + control target

Runtime Decision Instance

  = definition identity + runtime target + request/correlation ID

```

For example:

```text

Definition:

  app: tetris-demo

  environment: production

  decision: tetris.dropInterval

  revision: 2

Control target:

  cohort:new_players

Runtime target:

  session:game-456

```

Then:

```text

Governed control state:

  tetris.dropInterval@2 / cohort:new_players

Runtime decision instance:

  tetris.dropInterval@2 / session:game-456 / request:abc-123

```

## Why definition revision must be part of the identity

A decision definition revision changes the semantic contract: intent, allowed action space, required context, evidence bindings, fallback, or limits may all change.

So an async intelligence proposal must always be attributable to the exact revision it interpreted:

```text

proposal:

  definition: tetris.dropInterval@2

  control target: cohort:new_players

  evidence views: ...

  proposal: activate numeric strategy

```

It should not be ambiguous whether a strategy was produced under:

```text

tetris.dropInterval@1

```

or:

```text

tetris.dropInterval@2

```

That is essential for auditability and for coexistence of old and new service versions.

---

# Does every versioned definition need a separate async intelligence path?

**Conceptually yes; operationally no.**

Each active definition revision needs its own **intelligence lifecycle** because it may have different:

- evidence bindings;

- intent and goals;

- action space;

- fallback;

- constraints;

- supported targets;

- context requirements.

But this does **not** mean running a separate agent loop constantly for every revision.

Instead, define an **intelligence work item**:

```text

Intelligence Work Item

  = Decision Definition Revision

  + Control Target

  + Analysis Trigger

```

Examples:

```text

tetris.dropInterval@2

  + cohort:new_players

  + nightly evidence review

```

```text

tetris.dropInterval@2

  + cohort:new_players

  + early-loss-rate drift detected

```

```text

tetris.dropInterval@1

  + global

  + operator requested review

```

The async system schedules or triggers work only when needed:

- new definition revision registered;

- enough new evidence arrives;

- evidence quality changes;

- a guardrail regresses;

- a rollout reaches a review threshold;

- an operator requests analysis;

- a strategy nears expiry;

- a migration requires validation.

So: **each definition revision has an independent analysis identity, not necessarily a continuously running agent.**

---

# Does each versioned definition need distinct governed state?

**Yes for semantic authority, but state should be partitioned more carefully than “one state object per definition.”**

A useful model is three state levels.

## 1. Definition state

This is state about the definition revision itself.

```text

tetris.dropInterval@2

lifecycle: active

registered at: ...

compatibility with @1: requires-client-update

default fallback: 800ms

supported target kinds: session, user, cohort, global

```

Key:

```text

DefinitionStateKey

  = DecisionDefinitionRef

```

There should be distinct definition state for each revision.

---

## 2. Governed control state

This is the durable authority that async intelligence and governance produce.

```text

tetris.dropInterval@2

  + cohort:new_players

active strategy:

  base: 800ms

  allowed range: 700–1000ms

  max adjustment: 50ms

  rollout: 10%

operator mode: active

override: none

approved proposal: proposal-789

```

Key:

```text

GovernedControlStateKey

  = DecisionDefinitionRef

  + ControlTargetRef

```

This is usually the important handoff from async intelligence to online runtime.

It should be separate across revisions by default:

```text

tetris.dropInterval@1 / cohort:new_players

tetris.dropInterval@2 / cohort:new_players

```

Otherwise a strategy approved under the old definition could silently operate under a changed action space, goal, or fallback contract.

---

## 3. Runtime target state

This is short-lived or target-specific state used by online execution.

```text

tetris.dropInterval@2

  + session:game-456

last applied value: 850ms

last applied at: ...

cooldown until: ...

previous safe value: 800ms

```

Key:

```text

RuntimeTargetStateKey

  = DecisionDefinitionRef

  + RuntimeTargetRef

```

For the Tetris scenario, this is likely session-scoped and has a TTL. It exists so online runtime can enforce max delta and cooldown without asking async intelligence to reason per session.

---

# The critical distinction: runtime target is not necessarily control target

This avoids accidentally launching async intelligence for every session.

```text

Online decision target:

  session:game-456

Async control target:

  cohort:new_players

```

The async path proposes the strategy at the cohort level:

```text

tetris.dropInterval@2 / cohort:new_players

```

The online path applies it to a session:

```text

tetris.dropInterval@2 / session:game-456

```

The session’s current context determines whether the approved strategy returns `800ms`, `850ms`, or another bounded value.

So the hierarchy is:

```text

Async intelligence:

  learns and governs at a manageable control boundary

Online runtime:

  applies that governed behavior at high-cardinality runtime targets

```

That is much more scalable than creating one async intelligence process and one durable strategy per user or session.

---

# Version evolution and state migration

A new definition revision should normally start with **new governed control state**, but it should not necessarily start from zero.

For example:

```text

tetris.dropInterval@1

  range: 200–1500ms

  fallback: 800ms

  active cohort strategy: 700–1000ms

tetris.dropInterval@2

  range: 300–1300ms

  fallback: 800ms

  same intent and evidence bindings

```

When `@2` is registered, Flaggo can perform a migration assessment:

```text

Can the @1 strategy be safely migrated to @2?

```

Possible outcomes:

| Result | Meaning |

|---|---|

| `reuse` | The state/strategy is semantically identical and can be referenced by the new revision |

| `transform` | The strategy can be mechanically constrained or translated into the new action space |

| `revalidate` | Historical evidence is reusable, but intelligence must generate and govern a fresh proposal |

| `reset` | The semantic change is too significant; begin with fallback or a fixed safe value |

For the example above, the likely result is:

```text

historical evidence: reusable

old strategy: transform or revalidate

new governed control state: distinct

```

The new revision can immediately use historical evidence such as:

```text

gameplay.placement_time@1

gameplay.early_loss_rate@1

```

But it should not automatically inherit active authority unless compatibility rules explicitly allow it.

---

# Suggested canonical references

```ts

type DecisionDefinitionRef = {

  appId: string;

  environment: string;

  key: string;          // "tetris.dropInterval"

  revision: string;     // immutable semantic revision

};

type DecisionTargetRef = {

  type: string;         // "session", "user", "cohort", "global"

  id: string;

};

type GovernedControlStateRef = {

  definition: DecisionDefinitionRef;

  controlTarget: DecisionTargetRef;

};

type RuntimeDecisionInstanceRef = {

  definition: DecisionDefinitionRef;

  runtimeTarget: DecisionTargetRef;

  correlationId: string;

};

```

And for async work:

```ts

type IntelligenceWorkItemRef = {

  definition: DecisionDefinitionRef;

  controlTarget: DecisionTargetRef;

  trigger:

    | "definition-activated"

    | "evidence-updated"

    | "drift-detected"

    | "scheduled-review"

    | "operator-request"

    | "rollout-review";

};

```

## Bottom line

- **Yes:** a decision definition revision is a separate semantic unit and needs independently auditable intelligence and governance.

- **No:** do not run one async loop or create one strategy state for every individual runtime target.

- Use **definition revision + control target** as the identity for async proposals and durable governed control state.

- Use **definition revision + runtime target** for online cooldown, previous-value, and session-specific state.

- Reuse durable evidence across revisions; migrate or revalidate governed state explicitly based on compatibility.
 
Not sure my understanding is correct. So async learning should be decision definition version independent with different trigger that can async generate governed states of different versioned definition of the same decision. Then online runtime path should ingest runtime context that has version so that a decision could be fast delivered
 
Close, but I would separate **three versioning concerns** so they do not get conflated.

```text

1. Evidence learning is definition-version independent.

2. Governed state is definition-version specific.

3. Runtime context has its own schema version, separate from decision-definition version.

```

## 1. Async learning is independent of a decision-definition version

Yes. The async learning system should operate over durable evidence and produce reusable findings—not one learning loop per definition revision.

For example, it may learn:

```text

Finding:

  For new players, high board pressure combined with slow placement

  strongly correlates with early game loss.

Evidence:

  gameplay.placement_time@1

  gameplay.early_loss_rate@1

Applicable domain:

  gameplay / Tetris

```

That finding is not inherently about:

```text

tetris.dropInterval@1

```

or:

```text

tetris.dropInterval@2

```

It could potentially inform multiple decisions:

```text

tetris.dropInterval

tetris.difficultyMode

tetris.recoveryHint

```

Async triggers are also independent of definition revision:

```text

- new evidence arrives

- evidence window closes

- drift or guardrail regression is detected

- scheduled review runs

- an operator requests analysis

- a new decision definition revision becomes active

```

So the async subsystem can maintain:

```text

durable evidence

→ derived evidence

→ reusable findings / models / features / insights

```

## 2. Turning learning into governed behavior is version-specific

A finding cannot directly become runtime authority. It must be interpreted through a particular decision definition revision.

```text

Reusable finding:

  High pressure + slow placement predicts early loss.

Decision definition:

  tetris.dropInterval@2

Definition @2:

  - output: number

  - allowed range: 300–1300ms

  - fallback: 800ms

  - intent: reduce early loss while retaining challenge

  - constraints: max delta 50ms, cooldown 20s

```

Only then can async intelligence propose a concrete strategy:

```text

Proposal:

  definition: tetris.dropInterval@2

  control target: cohort:new_players

  strategy:

    base: 800ms

    when boardPressure is high and placement is slow:

      increase interval by 50ms

```

Governance may approve it, creating state such as:

```text

Governed state:

  decision family: tetris.dropInterval

  definition revision: 2

  control target: cohort:new_players

  active strategy: strategy-123

```

So the flow is:

```text

Evidence

  → async learning / reusable findings

  → evaluate finding against decision definition @ revision

  → proposal

  → governance

  → governed state for that revision

```

A new definition revision can trigger re-evaluation:

```text

tetris.dropInterval@3 activated

  → inspect compatible prior governed state and reusable findings

  → carry forward, transform, revalidate, or replace the strategy

```

It does **not** require rebuilding learning history from scratch.

---

## 3. Runtime context should have a schema version—but that is not what selects governed state

Your idea that runtime context has a version is correct and useful, particularly for coexisting service instances.

For example:

```json

{

  "contextSchema": "tetris.gameplay-context@2",

  "context": {

    "sessionId": "game-456",

    "userId": "user-123",

    "boardPressure": "high",

    "recentPlacementTimeMs": 1420,

    "recoveryFailures": 2

  }

}

```

This lets Flaggo validate and evolve context safely:

```text

context schema @1:

  sessionId

  boardPressure

context schema @2:

  sessionId

  boardPressure

  recentPlacementTimeMs

  recoveryFailures

```

However, a context version alone should **not** decide which governed decision state the runtime uses.

The online request needs two separate identities:

```text

Decision contract identity:

  Which definition revision or compatibility contract does this client expect?

Context schema identity:

  Which shape/version of runtime facts is this caller supplying?

```

For example:

```json

{

  "decision": {

    "key": "tetris.dropInterval",

    "expectedDefinitionRevision": "2"

  },

  "target": {

    "type": "session",

    "id": "game-456"

  },

  "context": {

    "schema": "tetris.gameplay-context@2",

    "values": {

      "userId": "user-123",

      "boardPressure": "high",

      "recentPlacementTimeMs": 1420,

      "recoveryFailures": 2

    }

  }

}

```

The online runtime can then quickly:

```text

1. Resolve tetris.dropInterval@2.

2. Validate or adapt context schema @2.

3. Identify target: session:game-456.

4. Load applicable governed state:

     tetris.dropInterval@2 / cohort:new_players.

5. Load session runtime state:

     tetris.dropInterval@2 / session:game-456.

6. Execute the already-approved strategy.

7. Enforce final runtime policy.

8. Return value or fallback.

```

That remains fast because it does **not** run open-ended async learning. It loads pre-approved state and executes a bounded strategy.

---

## The compact mental model

```text

Evidence

  Durable observations and reusable learned findings.

Async learning

  Triggered by evidence changes, schedules, drift, operator requests,

  or definition activation. It is independent of one definition version.

Decision definition revision

  Interprets evidence and findings for one particular decision contract.

Governed state

  A version-aware, approved value/strategy for a control target.

Runtime context schema

  Versioned facts supplied by a particular application version.

Online runtime

  Uses definition version + compatible context + active governed state

  to deliver a fast decision.

```

Or as a flow:

```text

Durable evidence

  → reusable learning/findings

  → evaluate against tetris.dropInterval@2

  → governance

  → active governed state for @2

Runtime request:

  tetris.dropInterval@2

  + context schema @2

  + session target

  → load active state

  → execute bounded strategy

  → return decision quickly

```

The key correction is:
> **Async learning can be revision-independent; governed state cannot be revision-ambiguous.**  
> And **runtime context should be schema-versioned, but the decision-definition revision is the primary key for selecting compatible governed behavior.**
 
I didn't just mean the runtime context should have a schema, but the runtime context should be aware of the current decision definition versioned. Imagine two versions of code of the same service, one version has decision definition includes on additional metrics to move. The runtime context collected by the online runtime intelligence should contains the info of definition versioned. So that online runtime path could deliver a decision differently. Make sense?
 
Yes, that makes sense—and it corrects an important ambiguity in my earlier framing.

The **online runtime evaluation context should be bound to a versioned decision definition**. Not merely carry a schema version for its facts.

In other words, the request is not just:

```text

Here are the current facts for session:game-456.

```

It is:

```text

Evaluate tetris.dropInterval using decision definition revision 2

for session:game-456, given these current facts.

```

That definition version determines how the online path interprets runtime facts, which evidence views it may use, which strategy/state is eligible, and what result it may return.

## Refined runtime model

Split the runtime input into two parts:

```text

Runtime Decision Context

  = Definition Binding

  + Target

  + Current Facts

```

### Definition binding

```text

decision family: tetris.dropInterval

definition revision: 2

definition digest: sha256:...

```

### Target

```text

session:game-456

```

### Current facts

```text

boardPressure: high

recentPlacementTimeMs: 1420

recoveryFailures: 2

currentLevel: 3

```

Conceptually:

```ts

type RuntimeDecisionContext = {

  definition: {

    key: "tetris.dropInterval";

    revision: "2";

    digest?: string;

  };

  target: {

    type: "session";

    id: "game-456";

  };

  facts: {

    boardPressure: "high" | "medium" | "low";

    recentPlacementTimeMs: number;

    recoveryFailures: number;

    currentLevel: number;

  };

};

```

The application code may not need to manually pass the version every time. A version-bound SDK declaration can do so automatically:

```ts

const dropInterval = flaggo.decision("tetris.dropInterval", {

  definitionRevision: "2",

});

const interval = await dropInterval.decide({

  target: { type: "session", id: sessionId },

  facts: {

    boardPressure,

    recentPlacementTimeMs,

    recoveryFailures,

    currentLevel,

  },

});

```

But it must be present in the runtime protocol.

---

## Why this matters for coexisting service versions

Consider two deployed versions of the Tetris app.

### Application v1

Its definition is:

```text

tetris.dropInterval@1

Inputs:

- boardPressure

- currentLevel

Evidence:

- gameplay.placement_time@1

- gameplay.early_loss_rate@1

Intent:

- reduce early losses

```

### Application v2

Its definition evolves:

```text

tetris.dropInterval@2

Inputs:

- boardPressure

- currentLevel

- recentPlacementTimeMs

- recoveryFailures

Evidence:

- gameplay.placement_time@1

- gameplay.early_loss_rate@1

- gameplay.recovery_success_rate@1

Intent:

- reduce early losses while preserving challenge

```

During rollout, both clients may call the same logical decision:

```text

tetris.dropInterval

```

But they are not requesting the same **decision contract**.

```text

v1 client:

  tetris.dropInterval@1

  + only v1 facts

  → receives a decision from v1-compatible governed state

v2 client:

  tetris.dropInterval@2

  + additional v2 facts

  → receives a decision from v2-compatible governed state

```

That lets online runtime behavior evolve safely:

```text

v1:

  high board pressure

  → return 850ms

v2:

  high board pressure + slow placement + recovery failures

  → return 900ms

```

The difference is valid because each decision was evaluated under a different versioned definition.

---

## What version selects in the online path

The definition revision should select all of the following:

| Concern | Selected by definition revision |

|---|---|

| Result contract | Number/string/boolean; bounds; allowed values; fallback |

| Current-fact contract | Required and optional runtime facts |

| Evidence bindings | Which evidence resources and views are relevant |

| Intent | Goals and tradeoffs for this revision |

| Constraints | Definition-level limits and compatibility rules |

| Strategy interface | Which approved strategy shapes and inputs are valid |

| Governed state | Active values/strategies approved for that definition revision |

| Audit interpretation | How to reconstruct why that decision was returned |

So the fast runtime lookup is not merely:

```text

decision key + target

```

It is:

```text

decision definition revision + target + current facts

```

More specifically:

```text

tetris.dropInterval@2

  + runtime target session:game-456

  + current facts

  → resolve v2-compatible governed state

  → execute v2 strategy

  → return v2-compatible result

```

---

## Revised relationship between async and online paths

### Async learning remains reusable

Async learning can produce reusable, definition-independent findings:

```text

Finding:

  High board pressure, slow placement, and repeated recovery failures

  are associated with early loss for new players.

```

That finding may come from durable evidence resources:

```text

gameplay.placement_time@1

gameplay.early_loss_rate@1

gameplay.recovery_success_rate@1

```

### Async decision evaluation is definition-bound

The async path then evaluates that finding against each active definition revision that may benefit from it:

```text

Finding

  + tetris.dropInterval@1

  → perhaps propose a simple pressure-based strategy

Finding

  + tetris.dropInterval@2

  → perhaps propose a richer pressure + placement + recovery strategy

```

That results in separate governed states:

```text

Governed state:

  tetris.dropInterval@1

  + control target cohort:new_players

  + strategy-v1

Governed state:

  tetris.dropInterval@2

  + control target cohort:new_players

  + strategy-v2

```

The evidence and learning are shared. The operational behavior remains version-specific.

---

## A useful terminology adjustment

Instead of treating `Runtime Decision Context` as only business facts, define it as the complete evaluation envelope:

```text

Runtime Decision Context

  = definition binding

  + target binding

  + current facts

  + optional caller/deployment identity

```

Then distinguish internally:

```text

Definition binding

  Which versioned decision contract is requested?

Target binding

  Where will the decision apply?

Runtime facts

  What is true right now?

Deployment identity

  Which service/code version is making the request?

```

For example:

```json

{

  "definition": {

    "key": "tetris.dropInterval",

    "revision": "2",

    "digest": "sha256:abc123"

  },

  "target": {

    "type": "session",

    "id": "game-456"

  },

  "facts": {

    "boardPressure": "high",

    "recentPlacementTimeMs": 1420,

    "recoveryFailures": 2,

    "currentLevel": 3

  },

  "caller": {

    "service": "tetris-web",

    "release": "2026.07.26.4"

  }

}

```

## Final model

```text

Durable Evidence

  → reusable observations, measures, findings, and learned models

Versioned Decision Definition

  → specifies how a decision revision interprets evidence and runtime facts

Async Intelligence

  → uses reusable learning plus each active definition revision

  → proposes version-specific governed states

Runtime Decision Context

  → explicitly identifies the requested definition revision,

    target, and current facts

Online Runtime

  → loads governed state compatible with that definition revision

  → executes bounded logic quickly

  → returns a decision or fallback

```

So yes: **the definition version should travel with the online runtime context and be a first-class selector for online behavior.** It is what allows the same logical decision to evolve while old and new service versions coexist safely.
 