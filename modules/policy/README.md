# Policy Module

This current internal library implements decision-constraint evaluation for
Contract Service and Decision Service. Policy is not a first-class product
component or deployable service.

Constraint results use the current policy-named executable domain types with
stable reason codes. A blocked candidate cannot be returned as approved, and
constraint evaluation never performs activation or storage operations directly.
Collaborators are constructor-injected async ports.

## Current implementation

`src/Flaggo.Policy` owns `IPolicyEvaluator`, `PolicyEvaluationRequest`,
`PolicyDecision`, and `DefaultPolicyEvaluator`. The local evaluator enforces
pause, bounds, step, maximum delta, cooldown, evidence quality, model
uncertainty, expected outcome, and sample-size constraints. Reasoning receives
the evaluator through constructor injection.
Cooldown accepts every finite nonnegative value allowed by the frozen v1
contract. Invalid negative or nonfinite values use the stable
`invalid_cooldown` reason. Evaluation compares elapsed time instead of adding
seconds to persisted timestamps, so huge finite cooldowns cannot overflow. A
future `LastChangedAt` fails closed as `cooldown_active`, including when the
configured cooldown is zero.

Issue #40 owns executable type/module renaming. Update this document when
constraint composition or fallback authority changes.
