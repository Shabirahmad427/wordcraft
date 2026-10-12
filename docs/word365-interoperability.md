# Word 365 interoperability benchmark

**Status: benchmark blocked pending a consented, provenance-documented corpus; no Microsoft Word 365 verification completed.**
This report does not infer Office provenance from OOXML shape, filenames, metadata, or passing
WordCraft tests.

## Corpus provenance

No legally cleared, independently verified Microsoft Word 365-generated reference DOCX files were
available for this benchmark. Private DOCX files exist in the machine's user folders, but none was
selected, processed, or treated as a reference: their Word provenance and reuse permission have not
been established. Microsoft Word and Wine are not available from this Linux checkout; LibreOffice is
present but is not a substitute for Word and was not used as a Word reference. No private documents
were copied, fingerprinted, or added to Git. The verified Word 365 case count is **zero**.

WordCraft's source-generated fixtures remain useful for deterministic internal regressions. They are
synthetic public-OOXML test packages, not documents produced by Word and not evidence that Word opens
them without a repair warning. The existing chart, academic compatibility and engine tests cover
combinations of equations, reference fields, tables, figures, embedded workbooks, chart error bars,
sections, notes, comments and revisions, but they are not a licensed real-world corpus.

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
The five required document classes and their pending fields are listed in
[`examples/word365-corpus-manifest.template.json`](examples/word365-corpus-manifest.template.json).
Every class is currently `blocked-no-consented-source`; this is a status record, not a completed
benchmark.
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

To finish the application-level pass when Word is available, open the saved WordCraft DOCX in the
recorded Microsoft Word build, note any repair/recovery warning, save a copy from Word, reopen that
copy in WordCraft and compare its package report. For visual comparison, export the original and
WordCraft output to PDF from their respective applications. Record page count, text anchor bounding
box displacement, figure/table/equation bounds, and a raster comparison at a fixed DPI with font and
platform details. The planned acceptance tolerances are exact page count, text-anchor p95 displacement
≤3 pt and figure/table/equation edge displacement ≤3 pt; raster similarity is diagnostic only because
font rasterizers differ. No PDF reference comparison or visual measurement has been run here, and
these tolerances are protocol targets, not achieved results.

## Capability and evidence matrix

“Implemented” describes a constrained WordCraft path. “Internal tests” means synthetic Rust tests;
it does not imply Word-generated input. Word 365 verification requires opening and inspecting the
case in the actual application, which has not happened in this benchmark.

| Scientific/document feature | Implemented in WordCraft | Internal regression evidence | Word-generated input tested | Verified in Word 365 | Current scope/limitations |
|---|---|---|---|---|---|
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

No defects were discovered from Word-produced documents because none were available. Synthetic
transaction tests detect cache/workbook mismatches and unsafe error-bar references, but those are
validation cases, not findings from the real-world corpus. Add one row per reproducible real-file
issue with case ID, source hash, Word build, steps, expected/actual behavior, severity, and the report
artifact path. Severity definitions: **S0** data corruption/security, **S1** document repair or
content loss, **S2** major layout/editing failure, **S3** cosmetic difference.

## Current verification

Phase 7E added the explicit-policy raster comparator (`cargo xtask visual compare`) and a synthetic
CLI performance harness. Its 2026-10-12 smoke run covered 45 pages, 8 tables, 8 workbook-backed
charts, 8 vector figure shapes, 12 footnotes, 8 endnotes and an equation; measured stage medians and
the exact invocation are in [the performance benchmark report](performance-benchmarks.md). These
results are internal-only. The synthetic UI/CLI image and vertical-textbox checks described above
continue to pass, but do not reproduce issue #963 or constitute Word reference evidence.

- Word 365-generated reference files: **0**.
- Five-document corpus: **blocked**; no user-selected source documents with verified provenance and
  processing permission.
- Source DOCX fingerprint / WordCraft unchanged round-trip / supported-edit pass: **not run** for
  the five requested classes. Synthetic tests are not substituted.
- Word 365 repair-warning/editability checks: **not run**.
- Word PDF page/raster comparisons: **not run**.
- WordCraft package fingerprint/allowlist utility: added with unit tests; no external corpus has yet
  been processed.
- Synthetic compatibility and chart/recovery regressions: tracked in the respective Rust test suites.
- Microsoft Word 365 parity claim: **none**.

The next evidence-producing step requires a consented, redistributable Word-created corpus and a
Windows/macOS environment that can run the actual target application. Until those exist, improvements
can be proven internally but not certified as Word 365 interoperability.
