# Where WordCraft falls short of Word

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7T packaging audit and local macOS DMG correction; ranked gap estimates are unchanged · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

The work list. One entry per shortfall, ranked by how much it stops a Word user from switching
(**B** = blocks beta). Each says what's missing, the evidence, who it hurts, an Opus 5.5 agent
wall-clock estimate, and which parity document tracks it. Agents: pick from the top, check open PRs
first (`gh pr list`), and update this file and the parity doc when a gap closes.

Native save reliability: synthetic cases on macOS reproduced loss through the predictable temporary-file name. Saves now use owned, exclusively created siblings, synchronize before replacement and clean up on failure; originals survive pre-replacement errors. This narrows save-path reliability under gap #3; the listed field failures and broader stability work remain open. See [save-path guarantees](file-format-parity.md#native-save-path-guarantees).

The [cross-platform audit](cross-platform-audit-2026-10-12.md) ranks the five current evidence gaps across Word reference files, native regression coverage, pagination/PDF comparison, complex OOXML edits and recovery/runtime testing. Older row descriptions below retain historical estimates and may understate preservation features already implemented; the audit distinguishes current source evidence from unverified application behavior.

Phase 7M verifies that retained theme font/color selectors rebind in the shared model and that
unknown style XML survives safe text edits. Guarded imports still reject theme/style mutations
until a safe package merge is available; see the [DOCX preservation contract](docx-compatibility.md#phase-7m-theme-bindings-and-style-preservation).

Phase 7I adds [synthetic DOCX/PDF layout regressions](cross-platform-layout-regressions.md) and fixes a missed clipping warning for header-anchored images. Gap #4 still needs independent Word page geometry and font metrics; this internal corpus does not change its estimate.

Phase 7R fixes a local editing focus defect in Insert › Table's size grid. This interaction was not a separately ranked gap and does not change the gap estimates; desktop validation limits are recorded in [desktop GUI validation](desktop-testing.md).

Phase 7T found that a copied macOS DMG app failed strict signature verification because HFS
imaging added FinderInfo attributes after bundle signing. The corrected local DMG and copied app
now verify and pass an isolated native edit/open/quit smoke. Linux and Windows installed-package
runtime jobs are pending. This packaging gap is tracked in the [native installation audit](native-installation.md)
and does not change the ranked Word-feature estimates.

## Ranked

| # | Gap | B | Evidence | Impact | Hours | Doc |
|---|---|---|---|---|---|---|
| 1 | **DOCX never tested on real-world files.** No consented corpus of real-world files has been opened, rendered and round-tripped and compared with Word page by page | B | `cargo xtask interop` now fingerprints package parts and checks per-case allowlists; no independently verified Word 365 source files are available (0 Word cases). Existing `crates/docx/tests` and the generated academic corpus are synthetic fixtures | Everyone: the files people receive are the files that break | 30–50 | [Word 365 benchmark status](word365-interoperability.md) |
| 2 | **Ink and drawing canvases are dropped on open and lost on save, silently**; OLE objects show only their picture | B | No ink or drawing-canvas (`wpc`) handling in `crates/docx`; group shapes (`wpg`) read and written (#267). Charts and SmartArt are drawn (#292) and, with OLE objects, written back with all their parts on save (#319) | Documents with diagrams drawn in Word or pen notes lose content without warning | 4–8 to preserve ink and drawing canvases; 45–70 to render and edit charts and SmartArt (#13) | [file-format-parity.md](file-format-parity.md) |
| 3 | **Crashes and freezes in the field**: startup crash on Intel UHD Graphics (#170) (lag on GPUs without a DirectX 12 driver fixed, #316), freezes (#77, #59 Thai fonts, #31 font selector), no writes without admin on Windows 11 (#218), broken shape tool (#142), images can't be moved (#82) | B | 62 open issues, ~15 of them crashes, hangs or data-path failures | Anyone hit loses work or trust; the never-crash standard covers panics, not hangs or GPU driver faults | 20–30 | [target-app-parity.md](target-app-parity.md) (Stability) |
| 4 | **Pagination differs from Word**: Aptos has no metric-matched substitute; columns don't balance; custom footnote continuation separators and notices aren't read; legacy compatibility options ignored; no page-by-page comparison harness | B | [layout-parity.md](layout-parity.md); [typing-parity.md](typing-parity.md) Known gaps | Page counts and line breaks differ, which matters for forms, legal and academic work | 30–50 + owner (font) | [layout-parity.md](layout-parity.md) |
| 5 | **Move tracking and content controls** are lost (`w:moveFrom`/`w:moveTo` read as insert/delete; `w:sdt` unwrapped). Formatting revisions landed (#41): `w:rPrChange`/`w:pPrChange`/`w:tblPrChange`/`w:trPrChange`/`w:tcPrChange`/`w:sectPrChange`/`w:numberingChange` round-trip, are recorded while tracking, accepted/rejected and shown in balloons; still missing: tracked style-definition changes and table/section changes made in WordCraft | B | Issue #41; source grep | Legal and editorial review workflows; templates with form controls | 8–12 | [file-format-parity.md](file-format-parity.md) |
| 6 | **Desktop printing is partial**: shared preview; Linux/macOS CUPS; Windows PrintTicket/Windows.Data.Pdf adapter compile-checked but driver behavior, nonblocking submission and app-level cancellation remain unverified/incomplete; web uses browser printing | B | `crates/ui-egui/src/backstage.rs`, `crates/ui-egui/src/printing.rs`, `apps/wordcraft/src/print.rs`; issue #15 | Everyone who prints | 5–8 | [printing.md](printing.md) |
| 7 | **Dialog depth**: multiple Word workflows and advanced options remain unavailable; the upstream sync added ribbon/keyboard customization and more dialogs but does not close the gap | B | `Dialog` enum in `crates/ui-egui/src/dialogs.rs` | Power users reach for dialogs the ribbon alone doesn't cover | 20–30 | [ui-parity.md](ui-parity.md) |
| 8 | **Proofing is English only**; no language per run, no dictionaries for other languages | | `crates/proof` has one dictionary; Word ships 120 proofing tools; issues #25, #40, #100 | Every non-English writer | 25–45 | [localization-parity.md](localization-parity.md) |
| 9 | **Password-protected files** can't be opened or saved | | #55; `docbin` detects and refuses RC4/XOR | Corporate and legal users | 6–10 | [file-format-parity.md](file-format-parity.md) |
| 10 | **Objects**: grouping and group resize/rotated-member fixes landed upstream; contour tight/through wrapping and wrap-side controls landed upstream (#354); rotated text-box text, shape effects, WordArt, connectors, Drawing Canvas and Icons remain incomplete | | [parity-checklist.md](parity-checklist.md) | Newsletters, flyers, reports with diagrams | 20–30 | [target-app-parity.md](target-app-parity.md) |
| 11 | **Interface languages**: 8 of the 12 key languages missing (hi, ar, fr, id, de, ko, vi, pt-PT); no mirrored RTL interface; ~5% of strings still English in the six translated catalogs | | [localization-parity.md](localization-parity.md) | Most of the world's writers | 50–80 | [localization-parity.md](localization-parity.md) |
| 12 | **Draw tab / ink**: pens, pencil, highlighter, eraser, Select and Hide Ink landed (#307); Lasso Select, Add Pen, Ink to Shape and Ink Replay landed (#395); missing: Ink to Math, arrows in Ink to Shape, pressure, Word's own ink (`w14:contentPart`/InkML) | | [parity-checklist.md](parity-checklist.md) (Draw 9/11) | Pen and tablet users | 4–8 | [hardware-parity.md](hardware-parity.md) |
| 13 | **Charts and SmartArt editing** (editing charts and SmartArt from Word, more chart types and formatting) | | `insert.chart` and Chart Design basics landed (#409): WordCraft's own charts are editable; charts from Word stay read-only (no embedded-workbook editing), no combo/3D/radar/stock charts, axis or series formatting; Insert › SmartArt landed (#497) for graphics WordCraft makes | Reports and business documents | 35–55 | [target-app-parity.md](target-app-parity.md) |
| 14 | **`.doc` can't be written; metafile pictures and Word 6/95 files refused** | | `crates/docbin` is read-only | Users exchanging files with old Word or legacy systems | 25–40 | [file-format-parity.md](file-format-parity.md) |
| 15 | **References depth**: 4 of Word's 12 bibliography styles; sources not saved in DOCX (#169 open); no EndNote/Mendeley desktop integration | | Word's `Resources/Style` lists 12 styles | Students and researchers | 10–15 | [target-app-parity.md](target-app-parity.md) |
| 16 | **Right-to-left completeness**: RTL sections and tables, kashida, RTL in HTML/ODT/RTF; eight older RTL bug reports to re-verify after #207 (#215, #211, #199, #66, #63, #48, #19) | | [layout-parity.md](layout-parity.md) | Arabic, Persian, Hebrew writers | 10–15 | [layout-parity.md](layout-parity.md) |
| 17 | **East Asian typography**: vertical text, ruby/Phonetic Guide, Enclose Characters, document grid, Asian Typography options | | Catalog misses `format.phonetic`, `format.enclose`, `para.asianTypography` | Chinese, Japanese and Korean documents | 20–30 | [layout-parity.md](layout-parity.md) |
| 18 | **Screen-reader access** to the document canvas untested | | AccessKit enabled; no tests | Blind and low-vision users; public-sector procurement | 10–15 | [ui-parity.md](ui-parity.md) |
| 19 | **RTF and ODT depth**: notes, comments, revisions, sections, RTL | | ~1,500 lines each in `crates/formats` | Users exchanging with LibreOffice or older tools | 18–27 | [file-format-parity.md](file-format-parity.md) |
| 20 | **Performance on large real-world documents** unmeasured (500+ pages, many pictures) | | Only the 188-page sample benchmark; scrolling a 1,096-page one-table document went from ~600 ms to ~2–5 ms a frame (#811), but opening it still lays out for seconds | Thesis and book authors | 10–20 | [layout-parity.md](layout-parity.md) |
| 21 | **Mail merge data sources**: no Outlook/Contacts, Access or database sources, no email merge; CSV, typed lists (#247) and Excel/OpenDocument spreadsheets (#334) only | | Select Recipients reads .csv/.tsv, .xlsx/.xlsm and .ods | Office administrators | 4–8 | [target-app-parity.md](target-app-parity.md) |
| 22 | **Ecosystem**: no VBA execution (macros preserved only), no Office add-ins, no cloud storage, no real-time co-authoring | | `.docm` keeps macros (#172) | Enterprise users with macro templates; teams | 40–80 + owner decisions | [target-app-parity.md](target-app-parity.md) |
| 23 | **PDF Reflow** (open a PDF as an editable document, Word for Windows) | | | Users who edit PDFs they receive | 30–50 | [file-format-parity.md](file-format-parity.md) |
| 24 | **Customize Ribbon / Keyboard**, Quick Access Toolbar customization; ~100 of Word's ~250 shortcuts | | [ui-parity.md](ui-parity.md) | Power users | 10–16 | [ui-parity.md](ui-parity.md) |
| 25 | **In-app AI assistant** (Copilot-style draft, rewrite, summarize) | | Chat PR #178 open | Users expecting Copilot | 15–30 + owner (provider) | [target-app-parity.md](target-app-parity.md) |
| 26 | **Minor formats**: MHT, Works/WordPerfect, saving Word 2003 XML | | Word XML Document (Flat OPC) opens and saves, Word 2003 XML opens (#548) | Rare | 15–28 | [file-format-parity.md](file-format-parity.md) |
| 27 | **View windows**: Side by Side, Synchronous Scrolling, Arrange All, Switch Windows | | [parity-checklist.md](parity-checklist.md) (View 25/29) | Comparing documents | 3–5 | [ui-parity.md](ui-parity.md) |
| 28 | **Dictate** | | `tools.dictate` missing | Dictation users | 8–15 + owner (speech model) | [hardware-parity.md](hardware-parity.md) |
| 29 | **Equations**: ink equations | | #191 notes; long display equations wrap at operators (#326) | Maths-heavy documents | 2–3 | [layout-parity.md](layout-parity.md) |

**Beta needs #1–#7** (~150–230 h with the stability and layout work they imply): they are the
blocking gaps, and closing them lifts file formats, layout, stability and UI enough to take the
full number from ~60% to ~75%. Everything else is depth on the way to full parity.

## By kind

The same gaps grouped the way the parity documents are, for agents working in one area.

- **Feature gaps:** #2 (preserve), #5, #10, #12, #13, #15, #17, #21, #25, #27, #28, #29; Clipboard
  pane, Manage Styles, Group (open PRs exist for the first three — review and merge before
  re-implementing). Paste Special, Advanced Find, Column Selection, Style Inspector, custom table
  styles, and Draw Table and Eraser (#303) landed on 2026-10-10. Paste from other programs keeps
  their formatting (system clipboard HTML/RTF), Merge Formatting keeps emphasis, Paste Special lists
  what the system clipboard holds, and Linux/Wayland clipboard pictures paste (#687, #833);
  still missing: the Paste Options button after a paste, and RTF on macOS and X11.
- **UI/UX gaps:** #7, #18, #24; crop handles on the canvas (rotation handle landed, #332); context menus for objects (the table's landed; it still lacks Border Styles); Linux title bar theming (#78); File ›
  Options › Animation effects › Use system setting reads macOS (Reduce motion) and the Linux/BSD
  desktops (`apps/wordcraft/src/animations.rs`) but hasn't been tried on a Mac or in a real
  GNOME or KDE session yet.
- **File-format gaps:** #1, #2, #5, #9, #14, #19, #23, #26; embedded fonts; glossary/building blocks.
- **Hardware gaps:** #6, #12, #28; scanner/Continuity Camera insert; KDE soft text (#140).
- **Localization gaps:** #8, #11, #16, #17; native-speaker review of every catalog (human).
- **Stability and performance gaps:** #3, #20.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-13 | trivial | Insert › Table's grid selection now returns keyboard focus to the first cell; verified with a failing-before/passing-after UI regression; native app recovery data was left untouched |
| 2026-10-13 | minor | Synthetic cross-platform layout/PDF corpus and header-float clipping diagnostic pass hosted Linux/macOS/Windows; independent Word comparison gap remains |
| 2026-10-12 | minor | Ranked five cross-platform evidence gaps; shared DOCX export now rejects XML-forbidden model text and validates final packages, without claiming Word 365 parity |
| 2026-10-12 | minor | Added bounded DOCX package fingerprint and round-trip allowlist benchmark; no Word-created corpus or Word visual result is claimed |
| 2026-10-11 | trivial | Formatting revisions landed (#41); gap #5 narrowed to move tracking and content controls, 8–12 h |
| 2026-10-11 | trivial | Omitted `w:pPrDefault` stays omitted across saves; explicit empty/default properties and later spacing edits remain writable. Synthetic preservation checks only; readiness estimates unchanged |
| 2026-10-11 | trivial | #12 narrowed: Lasso Select, Add Pen, Ink to Shape and Ink Replay landed (#395) |
| 2026-10-10 | trivial | Draw Table and Eraser landed (#303); removed from the feature-gap list |
| 2026-10-10 | trivial | Tabs and Borders and Shading dialogs landed (#320); removed from the dialog-depth list |
| 2026-10-10 | trivial | Columns, Symbol and Field dialogs landed (#321); removed from the dialog-depth list |
| 2026-10-10 | trivial | #2 narrowed: charts and SmartArt drawn (#292), and with OLE objects kept on save (#319); group shapes landed (#267); ink and drawing canvases remain |
| 2026-10-10 | trivial | #4: a document's own footnote separators and continuation notice are drawn and kept on save (#352) |
| 2026-10-10 | trivial | #4: long footnotes continue onto the next page (#352) |
| 2026-10-10 | trivial | Define New Multilevel List and Track Changes Options dialogs landed (#328); removed from the dialog-depth list |
| 2026-10-10 | trivial | #10: free rotation and flips for pictures, shapes and text boxes, with a rotation handle (#332); text boxes rotate their frame; their text stays upright |
| 2026-10-10 | trivial | #10: Drawing Canvas landed (#344); in-canvas dragging of members remains |
| 2026-10-10 | trivial | #29: long display equations wrap across lines (#326) |
| 2026-10-10 | trivial | #3: Windows lag on GPUs without a DirectX 12 driver fixed with an OpenGL fallback (#316) |
| 2026-10-10 | minor | #8: Polish proofing (spelling, grammar, hyphenation) and a proofing language per run landed; the gap is now the other languages (20–40 h) |
| 2026-10-10 | trivial | #6: desktop File › Print opens the system PDF viewer (#286) |
| 2026-10-10 | trivial | Table Properties dialog landed (#44); removed from the dialog-depth list |
| 2026-10-10 | trivial | Merged main: landed features removed from the feature-gap list |
| 2026-10-10 | minor | Beta note restated: full number back to the additive ~60% (method aligned with the standard, no new evidence) |
| 2026-10-11 | trivial | #491: vertical sections landed; vertical text removed from the East Asian gap |
| 2026-10-10 | major | First version, from the full re-measure: 29 ranked gaps with evidence, impact and hours |
| 2026-10-10 | trivial | #307: Draw tab pens, eraser and Hide Ink landed; ink gap narrowed |
| 2026-10-12 | trivial | #811: large-table scrolling fixed; performance gap (#20) narrowed |
