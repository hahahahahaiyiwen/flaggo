# Policy Module

Owns effective-policy resolution and safety evaluation for proposed or runtime
decisions.

Policy results are explicit domain types with stable reason codes. A blocked
candidate cannot be returned as approved, and policy never performs rollout or
storage operations directly. Collaborators are constructor-injected async
ports.

## Current implementation

### Lifecycle policy

`ILifecyclePolicyEvaluator` and `DefaultLifecyclePolicyEvaluator` review
proposed authority independently of runtime policy. They consume typed runtime
and intelligence definition projections, trusted actor scope, a current
baseline, scoped evidence snapshots, and an `ILifecyclePolicyContextProvider`.
Neither producer source labels nor configuration omissions grant permission.

The effective policy intersects definition constraints, environment policy,
and operator controls: ranges and target allowlists shrink, evidence floors
rise, uncertainty caps fall, approval requirements strengthen, and pause wins.
Missing policy holds; incompatible or empty intersections reject. Numeric
strategies must use permitted registered inputs and every output must satisfy
the definition's grid, action bounds, and strategy-baseline delta.
`limited` returns the concrete effective restrictions without modifying the
submitted value or granting activation.

Automatic approval requires explicit permission in both configured layers.
Human-required policy returns `pending-approval`; it does not materialize an
approval. Evidence references resolve to exact definition/target snapshots;
missing, expired, future, old, or insufficient evidence cannot approve.

`MaximumActivationDelta` and `MinimumActivationIntervalSeconds` apply to
durable baseline changes and activation history. They do not reinterpret
runtime `MaximumDelta` or `CooldownSeconds`, or add per-request history.
Lifecycle policy returns a disposition and stable reasons; lifecycle
orchestration and the atomic state boundary own approval and activation.

### Runtime policy

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

Update this document when policy composition, constraints, or fallback
authority changes.
