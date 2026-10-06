---
name: evidence-analysis
description: Analyze one immutable Flaggo contract cycle and persist a durable supersession note, handoff, or terminal result.
allowed-tools:
  - check_analysis_status
  - commit_evidence_cutoff
  - describe_evidence
  - query_evidence
  - propose_executable
---

# Evidence analysis workflow

Version: 4

Work only on the contract, cycle, and workspace already bound to this
session. Trusted identity, evidence scope, cutoff watermark, Candidate
lifecycle, and control status are enforced by tools and must never be
reconstructed or overridden.

1. Read `/workspace/contract.json`, `/workspace/cycle.json`, any existing
   `/workspace/analysis` or `/workspace/handoffs` artifacts, and relevant
   completed cycles under the read-only `/workspace/history` tree.
2. Call `check_analysis_status` before starting work, before each evidence
   query or Candidate proposal, and immediately before selecting a terminal
   outcome.
   - On `contractSuperseded`, stop analysis, write a concise
     `/workspace/analysis/supersession.md` note describing reusable work and
     uncertainty, and return `superseded`.
   - On `serviceShutdown`, save a concise checkpoint under
     `/workspace/handoffs` and return `handoff`.
   The status is coordinator-owned; never infer supersession independently.
3. Call `describe_evidence` before designing queries.
4. Choose an RFC 3339 evidence cutoff no later than the current time and call
   `commit_evidence_cutoff` exactly once. A resumed attempt must reuse the
   committed cutoff.
5. Use `query_evidence` for bounded, read-only SQL over `observations`. Keep
   population boundaries and the contract's objective and guardrails explicit.
   Before choosing any terminal outcome, directly query the primary objective
   evidence and every guardrail evidence source named by the contract. Decision
   routing alone cannot establish objective or guardrail performance.
   The trusted scope includes the contract-declared evidence sources and the
   exact-digest `@flaggo/sdk` / `flaggo.decision.received` observations. Those
   decision observations identify the executable digest, result, result hash,
   evaluation source/rule, decision ID, and explicit correlation attributes.
   They do not contain the active executable body or decision inputs that were
   not explicitly emitted as correlation attributes. Treat correlation
   attributes as decision-time context, not outcomes. Use observation and
   metric data-point times to distinguish later evidence from the window that
   triggered a decision. Metric observations may be cumulative OTLP snapshots;
   do not sum repeated cumulative points as independent events. Use the latest
   point or changes between points for each correlated population. When a
   deterministic rule assigns different results at a numeric threshold,
   compare neighboring populations on either side as a testable
   regression-discontinuity hypothesis while retaining confounding and sample
   uncertainty.
6. Persist query rationale, findings, uncertainty, and reproducible analysis
   artifacts under `/workspace/analysis`. Save a concise handoff under
   `/workspace/handoffs` before yielding incomplete work.
7. Propose rules only when the evidence supports an executable that satisfies
   the exact contract. `propose_executable` can persist only one validated
   inactive Candidate. It cannot activate it. Contract Service independently
   fences Candidate admission against the current contract digest. A Candidate
   is a testable hypothesis, not a claim that observational evidence proves
   causality. Repeated, well-separated evidence may support a conservative
   Candidate when it distinguishes a plausible adjustment that improves the
   objective or guardrails. Record the uncertainty instead of withholding such
   a Candidate solely because it has not yet been activated and evaluated.
8. Finish with exactly one structured outcome:
   - `candidate` only after `propose_executable` succeeds;
   - `noChange` when sufficient evidence supports retaining the current
     executable; do not call `propose_executable`;
   - `noCandidate` when available evidence is insufficient to determine an
     appropriate Candidate;
   - `handoff` when durable work remains for a later agent or shutdown was
     requested;
   - `failure` for a terminal analysis failure;
   - `superseded` only after `check_analysis_status` reports
     `contractSuperseded`.

Return the selected outcome with one concise, non-empty `explanation`.

Do not use a shell, network, host filesystem, external package, or undeclared
tool. Do not claim statistical certainty that the recorded evidence does not
support.
