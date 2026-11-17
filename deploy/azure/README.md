# Maintainer Azure backend preparation

This is **backend preparation, not an Azure deployment or deployment approval**.
It does not replace the Tetris development launcher or offer an adopter
installer. No resource creation, public ingress or provider credential is
authorized by these files. The existing website and its resource group are
not referenced by the infrastructure.

## Proposed infrastructure

`infra/main.bicep` is subscription-scoped. It declares the proposed separate
`rg-flaggosvc-dev-bf57` group in `eastus2`, a single D2as_v5-class Linux VM,
32-GiB OS/64-GiB data Standard SSDs, explicit outbound IPv4, deny-by-default
networking, identity, Standard Key Vault and non-public Blob containers.
Its required operator/image/SSH inputs are intentionally not populated.
VM secret access and container-scoped Blob roles default off; approving
identity permissions is separate from defining their templates. When approved,
artifact/checksum access is read-only and backup write access is limited to
the backup container, not the storage account.
Do not substitute a floating OS image version.

Current estimated fixed retail floor is $73.63/month at 730 hours; storage
operations/backups, egress, alerting, taxes and inference cost extra.
Budget alerts are delayed and are not a hard cap. SKU availability, policy,
full pricing and measured capacity remain deployment gates, not guarantees.
There is no HA or silent paid fallback.

Only a separate, informed deployment approval may authorize cloud writes.
Every Azure CLI command, including local Bicep commands, must set the isolated
configuration directory in the same process:

```powershell
$env:AZURE_CONFIG_DIR = Join-Path $HOME '.azure-flaggo'
az bicep build --file deploy\azure\infra\main.bicep --stdout > $null
```

Before any authorized Azure discovery/deployment, verify subscription
`0fefe390-5f18-42c2-ad1e-01517b57ebcd` and tenant
`1060b88f-71a3-4c0e-821a-b6662ed7f58b`. Do not change shared CLI defaults,
copy login caches or emit tokens.

## Process and persistence contract

`systemd/flaggo@.service` is a native service template. Instances for
`contract-service`, `decision-service`,
`otel-ingestion`, `evidence-materializer` and `async-analysis` select
`/opt/flaggo/current/bin/flaggo-<instance>`.

An operator must first create the dedicated `flaggo` account and mount the
managed data filesystem at `/var/lib/flaggo`. The template checks that exact
mount before starting, so an absent disk cannot silently create an empty
SQLite database on the OS disk. Create root-owned configuration under
`/etc/flaggo`; copy and review the matching `config/*.env.example` files,
with restricted access for any subsequently approved secrets.
Install root-owned, service-readable releases beneath `/opt/flaggo`, not inside
a developer's private home or build directory. Run unprivileged probes from the
unit's actual `/var/lib/flaggo` working directory. The supervisors use the
bundled Python interpreter, not a host interpreter with a different stdlib.

All store adapters share `/var/lib/flaggo/flaggo.db`. Keep its WAL/SHM beside
it on this ordinary local filesystem, never Azure Files. Workspaces, Copilot
state and LGTM data also belong on the data disk. Exactly one materializer
per database is supported. Unit cgroup cleanup is operational hygiene,
**not a Python sandbox**.

The instance drop-ins enforce bounded HTTP readiness ordering. The backend
target starts the four base services and observability, but neither an analysis
entrypoint nor the gateway. Select exactly one analysis unit explicitly;
the smoke and live units conflict. Workers have no HTTP readiness route:
verify their structured startup events and fresh work, not only a live PID.
There is deliberately no runtime `Requires` relationship to analysis or
observability; stopping those must not stop deterministic evaluation.

The live analysis configuration deliberately uses unsupported
`FLAGGO_ANALYSIS_INFERENCE=unconfigured` so copying it cannot implicitly
connect a developer's GitHub login. Provider selection, credentials and run
budgets must be explicitly approved before replacing that value. Deterministic
deployment smoke instead uses the existing example-owned
`flaggo-tetris-playground-analysis` binary, not a live-provider fallback.

The gateway supervisor reads actual systemd MainPIDs and restarts its Node
child when a worker restarts. A down worker remains visibly degraded; it is
not assigned a fake healthy PID. A gateway restart resets its in-memory demo
session, not durable authority. Its runtime configuration remains deliberately
invalid until the exact contract binding is supplied.

`compose.yaml` pins the same Collector/LGTM versions to their retrieved
image digests. Release preparation copies the canonical playground Collector
configuration rather than maintaining a fork. Native Linux Collector
uses host networking with loopback receivers; application telemetry fans out
to ingestion and LGTM, internal telemetry goes only to LGTM. Keep every backend
listener private. Generate the gateway runtime configuration from the deployed
contract binding. Do not publish the local gateway control routes.

## Release preparation

The manually dispatched `Backend preparation` workflow has an
`replay_installed=true` path for the one checksum-pinned retained release in
`release.provenance.json`. It verifies the original run, workflow Git blob,
successful packaging/probe steps, artifact identity and ZIP/archive/manifest
hashes before extracting or executing anything. Only its download step receives
the job's read-only GitHub token. The replay skips both Python resolution and
the release build; it does not install application dependencies.

The clean checkout is an isolated test driver linked to the sealed dependency
and compiled-JavaScript trees. `/opt/flaggo/release` remains the unchanged,
manifest-verified installation. Poll response bodies, correlation projections,
last state and browser artifacts are retained even on assertion failure.
Restart/recovery checks run independently; either gate failing keeps the overall
result failed.
There are no automatic dispatch/retry loops or Azure/provider credentials.
This runner rehearsal is not Azure deployment or live-provider acceptance.

Build from a clean recorded Git tree containing only approved backend changes.
The preserved website diff in the current worktree must not enter the release.
Record source and artifact hashes, target/toolchain, native dependencies,
exact Node runtime, gateway/UI assets, container digests and license notices.
Never build or download floating dependencies at service startup.

For skills, copy the analysis-agent module's four skill directories into the
release's `skills` directory. `FLAGGO_ANALYSIS_SKILL_DIRECTORY` selects this
directory; startup rejects a missing required `SKILL.md`. Embedded prompts
remain in the Rust binary.

The Linux scientific environment targets CPython 3.12/x86-64. Generate and
review the complete hashed lock, using an installed `uv` resolver:

```powershell
uv pip compile apps\async-analysis\requirements-linux.in --python-version 3.12 --python-platform x86_64-manylinux_2_28 --only-binary :all: --generate-hashes --default-index https://pypi.org/simple --output-file apps\async-analysis\requirements-linux.txt
```

Do not install from the unpinned input as a release fallback. A missing lock,
failed wheel download/hash, unsupported interpreter or failed imports blocks
release. Install the lock with hash verification in a clean Linux environment
and record all six scientific-library import/version results.

The checked-in lock was generated and installed with normal TLS on the
approved GitHub-hosted Linux runner. Its SHA256 is
`817ee1fd87fe78f68c97d24d29bf0eaedcb22c0cbdadad71bb1db53e1ecb8c0d`;
`requirements-linux.provenance.json` records its source, run and artifact.
This resolves the local download-TLS blocker without weakening verification.

Copilot also needs a clean-host runtime probe: Phase 5 used Rust SDK
`1.0.17-preview.5`, CLI `1.0.92-5`, protocol `3`. Verify the SDK's actual lookup
and distribution; do not assume copying the Rust binary packages the CLI or
that a maintainer's login/cache is available.

`prepare-release.py --output <new-absolute-path>` builds a clean committed
Linux x86-64 checkout with CPython 3.12 and the pinned Rust/Node toolchains.
It packages native services, the deterministic example entrypoint, skills,
SDK runtime assets, compiled gateway/UI and workspace dependencies, Node,
Python and scientific wheels. `release.json` records file hashes, links,
source/toolchain identities and native library dependencies. The target must
provide compatible Ubuntu 24.04 system libraries; this is not a static or
arbitrary-distribution binary release. No compiler, npm install or pip download
runs at service startup.

The approved Actions run
[43963760647](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43963760647)
built source `52d1508ee959a23917cb031a79196d03921f6432`, moved the release
away from its build location, and passed the protocol handshake and all
scientific imports with an empty HOME and no credentials. It requested no
inference. Its archive SHA256 is
`687bd7b60ea15f6405bdde15af4fbd7afd2ad2e2d858ecc37e892dd97cf95023`.
This is packaging evidence, not Azure acceptance or a live-provider cycle.

The extended focused workflow also contains `verify-installed.sh`, restricted
to an ephemeral GitHub-hosted runner. It installs the prepared units over a
local mounted ext4 test filesystem, never provisions Azure. It exercises the
existing deterministic browser assertions, real service restarts and offline
recovery. **The full installed rehearsal is blocked**, not passed.
[43974316189](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43974316189)
built source `8f249024291b22e640b9a2afc292ccb851c10401`, passed the actual
service-user SDK/Python probes, mounted service/observability readiness and
default/authored baseline parity. The deterministic browser demonstration
completed, but its correlated analysis/Candidate/activation assertion at
`closed-loop.spec.mjs:151` timed out. Native admissions/activations and later
duplicate-active Candidate conflicts are recorded; that run's browser
responses were not retained. Do not weaken the assertion or infer
successful installed recovery: restart, analysis-down, backup/restore/reset
and restored-parity checks were not reached.

`release.provenance.json` records the exact failed run, independently verified
download hashes, successful seams and unverified checks. Automatic retries
stopped at this blocker. The explicit artifact-only reproduction
[43980808838](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43980808838)
subsequently verified the original sealed release without a rebuild and
retained all 32 actual browser-poll bodies. Every poll had a rejected latest
Candidate with no digest; the active cycle also disappeared in the final 16
polls. Activation and active executable identity remained correct. This
establishes an observer/test correlation defect, not an activation failure.

The source fix correlates successful milestones by active contract/executable/
cycle identity, keeps latest failures separately visible, and pins at most
three matching chain events. The TypeScript build, 21 focused tests and replay
of all 32 captured event sets pass without changing the browser predicate.
At that stage the fixed gateway was not sealed or installed-validated.
The first artifact-only setup attempt failed exact symlink validation; safe
extraction now preserves original link text, with whole-artifact offline Linux
verification. Its one corrected dispatch was explicitly approved.
No Azure operations or provider connection occurred.
An explicitly approved new-release rehearsal uses manual dispatch with
`replay_installed=false`; this builds one new sealed release from the selected
committed source and runs installed smoke and the gated recovery sequence.
`replay_installed=true` reuses only `artifactReplayPin` in the checked-in
provenance file and never substitutes driver files into that release.
The approved current pin is source `6d7b62c`, run `43983014659`, artifact
`14284577237`; original old-release evidence remains historical. This mode has
no arbitrary run/artifact override. Both paths always retain
browser output and cleanup-time gateway HTTP body/transport diagnostics;
an unavailable gateway is recorded explicitly, not treated as a successful
snapshot. Workflow dispatch never authorizes Azure deployment.

The separately approved new build
[43983014659](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43983014659)
sealed source `6d7b62ceb8e62692c3dcbd93991dd73a0bb597d4`, including the
observer fix. All 23,531 manifest-covered files and 28 exact links were verified.
The installed original correlation predicate passed on its first poll while
latest rejected attempts remained separately visible. Exact learned SDK/REST
parity, reviewed Python/artifact/action checks and browser action inspection
also passed.

That new-build run was blocked by the later 60-second LGTM assertion
at `closed-loop.spec.mjs:445`. Final decision-service and evidence-materializer
queries returned HTTP 200/success with empty log streams, although both had
60 parsed spans. Other services had logs, spans and correlated traces. The
captured query window starts at `2026-11-17T00:02:09.644Z`; materializer logs
also require a nonempty trace ID. Subsequent retained-evidence diagnosis
established a test expectation mismatch: Decision Service's startup log was
emitted at `00:01:16.100656Z`, outside this window; successful decisions
deliberately have metrics/spans rather than per-request logs. Materializer
page-commit logs are present in the gateway's actual Loki-log timeline, with
batch IDs but no trace IDs. The synchronous trace filter excluded these valid
durable-worker logs, contrary to the documented domain-identity correlation.
No native emission or routing change is needed. Browser trace/raw responses,
final HTTP-200 gateway state and journal are retained in artifact `14284577237`.
Restart, analysis-down and backup/restore/reset/restored parity were not reached.
`correctedReleaseValidation` in the provenance file records this new release
while retaining the original historical release/replay evidence.

The corrected test requires Decision evaluation spans (with existing per-service
metrics checks), materializer page spans and committed batch-ID logs, and the
existing other-service logs/spans/correlated traces. Original correlation and
telemetry timeouts are unchanged. Focused offline regressions passed before
the separately approved artifact-only validation below.

The installed driver now records `installed-browser-result.json` and continues
independent restart, analysis-down and recovery checks even if the browser fails.
`installed-recovery-result.json` is written only after that sequence completes.
Recovery still fails fast, and browser failure remains a nonzero overall exit;
`installed-result.json` is produced only when both gates pass. This does not
retroactively establish any previously unreached recovery result.

The single corrected artifact-only replay
[43987207298](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43987207298)
used separate driver `ef8b488` against unchanged sealed runtime `6d7b62c`.
Both build jobs were skipped. Provenance/extraction/isolation and manifest
verification passed, and **the full deterministic browser test passed**.
The fail-fast recovery sequence then passed restart assertions, analysis-down
parity, quiesced backup, offline restore and database/workspace comparisons.
These stage results are supported by console/source reachability, not retained
per-file JSON/PID values.

**Recovery remains blocked:** reset's first `Path.rename` crosses from mounted
ext4 state into runner scratch and fails with `EXDEV` (`state-tool.py:175`).
Reset and restored live exact-digest parity did not complete. A second blocker
prevented evidence upload: the unprivileged artifact action cannot scan the
intentionally root-private `state-backup` directory. No artifact was published;
the new raw poll bodies, browser trace and per-file recovery results were lost.
The full job log and its authenticated input proof are retained. Do not equate
that log-only evidence with retained raw responses or complete recovery.
No automatic redispatch or rebuild followed. Backup permissions must not be
loosened merely to upload diagnostics; a future approved correction must stage
only the already-approved diagnostic files outside private backup state.

Artifacts expire after seven days: retain the reviewed archive, checksum,
manifest and evidence in an approved private release store before deployment.
No release-store upload is authorized by the workflow.

## Explicit installed-target smoke

The existing closed-loop test still launches and cleans up its own development
fixture by default. Setting `FLAGGO_DEPLOYED_TARGET` instead requires an absolute
`flaggo.deployed-smoke/v1` descriptor and `FLAGGO_DEPLOYED_SMOKE_CONFIRM` matching
the mounted state's exact ID. Empty, missing or mismatched identities fail;
they never select the development launcher. Only loopback endpoints and the
deterministic reviewed-Python scenario are accepted. Run operator-side on the
host where the release and sealed workspace paths can actually be inspected.

`bind-smoke.mjs` creates the descriptor/runtime binding and verifies baseline
SDK/REST parity. Tetris has authored exploration rules, not an active default;
a separate `deployment.default` probe reuses its canonical attributes/result
without authored rules or learning and verifies the actual default result and
`evaluation.source=default`. It does not replace Tetris's authored baseline.
The helper requires the
existing state marker to be `flaggo.state/v1`, kind `disposable-smoke`, with
the explicitly confirmed state ID. The prepared endpoint layout is fixed to
the unit examples; it is not an endpoint-discovery fallback or public installer.
First verify the installed release against an independently retained manifest
hash using `state-tool.py verify-release`.

The target test reuses the existing evidence, SQL/Python/artifact review chain,
activation, exact-digest parity, failure-seam and telemetry assertions. It
does not launch local services, delete target state, or stop the gateway.
`verify-bound-decision.mjs` separately checks exact active-digest SDK/REST
parity with analysis stopped and after restore. No target mode proves #86's
visitor controls or completed live inference.

## Offline operations

These are maintainer tools, not commands to run before deployment approval.
Root-owned releases are immutable and configuration stays under `/etc/flaggo`.
Create the `flaggo` account, mount and verify the data filesystem, install the
units/drop-ins and restricted configuration, and preload pinned container
images before starting the target. Compose startup uses `--pull never`.
Review ports, configuration and compatible native libraries on the actual
selected OS image before enabling anything. No VM bootstrap is silently run.

For updates: stage a new release beside the old one; verify its independently
retained manifest hash and every file/link with `state-tool.py verify-release`.
Stop the gateway, both analysis entrypoints, materializer, ingestion, decision,
contract and observability; require zero MainPIDs and no open state handles.
Back up before changing `current`. Switch the symlink only after schema
compatibility review, then start the base target and explicitly selected
analysis/gateway, checking health, structured worker starts and exact parity.
Never delete the prior release or automatically roll a migrated database back.

`state-tool.py backup` requires the configured `/var/lib/flaggo` mount's writers
stopped. It checkpoints and checks SQLite, copies durable database/workspaces/
observability coherently, and records release and configuration hashes. It
does **not** export configuration values or `copilot-home` auth/session caches.
The output directory is private. Store backup data encrypted and access-restricted;
it can contain sensitive evidence even without provider credentials.

`state-tool.py restore` checks the externally retained backup manifest hash,
all saved bytes and the exact matching release hash, then restores only into
a new separate path. It never switches or overwrites live state. Confirm
sealed authority/artifact identity, reinstall secrets separately and restore
`flaggo` ownership before any approved switch back to the mounted state tree.
Checksums/SQLite integrity alone do not prove application-level recovery.

`state-tool.py reset-disposable` additionally requires the exact confirmed
`disposable-smoke` state ID. It creates a verified backup and moves only named
demo state entries into a private archive; it deletes nothing, leaves the
mount intact and issues a new state ID. Unknown state entries or a persistent
identity block reset. Auth caches are neither archived nor removed. Rebind the
fresh default contract before a new smoke; do not reuse an old descriptor.
Actual-host recovery rehearsal, retention, access and recovery-point objectives
remain deployment acceptance gates.

### Reset/evidence correction and bounded installed result

Reset now rejects an existing, state-contained or incompatible archive before
checkpointing SQLite, creating backup directories or moving any state. Both
filesystem device and Linux mount identity must match for the archive's nearest
existing parent and each named source entry. Equal device IDs alone are not
enough: Linux also rejects rename across different bind mounts. There is no
copy/delete fallback.

The installed rehearsal mounts a private ext4 volume with sibling `state` and
`reset-archive` paths, then bind-mounts `state` at `/var/lib/flaggo`. Reset uses
`--state` to name the backing directory in the archive's mount view. That option
must identify the **same inode/device** as the still-mounted configured state
root, not an arbitrary alternative state directory. Stopped-writer/open-handle
checks cover both aliases; exact disposable marker/confirmation, unknown-entry,
external-archive, release and verified-backup guards remain required. Archives
stay root-owned/private. Restore extracts into the live state as `flaggo` rather
than recursively changing ownership of state or private backups.

An always-run collection step now copies only the explicit diagnostic allowlist
in `collect-diagnostics.py` into `flaggo-diagnostics`, with runner-owned0700
directories and0600 files. The upload action reads that directory, never
`state-backup`, restored copies or workspaces. Only a reduced backup proof
(manifest hash, release/source identities and file count) is staged; backup
contents, configuration values and evidence filenames are not. Browser poll
bodies/last state and applicable trace are selected by exact filenames from
the credential-free deterministic test output. The index records copied
hashes, missing files and collection errors. Browser and recovery status files
are recorded independently, including failure, without turning a failed gate
green; detailed recovery/parity files remain required when those stages pass.

Focused local Windows and Linux regressions cover unchanged-source rejection,
real tmpfs/bind-mount behavior, supported reset, identity/quiescence guards,
four browser/recovery outcomes and unprivileged diagnostic readability while
backup access stays denied. The separately approved new seal/run
[43990457633](https://github.com/hahahahahaiyiwen/flaggo/actions/runs/43990457633)
built source `b109a917d48c0bd63ecbc4260a8dd957bfab3ffd` without an overlay.
Artifact14287597978 uploaded and downloaded successfully; all23,533 sealed
files,28 exact safe links and15 indexed diagnostics were verified. The
diagnostic index has no collection errors and private state is excluded.

**The run still failed.** The browser stopped at the unchanged initial hard-drop
score assertion (line96,10 seconds), before starting the closed-loop demo or
authority assertions. Trace records command HTTP200 and final Game over,
not the exact pre-click game state/cause. Independent recovery completed
worker/gateway PID refresh, two authored-active decisions with analysis
stopped, verified backup/offline restore/comparisons and the actual guarded
reset (new state ID and matching backup proof retained). The next tar restore
failed with `.: Cannot unlink: Invalid argument` under `--unlink-first`;
restored live exact-digest parity was not reached. Partial restore is possible.
Browser exit1 and recovery exit2 remain independently recorded; successful
diagnostic collection did not mask either.

The failed trace/screenshot, journal, restart/reset/parity files and reduced
backup proof are retained. Later adaptive/correlation/telemetry outputs were
never produced, not lost during upload. There was no automatic retry,
assertion change, further build or Azure operation. Earlier6d7b62c browser
success does not establish a pass for this new seal. Investigate these two
new failures from retained evidence before another execution; complete
installed recovery and deployment acceptance remain blocked.

## Deployment acceptance and recovery gates

Required before deployment: named backend operator/release owner, actual
recurring budget and alert recipient, approved operator-access method,
backup/retention policy, pinned artifacts and complete configuration,
SKU/policy/capacity checks, and a separate resource/cost approval.

The following checks are not yet a deployed result or automated installer:

- Record exact installed manifest/config/image identities and mounted paths.
  Verify real health endpoints, unique workers and denied internal ingress.
- Exercise the explicit deployed-target mode against the actual installed
  Azure release. A successful local/runner fixture alone cannot validate it.
- Exercise default decision parity, real Collector/inbox/materializer evidence,
  saved SQL/Python artifacts, invalid draft/review/revision/final review,
  Candidate admission, activation and subsequent exact-digest SDK/REST parity.
  Private artifact inspection stays operator-side.
- Test service/PID-map restart, analysis failure without runtime loss, coherent
  backup/restore and a narrowly targeted disposable demo reset.

For backup, stop/drain all writers and close SQLite handles before checkpoint
and coherent database/workspace/configuration export. Never copy only a live
`.db` without its WAL. Backups must name their source release and checksums.
Restore into a separate empty state path and verify authority/artifact identity
before switching; a successful upload is not a recovery test. Retention and
recovery-point objectives remain unapproved. Secrets are delivered separately.

Updates stage an immutable release and retain the prior one. Binary rollback
requires compatible schema/state; otherwise restore the matching pre-update
backup. Preview breaking changes may require explicitly approved export/reset
and reinstallation; no compatibility layer or automatic data reset is offered.

Before public access, separately approve the gateway hostname/TLS and route
allowlist. #86 owns visitor controls, shared sessions/data notices and bounded
live-provider execution. Deterministic smoke does not prove a completed live
loop. Do not edit the existing website, expose Grafana/internal APIs, change
publication metadata or close #85 as a side effect of preparation.
