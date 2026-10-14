---
name: qualitative-analysis
description: Determine whether bounded evidence supports an explainable executable change.
---

# Qualitative data analysis

## Frame the question

<analysis_question>
Translate the contract into questions that can be answered from evidence:

- What observable outcome represents the objective?
- Which observations measure each guardrail?
- Which decision-time attributes define comparable populations?
- Which executable digest, result, and rule were applied to each population?
- What evidence would support changing the executable, retaining it, or
  concluding that no Candidate can be chosen?

Do not invent targets, guardrails, correlations, or decision inputs that the
contract and data do not provide.
</analysis_question>

## Check evidence quality

<evidence_quality>
- count records by source, kind, and time range;
- identify missing, malformed, or uncorrelated records;
- verify that compared populations use compatible windows and definitions;
- distinguish decision exposure from later outcome data; and
- retain source, time, and population boundaries.

`@flaggo/sdk` / `flaggo.decision.received` records describe a decision: its
executable digest, result, result hash, evaluation source or rule, decision ID,
and explicitly emitted correlation attributes. They do not contain the active
executable body or decision inputs that were not emitted. Treat correlation
attributes as decision-time context, not as outcomes.
</evidence_quality>

## Compare outcomes

<comparative_analysis>
- compare like populations exposed to different executable results;
- treat different branches within one executable as valid comparative
  exposure;
- around numeric thresholds, compare neighboring populations on both sides;
- use time ordering to ensure an outcome could follow the associated decision;
- prefer repeated, separated observations over one event;
- analyze the objective and every guardrail directly; and
- test whether time, population mix, missing correlations, or another observed
  factor explains the difference.

Metric records can be cumulative OTLP snapshots. Do not sum repeated
cumulative points as independent events. Use the latest comparable point or a
delta between ordered points within the same metric stream and population.

An independently emitted post-decision measurement of the objective is not
circular merely because the executable result directly affects it. Do not
substitute the decision result for outcome evidence, but do not discard a
measured outcome because the executable has a strong effect on it.
</comparative_analysis>

## Choose the analysis method

<query_strategy>
Use `describe` to discover available relations, columns and query limits.
Prefer `run_sql` for aggregation and comparisons expressible as a query.
Use `code_interpreter` for Python inspection or modeling when SQL is
insufficient. Choose the smallest analysis that answers the question while
retaining source, population and time boundaries.

Evidence is live and may include late-materialized events. Choose explicit
time windows and a suitable lag for the question; an older cutoff reduces
late-arrival risk but does not guarantee completeness. Record those choices
and reconsider comparisons when refreshing datasets changes their populations.

Samples support exploration, not automatic claims about the full population.
Use explicit ordering for reproducible samples, account for selection bias,
and do not infer an exact population size from a limited result. Seek broader
evidence when the sample is insufficient. Record modeling assumptions,
intermediate findings and material uncertainty.
</query_strategy>

## Reach an executable conclusion

<executable_decision>
1. State what the queried rows directly show.
2. State the executable hypothesis that could explain those observations.
3. Identify competing explanations and material missing data.
4. Determine whether the hypothesis achieves the objective while satisfying
   every guardrail.
5. Conclude that the evidence supports a narrow executable change, retaining
   the active executable, or no Candidate.

An executable proposal is a testable hypothesis, not proof of causality.
Change only populations supported by evidence. Do not claim unsupported
certainty or extrapolate beyond observed populations. Lack of causal proof or
a previously deployed alternative does not by itself block a narrow change
when repeated comparisons support it and every guardrail remains satisfied.
</executable_decision>
