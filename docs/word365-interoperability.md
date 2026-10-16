# Word 365 interoperability benchmark

**Status: one local-only Word reference pair exists. Simple directly mapped run selection formatting
now has structural, save/reopen and package-allowlist coverage. A controlled substring italic edit
to the private sample saved and reopened in WordCraft; Microsoft Word save/reopen remains unverified.
The visual comparison is diagnostic only, not an acceptance result.**
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

WordCraft opened the source through its CLI and reported one page. An unchanged save was refused by
the guarded-package policy. A direct append at document end was also refused because that edit had no
provable source mapping. Replacing the final, directly mapped body paragraph with its original text
plus a controlled annotation succeeded. The package comparison found exactly one changed part,
`word/document.xml`; all other package parts and relationships matched. Reopening and saving that
output again retained the annotation and passed the same allowlist. This demonstrates one supported
isolated edit, not general package editability. The saved DOCX was opened and visually inspected in
Word for Mac without a repair warning; Word-side editing/save/reopen remains **not run**. The source,
edited files, both PDFs, comparison reports and diagnostic rasters remain
under ignored `/corpus/interop/` storage; reports containing fingerprints are not suitable for
publication.

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

The WordCraft-saved DOCX was opened in the recorded Microsoft Word build and visually inspected.
Word displayed the edited document without a repair/recovery warning; it showed one page and retained
the inserted sentence, heading, mixed formatting, and multilingual paragraph. The file was not saved
or modified in Word, so a Word-save/reopen-back-in-WordCraft cycle remains untested. The one-page
raster diagnostic above was run, but
text-anchor and object-edge measurements were not. The planned acceptance tolerances are exact page
count, text-anchor p95 displacement ≤3 pt and figure/table/equation edge displacement ≤3 pt; raster
similarity is diagnostic only because font rasterizers differ. These remain protocol targets, not
achieved results.

## Capability and evidence matrix

“Implemented” describes a constrained WordCraft path. “Internal tests” means synthetic Rust tests;
it does not imply a broad Word-generated corpus. Word verification here is limited to opening and
visually inspecting one WordCraft-saved synthetic case in the actual application.

| Scientific/document feature | Implemented in WordCraft | Internal regression evidence | Word-generated input tested | Verified in Word 365 | Current scope/limitations |
|---|---|---|---|---|---|
| Basic typography and paragraph styles | Partial | Yes | One synthetic Word-generated case; single-run text edit saved and reopened | Yes, visual reopen only | Format-only edits are refused by the preservation guard; no Word-side save or acceptance-level visual validation |
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
The source opened in WordCraft; Phase 7U.9 subsequently verified one guarded single-run text edit,
package comparison, WordCraft reopen/resave and a visual reopen in Word. Formatting edits remain
refused, and acceptance-level visual analysis was not run. The other four manifest categories remain
blocked rather than being represented by synthetic fixtures.

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
- The local source opened in WordCraft and its PDF was raster-compared diagnostically. A directly
  mapped single-run text insertion passed package comparison and WordCraft reopen/resave; formatting-
  only and mixed-format edits were safely refused.
- Opening the saved output in Word for Mac: **visually inspected, no repair/recovery warning**. Word
  did not save the file; a Word-side save/reopen cycle remains untested.
- The one-page raster comparison is diagnostic only; acceptance-level visual measurements: **not
  run**.
- WordCraft package fingerprint/allowlist utility: added with unit tests; no external corpus has yet
  been processed.
- Synthetic compatibility and chart/recovery regressions: tracked in the respective Rust test suites.
- Microsoft Word 365 parity claim: **none**.

The next evidence-producing step requires a consented, redistributable Word-created corpus and a
Windows/macOS environment that can run the actual target application. Until those exist, improvements
can be proven internally but not certified as Word 365 interoperability.

## Phase 7U.11 follow-up (2026-10-16)

The guarded editor now maps decoded selection offsets to raw XML byte boundaries, including Unicode
and entity references. It splits only direct paragraphs made of simple text runs, preserving each
source run's raw markup and limiting changed package data to `word/document.xml`. Synthetic
regressions cover bold selection across mixed runs, an italic selection over an existing bold run,
paragraph-mark bold insertion, save/reopen and package-part preservation; an engine test covers
undo/redo. The authorized private Word sample accepted a controlled substring italic edit. WordCraft
reopen and both existing interop package-allowlist comparisons passed. The file was not opened,
edited or saved in Microsoft Word during this phase, so Word-side save/reopen is still unverified.

### Phase 7U.12 WordCraft reopen and export check (2026-10-16)

Reopened the Phase 7U.11 edited copy through the CLI: one page, seven paragraphs, 42 words, and the
expected source text remained present. PDF export completed. The package comparator passed against
the original with only `word/document.xml` changed; all 10 other package parts retained identical
expanded-content hashes. This confirms the saved file survives another WordCraft read and the
allowlisted package edit remains isolated. It does not verify formatting appearance or a Microsoft
Word save/reopen cycle. All generated files and reports remain ignored local data; no fingerprints
are published.

## Phase 7U.9 follow-up (2026-10-16)

On macOS arm64, controlled edits were attempted against the authorized, purpose-created Word sample.
The guard rejected formatting-only editing and replacing text in a paragraph with multiple differently
formatted runs; both refusals are intentional because the writer cannot prove those edits preserve the
original run structure. A single-run body paragraph accepted a text insertion. The source stayed
unchanged; after save, package comparison passed with only `word/document.xml` changed. A second
WordCraft open/save cycle retained the insertion and passed the same package allowlist. The existing
`formatting_structure_settings_and_object_edits_fail_safely` regression covers the guarded refusal;
no production code change was justified.

The saved output was opened and visually inspected in Microsoft Word for Mac 16.114 (26101123) on
macOS 27 arm64. Word showed one page, the inserted text and the original visible formatting, with no
repair/recovery warning. Word was not used to edit or save the file, so Word-side save/reopen remains
untested. These assets and comparison outputs remain in ignored local storage; no private hashes,
metadata, screenshots or documents are published.

The Arabic PDF extraction regressions `right_to_left_text_extracts_in_logical_order` and
`arabic_pdf_text_mapping_preserves_logical_source_characters` pass. Extraction of the authorized
sample retained the source Arabic characters in logical order (with bidi control characters around
the run). Comparison with Word's PDF exposed a different mapping in Word's output, not a demonstrated
WordCraft mapping defect. No PDF code change was made.

Windows ARM64 workflow [38893268625](https://github.com/Shabirahmad427/wordcraft/actions/runs/38893268625)
verified cross-compilation, ARM64 MSI installation, PE architecture, CLI version, and uninstall. It
did not run the GUI or validate editing, rendering, clipboard, DOCX save/reopen, or PDF export.

## Phase 7U.10 follow-up (2026-10-16)

Synthetic preservation tests now prove that a directly mapped whole source run can have bold/italic
changed and that a paragraph can switch to an existing style reference while all other ZIP parts
remain byte-identical. These are low-level model edits; they do not demonstrate that ordinary editor
selection commands can express the edits.

On the authorized local Word-created sample, `format.italic` selected through the CLI was refused
with the guard diagnostic `run formatting boundaries changed and cannot be mapped to source OOXML`.
The selected range splits a source run, and selecting through the paragraph end can also change the
paragraph mark. The current splice code cannot prove those changes safe. No guard was weakened, and
selection-based formatting remains unresolved. The WordCraft → Word → save/reopen cycle was not
performed because this session's desktop controls allowed inspection but not UI interaction. No
private sample, content, hashes, or screenshots were added to the repository.

An earlier pre-merge `cargo xtask ci` run failed one egui test because the Mac test process reported
WGPU `No adapter found`. After merging upstream, a fresh `cargo xtask ci` run passed all six gates:
formatting, Clippy, workspace tests (582 passed, two ignored), assets, layers, and WASM. The focused
DOCX preservation (19), DOCX chart preservation (8), and merged shared-PDF writer (44) suites also
passed. This local run does not establish GUI success on other platforms.

On 2026-10-16, the fork merged the fetched upstream main tip at merge commit `336a74f3`. It adds the
shared PDF writer, the Nix workflow and associated tests; fork-only Arabic extraction regressions
were retained. Targeted PDF and DOCX suites passed after the merge. No release or installed
application was updated.
Independent Intel Mac and Windows ARM64 GUI behavior remain unverified.

This remains one synthetic Word-created sample, not an external real-world corpus. Formatting edits,
Word-side save/reopen, and acceptance-level visual comparison remain unverified; no Microsoft Word
parity claim is made. Issues #2276, #2253, #2246, #2259 and #2278 retain their previously documented
native-verification limitations.

## Phase 7U.8 follow-up (2026-10-16)

On macOS arm64, the authorized local Word-generated synthetic sample was copied to temporary output,
edited by replacing one directly mapped final body paragraph, saved, reopened and saved again. Both
package comparisons passed with `word/document.xml` as the sole allowed changed part; the annotation
remained after reopening. The original DOCX was not modified. A direct append at document end remains
safely rejected because the imported model cannot prove its source mapping. All output and comparison
data remain local and ignored.

The Word PDF extraction maps a source Arabic Yeh to presentation-form Farsi Yeh. WordCraft's PDF
extracts the original logical Arabic word. This did not demonstrate a WordCraft text-mapping defect;
a regression now checks that WordCraft PDF text preserves the source Arabic characters.

FreeBSD run [38878636541](https://github.com/Shabirahmad427/wordcraft/actions/runs/38878636541)
built successfully and first exposed two layout pagination tests with platform-dependent default
font metrics. Those tests now specify bundled Source Serif 4 and 12 pt exact line spacing, retaining
their original assertions. On rerun [38921987082](https://github.com/Shabirahmad427/wordcraft/actions/runs/38921987082),
both layout tests passed; the PDF crate then exposed two Hebrew extraction failures because the VM
installed no Hebrew font. The VM now installs the open `noto-sans-hebrew` package. Run
[38928171576](https://github.com/Shabirahmad427/wordcraft/actions/runs/38928171576) confirmed that
resolved the Hebrew failures, but exposed one Arabic-vowel extraction mismatch. The VM now also
installs `noto-naskh-arabic`; run
[38941789177](https://github.com/Shabirahmad427/wordcraft/actions/runs/38941789177) confirmed the
same mismatch with Noto installed. Root cause: a shaped Arabic cluster had three glyphs for two
source characters (a mapped tanween mark and contextual letter glyphs). PDF extraction split
clusters only when glyph and character counts matched, so multiple glyphs inherited the same
source range and duplicated `ب`. PDF export now maps identifiable marks and one source character,
then assigns any extra visual glyph an empty Unicode range. A focused regression and the full PDF
suite pass locally with Noto Naskh selected. The required Linux/macOS/Windows workflows remained
green; FreeBSD is a separate nightly matrix, not part of the previously green 7U.6 gates. The fix
still needs a new FreeBSD run on its final commit.

Windows ARM64 run [38893268625](https://github.com/Shabirahmad427/wordcraft/actions/runs/38893268625)
cross-compiled the GUI and CLI, built an ARM64 MSI, installed it on a hosted ARM64 runner, checked
executable architecture, ran the CLI version command and uninstalled. It did not launch the GUI or
exercise native editing, rendering, clipboard, DOCX save/reopen or PDF export. The MSI was unsigned
because signing secrets were unavailable.
