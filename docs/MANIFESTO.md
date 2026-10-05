# Flaggo manifesto

## Why Flaggo exists

Software increasingly uses AI to write, review, test, and ship code, yet many
runtime choices remain scattered across branches, fixed thresholds, remote
configuration, and manually operated control loops.

Flaggo explores a better primitive for selected runtime decisions:

> Given an exact `DecisionContract`, one complete `RuntimeInput`, and the
> immutable `DecisionExecutable` active for that contract version, what
> deterministic contract-valid result should the application receive now?

The application still owns execution. Product and platform teams define the
decision interface and intent. Flaggo makes delegated decisions explicit,
versioned, deterministic online, observable after application, and capable of
gaining an asynchronous evidence-driven learning loop.

## Implementation principles

1. **Build the smallest complete end-to-end slice.** Prove contract
   acceptance, default executable activation, runtime evaluation, applied
   exposure, and correlated outcome before expanding the platform.
2. **Use cohesive boundaries.** Contract Service owns immutable contract
   versions and executable activation. Decision Service owns stateless runtime
   evaluation. The SDK owns complete input construction and decision-receipt
   telemetry; the application owns result application and ordinary telemetry.
   Evidence ingestion, materialization, and learning remain asynchronous.
3. **Keep runtime behavior explicit and bounded.** Attribute and result schemas
   define the interface; immutable executables define deterministic behavior.
   Runtime never invokes generation or analysis.
4. **Add contracts only where boundaries require them.** Public APIs,
   cross-process behavior, persisted stores, and lifecycle concurrency need
   explicit contracts. Internal helpers do not become components by default.
5. **Preserve authority and accountability.** Authenticated scope, immutable
   contract and executable identities, validated candidates, and atomic
   activation are non-negotiable. Generation alone never grants runtime
   authority.
6. **Define failure and fallback explicitly.** Do not use broad catches,
   silent defaults, success-shaped errors, or ambiguous authority. Contract
   no-match behavior and SDK-local failure fallback are different concerns.
7. **Remove obsolete paths.** Update callers and delete superseded contracts
   and documentation instead of adding compatibility layers or parallel
   names.
8. **Test meaningful boundaries.** Cover expected outcomes, failures,
   concurrency, persistence, activation replacement, and real service/store
   seams.

## Product boundary

Online evaluation remains deterministic, bounded, and tied to an exact
contract digest:

```text
RuntimeActivation[contractDigest]
  -> immutable DecisionExecutable

DecisionExecutable + complete RuntimeInput
  -> RuntimeDecision
```

Longer-running analysis belongs in an asynchronous pipeline:

```text
DecisionContract + correlated evidence
  -> bounded CandidateExecutable
  -> candidate validation
  -> atomic activation
  -> deterministic Decision Service evaluation
```

The initial learning policy uses `mode: auto-activation`: a valid candidate
from the current contract immediately attempts atomic activation. Runtime
still reads only the activation mapping; it never discovers the latest
generated artifact.

Not every branch should become a decision call, and not every decision needs
AI. Flaggo is for contextual, high-change decisions where an explicit
versioned contract and observable activation boundary are better than
hard-coded logic plus manual operations.
