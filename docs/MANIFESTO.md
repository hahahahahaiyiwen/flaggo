# Flaggo manifesto

## Why Flaggo exists

Software increasingly uses AI to write, review, test, and ship code, yet most
runtime behavior is still governed by static branches, fixed thresholds,
configuration values, and manually operated control loops.

Flaggo explores a better primitive for selected runtime decisions:

> Given an explicit decision contract, current context, approved authority,
> and policy, what safe value should the application receive now?

The goal is not to replace code or let an unbounded model control production.
The application owns execution. Product and platform teams define intent,
boundaries, and accountability. Flaggo makes the delegated decision explicit,
governed, deterministic online, auditable, and capable of gaining a closed
loop when separately approved asynchronous intelligence is useful.

## Implementation principles

1. **Build the smallest complete end-to-end slice.** Prove one useful decision
   from declaration through approval, runtime use, audit, exposure, and outcome
   before expanding the platform.
2. **Use cohesive modules with explicit boundaries.** Separate definition,
   evidence, authority, runtime, policy, state, and audit responsibilities.
   Depend on small domain contracts rather than infrastructure details.
3. **Add contracts where real boundaries require them.** Public APIs,
   cross-process behavior, module ports, persisted state, and lifecycle
   concurrency need explicit contracts. Avoid abstractions, configuration, and
   indirection without a current consumer.
4. **Make security and policy proportional and explicit.** Model the risks and
   requirements the product actually has. Authenticated approval, credential
   separation, fail-closed identity validation, durable audit, and mandatory
   policy remain non-negotiable where the contract requires them; generalized
   workflow machinery does not.
5. **Define failure, fallback, audit, and authorization behavior.** Do not use
   broad catches, silent defaults, success-shaped errors, or ambiguous
   authority. A returned decision must be reconstructable.
6. **Remove obsolete paths.** When the accepted design changes, update callers
   and delete superseded contracts and documentation instead of adding
   compatibility layers or parallel sources of truth.
7. **Test behavior at meaningful boundaries.** Cover expected outcomes, edge
   cases, failures, concurrency, replay, persistence, and real integration
   seams. Prefer simple test doubles and integration tests where contracts meet
   actual systems.

## Product boundary

Online execution must remain deterministic, bounded, policy-gated, and
compatible with approved `GovernedDecisionState`. It must never invoke an
unbounded request-time agent loop.

AI or other analysis belongs primarily in an asynchronous proposal path:

```text
evidence + outcomes + objectives
  -> bounded proposal
  -> governance
  -> governed state
  -> deterministic runtime execution
```

Not every branch should become a decision call, and not every decision needs
AI. Flaggo is for contextual, high-change, policy-constrained decisions where
an explicit governed interface is better than hard-coded logic plus manual
operations.
