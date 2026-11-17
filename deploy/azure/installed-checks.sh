#!/usr/bin/env bash

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
