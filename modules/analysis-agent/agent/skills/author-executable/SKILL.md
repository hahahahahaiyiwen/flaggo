---
name: author-executable
description: Turn supported analysis into one valid Flaggo executable proposal.
---

# Author an executable

## Apply contract constraints

<executable_constraints>
Use the decision-contract objective, guardrails, attributes, result schema,
default, and authored baseline identified by `understand-decision-contract`.
</executable_constraints>

## Use supported expressions

<expression_syntax>
Candidate rules use `flaggo.cel/v1` expressions:

- predicates reference declared inputs as `attributes.ATTRIBUTE_NAME` and must return
  `bool`;
- return expressions must satisfy `result.schema`;
- literals, grouping, arithmetic, comparison, equality, boolean, and
  conditional operators are supported;
- `bool`, `double`, `int`, `string`, and `size` are supported;
- comprehensions, macros, regular expressions, time types, optionals,
  reflection, dynamic functions, and access outside `attributes` are not
  supported.

Prefer literal returns and simple predicates when they express the supported
change.
</expression_syntax>

## Build ordered rules

<executable_rules>
Create the smallest ruleset supported by the analysis:

- change only populations justified by the analysis;
- preserve independent safety and guardrail branches;
- account for first-match-wins ordering and the no-match default; and
- avoid unrelated cleanup, speculative branches, or broader extrapolation.

In the `executable_conclusion` section of
`/workspace/analysis/findings.md`, record the rule change, supporting evidence,
expected objective effect, guardrail reasoning, and uncertainty.
</executable_rules>

## Submit the Candidate

<candidate_submission>
Call `check_cycle_status` before submission. Then call
`propose_executable` with one complete `rules` array. The tool validates and
persists an inactive Candidate. Submit one proposal for the cycle and return
`candidate` only after the tool succeeds.
</candidate_submission>
