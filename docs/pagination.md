# Pagination audit and verification

## Audit from 207c442

The existing pure-Rust layout engine already shapes and wraps text, paginates sections and columns, applies keep/widow rules, places footnotes, splits table rows, repeats table headers, wraps floating objects and renders equations. Headers/footers, field evaluation, cache reuse, PDF and raster output already exist. This arc reuses them rather than adding a second layout implementation.

The five highest-impact deficiencies found by inspection are:

1. **Section page finalization used the incoming section setup.** This is corrected: outgoing pages now finalize with their own geometry/alignment, and odd/even section starts use physical pages before display-number restarts. Filler-page decoration behavior still needs comparison against Word.
2. **Long footnotes/endnotes.** Long footnotes now split at line boundaries, reserve a bounded page area, continue in note order and produce continuation pages after body text ends. Oversized first lines and indivisible objects advance whole. Endnotes remain laid out as boxes and do not yet continue reliably; notes with complex tables, objects or section transitions need more coverage. The internal 40% page-area cap has not been compared with Word.
3. **Multicolumn footnote reservation is not retroactive.** A note discovered in a later column reduces the current body bottom but does not repaginate already placed earlier columns. Earlier text can overlap that note region. Final-page balancing now handles only complete plain paragraphs in equal-width columns; continuous-section transitions need a section-wide pagination pass. `NextColumn` section starts still behave as page starts.
4. **Header/footer clearance is conservative.** Pagination measures the tallest inherited/default/first/even header and footer story selected by a section, so body text reserves enough room across page types. It does not yet compute the exact page-specific story before pagination, which can leave extra whitespace. Extremely tall header/footer stories can consume the entire body area.
5. **Unsplittable content can overflow.** Tall table headers/non-splittable rows and unbreakable inline objects intentionally overflow an empty page to guarantee progress. Existing bounded table pagination terminates for tested oversized headers/rows, but there is no explicit overflow report or user choice. Text boxes clip excess text; long display equations have no line-breaking implementation. Floats, notes inside tables and vertically merged/nested tables require further auditing.

These are implementation findings, not independently measured Microsoft Word discrepancies. The format vocabulary is described by [OOXML section break values](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.sectionmarkvalues?view=openxml-3.0.1). The physical-page rule used here is internally tested; its interaction with numbering restarts and filler-page decorations still needs independent Word comparison.

## Implemented corrections

### Section page finalization

A page is finalized exactly once before replacing the outgoing section setup. Existing vertical alignment and footnote placement now run with that page's own margins, width and alignment. Subsequent page creation cannot apply alignment a second time. Page creation also clears contextual paragraph-spacing state.

Odd/even section breaks choose the next physical page before applying an incoming page-number restart. When needed, a filler page uses the outgoing section's page dimensions and section identity. It does not consume the incoming section's first-page header or restarted number. Filler pages currently inherit outgoing section header/footer and border behavior; whether Word suppresses those decorations remains unverified. Continuous sections and web view retain their existing flow behavior.

Page-number increments saturate at `u32::MAX` instead of panicking on hostile imported section properties. Saturation prevents a crash; it does not promise meaningful numbering beyond the representable limit.

DOCX import/export and guarded editing restrictions are unchanged. Layout does not mutate the document or preserved package. Recovery, Undo, failed-save protection and atomic file replacement remain existing engine responsibilities. No dependency, UI command or native-print backend is added.

### Long footnote continuation

Footnote stories are laid out as fragments with a 40% page-body budget. Each page reserves the current fragment before body lines are accepted. Remaining lines retain their original order and move to the following page; pages are appended until the final continuation is emitted, including when the body ends first. Line fragments preserve the existing paragraph shaping and are split only at line boundaries. An oversized first line or unbreakable object is emitted whole to guarantee progress. DOCX state and serialization are unaffected. The cap is a WordCraft policy that needs comparative testing. Footnote tables and objects, endnote continuation and mixed-section reflow remain limited.

### Column balancing, story clearance and oversized content

Equal-width multicolumn sections balance complete paragraphs on the final page when the page contains only ordinary body paragraphs. The algorithm preserves existing line breaks, original order and within-column spacing; it declines to move content on complex pages with floats, tables, forced breaks, borders or unsupported layout conditions. This does not balance columns across pages or fully implement `NextColumn` section starts.

Before page creation, layout now resolves header/footer references using link-to-previous inheritance and measures default, first-page and even-page stories. The tallest header moves the body start down; the tallest footer reduces the body bottom with a six-point separation. This safe section-wide reservation may leave extra space on pages selecting shorter stories.

Oversized inline objects stay intact and still allow later body blocks to paginate. The existing table paginator splits rows at line boundaries when possible, while unsplittable rows and oversized repeated headers are placed whole on an empty page to guarantee progress. Such objects may extend beyond page bounds; automatic rescaling, clipping warnings and user-selectable overflow handling are not implemented.

### Regression corpus

Fifteen new tests since the pagination baseline cover:

- Long-note line coverage/order/bounds, body separation, continuation after body end, and Undo/recovery/DOCX reopen with pixel comparison.
- Old-page footnote coordinates against a standalone section; all nine combinations of old/new vertical alignment.
- Odd/even starts with four page-number restarts each, filler ownership, incoming first-page headers, continuous/web flow and maximum page-number overflow.
- A five-page synthetic academic document with centered introduction, footnote, landscape results, odd-page transition, restarted numbering and a multipage table with repeated header rows and split cells. Every paragraph's text clusters must be represented and placed lines must remain inside the page body.
- Equal-width final-page column balance with complete paragraph order preserved.
- Tall first/even/default header and footer variants with the body kept clear on each physical page.
- An oversized inline image that remains intact exactly once and does not prevent following text from being reached.
- Edit → Undo → Redo → durable recovery → atomic DOCX save → reopen, with unchanged tested pagination and exact raster self-comparison across every page.
- The preceding page's raster against its standalone-section rendering, independently of save/reopen.
- Guarded packages remain byte-identical after layout/no-op saving; changing section orientation still fails safely at save and Undo restores the original model.

Generate original synthetic page images locally:

```sh
WORDCRAFT_LAYOUT_CORPUS_DIR=/tmp/wordcraft-section-corpus cargo test -p wordcraft-engine --test section_pagination
cargo test -p wordcraft-layout tests_sections
cargo xtask ci
cargo build --release -p wordcraft -p wordcraft-cli
```

All six CI gates passed on the Phase 4B changes with 865 workspace tests across 47 suites. GUI and CLI release builds passed. The WASM check reports existing target-specific dead-code warnings in speech and file-dialog code; these do not fail CI. Native Clippy passes with warnings denied.

The generated images are WordCraft's own output, with original text and installed fonts; no Microsoft assets or Word-produced files are committed. The portrait introduction and landscape table images were visually inspected on Linux. Visual tests compare WordCraft with itself and with an isolated subsection, not with Microsoft Word reference output. Installed system fonts and font substitution can differ across operating systems. Native printing and Windows/macOS visual verification remain outstanding. There is no Microsoft Word 365 pagination parity claim.
