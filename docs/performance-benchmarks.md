# Synthetic visual and performance regression benchmarks

These benchmarks exercise WordCraft's own document model, layout and renderers. They are not
Microsoft Word reference comparisons. Never use a private manuscript as a benchmark input without
its owner's explicit authorization.

## Raster comparison

`cargo xtask visual compare` reads two PNG page rasters, requires an explicit policy JSON, and emits
a JSON report with dimensions, mean absolute RGBA-channel error, changed-pixel percentage and the
largest per-channel difference. The input dimensions must match, and images larger than 25 million
pixels are rejected. A failing comparison still writes its report. The command never writes or
updates an expected raster:

```json
{
  "maxMeanAbsoluteChannelError": 0.0,
  "maxChangedPixelPercent": 0.0,
  "changedChannelThreshold": 0
}
```

```sh
cargo xtask visual compare expected-page.png actual-page.png policy.json raster-report.json
```

For each pixel, a pixel is counted as changed when any RGBA channel differs by more than
`changedChannelThreshold`; MAE averages all four channel errors over the full image. The report path
must be new, so an existing reference, actual raster, or other file cannot be overwritten as output.

Thresholds are per fixture. Keep strict equality for a pinned renderer/font environment; if the
fixture intentionally allows antialiasing differences, review and record its tolerances explicitly.
Do not regenerate a reference merely to make a failing comparison pass. Raster comparisons should
be paired with geometry/content assertions: pixel similarity alone cannot establish that text,
relationships or unsupported DOCX parts survived.

Existing synthetic regressions cover a WordCraft-rendered embedded image, an inline image, a
floating image, a rotated/vertical text box, tables, charts, equations, multipage text and PDF
output. The GUI-open test checks visible image ink, text-orientation bounds and exact equality
between the app page raster and `file.exportPng`. These are generated test documents; no checked-in
Word-derived golden raster exists. Visual comparison against Microsoft Word remains blocked.

## Large synthetic document pipeline

Build the Linux CLI and run the source-generated workload. The script refuses to reuse a nonempty
output directory and records wall time and peak resident memory for generation, DOCX loading, page
rasterization and PDF export. GNU `/usr/bin/time` reports RSS in KiB; other systems retain timing
but report RSS as `NA` and use Bash's one-second `SECONDS` timing resolution.

```sh
cargo build --release -p wordcraft-cli
BENCH_REPEATS=3 ./scripts/benchmark-synthetic-documents.sh \
  "$PWD/target/release/wordcraft-cli" /tmp/wordcraft-benchmark 1
```

The optional scale is 1–20, and `BENCH_REPEATS` is 1–20 (default 3). Scale 1 creates an original
synthetic 45-page report with repeated academic-style prose, 8 tables, 8 embedded-workbook charts,
8 vector figure shapes, 12 footnotes, 8 endnotes and an equation, then runs load, first-page raster
and PDF export repeatedly. The chart data are validated on import; charts can still be reported
read-only by guarded package editing. This is a timing workload, not a chart-edit interoperability
test. The output directory contains the DOCX, PDF, PNG, metrics TSV and fixture manifest. The DOCX
is generated locally and is not checked in.

### Recorded smoke run

One scale-1 run was collected on 2026-10-12 with the release CLI from code commit
`6535bb260c8e150ae0669f6be61dd57cac4ba588`, Linux x86_64, 8 reported processors, Rust 1.98.1 and
GNU time. The document was 45 pages and 3,012 words. Values below are medians of three process
runs, rounded to hundredths of a second; the generator ran once. Peak RSS is the maximum observed
for each independently launched process, not the combined suite.

| Stage | Median wall time | Median peak RSS |
|---|---:|---:|
| Generate DOCX | 0.03 s | 26,460 KiB |
| Load DOCX and calculate layout/info | 0.04 s | 29,456 KiB |
| Rasterize page 1 at 1 px/pt | 0.04 s | 30,156 KiB |
| Export PDF | 0.05 s | 28,076 KiB |

This is a single host smoke baseline, not a cross-machine performance target. Compare like-for-like
CPU, operating system, font set, build profile, scale and repeat count. The measured fixture does
not yet include raster photographs or very large source packages; it does not measure GUI frame
latency, recovery writes, peak memory across a multi-page render, or chart-workbook editing.

## Compatibility and defect findings

The original reporter's issue #963 remains open and unreproduced: no reporter DOCX or exact
reproduction sequence is available. Synthetic UI and CLI tests verify visible embedded images and
vertical text orientation for their generated cases, including save/reopen and PDF export. They
provide useful regression evidence but do not identify or fix the reported root cause. No
Word-generated files or private documents were processed for this phase.
