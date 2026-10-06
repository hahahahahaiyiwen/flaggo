---
name: analysis-cycle-protocol
description: Understand and safely execute one bounded Flaggo analysis cycle.
---

# Analysis cycle protocol

## Load the cycle

<cycle_context>
1. Read `/workspace/contract.json` and `/workspace/cycle.json`.
2. Read `/workspace/cutoff.json` when it exists, relevant files under
   `/workspace/analysis` and `/workspace/handoffs`, and useful completed cycles
   under `/workspace/history`.
3. Treat the recorded identity, contract digest, authority, objective,
   guardrails, and data sources as authoritative.
4. Load `understand-decision-contract`, then `qualitative-analysis`.
</cycle_context>

## Check cycle status

<cycle_status>
Use `check_cycle_status` occasionally during long analysis and at natural
boundaries such as before authoring an executable.

- On `contractSuperseded`, stop, complete the `reusable_work` and
  `remaining_uncertainty` sections in
  `/workspace/analysis/supersession.md`, then return `superseded`.
- On `serviceShutdown`, complete the `completed_work` and `next_step` sections
  in `/workspace/handoffs/checkpoint.md`, and return `handoff`.

Only `contractSuperseded` returned by `check_cycle_status` establishes
supersession.
</cycle_status>

## Establish the evidence snapshot

<evidence_snapshot>
Call `describe`, then `commit_cutoff` before the first `run_sql`. Reuse the
cutoff in `/workspace/cutoff.json`; otherwise commit an RFC 3339 time no later
than now. All queries use that immutable cycle snapshot.
</evidence_snapshot>

## Record the analysis

<analysis_record>
Before returning `candidate`, `noChange`, or `noCandidate`, complete every
tagged section in `/workspace/analysis/findings.md` without removing its
headings or XML tags:

- `questions_and_queries`;
- `observed_findings`;
- `interpretation_and_uncertainty`;
- `objective_and_guardrails`; and
- `executable_conclusion`.
</analysis_record>

## Finish the cycle

<cycle_outcome>
When evidence supports changing the executable, load `author-executable` and
follow it before returning `candidate`.

Return exactly one structured outcome with a concise, non-empty explanation:

- `candidate` only after `propose_executable` succeeds;
- `noChange` when sufficient data supports retaining the active executable;
- `noCandidate` when the data is insufficient to choose an executable;
- `handoff` when durable work remains or shutdown was requested;
- `superseded` only after `check_cycle_status` returns
  `contractSuperseded`; or
- `failure` for a terminal analysis failure.
</cycle_outcome>
