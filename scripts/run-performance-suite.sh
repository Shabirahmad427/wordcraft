#!/usr/bin/env bash
# Collect repeatable small and larger synthetic workloads; timing changes remain advisory.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <wordcraft-cli> <new-output-directory>" >&2
  exit 2
fi
cli=$1
out=$2
root=$(cd "$(dirname "$0")/.." && pwd)
if [[ ! -x "$cli" ]]; then echo "CLI is not executable: $cli" >&2; exit 2; fi
if [[ -e "$out" ]] && [[ -n "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "output directory must be absent or empty: $out" >&2
  exit 2
fi
mkdir -p "$out"
repeats=${BENCH_REPEATS:-3}
"$root/scripts/benchmark-synthetic-documents.sh" "$cli" "$out/scale-1" 1
"$root/scripts/benchmark-synthetic-documents.sh" "$cli" "$out/scale-3" 3
printf 'scenarios\tscale-1 (45-page reference), scale-3 (larger academic workload)\nrepeats\t%s\n' "$repeats" > "$out/suite.tsv"
printf 'Performance artifacts: %s\n' "$out"
