# Policy Design

## Purpose

Policy is the deterministic safety gate used by both decision lifecycles and runtime decision execution. Decision intelligence proposes bounded behavior; lifecycle policy determines whether it may become authority, while runtime policy determines whether approved authority may be safely applied to a request.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

The MVP policy component should enforce:

- result type compatibility,
- number min/max bounds,
- number step alignment,
- max delta from previous value,
- cooldown,
- minimum evidence quality when evidence-backed decisioning is required,
- maximum model uncertainty when model-backed decisioning is required,
- minimum expected outcome when optimization estimates are used,
- minimum sample size when configured,
- pause state,
- fallback when no safe candidate exists,
- explicit client-fallback permission for required-evidence unavailability.

Policy must be provider-neutral and deterministic. It should not call an AI model in the MVP runtime path.

## Effective policy composition

Effective policy is the intersection of three layers:

```text
definition constraints
  ∩ environment policy
  ∩ operator controls
  = effective policy
```

Less-trusted or narrower layers may only narrow constraints, never widen them. For example, a decision definition may request a smaller numeric range or stricter cooldown than the environment default, and an operator may pause or further limit rollout. But an application-authored definition cannot raise environment maximums, bypass approval requirements, lower mandatory evidence-quality floors, or override operator pause.

When layers conflict, the safest applicable constraint wins or policy returns fallback/blocked with a stable reason code.

## Lifecycle policy port

The current internal .NET lifecycle boundary is `ILifecyclePolicyEvaluator`.
Its request contains the typed proposal, replacement intent, trusted actor,
exact runtime and intelligence projections, environment/operator policy
context, scoped evidence snapshots, captured baseline, and current time.
`ILifecyclePolicyContextProvider` supplies policy independently of producers.

It returns `approved`, `limited`, `pending-approval`, `hold`, or `rejected`,
stable reasons, and the effective restrictions. Only explicit automatic
permission in both configured layers permits an automatic approval record.
Missing policy holds, conflicting intersections reject, and no non-approved
disposition grants state authority. `limited` never silently clamps.

Targets must be explicit and inside intersected kind/identity permissions.
Strategies validate every possible output against action bounds and numeric
grid, and every input against registered workflow/signal permissions.
Evidence must resolve to the exact definition/target and meet quality,
uncertainty, outcome, sample-size, freshness, and expiry constraints.

`maximumActivationDelta` and `minimumActivationIntervalSeconds` constrain
durable baseline changes and activation history. The definition's strategy
maximum delta still bounds outputs relative to the strategy's initial value.
This does not add per-request stabilization or change runtime cooldown
semantics. The lifecycle service re-evaluates before activation; state performs
the final recorded-approval and compare-and-swap checks.

## Runtime policy port

```ts
interface IPolicyEvaluator {
  evaluate(input: PolicyEvaluationRequest): Promise<PolicyEvaluationResult>;
}

type PolicyEvaluationRequest = {
  definition: DecisionDefinition;
  state: DecisionState | null;
  evidence: EvidenceSnapshot;
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
  clientFallback?: {
    requiredEvidenceUnavailable: "allow" | "forbid";
  };
};
```

Reason codes should be stable because clients, audit records, tests, and operator views may depend on them.

Initial reason codes:

| Code | Meaning |
| --- | --- |
| `value_out_of_range` | Candidate is outside action-space or strategy bounds. |
| `invalid_step` | Numeric value does not align to configured step. |
| `max_delta_exceeded` | Candidate changes too much from previous value. |
| `cooldown_active` | Candidate change is too soon after the prior decision. |
| `insufficient_evidence_quality` | Evidence quality is below policy requirement. |
| `excessive_model_uncertainty` | Model uncertainty is above policy requirement. |
| `insufficient_expected_outcome` | Expected outcome estimate is below policy requirement. |
| `insufficient_sample_size` | Evidence sample size is below policy requirement. |
| `decision_paused` | Operator pause blocks adaptive decisioning. |
| `retired_contract` | Contract lifecycle prevents approved decisions. |
| `missing_state` | Required state is unavailable. |
| `fallback_required` | No safe candidate can be approved. |

## Runtime behavior

```text
candidate value
  -> validate action space
  -> validate state and lifecycle
  -> validate cooldown and delta
  -> validate evidence requirements
  -> return approved, blocked, or fallback
```

For runtime decision calls, policy failures should normally produce an HTTP 200 response with `decisionMode = "fallback"` unless the request itself is malformed. If required evidence is unavailable and effective policy forbids governed fallback, the service returns `503 required-evidence-unavailable`.

That `503` does not authorize SDK-local fallback by status alone. Effective policy must separately set `clientFallback.requiredEvidenceUnavailable = "allow"`; omission means `forbid`. The Decision API projects the evaluated permission into the Problem Details `clientFallback.eligible` extension. Environment/operator policy may narrow an application request from allow to forbid, never widen forbid to allow.

## Tetris MVP policy

For `tetris.dropInterval`:

- min: `200`
- max: `1500`
- step: `50`
- max delta: `50`
- cooldown: `20s`
- fallback: `800`
- minimum evidence quality: `0.7` when evidence is required
- maximum model uncertainty: `0.35` when model-backed strategy is used

The strategy executor may calculate `850ms`, but policy is still responsible for verifying the value before it is returned.
