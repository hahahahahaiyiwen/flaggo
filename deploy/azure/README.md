# Maintainer Azure backend preparation

This is **local scaffolding, not an installed or validated deployment**.
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

`systemd/flaggo@.service` is a native **service template**, not a complete
installation. Instances for `contract-service`, `decision-service`,
`otel-ingestion`, `evidence-materializer` and `async-analysis` select
`/opt/flaggo/current/bin/flaggo-<instance>`.

An operator must first create the dedicated `flaggo` account and mount the
managed data filesystem at `/var/lib/flaggo`. The template checks that exact
mount before starting, so an absent disk cannot silently create an empty
SQLite database on the OS disk. Create root-owned configuration under
`/etc/flaggo`; copy and review the matching `config/*.env.example` files,
with restricted access for any subsequently approved secrets.

All store adapters share `/var/lib/flaggo/flaggo.db`. Keep its WAL/SHM beside
it on this ordinary local filesystem, never Azure Files. Workspaces, Copilot
state and LGTM data also belong on the data disk. Exactly one materializer
per database is supported. Unit cgroup cleanup is operational hygiene,
**not a Python sandbox**.

The template does not enable any service or imply readiness ordering.
Start Contract Service and OTel Ingestion first and wait for their actual
`/health/ready`; then Decision Service, materializer and the deliberately
selected analysis entrypoint. Workers have no HTTP readiness route. A future
supervisor installation must enforce this order and verify logs/freshness.
Runtime availability must not depend on continued analysis/ingestion uptime.

The live analysis configuration deliberately uses unsupported
`FLAGGO_ANALYSIS_INFERENCE=unconfigured` so copying it cannot implicitly
connect a developer's GitHub login. Provider selection, credentials and run
budgets must be explicitly approved before replacing that value. Deterministic
deployment smoke instead uses the existing example-owned
`flaggo-tetris-playground-analysis` binary, not a live-provider fallback.

Node gateway and Collector/LGTM installation remain pending release packaging.
Reuse their actual configurations and pin image digests. Native Linux Collector
uses host networking with loopback receivers; application telemetry fans out
to ingestion and LGTM, internal telemetry goes only to LGTM. Keep every backend
listener private. Generate the gateway runtime configuration from the deployed
contract binding and refresh its service PID map after process restarts.
Do not fabricate healthy PIDs or publish the local gateway control routes.

## Release preparation

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

**Current blocker:** dependency resolution failed TLS handshakes to
`files.pythonhosted.org` with Windows uv (including native trust), pip's dry-run
resolver, and uv in an existing local Linux container. No lock was fabricated and certificate
verification was not disabled. Generate/verify it on an approved working
Linux build environment before claiming a reproducible Python release.

Copilot also needs a clean-host runtime probe: Phase 5 used Rust SDK
`1.0.17-preview.5`, CLI `1.0.92-5`, protocol `3`. Verify the SDK's actual lookup
and distribution; do not assume copying the Rust binary packages the CLI or
that a maintainer's login/cache is available. The unavailable SDK-specific
Azure recipe has not validated this placement.

## Deployment acceptance and recovery gates

Required before deployment: named backend operator/release owner, actual
recurring budget and alert recipient, approved operator-access method,
backup/retention policy, pinned artifacts and complete configuration,
SKU/policy/capacity checks, and a separate resource/cost approval.

The following checks are not yet a deployed result or automated installer:

- Record exact installed manifest/config/image identities and mounted paths.
  Verify real health endpoints, unique workers and denied internal ingress.
- Add a deployed-target mode to the existing closed-loop assertions. The
  current browser fixture launches fresh local processes; running it alone
  cannot validate this infrastructure. Missing target identities must fail,
  never select a local fallback.
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
