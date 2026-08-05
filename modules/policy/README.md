# Policy Module

Owns effective-policy resolution and safety evaluation for proposed or runtime
decisions.

Policy results are explicit domain types with stable reason codes. A blocked
candidate cannot be returned as approved, and policy never performs rollout or
storage operations directly. Collaborators are constructor-injected async
ports.

## Current implementation

`src/Flaggo.Policy` owns `IPolicyEvaluator`, `PolicyEvaluationRequest`,
`PolicyDecision`, and `DefaultPolicyEvaluator`. The local evaluator enforces
pause, bounds, step, maximum delta, cooldown, evidence quality, model
uncertainty, expected outcome, and sample-size constraints. Reasoning receives
the evaluator through constructor injection.

Update this document when policy composition, constraints, or fallback
authority changes.
