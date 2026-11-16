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
Restart/recovery checks run only after the unchanged browser gate passes.
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
duplicate-active Candidate conflicts are recorded; the precise browser/
read-model cause is not established. Do not weaken the assertion or infer
successful installed recovery: restart, analysis-down, backup/restore/reset
and restored-parity checks were not reached.

`release.provenance.json` records the exact failed run, independently verified
download hashes, successful seams and unverified checks. Automatic retries
stopped at this blocker. The workflow's browser artifact path was corrected
to the actual example-local output directory; that collection-only change
has not been rerun. No Azure operations or provider connection occurred.
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
