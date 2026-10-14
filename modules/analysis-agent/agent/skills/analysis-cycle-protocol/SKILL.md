---
name: analysis-cycle-protocol
description: Understand analysis-cycle identity, history, resumption, and outcomes.
---

# Analysis cycle protocol

## Identify the cycle

<cycle_context>
A workspace retains cycles for one contract name. Each cycle binds one
accepted contract digest and its authority/source scope; an attempt is one
execution of that cycle, not a new cycle.

Use the bound task identity and read `/workspace/cycle.json` and
`/workspace/contract.json`. Load `understand-decision-contract` for authored
semantics, then `qualitative-analysis` for analytical method. The recorded
contract and cycle identity are authoritative.
</cycle_context>

## Find work products and resume

<cycle_history>
Current work products are under `/workspace/analysis`: findings in
`findings.md`, query artifacts/history in `queries/`, execution records in
`executions/`, and other scripts/modeling outputs. Checkpoints are under
`/workspace/handoffs`.

Other cycles are available read-only under `/workspace/history/<cycle-id>`,
with the same relative layout. Inspect their `contract.json`, `cycle.json`,
`outcome.json` when present before reusing work. Historical cycles may have
different contract digests and evidence windows; saved datasets retain their
original scope and query times. Files alone do not establish a completed outcome.

On resumption, read existing findings, artifacts and checkpoints first.
Continue unfinished work using saved datasets, or deliberately refresh evidence
when needed. Live queries can see newly materialized observations; resumption
does not restore a database snapshot. Reuse historical work only after checking
its applicability to the current contract and analysis.
</cycle_history>

## Check status and checkpoint

<cycle_status>
Use `check_cycle_status` at natural boundaries, especially before submission.
On `contractSuperseded`, stop analysis, complete the tagged sections in
`/workspace/analysis/supersession.md`, and return `superseded`. On
`serviceShutdown`, checkpoint unfinished work in
`/workspace/handoffs/checkpoint.md` and return `handoff`.

Only the coordinator status establishes supersession. The runtime owns cycle
control records; do not edit them.
</cycle_status>

## Record the analysis

<analysis_record>
Before returning `candidate`, `noChange`, or `noCandidate`, complete every
tagged section in `/workspace/analysis/findings.md` without removing its
headings or XML tags. Record supporting evidence, interpretation, uncertainty,
guardrail reasoning and the executable conclusion.
</analysis_record>

## Return the attempt outcome

<cycle_outcome>
When proposing a change, load `author-executable`. Return one structured
outcome with a concise, non-empty explanation:

- `candidate` only after `propose_executable` succeeds;
- `noChange` when evidence supports retaining the active executable;
- `noCandidate` when evidence cannot support a proposal;
- `handoff` after checkpointing unfinished work for a later attempt;
- `superseded` only after the coordinator reports `contractSuperseded`; or
- `failure` when this attempt fails.

Handoff or failure can leave the same cycle resumable. The runtime records
the outcome and decides whether the cycle completes or continues.
</cycle_outcome>
