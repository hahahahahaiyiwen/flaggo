#!/usr/bin/env bash
# Only for an ephemeral GitHub-hosted Linux runner, never a deployment command.
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
test "$(id -u)" = 0
root="$(realpath "$1")"
scratch="$(realpath "$2")"
repository="$(cd "$(dirname "$0")/../.." && pwd)"
test ! -e /opt/flaggo
test ! -e /etc/flaggo
test ! -e /var/lib/flaggo
test -z "$(docker ps -aq --filter label=com.docker.compose.project=flaggo-backend)"
systemctl show-environment > /dev/null
release_hash="$(sha256sum "$root/release.json" | cut -d ' ' -f 1)"
python3 "$root/deploy/azure/state-tool.py" verify-release --release "$root" --release-sha256 "$release_hash"
export PYTHONDONTWRITEBYTECODE=1
units=(flaggo-gateway flaggo-smoke-analysis flaggo@async-analysis
  flaggo@evidence-materializer flaggo@otel-ingestion
  flaggo@decision-service flaggo@contract-service flaggo-observability)

cleanup() {
  result="$?"
  trap - EXIT
  if ! systemctl stop "${units[@]}"; then
    echo "Cleanup: a service stop failed" >&2
    result=1
  fi
  if ! docker compose --project-name flaggo-backend -f "$root/deploy/azure/compose.yaml" down --timeout 30; then
    echo "Cleanup: Compose shutdown failed" >&2
    result=1
  fi
  journal_args=()
  for unit in "${units[@]}"; do journal_args+=(-u "$unit"); done
  journalctl --no-pager "${journal_args[@]}" > "$scratch/installed-journal.log" 2>&1 || result=1
  if mountpoint --quiet /var/lib/flaggo; then
    if ! umount /var/lib/flaggo; then
      echo "Cleanup: state filesystem remains mounted" >&2
      result=1
    fi
  fi
  exit "$result"
}
trap cleanup EXIT

mkdir /opt/flaggo /etc/flaggo /var/lib/flaggo
ln -s "$root" /opt/flaggo/current
useradd --system --home-dir /var/lib/flaggo --no-create-home --shell /usr/sbin/nologin flaggo
truncate -s 4G "$scratch/flaggo-smoke.img"
mkfs.ext4 -F -q "$scratch/flaggo-smoke.img"
mount -o loop "$scratch/flaggo-smoke.img" /var/lib/flaggo
chown flaggo:flaggo /var/lib/flaggo
chmod 700 /var/lib/flaggo
for source in "$root"/deploy/azure/config/*.env.example; do
  install -o root -g flaggo -m 640 "$source" "/etc/flaggo/$(basename "${source%.example}")"
done
cp -R "$root/deploy/azure/systemd/." /etc/systemd/system/
systemctl daemon-reload
unit_paths="$root/deploy/azure/systemd:/etc/systemd/system:/run/systemd/system:/usr/lib/systemd/system"
SYSTEMD_UNIT_PATH="$unit_paths" systemd-analyze verify \
  flaggo@contract-service.service flaggo@decision-service.service flaggo@otel-ingestion.service \
  flaggo@evidence-materializer.service flaggo@async-analysis.service \
  flaggo-smoke-analysis.service flaggo-gateway.service flaggo-observability.service flaggo-backend.target
docker compose --project-name flaggo-backend -f "$root/deploy/azure/compose.yaml" pull
systemctl start flaggo-backend.target
python3 "$root/deploy/azure/wait-ready.py" \
  http://127.0.0.1:8080/health/ready http://127.0.0.1:8081/health/ready \
  http://127.0.0.1:5090/health/ready http://127.0.0.1:13133/ \
  http://127.0.0.1:3000/api/health http://127.0.0.1:3100/ready \
  http://127.0.0.1:3200/ready http://127.0.0.1:9090/api/v1/status/runtimeinfo

state_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
printf '{"format":"flaggo.state/v1","kind":"disposable-smoke","stateId":"%s"}\n' "$state_id" \
  > /var/lib/flaggo/.flaggo-state.json
export FLAGGO_DEPLOYED_SMOKE_CONFIRM="$state_id"
target="$scratch/installed-target.json"
"$root/node/bin/node" "$root/deploy/azure/bind-smoke.mjs" \
  "$root" /var/lib/flaggo "$target" /etc/flaggo/gateway.env
systemctl start flaggo-smoke-analysis flaggo-gateway
python3 "$root/deploy/azure/wait-ready.py" http://127.0.0.1:4310/api/health
export FLAGGO_DEPLOYED_TARGET="$target"
export PLAYWRIGHT_BROWSERS_PATH="$scratch/flaggo-browsers"
cd "$repository"
browser="$("$root/node/bin/node" --input-type=module -e 'import { chromium } from "@playwright/test"; console.log(chromium.executablePath());')"
if ! test -x "$browser"; then
  echo "Pinned Playwright Chromium is missing; installing its required browser and system dependencies."
  "$root/node/bin/node" node_modules/@playwright/test/cli.js install --with-deps chromium
fi
"$root/node/bin/node" node_modules/@playwright/test/cli.js test \
  --config examples/tetris-playground/playwright.config.mjs \
  examples/tetris-playground/test/closed-loop.spec.mjs

old_worker="$(systemctl show flaggo@evidence-materializer --property=MainPID --value)"
wrapper="$(systemctl show flaggo-gateway --property=MainPID --value)"
old_gateway="$(pgrep -P "$wrapper" -x node)"
systemctl restart flaggo@evidence-materializer
new_worker="$(systemctl show flaggo@evidence-materializer --property=MainPID --value)"
test "$old_worker" != "$new_worker"
new_gateway=""
for attempt in $(seq 1 30); do
  new_gateway="$(pgrep -P "$wrapper" -x node || test "$?" = 1)"
  if test -n "$new_gateway" && test "$new_gateway" != "$old_gateway"; then break; fi
  sleep 1
done
test -n "$new_gateway"
test "$new_gateway" != "$old_gateway"
python3 "$root/deploy/azure/wait-ready.py" http://127.0.0.1:4310/api/health
printf '{"workerBefore":%s,"workerAfter":%s,"gatewayBefore":%s,"gatewayAfter":%s}\n' \
  "$old_worker" "$new_worker" "$old_gateway" "$new_gateway" > "$scratch/installed-restart.json"
systemctl stop flaggo-smoke-analysis
"$root/node/bin/node" "$root/deploy/azure/verify-bound-decision.mjs" \
  "$target" "$scratch/analysis-stopped-parity.json"

systemctl stop "${units[@]}"
release_hash="$(sha256sum "$root/release.json" | cut -d ' ' -f 1)"
backup_hash="$(python3 "$root/deploy/azure/state-tool.py" backup \
  --output "$scratch/state-backup" --release "$root" --release-sha256 "$release_hash")"
python3 "$root/deploy/azure/state-tool.py" restore --source "$scratch/state-backup" \
  --destination "$scratch/restored-state" --backup-sha256 "$backup_hash" --release-sha256 "$release_hash"
cmp /var/lib/flaggo/flaggo.db "$scratch/restored-state/flaggo.db"
diff -r /var/lib/flaggo/analysis-workspaces "$scratch/restored-state/analysis-workspaces"
python3 "$root/deploy/azure/state-tool.py" reset-disposable --archive "$scratch/reset-archive" \
  --release "$root" --release-sha256 "$release_hash" --confirm-state-id "$state_id" \
  > "$scratch/installed-reset.json"
test ! -e /var/lib/flaggo/flaggo.db
cp -a "$scratch/restored-state/." /var/lib/flaggo/
chown -R flaggo:flaggo /var/lib/flaggo
systemctl start flaggo-backend.target flaggo@contract-service flaggo@decision-service \
  flaggo@otel-ingestion flaggo@evidence-materializer flaggo-observability
python3 "$root/deploy/azure/wait-ready.py" http://127.0.0.1:8081/health/ready
"$root/node/bin/node" "$root/deploy/azure/verify-bound-decision.mjs" \
  "$target" "$scratch/restored-parity.json"
"$root/node/bin/node" --input-type=module -e '
  import assert from "node:assert/strict";
  import { readFileSync } from "node:fs";
  const [before, after] = process.argv.slice(1).map((path) => JSON.parse(readFileSync(path, "utf8")));
  assert.equal(before.activeExecutable.executableDigest, after.activeExecutable.executableDigest);
  assert.deepEqual(before.decisions.map(({ sdk }) => [sdk.result, sdk.evaluation]),
    after.decisions.map(({ sdk }) => [sdk.result, sdk.evaluation]));
' "$scratch/analysis-stopped-parity.json" "$scratch/restored-parity.json"
printf '{"scope":"ephemeral Linux runner, mounted ext4","deployedToAzure":false,"liveInference":false,"backupSha256":"%s","releaseSha256":"%s","checks":["installed deterministic closed loop","worker and gateway PID refresh","analysis-independent evaluation","quiesced restore","guarded disposable reset","restored exact-digest parity"]}\n' \
  "$backup_hash" "$release_hash" > "$scratch/installed-result.json"
