# DOCX preservation contract and verification

## Audit from 199822f

Existing implementations already cover custom XML/bibliography package graphs, complex citation fields (including Zotero/Mendeley field instructions), OMML equations, footnotes, tables, lists, sections, headers/footers, tracked changes, atomic saves and complete native recovery archives. Those implementations are reused. They do not establish lossless preservation of arbitrary OOXML.

The first selected defect was chart loss: `read_drawing` recognized pictures and shapes but discarded chart references; export consequently omitted charts and embedded workbook dependencies. Reproduction with the installed 199822f CLI and our original scientific manuscript fixture removed all four chart-related parts: chart XML, chart relationships, chart style XML and embedded XLSX. This arc fixes that narrowly scoped failure.

## Chart preservation

Chart drawings use the existing opaque inline-object model. Their drawing markup is canonicalized, relationship IDs are rebound in the containing story, and drawing object IDs are regenerated uniquely. Inline and anchored chart drawing markup is retained for DOCX output; WordCraft layout does not reproduce chart positioning or appearance.

The existing bounded custom XML dependency traversal now also preserves chart graphs. Chart XML, dependent styles, embedded workbooks and dependent relationship files retain their exact imported bytes, part names and content types. External relationships are retained without fetching their targets. Unsupported extension markup inside these copied parts is retained unchanged. This is a part preservation promise, not byte identity of the entire ZIP or original drawing markup.

Limits: 512 parts/relationships, 64 MiB preserved graph data, 128 KiB bookkeeping and 1 MiB per drawing, with a 16,384-node drawing bound. Missing dependencies/content types, corrupt ZIP entries, ambiguous case-insensitive ZIP names, malformed relationship records, unknown drawing namespaces, unsupported drawing types and generated-part collisions cause explicit errors on the checked paths. Failure during save leaves the original file and live document metadata unchanged. Markup-compatibility attributes within chart drawings are rejected when namespace canonicalization cannot safely retain their meaning.

`file.open`, `file.save` and `file.info` expose `compatibilityWarnings`; the desktop File → Info page displays them. Charts appear as visible placeholders. DOCX/DOTX/DOCM/DOTM preserve chart data, but chart rendering, printing and editing remain unavailable. Other export formats may lose chart data. Existing macro behavior is unchanged; the new repeated round-trip fixture test covers DOCX, DOTX and DOCM.

Undo and recovery retain both the opaque object and graph bytes. Recovery also retains the exact opened package independently. Removing a chart can leave preserved orphan parts in that save; preservation does not synchronize embedded workbook data with edits to nearby tables. Graphs shared with regenerated document parts, or colliding with custom XML graph output, can be rejected rather than rewritten unsafely.

## Automated compatibility corpus

The corpus is generated from original Rust fixture code in `crates/docx/tests/common/charts.rs`; no proprietary documents or assets are committed. Scientific manuscript, thesis and tracked report variants combine charts and XLSX data with citation fields, bibliography settings, equations, footnotes, a table, nested numbering, a header, PAGE footer, caption cross-reference and tracked insertion. These are synthetic regressions, not certified Word-produced reference files.

Generate inspectable fixtures and run the package regressions:

```sh
WORDCRAFT_COMPAT_CORPUS_DIR=/tmp/wordcraft-compatibility-corpus cargo test -p wordcraft-docx --test charts
cargo test -p wordcraft-engine --test opaque_charts
cargo xtask ci
cargo build --release -p wordcraft -p wordcraft-cli
```

Six package integration tests exercise three generations and multiple output flavors, body/header relationship scopes, unique drawing IDs, missing/corrupt/unsafe dependencies and rejected exports. Two engine tests cover edit/save/reopen, Undo, recovery restoration, failed-save integrity and multi-page layout/raster self-comparison. Two unit tests cover corrupt secondary ZIP data, ambiguous package names and hostile drawing/bookkeeping limits. All six CI checks passed with **831 tests**, including **10 new tests** in this arc.

The pixel comparison is WordCraft before/after restoration and round-tripping, not a Microsoft Word reference comparison. Linux verification does not establish native Windows/macOS behavior. Independent Word reference outputs and a consented real-world corpus are unavailable in this environment. There is no full Microsoft Word 365 parity claim.

## Remaining high-impact deficiencies

1. General preservation of unknown OOXML markup, styles, properties and package relationships is still incomplete. Ordinary export regenerates modeled parts and can lose unmodeled content outside the new checked drawing paths.
2. OLE, content-control bindings, legacy VML and SmartArt need separate preservation contracts. An unrecognized DrawingML graphic type can now be rejected, but this is not a comprehensive detector of every unsupported object.
3. Citation/bibliography and equation models need independent real-world interoperability verification; preserved bibliography XML is not synchronized with edited citation sources.
4. Pagination, complex wrapping, footnote continuation and column balancing still need independent Word/PDF comparisons. Charts currently contribute placeholder layout rather than their rendered geometry.
5. Real-world package coverage and independently validated visual references remain required before accepting arbitrary DOCX files as lossless.

Public format references: [ECMA-376 Part 1 fundamentals](https://download.microsoft.com/download/e/1/4/e14fb96f-83b8-4a2a-84db-7fa8acbe061a/Office%20Open%20XML%20Part%201%20-%20Fundamentals.pdf), [chart reference relationship definition](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.charts.chartreference?view=openxml-3.0.1), and [Office Open XML chart/package examples](https://learn.microsoft.com/en-us/office/dev/add-ins/word/create-better-add-ins-for-word-with-office-open-xml). These document format syntax; they do not certify WordCraft interoperability.
