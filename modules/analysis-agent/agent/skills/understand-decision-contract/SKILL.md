---
name: understand-decision-contract
description: Interpret the accepted decision contract that governs one analysis cycle.
---

# Understand a decision contract

## Locate contract data

<contract_structure>
`/workspace/contract.json` contains cycle metadata and the accepted decision
contract:

- top-level fields identify the name, digest, authority, cadence, and scoped
  evidence sources;
- `contract` contains the accepted DecisionContract.

Use `contract` for authored semantics. Use the top-level digest and authority
for cycle identity and scope.
</contract_structure>

## Read the learning goal

<learning_goal>
Within `contract.learning`:

- `evidence` maps named observations to decision attributes and telemetry
  sources;
- `objective.primary` names the evidence to minimize or maximize;
- `objective.guardrails` defines conditions every executable must satisfy.

Analyze the primary objective and every guardrail. Do not replace an outcome
with the decision attribute used to produce that outcome.
</learning_goal>

## Read executable constraints

<contract_executable>
- `expression_syntax` identifies the expression profile.
- `attributes` defines valid runtime inputs and their schemas.
- `result.schema` constrains every returned value.
- `result.default` is returned when no rule matches.
- `authoredExecutable.rules` is an ordered, first-match-wins authored baseline.

The authored baseline is not proof of the currently active executable body.
Decision observations identify active executable digests, results, and matched
rules, but do not expose the body.
</contract_executable>
