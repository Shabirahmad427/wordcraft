#!/usr/bin/env bash
# Render an original fixture and compare it to reviewed, platform-specific page goldens.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <wordcraft-cli> <new-output-directory>" >&2
  exit 2
fi
cli=$1
out=$2
root=$(cd "$(dirname "$0")/.." && pwd)
baseline="$root/tests/visual/golden/linux-x86_64"
policy="$root/tests/visual/policy.json"
if [[ ! -d "$baseline" ]]; then echo "reviewed visual baselines are missing: $baseline" >&2; exit 2; fi
"$(dirname "$0")/render-visual-fixture.sh" "$cli" "$out"
cargo xtask visual compare-pages "$baseline" "$out/pages" "$policy" "$out/report.json" "$out/diffs"
printf 'Report: %s/report.json\n' "$out"
