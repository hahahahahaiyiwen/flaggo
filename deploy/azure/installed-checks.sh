#!/usr/bin/env bash

restore_installed_state() {
  local source="$1" destination="$2" owner="$3"
  test -f "$source/.flaggo-state.json"
  test -f "$source/flaggo.db"
  test "$(id -u "$owner")" != 0
  # Archive only children: tar 1.35 --unlink-first also tries to remove ".".
  find "$source" -mindepth 1 -maxdepth 1 -printf '%f\0' \
    | tar -C "$source" --null -T - -cf - \
    | runuser -u "$owner" -- tar -C "$destination" --no-same-owner --unlink-first -xf -
}

run_installed_checks() {
  local browser_status=0
  "$1" || browser_status="$?"
  local browser_passed=false
  if test "$browser_status" = 0; then browser_passed=true; fi
  printf '{"passed":%s,"exitCode":%s}\n' "$browser_passed" "$browser_status" \
    > "$3/installed-browser-result.json"
  if test "$browser_status" != 0; then
    echo "Browser gate failed ($browser_status); collecting independent recovery checks." >&2
  fi
  (
    recovery_result="$3/installed-recovery-status.json"
    trap 'status=$?; capture_status=0; printf "{\"passed\":%s,\"exitCode\":%s}\n" \
      "$([ "$status" = 0 ] && printf true || printf false)" "$status" \
      > "$recovery_result" || capture_status=$?
      if test "$status" != 0; then exit "$status"; fi
      exit "$capture_status"' EXIT
    "$2"
  )
  return "$browser_status"
}
