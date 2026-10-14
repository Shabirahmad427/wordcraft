#!/usr/bin/env bash
# Run one command in an isolated Xvfb session with Openbox, preserving its exit status.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $0 command [arg ...]" >&2
  exit 2
fi

# shellcheck disable=SC2016 # The single-quoted program is intentionally passed to the nested Bash.
exec xvfb-run -a bash -euo pipefail -c '
  openbox --sm-disable &
  wm_pid=$!
  trap "kill $wm_pid 2>/dev/null || true; wait $wm_pid 2>/dev/null || true" EXIT
  for _ in {1..50}; do
    if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q "window id"; then break; fi
    sleep 0.1
  done
  xprop -root _NET_SUPPORTING_WM_CHECK | grep -q "window id"
  "$@"
' wordcraft-xvfb-openbox "$@"
