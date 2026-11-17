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
  "$2"
  return "$browser_status"
}
