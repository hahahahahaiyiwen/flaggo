# Flaggo manifesto

## Why Flaggo exists

Software increasingly uses AI to write, review, test, and ship code, yet many
runtime choices remain scattered across branches, fixed thresholds, remote
configuration, and manually operated control loops.

Flaggo explores a better primitive for selected runtime decisions:

> Given an explicit decision definition, current context, approved authority,
> and deterministic constraints, what safe value should the application receive
> now?

The application still owns execution. Product and platform teams define intent,
constraints, and accountability. Flaggo makes the delegated decision explicit,
approved, deterministic online, reconstructable, and capable of gaining a
closed loop when separately approved asynchronous analysis is useful.

## Implementation principles

1. **Build the smallest complete end-to-end slice.** Prove one useful decision
   from definition through approval, runtime use, durable record, exposure, and
   outcome before expanding the platform.
2. **Use cohesive boundaries.** Contract Service, Decision Service, OTel
   Ingestion, Async Analysis Pipeline, and the Contract, State, and Evidence
   Stores each own one meaningful domain or external-system boundary.
3. **Treat constraints as contract data and boundary behavior.** Definitions
   declare deterministic constraints; Contract Service validates them and
   Decision Service evaluates them. A separate Policy service is not required.
4. **Add contracts only where boundaries require them.** Public APIs,
   cross-process behavior, persisted stores, and lifecycle concurrency need
   explicit contracts. Internal helpers do not become components by default.
5. **Preserve authority and accountability.** Authenticated approval,
   credential separation, fail-closed identity validation, stable-head
   compare-and-swap, immutable state, and durable records are non-negotiable.
6. **Define failure and fallback explicitly.** Do not use broad catches, silent
   defaults, success-shaped errors, or ambiguous authority. Every server
   decision must be reconstructable.
7. **Remove obsolete paths.** Update callers and delete superseded contracts
   and documentation instead of adding compatibility layers or parallel names.
8. **Test meaningful boundaries.** Cover expected outcomes, failures,
   concurrency, replay, persistence, and real service/store seams.

## Product boundary

Online execution remains deterministic, bounded, constraint-checked, and
compatible with approved decision state. It never invokes an unbounded
request-time agent loop.

Longer-running analysis belongs in an asynchronous pipeline:

```text
contracts + evidence + outcomes + current state
  -> bounded candidate
  -> Contract Service approval
  -> State Store activation
  -> deterministic Decision Service execution
```

Not every branch should become a decision call, and not every decision needs
AI. Flaggo is for contextual, high-change decisions where an explicit approved
interface is better than hard-coded logic plus manual operations.
