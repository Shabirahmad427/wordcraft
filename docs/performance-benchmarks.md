# Synthetic visual and performance regression benchmarks

> **Last updated:** 2026-10-15 · **Change:** Phase 7U.4 headless editor responsiveness measurements, zoom-slider reproduction and remaining native rendering limits

These benchmarks exercise WordCraft's own document model, layout and renderers. They are not
Microsoft Word reference comparisons. Never use a private manuscript as a benchmark input without
its owner's explicit authorization.

## Reviewed page-raster regressions

`cargo xtask visual compare-pages` compares every numbered page in two directories, requires an
explicit policy JSON, checks the page count and dimensions, and emits a JSON report with per-page
mean absolute RGBA-channel error, changed-pixel percentage, maximum channel delta and the bounding
box of changed pixels. On a failed same-sized page it writes a red-highlighted diff PNG. A missing
page or geometry mismatch is recorded with its page number and diagnostic. Reports and diff files
use create-new semantics; an existing report or diff directory is never overwritten. The single
page `cargo xtask visual compare` command remains available. PNGs are bounded to 25 million pixels
and 128 MiB each.

```json
{
  "maxMeanAbsoluteChannelError": 0.45,
  "maxChangedPixelPercent": 1.0,
  "changedChannelThreshold": 24
}
```

```sh
cargo build --release -p wordcraft-cli -p xtask
./scripts/run-visual-regression.sh target/release/wordcraft-cli /tmp/wordcraft-visual-run
# Or compare two already-rendered page sets directly:
cargo xtask visual compare-pages expected-pages/ actual-pages/ tests/visual/policy.json report.json diff-pages/
```

For each pixel, a pixel is counted as changed when any RGBA channel differs by more than
`changedChannelThreshold`; MAE averages all four channel errors over the full image. The report path
must be new, so an existing reference, actual raster, or other file cannot be overwritten as output.

The committed reviewed baseline is `tests/visual/golden/linux-x86_64/`. It is a two-page synthetic
WordCraft document containing academic body text, OMML equations, a table, an embedded-workbook
column chart, an original vector illustration, an embedded PNG figure, a vertical text box, a
footnote and an explicit page break. Its manifest records fixture provenance, renderer scale and
font configuration. These images were visually reviewed and are not Microsoft Word output.

The fixture and baseline use WordCraft's bundled open fonts. `WORDCRAFT_FONT_DIRS` is a native-only
font-search override: when set to an empty directory, system-font discovery is disabled while the
bundled Source Sans 3, Source Serif 4, Inter and JetBrains Mono faces remain available. Normal users
who do not set this variable retain system-font discovery. The visual workflow sets this override
for every CLI process, so proprietary or host-installed fonts cannot affect these committed
rasters. It runs on `ubuntu-24.04` x86_64; other platforms need separate reviewed baselines before
their raster output is treated as a gate. Rendering remains sensitive to deliberate changes in the
bundled font files, shaping, pagination or drawing code.

The policy permits MAE up to 0.45 RGBA levels and at most 1% of pixels with a channel delta above
24. Those values tolerate minor raster edge noise while page count and raster dimensions remain
exact. Changed bounds and diff images identify where to investigate. No test or CI job updates a
baseline. To propose one, render into a fresh directory with `scripts/render-visual-fixture.sh`,
inspect every page and diff, then add a new reviewed PNG and update the manifest and
`ATTRIBUTION.md` in the same change. Do not refresh a baseline merely to make a failing test pass.
The fixture also asserts required text markers and exact page count before comparison. Pixel
similarity still cannot establish DOCX package integrity or Microsoft Word compatibility.

Existing synthetic regressions cover a WordCraft-rendered embedded image, an inline image, a
floating image, a rotated/vertical text box, tables, charts, equations, multipage text and PDF
output. The GUI-open test checks visible image ink, text-orientation bounds and exact equality
between the app page raster and `file.exportPng`. These are generated test documents; no checked-in
Word-derived golden raster exists. Visual comparison against Microsoft Word remains blocked.

## Large synthetic document pipeline

Build the Linux CLI and run the source-generated workload. The script refuses to reuse a nonempty
output directory and records wall time and peak resident memory for generation, DOCX loading plus
layout calculation, rasterization of every page, and PDF export. GNU `/usr/bin/time` reports RSS in
KiB; other systems retain timing but report RSS as `NA` and use Bash's one-second `SECONDS` timing
resolution. Rasterization runs one CLI process per page; its stage time is total wall time for all
pages and peak RSS is the largest per-process peak, not a combined simultaneous allocation.

```sh
cargo build --release -p wordcraft-cli
BENCH_REPEATS=3 ./scripts/run-performance-suite.sh \
  "$PWD/target/release/wordcraft-cli" /tmp/wordcraft-performance
```

Scale 1 is the 45-page, 2,568-word report with repeated academic-style prose, 8 tables,
8 embedded-workbook charts, 8 vector figure shapes, 12 footnotes, 8 endnotes and an equation. Scale
3 is a larger 133-page, 7,600-word generated document with 24 tables, 24 charts, 24 shapes, 36
footnotes and 24 endnotes. Both remain synthetic; the chart mapping is inspected but this is not a
chart-edit interoperability test. Each scenario produces the DOCX, PDF, every page raster, raw
metrics TSV, environment manifest and statistical JSON summary. No generated document is checked
in.

`cargo xtask perf summarize metrics.tsv summary.json manifest.tsv` reports sample count, median,
median absolute deviation (MAD), minimum/maximum wall time, median RSS and maximum RSS per stage.
`cargo xtask perf compare baseline.json current.json report.json` emits percentage deltas and a
`largeChangeAdvisory` at 25% or greater. It always exits successfully for valid reports: timing is
never a shared-runner CI failure. The report marks `comparisonAllowed` only when scale, repeat count,
build profile, OS/kernel, Rust version, processor count/model and bundled font configuration match. For meaningful trend
comparisons, archive CI artifacts from the same runner class and compare matching scales; otherwise
the report still includes measurements but must be treated as non-comparable. The workflow retains
raw PNGs, PDFs, DOCX files, summaries and logs as 30-day run artifacts.

The dedicated `Document regressions` workflow runs synthetic visual and performance jobs for
relevant pull requests, main-branch changes and manual dispatch. It has `contents: read`, no
secrets, and immutable action pins. Its timing job preserves artifacts without setting machine-time
thresholds. The Linux raster job uses bundled fonts and explicit tolerances. Neither job reads
private files or requires external services.

### Current bundled-font run

On 2026-10-12, the final release CLI was measured on Linux x86_64 (kernel 6.8.0-146-generic),
Rust 1.98.1, 24 reported processors, GNU `/usr/bin/time`, and isolated bundled open fonts. The
benchmark ran three load/layout, all-page raster and PDF-export repetitions per workload; generation
ran once. The actual page counts were exactly 45 and 133. Times below are medians; MAD is included
to show run-to-run spread. RSS is the median of the process peak measurements. Full summaries also
include maximum RSS and minimum/maximum wall time. `layout-core` comes from the CLI's internal
layout timer and therefore has no separate RSS measurement.

| Workload | Stage | Median wall time | MAD | Median peak RSS |
|---|---|---:|---:|---:|
| 45 pages | Generate DOCX | 0.030 s | — | 26,772 KiB |
| 45 pages | Load and layout | 0.030 s | 0.000 s | 28,424 KiB |
| 45 pages | Layout core | 0.022 s | 0.001 s | n/a |
| 45 pages | Rasterize all pages | 1.820 s | 0.010 s | 29,696 KiB |
| 45 pages | Export PDF | 0.040 s | 0.000 s | 27,532 KiB |
| 133 pages | Generate DOCX | 0.060 s | — | 50,400 KiB |
| 133 pages | Load and layout | 0.110 s | 0.010 s | 39,916 KiB |
| 133 pages | Layout core | 0.041 s | 0.000 s | n/a |
| 133 pages | Rasterize all pages | 10.550 s | 0.040 s | 40,884 KiB |
| 133 pages | Export PDF | 0.110 s | 0.010 s | 38,372 KiB |

The local metric files and full summaries were generated under `/tmp/wordcraft-performance-commit-41db640`;
the recorded source commit is `41db640e243f2246521009bb57cfada995610b15`. CI preserves equivalent
raw metrics and summaries as run artifacts. These are single-host reference measurements, not
performance limits or comparisons against Microsoft Word. In particular,
all-page raster duration grows with the number of separate CLI processes and is sensitive to host
scheduling.

## Phase 7U.4 — editor responsiveness and zoom (#2259, #2256)

At baseline commit `ac748b3a`, the existing `perf_probe` example was run in release mode on the
Apple Silicon M4 macOS development host (Darwin 27.0.0, Rust 1.99.0). The probe uses the real
`WordApp::logic`/`WordApp::ui`, document layout and page raster code through `egui_kittest`; it has
no native window, GPU presentation surface or physical input device. The bundled open fonts were
used with system-font discovery disabled. Each deterministic, source-generated DOCX contains
ordinary A4 paragraphs and paginates to 3, 35 or 140 pages (36, 420 or 1,680 paragraphs). Three
complete probe runs were collected per size; each run samples 40 typed characters in the longest
paragraph, 120 wheel-scroll frames, 300 trackpad frames, six zoom transitions and 34 resizes. The
probe reports synchronous elapsed time around each headless frame to tenths of a millisecond;
`layout_ms` and `render_ms` are the engine's layout and page-raster timers. Three run medians/MADs
are therefore rounded summaries, not microsecond-precision claims.

| Workload | Typed-key frame median, before → after | Per-run key-frame p95, before → after | Layout median, before → after | Page raster median, before → after |
|---|---:|---:|---:|---:|
| 3 pages | 4.0 → 4.0 ms (MAD 0.0 → 0.0) | 4.3 → 4.1 ms | 0.1 → 0.1 ms | 3.5 → 3.5 ms |
| 35 pages | 8.2 → 8.3 ms (MAD 0.0 → 0.1) | 8.7 → 8.6 ms | 0.5 → 0.5 ms | 7.2 → 7.2 ms |
| 140 pages | 10.0 → 10.0 ms (MAD 0.1 → 0.0) | 10.5 → 10.4 ms | 2.0 → 2.0 ms | 7.3 → 7.2 ms |

“After” is the same probe and fixtures with the status-bar zoom readout fix in this phase. The
changes are within run variability; this UI-only fix did not measurably improve typing. The reported
~50 ms typing delay was not reproduced on this M4 headless harness, but this is not the reporter's
MacBook Neo or native keyboard-to-display latency. Cursor movement, selection, Undo/Redo, editing
near tables/images, GUI redraw cadence and CPU utilization were not independently timed by this
probe; OS process inspection is denied in this environment. No comparison with v0.5.0 is claimed
because an equivalent historical UI benchmark is unavailable.

Issue #2256 was reproduced in a held-pointer UI event sequence: the zoom readout changed its
intrinsic width as its digits changed, which moved the neighboring slider track under a stationary
pointer. Before the fix, the same held location alternated between 90% and 95%; with smart aiming
disabled during diagnosis it cycled 93%, 94%, 95%. The readout now reserves a fixed width, leaving
the slider track stationary. A 30-frame held-at-95% regression passes and asserts both a constant
95% value and unchanged slider position. This is a synthetic egui interaction test, not physical
trackpad or mouse validation.

Issue #2278 was not reproduced. No trigger or document is available from its reporter. This Mac
probe has no native graphics surface and does not exercise device loss, surface recreation,
minimize/restore or Intel HD Graphics 4600. The issue remains open; the Windows backend's existing
adapter/backend diagnostics are retained. No renderer fallback or black-background behavior was
changed.

### Earlier smoke run

Previous scale-1 smoke results were captured before the all-pages and bundled-font controls were
added, so they are not comparable to the current workload and are retained only as historical
context. Fresh values must come from `run-performance-suite.sh`; compare them only when the summary
reports a matching environment.

| Stage | Median wall time | Median peak RSS |
|---|---:|---:|
| Generate DOCX | 0.03 s | 26,460 KiB |
| Load DOCX and calculate layout/info | 0.04 s | 29,456 KiB |
| Rasterize page 1 at 1 px/pt | 0.04 s | 30,156 KiB |
| Export PDF | 0.05 s | 28,076 KiB |

This is the earlier 2026-10-12 single-host smoke baseline at code commit
`6535bb260c8e150ae0669f6be61dd57cac4ba588`, not the current all-page workload or a cross-machine
performance target. The current script also does not measure GUI frame latency, recovery writes,
simultaneous multi-page rendering memory or chart-workbook editing.

## Compatibility and defect findings

The original reporter's issue #963 remains open and unreproduced: no reporter DOCX or exact
reproduction sequence is available. Synthetic UI and CLI tests verify visible embedded images and
vertical text orientation for their generated cases, including save/reopen and PDF export. They
provide useful regression evidence but do not identify or fix the reported root cause. No
Word-generated files or private documents were processed for this phase.
