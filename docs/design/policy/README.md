# Policy Design

## Purpose

Policy is the deterministic safety gate used by both decision lifecycles and
runtime decision execution. Lifecycle policy validates bundle candidates or
independent proposals before activation, while runtime policy determines
whether approved authority may be safely applied to a request.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

The MVP policy component should enforce:

- result type compatibility,
- number min/max bounds,
- number step alignment,
- max delta from an explicit contract baseline when configured,
- initial-authority target and inference-input compatibility,
- fallback when no safe candidate exists.

Conditional proposal-managed or evidence-backed constraints include:

- minimum evidence quality when evidence-backed decisioning is required,
- maximum model uncertainty when model-backed decisioning is required,
- minimum expected outcome when optimization estimates are used,
- minimum sample size when configured,
- explicit client-fallback permission for required-evidence unavailability.

Cooldown, previous-result delta, hysteresis, pause, and other temporal/operator
semantics require their separately approved contracts; they are not implied by
the Phase 3 bundle rule.

Policy must be provider-neutral and deterministic. It should not call an AI model in the MVP runtime path.

## Effective policy composition

Effective policy is the intersection of three layers:

```text
definition constraints
  ∩ environment policy
  ∩ operator controls
  = effective policy
```

Less-trusted or narrower layers may only narrow constraints, never widen them.
For example, a decision definition may request a smaller numeric range. Where
separate temporal or operator-control contracts exist, it may request a
stricter cooldown and an operator may pause or further limit rollout. But an
application-authored definition cannot raise environment maximums, bypass
approval requirements, lower mandatory evidence-quality floors, or override
operator authority.

When layers conflict, the safest applicable constraint wins or policy returns fallback/blocked with a stable reason code.

## Core port

```ts
interface IPolicyEvaluator {
  evaluate(input: PolicyEvaluationRequest): Promise<PolicyEvaluationResult>;
}

type PolicyEvaluationRequest = {
  definition: DecisionDefinition;
  state: DecisionState | null;
  evidence?: EvidenceSnapshot;
  candidate: {
    value: DecisionValue;
    decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
    strategyId?: string;
    reason: string;
  };
  now: string;
};
```

## Result shape

```ts
type PolicyEvaluationResult = {
  result: "approved" | "blocked" | "fallback";
  reasons: string[];
  appliedConstraints: string[];
};
```

Reason codes should be stable because clients, audit records, tests, and operator views may depend on them.

Reason-code vocabulary, including future conditional policies:

| Code | Meaning |
| --- | --- |
| `value_out_of_range` | Candidate is outside action-space or strategy bounds. |
| `invalid_step` | Numeric value does not align to configured step. |
| `max_delta_exceeded` | Candidate changes too much from the explicit contract baseline. |
| `cooldown_active` | A future temporal policy says the candidate change is too soon. |
| `insufficient_evidence_quality` | Evidence quality is below policy requirement. |
| `excessive_model_uncertainty` | Model uncertainty is above policy requirement. |
| `insufficient_expected_outcome` | Expected outcome estimate is below policy requirement. |
| `insufficient_sample_size` | Evidence sample size is below policy requirement. |
| `decision_paused` | Operator pause blocks adaptive decisioning. |
| `retired_contract` | Contract lifecycle prevents approved decisions. |
| `missing_state` | No permitted target has compatible active state. |
| `fallback_required` | No safe candidate can be approved. |

## Runtime behavior

```text
candidate value
  -> validate action space
  -> validate state and lifecycle
  -> validate applicable baseline delta
  -> validate evidence requirements only when declared
  -> return approved, blocked, or fallback
```

For runtime decision calls, policy failures should normally produce an audited
HTTP 200 response with `decisionMode = "fallback"` unless the request itself
is malformed or effective policy forbids governed fallback. If required
evidence is unavailable and governed fallback is forbidden, the service
returns `503 required-evidence-unavailable` with
`clientFallback.eligible: false`. Evidence and policy outcomes never authorize
SDK-local fallback.

## Tetris MVP policy

For `tetris.dropInterval`:

- min: `200`
- max: `1500`
- step: `50`
- max delta from contract baseline `actionSpace.default = 800`: `50`
- fallback: `800`

The strategy executor may calculate `750ms` or `850ms`, but policy is still
responsible for verifying the value before it is returned. This
bundle-authored rule has no evidence-quality or model-uncertainty requirement.
