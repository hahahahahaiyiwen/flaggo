# Async Analysis

## Purpose and status

Async Analysis turns materialized evidence into inactive Candidate executables
outside the runtime request path. Contract Service retains Candidate validation
and activation authority.

This document describes the metadata-driven workflow handoff model implemented
for #73 on top of #72's live evidence capabilities. It does not introduce a fixed
proposer/reviewer sequence into the coordinator.

See [Application boundaries](APP_BOUNDARIES.md) for service ownership,
[Evidence and learning](EVIDENCE.md) for evidence semantics, and the
[application README](../../../apps/async-analysis/README.md) for configuration
and execution limitations.

## Components and ownership

| Component | Responsibility |
| --- | --- |
| Analysis service workers | Periodically discover work and execute coordinator runs |
| Coordinator | Discover current contracts, claim workspaces, prepare or resume cycles, acquire sessions, handle shutdown/supersession, and record outcomes |
| Workspace provider | Provide durable contract-name workspaces, exclusive claims, cycle records, artifacts, history, and recovery |
| Agent pool | Bound concurrent session leases using configured capacity |
| Agent provider | Select task/profile and enforce operations/outcomes from committed handoff metadata, then create a session through the concrete adapter |
| Agent session | Perform one assigned execution and return a structured outcome |
| Analysis tools | Expose trusted scoped SQL, local Python execution, status checks, discovery, and Candidate submission |

The coordinator owns the workspace claim. Agents do not independently claim the
workspace. An agent-pool lease and a workspace lease are different resources:
one reserves execution capacity, the other prevents competing owners from
advancing the same contract's analysis.

The production composition uses a Copilot agent provider. The deterministic
Tetris provider implements the same provider interface without model execution.
The provider creates sessions; it does not currently scan contracts or
workspaces.

## Workspace, cycle, and attempt

**Workspace:** durable history for one contract name. Its stable identity is
derived from that name, not the current contract digest. Contract replacements
therefore retain prior cycles in the same workspace.

**Workspace claim:** exclusive ownership enforced by an in-process claim and
an OS file lock. `claim.json` records the claimant but is not the lock itself.
This is not a time-expiring distributed lease with heartbeats. Ownership lasts
while the lease is retained; releasing it or process exit releases the lock.

**Cycle:** one logical analysis bound to an exact accepted contract digest,
authority/source scope, and analysis profile. A cycle may span several attempts.
Its durable control status is currently `active`, `superseding`, or `terminal`.

**Attempt:** one execution with a new attempt identity. Recovery resumes the
same unfinished cycle with a new attempt and a fresh agent session. It uses
durable work products, not restored conversational memory.

The current physical layout is:

```text
<workspace-root>\<contract-name-hash>\
  workspace.json
  claim.lock
  claim.json
  schedule.json
  current-cycle.json
  cycles\<cycle-id>\
    cycle.json
    budget.json
    contract.json
    analysis\
      findings.md
      queries\
      executions\
      attempts\<attempt-id>\     (versioned drafts, reviews and modeling files)
    attempts\
    handoffs\
    audit\
    outcome.json                 (when terminal)
    analysis-manifest.json       (when sealed)
```

Agents see a virtual filesystem rooted at `/workspace`: current cycle records,
analysis artifacts and handoffs are projected there, and other cycles appear
read-only under `/workspace/history/<cycle-id>`. Trusted runtime code owns
cycle control records. Agents write only through the supported writable
filesystem areas; this adapter restriction is not a Python sandbox.

## Discovery and scheduling today

```text
Worker wakes
  -> refresh learning-contract catalog
  -> read current contracts in catalog order
  -> try to claim a contract-name workspace
  -> resume an unfinished cycle or check new-cycle eligibility
  -> acquire an agent-pool slot and create a session
  -> run, record result, close session, and release ownership
```

Discovery is contract-driven, not a scan of every directory. Workers refresh the
catalog conditionally using its revision and inspect workspace state when
processing a current contract. Busy workspaces and contracts without declared
evidence sources are skipped.

`FLAGGO_ANALYSIS_MAX_AGENTS` controls both worker count and pool capacity;
the default is one. Each worker polls at
`FLAGGO_ANALYSIS_POLL_INTERVAL_MS`, default five seconds, with an immediate first
tick. It awaits its selected execution before returning to discovery. If all
workers are executing, no independent discovery worker scans in the background.
The same configured interval is used for current-contract supersession checks
during an execution.

A new cycle first becomes eligible after the contract's evaluation interval
following acceptance. Subsequent cycles are scheduled from terminal completion.
An unfinished cycle for the same digest resumes without waiting for another
evaluation interval. A changed digest supersedes the old cycle.

## Outcomes and existing handoff

| Agent outcome | Current cycle behavior |
| --- | --- |
| `candidate` | Terminal; requires a successful proposal tool call |
| `noChange` | Terminal; evidence supports retaining the active executable |
| `noCandidate` | Terminal; no supported Candidate can be produced |
| `failure` | Terminal failure returned by the agent |
| `handoff` | Persist checkpoint and leave the cycle unfinished/recoverable |
| `superseded` | Terminal only when supported by coordinator-owned supersession |

A provider/tool execution error differs from an agent-returned `failure`:
the coordinator can record an attempt failure and leave the cycle recoverable.
Shutdown likewise preserves unfinished work rather than reporting completion.

`Handoff { summary, metadata }` writes a runtime-owned JSON record under
`handoffs\<attempt-id>.json` with schema version, attempt identity, recording
time, summary, optional workflow metadata, cycle/digest binding and prior-handoff
reference. The coordinator closes the session and returns a recoverable
result without completing the cycle. A later claim can continue it.

Metadata-free handoffs are checkpoints. Workflow handoffs publish
`cycle.json.latestHandoff`; the next session specification carries that exact
record. An absent reference denotes the initialized `AnalysisReady` state,
not a search for the newest file.

## State-driven workflow handoff

The existing handoff carries an explicit workflow outcome and artifact
metadata. The coordinator passes persisted workflow state to the provider, which
selects an agent and enforces its permitted operations. There is no persisted
session plan, separate submitter profile, workflow engine, or role-specific
coordinator sequence.

The cycle records what work has been completed, not which profile must run next.
One agent executes per contract workspace at a time. The provider can choose a
specialized or general agent without changing the cycle's identity.

### Profiles and outcomes

| Profile | Workflow outcomes | Candidate tool |
| --- | --- | --- |
| Proposer | `DraftReady`, `NoChange`, `NoCandidate` | Not provided |
| Reviewer | `Candidate`, `Reject`, `RequestChange` | `propose_executable` provided |
| General | All outcomes above, subject to work-state transitions | `propose_executable` provided in `DraftReady` |

All profiles can checkpoint interrupted work, report failure, and acknowledge
coordinator-signaled supersession. The provider validates session results
against the permissions actually granted, not an agent-declared profile name.
The general profile supports either function. The agent protocol requires
independent review in a different attempt from draft creation; this does not
require different model vendors or models. For this trusted-local iteration,
review authorship and independence are protocol responsibilities, not additional
trusted submission gates.

| Workflow outcome | Meaning and lifecycle effect |
| --- | --- |
| `DraftReady` | A versioned draft and supporting artifacts are ready; persist a handoff and leave the cycle unfinished |
| `RequestChange` | Review findings request a revised draft; persist a handoff and leave the cycle unfinished |
| `Candidate` | Review is saved and submission of the exact reviewed draft succeeded; use the existing Candidate outcome |
| `Reject` | Review rejects the proposal without revision; complete without a Candidate, preserving the rejection decision |
| `NoChange` | Complete with the existing no-change outcome and supporting findings |
| `NoCandidate` | Complete with the existing no-Candidate outcome and supporting findings |

`Reject` maps to the existing terminal no-Candidate outcome, with the structured
review and provider-written attempt `rejection.json` retaining the distinction.
The provider normalizes this result before returning it to the coordinator;
the coordinator does not interpret the review decision. `Candidate` cannot be returned merely because
the agent intends to call the submission tool. A failed submission is not a
Candidate completion; its recovery follows the existing idempotent submission
path.

The cycle starts with explicit `AnalysisReady` workflow state. `DraftReady`
normally leads to assessment and `RequestChange` normally leads to revision,
but the provider chooses the agent profile. These are work-state transitions,
not a persisted proposer/reviewer ordering.

### Workflow state and transition table

Workflow state describes the last completed work step, not the executing
profile. It is separate from lifecycle status: a cycle with `DraftReady` work
is still `active`; a successful `Candidate` result makes it `terminal`.

A new cycle has no published workflow handoff, explicitly denoting `AnalysisReady`.
Thereafter the latest published workflow
handoff is the authoritative nonterminal state. Do not maintain an independently
editable copy of its outcome elsewhere in the cycle. Terminal lifecycle and
result remain recorded through the existing completion mechanism.

The following is the implemented transition contract. A transition must satisfy
both this table and the executing profile's allowed outcomes. The provider owns
these checks and returns generic handoff/completion results to the coordinator.

| Current work state | Result/event | Next work or completion |
| --- | --- | --- |
| `AnalysisReady` | `DraftReady` | `DraftReady`, with draft and evidence references |
| `RequestChange` | `DraftReady` | `DraftReady`, with a new draft version and retained prior review |
| `DraftReady` | `RequestChange` | `RequestChange`, with findings and the referenced draft; revision bound applies |
| `DraftReady` | `Reject` | Terminal no-Candidate result, preserving structured rejection |
| `DraftReady` | `Candidate` after successful submission | Terminal Candidate result |
| Any valid nonterminal work state | `NoChange` or `NoCandidate` from an allowed profile | Corresponding terminal result with findings |
| Any valid nonterminal work state | Shutdown/checkpoint or recoverable execution error before sealing | Same work state; record checkpoint/failure and resume unfinished work |
| Any valid nonterminal work state | Agent-returned `failure` or exhausted cycle limits | Terminal failure |
| Any unfinished cycle | Coordinator-confirmed contract replacement | Terminal superseded outcome through the existing lifecycle path |
| Missing/corrupt persisted work state | Attempt to resume | Explicit integrity failure; do not route an agent from a guessed state |
| Terminal cycle | Discovery | No further agent work in this cycle; ordinary new-cycle scheduling applies |

Results not permitted by the state/profile combination fail explicitly without
publishing a transition. Such attempt failures consume the cycle's durable
limits. Returning a no-Candidate/no-change result after successful submission
remains an invalid contradiction; recovery must preserve the admitted Candidate.
Sealed-submission recovery below takes precedence over ordinary work routing,
regardless of the last handoff.

### Handoff and checkpoint contract

Runtime completed-step payload (metadata outcome names use camelCase):

```json
{
  "kind": "handoff",
  "summary": "Draft prepared for independent assessment.",
  "metadata": {
    "outcome": "draftReady",
    "proposalPath": "attempts/attempt-123/proposal.json",
    "proposalDigest": "sha256:...",
    "evidencePaths": ["queries/query-123.json"],
    "evidenceDigests": {"queries/query-123.json": "sha256:..."},
    "revision": 0
  }
}
```

Artifact paths are relative to the cycle analysis directory. The runtime binds
the record to the producing cycle, attempt and contract; agent metadata cannot
replace those identities. A draft's author attempt comes from this envelope,
not a claim inside the proposal file.

Metadata is an object of at most 16 KiB. Its strict schema requires `outcome`,
`proposalPath`, `proposalDigest`, `evidencePaths`, `evidenceDigests` and `revision`, with optional
`reviewPath`; unknown fields fail. Initial drafts use revision zero; revisions
increment it by one. `RequestChange` preserves the reviewed proposal path,
digest, evidence references/digests and revision, and requires a new review path.
`evidenceDigests` maps every saved evidence path to exactly one SHA-256 digest.
The provider verifies those saved bytes before routing or committing a transition.
Draft JSON requires a non-empty `rules` array. Review JSON requires an object;
the agent protocol supplies the decision, method, draft identity and reasoning,
without turning those fields into a trusted approval gate.

The Copilot final-result schema is flat, avoiding unsupported schema unions:
`outcome`, non-empty `explanation`, optional object `metadata` and optional
string `reviewPath`. `draftReady` and `requestChange` map to runtime handoffs;
`reject` requires `reviewPath`. A plain `handoff` has no workflow metadata.

A completed workflow step requires a valid outcome and its required metadata.
Absent, malformed, or unsupported state is an explicit integrity failure, not
default-agent routing and not supersession. Supersession means that the bound
contract was replaced.

An interrupted attempt is different from a completed step. On shutdown or
recoverable provider failure, preserve the last valid workflow state and save
checkpoint artifacts; do not manufacture a workflow transition. On the next
claim, the provider continues that unfinished work from the same state with a
fresh attempt. A newly initialized cycle's `AnalysisReady` state makes the first
execution explicit. No persisted task plan or profile is needed for recovery.

### Responsibility and permissions

```text
Coordinator claims workspace and loads cycle + latest workflow handoff
  -> provider selects/configures an agent from the state
  -> session executes with enforced tool/path permissions
  -> provider validates its structured result
  -> workspace records handoff/state or coordinator records terminal outcome
  -> close session and release workspace
```

The coordinator handles generic claims, execution limits, handoff persistence,
terminal completion, current-contract checks and cancellation. It does not
branch on proposer, reviewer, or individual review decisions.

The provider interprets the workflow protocol, chooses task/profile, restricts
tools and writable paths, and validates resulting transitions. The workspace
provider owns durable storage and the generic record envelope. SDK-specific
session creation remains an adapter.

The coordinator can continue supplying base scoped capabilities. The provider
exposes only permitted operations through restricted tool/filesystem adapters;
runtime handlers enforce the restrictions. It must not give a proposer access
to an unrestricted submission handler. Prompt instructions or hidden tool names
alone are not enforcement. No two-phase session-plan API is required.

The cycle need not store a selected profile. Any actual profile/permissions
recorded with an attempt are execution provenance, not routing instructions.
The cycle's fixed `analysis_profile` remains sealed skill-list provenance, not
a routing constraint. Recovery no longer rejects a changed selected skill list;
the role profile is chosen per attempt and observed through
`flaggo.analysis.profile.selected`.
The session-start action span carries that role, and
`flaggo.analysis.profile.selections` counts selections with only the three-value
profile dimension; exact contract/cycle/attempt/artifact identities remain out
of metric dimensions.

Native writes are restricted to the executing attempt's
`/workspace/analysis/attempts/<attempt-id>` directory, shared lifecycle notes,
and SDK-internal `.copilot` state. Draft/review transitions require new artifacts
in that attempt directory. Prior attempts remain read-only through these tools;
trusted SQL/Python capabilities still persist their own query/execution artifacts.

### Draft, review, and submission

Drafts use native file creation/editing, not a new draft tool. They include
rules, hypothesis, evidence references, uncertainty and guardrail assessment.
`DraftReady` validates the artifacts and records their version/digest without
sealing the whole cycle. Revisions produce new artifacts and retain prior drafts
and reviews.

The reviewer or general agent saves a structured review in the cycle before
calling `propose_executable`. The agent protocol requires it to identify the
reviewed draft and explain the submission decision. The tool does not add a
review-authorship or approval-record validation gate in this iteration.

Submission retains tool permission enforcement, existing proposal/contract
validation and cycle integrity checks. It checks that the bound contract digest
is still current; Contract Service checks currentness again at admission, so a
local preflight check is not the authority against a replacement race.

The review artifact is included in the existing sealed analysis manifest at
submission. There is no additional approval-store write, review-commit tool, or
approval pointer. The existing proposal seal/submission/recovery path remains
the finalization boundary. Once sealed, recovery retries that exact submission,
not a new revision. The agent protocol requires another review of a changed
draft before finalization.

Contract Service still validates and admits the inactive Candidate; only its
separate activation authority can make the executable active. Tool restrictions
do not imply host isolation against unsandboxed Python.

### Submission idempotency

Reuse the existing workspace proposal journal and Contract Service admission
identity. No new idempotency key, submission store, or approval transaction is
needed.

| Submission condition | Tool behavior |
| --- | --- |
| No sealed proposal yet, valid rules and current contract | Persist/seal the submission, call Contract Service, and record the Candidate |
| Same rules as the sealed proposal, Candidate recorded locally | Return the original Candidate result without creating another |
| Same rules as the sealed proposal, local result missing | Retry the original persisted request and record the returned Candidate |
| Different rules from the sealed proposal | Explicit proposal conflict; never replace the sealed submission |
| Bound digest superseded | Explicit stale-contract/supersession error, including on the local cached-result path |

The original request retains its attempt ID and analysis-manifest digest across
recovery attempts. Contract Service admission is keyed by `(workspaceId,
cycleId)`: matching request content returns the existing admission with
`created: false`, and conflicting content returns a conflict. A new session's
attempt identity must not replace the original submission provenance.

If admission succeeded but its response was lost, retrying the identical
request obtains the original result. Multiple HTTP requests are allowed; multiple
different admissions for the same cycle are not. At the tool's local cached
path, returning the original stored result is sufficient; `created` describes
that recorded response, not a promise that this invocation created something.

On recovery, a sealed/pending submission or recorded Candidate is reconciled
before ordinary workflow routing. Resume the exact submission without rewriting
sealed findings or review files, then complete the cycle from its result. A
successful tool call repeated in the same attempt is likewise idempotent.
Currentness checks still apply; supersession ends unfinished cycle work rather
than undoing an existing admission.

Required cases include identical repeat, changed-rules conflict, lost response,
restart with a new attempt, and supersession on both remote and cached paths.
Error classifications must distinguish proposal conflict, stale contract and
transient transport failure.

### Handoff persistence and recovery

Retain per-attempt handoff records and an authoritative latest-workflow-handoff
reference in runtime-owned cycle state. Persist validated artifacts and handoff
before publishing the reference. Readers never infer workflow order from file
timestamps or directory enumeration. This extends existing handoff persistence;
it is not a separate review commit or second workflow journal.

Publishing the reference makes a completed step visible to the next attempt.
An unreferenced record is interrupted work, not a completed transition. A missing
or corrupt referenced record fails explicitly rather than falling back.
Checkpoint records do not replace the last completed workflow transition.
Recovery preserves the last valid state and does not implicitly replay completed
steps; interrupted execution may retry. Candidate submission retains its own
idempotency.

### Durable cycle limits

Keep the existing global concurrency limit and one executor per workspace.
Additionally bound total cycle attempts, proposer revisions and accumulated
execution time across handoffs and failures. Reserve attempt/time allowance
before starting a session; settle actual usage on completion and conservatively
charge the reserved allowance after an unclean crash. Downtime is not execution
time. Limit exhaustion records an explicit terminal failure with artifacts.

Defaults are eight attempts, one revised draft, and sixty minutes accumulated
execution. Each attempt reserves up to the configured attempt timeout (default
twenty minutes), capped by the remaining cycle allowance. Failed starts and
execution errors consume attempts; ordinary completion refunds unused time,
while an unclean crash retains the full reservation charge. Runtime budgets are
control records excluded from sealed analysis provenance.

No token/provider-usage ceiling is configured or claimed: the SDK does not
provide the required accounting/enforcement. Shutdown and supersession
must reach the active session regardless of profile.

Sealed-submission reconciliation does not start an agent and precedes these
agent-execution limits, including after the last allowed attempt loses its
admission response. It retries only the original submission under the existing
HTTP timeout/currentness fencing; it cannot create another draft or model
session. This preserves an admitted Candidate rather than misclassifying it as
an exhausted-agent failure. Transport-only retries are not token usage or
additional agent attempts.

## Evidence shared across attempts and roles

SQL reads live scoped evidence. There is no cycle-wide timestamp cutoff or
ingestion watermark. Agents choose analytical windows and materialization lag;
query execution times do not certify evidence completeness.

Handoffs reference the saved datasets actually used, along with their scope and
digests where applicable. Another agent can inspect the same saved inputs or
deliberately obtain fresh evidence through SQL. This preserves reviewable work
without restoring the frozen-snapshot design removed in #72.

Saved artifacts are durable inputs, not universal immutability guarantees.
Provider validation verifies handoff artifact references/digests; agents use
these references to assess the same saved work. Review correctness is not an
additional admission guarantee. Python remains trusted-local
and unsandboxed; workspace projection and role capability controls do not claim
host filesystem, network, credential, or subprocess isolation.

## Implementation boundary for #73

The agreed direction is to extend `AgentRunOutcome::Handoff`, its durable record,
cycle preparation/session context, and provider routing rather than add a
role-specific coordinator state machine.

The local implementation uses `WorkflowAgentProvider` around either the Copilot
or deterministic provider. It validates state and referenced artifacts before
starting the inner session, and validates outcomes before publishing a handoff.
The deterministic Tetris provider now drafts from saved evidence, then independently
reproduces that analysis in a fresh review attempt before submission.

Current code anchors:

- `apps/async-analysis/src/service.rs`: workers and composition.
- `apps/async-analysis/src/coordinator.rs`: discovery and lifecycle.
- `modules/analysis-workspace/src/provider.rs` and `local.rs`: claims and storage.
- `modules/analysis-domain/src/model.rs`: cycle, attempt and terminal outcome.
- `modules/analysis-agent/src/provider.rs`: pool, provider and session contracts.
