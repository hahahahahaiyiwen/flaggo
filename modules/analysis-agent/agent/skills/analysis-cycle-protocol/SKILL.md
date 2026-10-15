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

## Collaborate through versioned artifacts

<workflow_protocol>
The task's `workflow-context` selects your profile, current state, writable
attempt directory and revision limit. It is authoritative; do not invent
a role sequence or edit the cycle's latest-handoff reference.

`analysisReady` starts with no draft. A proposer (or general agent) creates
a JSON draft in its `writeDirectory` containing a non-empty `rules` array,
analytical method, supporting evidence references, uncertainty and guardrail
reasoning. Save reviewable evidence; a live database query is not a frozen
review input. Return `draftReady`, not `candidate`. Proposers have no
`propose_executable` tool.

`draftReady` routes to a reviewer (or general agent). Read the bound draft
and saved evidence, independently assess method, relevance, objective and
guardrails, and save a JSON review under your own `writeDirectory` before
submitting. Do not overwrite the draft or another attempt's artifacts.
Independent review is your protocol responsibility, not a trusted approval
gate. Return `candidate` only after successful submission, `reject` with
`reviewPath` to end without a Candidate, or `requestChange` to request revision.
Do not request another revision once the bound limit is reached.

`requestChange` routes back to a proposer (or general agent). Read the review,
write a new versioned draft, increment `revision` by one, and return
`draftReady`. Never mutate the previously reviewed draft.

For `draftReady` and `requestChange`, supply an object `metadata` containing:
`outcome` (`draftReady` or `requestChange`), `proposalPath`, `proposalDigest`
(SHA-256 with `sha256:` prefix over the exact UTF-8 draft bytes),
`evidencePaths` (array), `evidenceDigests` (object mapping each path to its
SHA-256 content digest), and `revision` (initial draft is zero).
For saved SQL results, reuse the tool's `contentDigest`. For authored datasets,
hash the exact saved UTF-8 bytes. The runtime verifies these saved inputs.
All artifact paths are relative to `/workspace/analysis`, for example
`attempts/<attempt-id>/proposal.json`. `requestChange` preserves the current
proposal path, digest, evidence references/digests and revision, and adds `reviewPath`
pointing to your saved review. A revised draft uses a new proposal path.

A plain `handoff` has no metadata and checkpoints interrupted work without
advancing state. Your output is committed by the runtime after validation;
writing files alone does not advance state. Failure and shutdown preserve
the last valid state for a fresh attempt. Candidate submission seals all
analysis artifacts, including your saved review, into the existing manifest.
There is no separate review-commit tool or submitter profile.
</workflow_protocol>

## Return the attempt outcome

<cycle_outcome>
When proposing a change, load `author-executable`. Return one structured
outcome with a concise, non-empty explanation:

- `candidate` only after `propose_executable` succeeds;
- `draftReady` after saving a versioned draft and its metadata;
- `requestChange` after saving a review and preserving the reviewed version;
- `reject` with a saved `reviewPath` when the draft should not be admitted;
- `noChange` when evidence supports retaining the active executable;
- `noCandidate` when evidence cannot support a proposal;
- `handoff` after checkpointing unfinished work for a later attempt;
- `superseded` only after the coordinator reports `contractSuperseded`; or
- `failure` when this attempt fails.

Only the proposer/general profile may return `noChange` or `noCandidate`;
reviewers use `reject` instead. A returned `failure` terminates the cycle;
an execution error or checkpoint leaves it resumable. The runtime records
the outcome and decides whether the cycle completes or continues.
</cycle_outcome>
