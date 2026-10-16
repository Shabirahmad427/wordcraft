# Word 365 interoperability benchmark

**Status: one local-only Word reference pair created; DOCX round-trip is blocked and the WordCraft
visual comparison is diagnostic only, not an acceptance result.**
This report does not infer Office provenance from OOXML shape, filenames, metadata, or passing
WordCraft tests.

## Corpus provenance

One purpose-created synthetic styles/typography document was composed for this benchmark, saved as
DOCX in Microsoft Word for Mac, and exported to PDF from Word on macOS 27.0.0 / arm64 Apple Silicon
using the local “Best for printing” export option. Word reported version 16.114 (26101123). The
source DOCX carries the local Word profile identity in its core properties, so the source, reference
PDF, outputs and hashes are kept only in ignored local storage; none is committed or uploaded. No
personal documents were used or processed as corpus material. Counts: **zero authorized external
real-world documents**, **one genuine Word-generated synthetic DOCX**, and **one Word-exported PDF
reference**. Four planned case slots remain blocked.

WordCraft's source-generated fixtures remain useful for deterministic internal regressions. They are
synthetic public-OOXML test packages, not documents produced by Word and not evidence that Word opens
them without a repair warning. The existing chart, academic compatibility and engine tests cover
combinations of equations, reference fields, tables, figures, embedded workbooks, chart error bars,
sections, notes, comments and revisions, but they are not a licensed real-world corpus.

## Local Word reference smoke case

The local-only `simple-formatting` case has one page (Letter, 612 × 792 pt) and includes headings,
bold/italic/underline runs, font faces, Portuguese accents and Unicode. Word and WordCraft PDFs both
have one Letter page and rasterize to 1275 × 1650 at 150 dpi. The diagnostic raster MAE is
204.647/65535 (normalized 0.003123); RMSE is 2970.19/65535 (normalized 0.045322), with 6,569 pixels
changed. Extracted text retained the fixture content; Arabic glyph extraction/shaping differs and
needs review. These numbers are diagnostics, not an acceptance pass.

WordCraft opened the source through its CLI and reported one page. The no-edit DOCX save was blocked
by the existing guarded-package policy with “this edit changes protected document state; saving
would risk unsupported OOXML”; no output DOCX was created. This is a protective rejection, not data
loss, and its model-state cause was not isolated. WordCraft PDF export succeeded. Consequently, DOCX
package round-trip, controlled edits, and reopening a WordCraft-saved DOCX in Word are **blocked/not
run** for this case. The per-case report, DOCX, both PDFs and diagnostic rasters remain under ignored
`/corpus/interop/`; the report contains hashes and is not suitable for publication.

## Synthetic opening-path check for upstream issue #963

On 2026-10-12, two original synthetic DOCX fixtures were used to test image and vertical-textbox
opening through WordCraft. The desktop UI test opens through `file.open`, checks the rendered image
pixels and top-to-bottom text orientation, and compares its page PNG byte-for-byte with the engine's
`file.exportPng` result. A separate real-process CLI test edits, saves, reopens, rasterizes and exports
a multipage document with inline and floating images; both image regions must appear in the rendered
PNG, and the source file bytes must remain unchanged. The headless app window and installed CLI
raster were visually inspected.

These results verify only WordCraft's synthetic path. They did not reproduce the reporter's missing
images or horizontal textbox behavior, and they do not establish that a Microsoft Word-generated
document opens correctly. Issue #963 remains open and unverified pending a minimal, shareable
reproducer. This section is regression evidence, not an addition to the Word-generated reference
corpus above.

Reference documents supplied later must have documented provenance and a redistribution license (or
remain outside the repository under `/corpus/`). Record whether the file was actually saved by Word,
the Word product/channel/build, source or author permission, license, and the original archive SHA-256.
The five required document classes and their evidence fields are listed in
[`examples/word365-corpus-manifest.template.json`](examples/word365-corpus-manifest.template.json).
Schema v2 records public/private/blocked visibility, source and redistribution authorization,
whether Word generated the source, Word channel/build/platform/architecture, fonts and page/export
settings, source and reference hashes, distinct WordCraft DOCX/PDF outputs and hashes, allowed
edits, expected properties, and a result classification. The public template remains a set of
blocked placeholders; it contains no local case paths, hashes,
Word profile metadata or results. The local-only case record tracks the available pair and its
blocked/not-run outcomes outside the repository. A reference pair is not a WordCraft pass.
Do not publish a private manuscript, its PDF, or package fingerprints without the owner's permission.
Word-produced reference files and screenshots remain uncommitted under the clean-room rules; store
them locally in the ignored `/corpus/interop/` directory. Synthetic fixtures can be committed only
as original project-authored test data and must be labelled synthetic.

## Reproducible package round-trip benchmark

`cargo xtask interop` fingerprints a DOCX archive using bounded ZIP reads. It records the archive
SHA-256 and, for every part, its expanded-content SHA-256, CRC, compressed/expanded sizes, compression
method, modification time, Unix mode when present, and part family. Relationship parts and
`[Content_Types].xml` therefore receive explicit individual fingerprints. Duplicate names, unsafe
part paths, unreadable entries, more than 512 entries, a part over 64 MiB, an archive over 128 MiB,
or total expanded content over 64 MiB are refused. These bounds intentionally match WordCraft's
current preservation envelope; documents above them require a separately reviewed benchmark mode.

For each case, save its metadata beside the private files using the schema in
[`examples/interop-case.template.json`](examples/interop-case.template.json). Inspect the original,
make only listed supported edits in WordCraft, save a new DOCX, then compare:

```sh
cargo xtask interop inspect /corpus/interop/case/source.docx /corpus/interop/case/before.json
wordcraft-cli run --file /corpus/interop/case/source.docx \
  --cmd 'text.insert={"text":"Controlled benchmark annotation."}' \
  --save /corpus/interop/case/wordcraft.docx
cargo xtask interop compare /corpus/interop/case/source.docx \
  /corpus/interop/case/wordcraft.docx \
  /corpus/interop/case/case.json /corpus/interop/case/roundtrip.json
wordcraft-cli info /corpus/interop/case/wordcraft.docx
wordcraft-cli convert /corpus/interop/case/wordcraft.docx /corpus/interop/case/wordcraft.pdf
```

The case allowlist names every package part expected to change. Any added, removed, or modified
unlisted part fails the contract. This exposes relationship, content-type, custom XML, bibliography,
chart and embedded-workbook changes by part path and hash. It does not compare XML semantically, prove
that a permitted change is correct, or certify that Word can edit the result. Add a separate
`chart.editData` command and its expected chart/workbook/relationship part allowlist for chart cases.
Keep the original archive immutable; output is always a separate path. Reports contain package part
names and hashes, not text, but part names can still disclose document structure.

To finish the application-level pass, open a WordCraft-saved DOCX in the recorded Microsoft Word
build, note any repair/recovery warning, save a copy from Word, reopen that copy in WordCraft and
compare its package report. That DOCX was not created in the local smoke case because WordCraft's
preservation guard rejected its no-edit save. The one-page raster diagnostic above was run, but
text-anchor and object-edge measurements were not. The planned acceptance tolerances are exact page
count, text-anchor p95 displacement ≤3 pt and figure/table/equation edge displacement ≤3 pt; raster
similarity is diagnostic only because font rasterizers differ. These remain protocol targets, not
achieved results.

## Capability and evidence matrix

“Implemented” describes a constrained WordCraft path. “Internal tests” means synthetic Rust tests;
it does not imply a broad Word-generated corpus. Word verification requires opening and inspecting a
WordCraft-saved case in the actual application, which has not happened in this benchmark.

| Scientific/document feature | Implemented in WordCraft | Internal regression evidence | Word-generated input tested | Verified in Word 365 | Current scope/limitations |
|---|---|---|---|---|---|
| Basic typography and paragraph styles | Partial | Yes | One synthetic Word-generated case opened; save blocked | No | One-page diagnostic only; no round-trip or acceptance-level visual validation |
| Equations and numbering | Partial | Yes | No | No | OMML/model coverage and numbering tests; complex imported structures are preservation-guarded |
| Citations and bibliography | Partial | Yes | No | No | WordCraft citations plus preservation tests for manager fields; no live Zotero/Mendeley/Word check |
| Tables, figures and captions | Partial | Yes | No | No | Editing, pagination and OOXML preservation tests; floating-object fidelity is not independently compared |
| Charts with embedded Excel workbooks | Partial | Yes | No | No | Validated supported chart families; synchronized workbook transactions; complex chart structures remain read-only |
| Scatter charts with asymmetric workbook error bars | Constrained | Yes | No | No | Contiguous single-column literal numeric ranges; formulas, external links and complex ranges rejected |
| Section breaks and columns | Partial | Yes | No | No | Internal mixed-section pagination tests; no Word page-by-page comparison |
| Headers, footnotes and endnotes | Partial | Yes | No | No | Import/export and continuation regressions; Word-specific placement remains unverified |
| Comments and tracked changes | Partial | Yes | No | No | Model and round-trip tests; move tracking and some revision structures remain incomplete |
| DOCX package preservation and guarded saves | Implemented for bounded supported edits | Yes | No | No | Exact package integrity is tested on synthetic and generated inputs; not a general OOXML schema or malware audit |

### Defect log

No production defects were established from the single Word-generated synthetic smoke case: the
guarded save refusal is observed, but its cause was not isolated. Synthetic transaction tests detect
cache/workbook mismatches and unsafe error-bar references, but those are validation cases, not
findings from the real-world corpus. Add one row per reproducible real-file
issue with case ID, source hash, Word build, steps, expected/actual behavior, severity, and the report
artifact path. Severity definitions: **S0** data corruption/security, **S1** document repair or
content loss, **S2** major layout/editing failure, **S3** cosmetic difference.

## Current verification

### Phase 7U.6 local smoke (2026-10-16)

The five-case public manifest template validates as JSON and contains only blocked placeholders;
there are no source paths, fingerprints, or local Word identity in it. The purpose-created local
Word DOCX was inspected with `cargo xtask interop inspect`: 11 package parts and 72,503 expanded
bytes. The resulting fingerprint is in ignored `/corpus/interop/private/outputs/` storage. The
source opened in WordCraft, but the unchanged DOCX save was refused by the preservation guard, so
the package comparator could not be run against a WordCraft DOCX output. The DOCX crate suite passed
(including its 150 unit tests and integration tests), as did `cargo fmt --all -- --check` and
`git diff --check`.

`cargo xtask ci` was attempted on this Mac but did not pass: 581 workspace tests passed, 1 UI test
failed because the local WGPU test environment returned “No adapter found,” and 2 tests were ignored.
Explicit `gl` software and `metal` backend reruns of the failing headless test had the same adapter
failure. No application code was changed in this phase. Hosted workflows passed on implementation
commit `0873ac0c0d04866e48e672dc2d2839906e775a6e`:

- [Printing](https://github.com/Shabirahmad427/wordcraft/actions/runs/38847139170): Linux, macOS and
  Windows passed.
- [Document regressions](https://github.com/Shabirahmad427/wordcraft/actions/runs/38847141664):
  Linux, macOS and Windows regression jobs passed; Linux visual and performance jobs passed.
- [Native packages](https://github.com/Shabirahmad427/wordcraft/actions/runs/38847120300): Linux,
  macOS and Windows package/install validation passed (DEB, DMG and MSI).
- [Packaging lint](https://github.com/Shabirahmad427/wordcraft/actions/runs/38847154127): passed.

These actions validated the implementation and manifest at the listed SHA; this paragraph is a
report-only follow-up and does not change application code or corpus assets.

### Phase 7G baseline check (2026-10-12)

The fork `main` baseline was fetched and verified at `9336c2008da5a9009bfaa066d3f917a2c416fbda`
(WordCraft v0.6.1 source). `cargo xtask ci` passed all six local gates; the workspace reported
2,097 passed, 0 failed and 2 ignored. Linux GUI/CLI release builds and release-mode CLI checks for
Windows GNU and macOS ARM64 completed successfully.

Hosted evidence for this baseline:

- [Document Regressions](https://github.com/Shabirahmad427/wordcraft/actions/runs/38316357991):
  passed on `9336c20`, including Linux golden comparison and both advisory performance workloads.
- [Packaging lint](https://github.com/Shabirahmad427/wordcraft/actions/runs/38316358241): passed on
  `9336c20`.
- [Printing](https://github.com/Shabirahmad427/wordcraft/actions/runs/38310906468): passed on Linux,
  Windows and macOS at `f71d779`; this workflow was not triggered by later regression-only changes.
- [FreeBSD](https://github.com/Shabirahmad427/wordcraft/actions/runs/38306533331) and
  [Windows ARM64](https://github.com/Shabirahmad427/wordcraft/actions/runs/38306533411): passed on
  `6535bb2`; neither workflow was triggered by the later regression-only changes.

These hosted successes verify their respective commits and workflows; they are not one full
cross-platform run on `9336c20`. This Phase 7U.6 update adds one local-only Word-generated synthetic
styles/typography DOCX and a Word-exported PDF. It is not an authorized external real-world document.
The source opened in WordCraft and a PDF comparison was run for diagnostics, but the guarded DOCX
save rejected the no-edit round trip. Word reopen of a WordCraft-saved DOCX, controlled edits,
structural comparison and acceptance-level visual analysis were therefore not run. The other four
manifest categories remain blocked rather than being represented by synthetic fixtures.

Phase 7E added the explicit-policy raster comparator (`cargo xtask visual compare`) and a synthetic
CLI performance harness. Its 2026-10-12 smoke run covered 45 pages, 8 tables, 8 workbook-backed
charts, 8 vector figure shapes, 12 footnotes, 8 endnotes and an equation; measured stage medians and
the exact invocation are in [the performance benchmark report](performance-benchmarks.md). These
results are internal-only. The synthetic UI/CLI image and vertical-textbox checks described above
continue to pass, but do not reproduce issue #963 or constitute Word reference evidence.

Phase 7E.2 adds reviewed, two-page Linux page-raster goldens for an original synthetic document
covering equations, a table, embedded-workbook chart, vector art, a PNG figure, vertical textbox,
footnote and page break. The checked-in PNGs use only WordCraft's bundled open fonts through the
`WORDCRAFT_FONT_DIRS` isolation override. Full page sets are compared with explicit tolerances,
page-count assertions, changed-pixel bounds and archived diff images. A separate advisory benchmark
workflow measures all-page rasterization, layout/load, PDF export and peak process RSS for 45-page
and 133-page synthetic academic documents. It reports medians and MAD without timing-based CI
failure. These baselines remain WordCraft-only evidence; neither the fixtures nor the visual tests
represent Microsoft Word output.

- Local Microsoft Word-generated synthetic DOCX/reference PDF pairs: **1**; authorized external
  real-world cases: **0**; four of five planned corpus categories remain blocked.
- The local source opened in WordCraft and its PDF was raster-compared diagnostically; the unchanged
  DOCX save was rejected by the preservation guard, so package comparison and controlled edits were
  not run.
- Opening a WordCraft-saved DOCX in Word and checking repair warnings/editability: **not run** because
  WordCraft did not produce an output DOCX.
- The one-page raster comparison is diagnostic only; acceptance-level visual measurements: **not
  run**.
- WordCraft package fingerprint/allowlist utility: added with unit tests; no external corpus has yet
  been processed.
- Synthetic compatibility and chart/recovery regressions: tracked in the respective Rust test suites.
- Microsoft Word 365 parity claim: **none**.

The next evidence-producing step requires a consented, redistributable Word-created corpus and a
Windows/macOS environment that can run the actual target application. Until those exist, improvements
can be proven internally but not certified as Word 365 interoperability.
