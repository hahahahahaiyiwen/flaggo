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
For `analysisReady` or `requestChange`, save rules and analytical support as a
versioned draft in the task's attempt directory and return `draftReady` with
the metadata described by `analysis-cycle-protocol`. Do not submit directly.

For `draftReady`, independently review the saved draft and evidence. Save your
JSON review in your own attempt directory before submitting the bound rules.
If unsupported, return `reject` or a bounded `requestChange` instead.

Submit the supported rules through `propose_executable`; Candidate admission
is not activation. An identical retry returns the original Candidate;
a changed sealed submission conflicts. Never treat conflict as permission to
alter or replace a previously admitted Candidate. Follow `analysis-cycle-protocol` for status checks,
checkpointing and the attempt outcome.
</candidate_submission>
