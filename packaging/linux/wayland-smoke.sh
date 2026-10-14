#!/usr/bin/env bash
# Start a disposable headless Weston compositor and exercise native Wayland startup/shutdown.
# Weston headless has no input devices, so this checks the window, renderer, control endpoint,
# recovery checkpoint and app.quit. The X11 package smoke covers UI input separately.
set -euo pipefail

runtime="$(mktemp -d "${RUNNER_TEMP:-/tmp}/wordcraft-wayland.XXXXXX")"
chmod 700 "$runtime"
export XDG_RUNTIME_DIR="$runtime"
export WAYLAND_DISPLAY=wayland-wordcraft
export WORDCRAFT_WAYLAND=1
unset DISPLAY

weston --backend=headless-backend.so --renderer=pixman --socket="$WAYLAND_DISPLAY" \
  --idle-time=0 --log="$runtime/weston.log" &
weston_pid=$!
cleanup() {
  kill "$weston_pid" 2>/dev/null || true
  wait "$weston_pid" 2>/dev/null || true
}
trap cleanup EXIT

for _ in {1..100}; do
  if [ -S "$runtime/$WAYLAND_DISPLAY" ]; then break; fi
  if ! kill -0 "$weston_pid" 2>/dev/null; then
    cat "$runtime/weston.log" >&2
    echo 'Weston exited before its Wayland socket was ready' >&2
    exit 1
  fi
  sleep 0.1
done
test -S "$runtime/$WAYLAND_DISPLAY" || { cat "$runtime/weston.log" >&2; exit 1; }

cargo test -p wordcraft --test native_window --locked -- \
  isolated_native_shutdown_checkpoints_recovery_before_exit --ignored --nocapture --test-threads=1
