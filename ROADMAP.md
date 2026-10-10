# WordCraft roadmap

**Stage: alpha** · next: beta, ~15 points (60% → 75% ready for real work) and ~150–230 h away

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (readiness table with hours per audience; full number back to the additive weighted sum, 60%; mainstream and essentials numbers added; alpha gate checked; previously major: full re-measure against Word for Mac 16.113.4; restructured to the craftrules progress-docs standard) · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

WordCraft aims for complete parity with Microsoft Word, then goes further on speed, openness and
agent control. This page is the summary; the assessment is
[`docs/target-app-parity.md`](docs/target-app-parity.md), the work list is
[`docs/gaps.md`](docs/gaps.md), and milestones and the current focus are in
[`docs/roadmap.md`](docs/roadmap.md).

## Headline numbers (2026-10-10)

## Alpha

**What we mean by alpha:** someone can write, format and review real documents every day, open
the .docx files they receive and send back files Word opens cleanly, on every desktop platform
from a signed download, without losing work.

| Alpha requirement | State |
|---|---|
| Editing, formatting, styles, lists, tables, headers/footers, references, review | done (first versions, tested) |
| Pagination fidelity: wrap, row splitting, hyphenation, drop caps, line numbers, borders | done this arc |
| Comments in margin balloons, track changes | done |
| Never-crash standard (no panics, hostile-param fuzzing, panic guard) | done |
| Autosave and crash recovery | AutoSave plus separate native recovery snapshots, restart restore/discard/text comparison; Linux crash tests pass, Windows/macOS power-loss verification outstanding |
| **DOCX fidelity against a corpus of real-world files** (open, render, round-trip, fix) | Synthetic preservation regressions started; independent real-world corpus verification still missing |
| Long footnotes that continue onto later pages | landed (40% page-area cap; continuation pages keep remaining lines visible). Page-specific note styles and Word parity are unverified |
| Balanced multicolumn layout | missing (≈4 h) |
| Native printing (today printing goes through PDF) | missing (≈4 h) |
| Signed builds for macOS, Windows, Linux, FreeBSD and web | pipeline written; **needs the GitHub remote and release secrets from the owner** |
| Real app icon / mascot art | placeholder; **needs owner art** |

So: alpha is roughly **85% of the way**, with about **20–25 hours** of agent work left, plus two
owner actions (push to `github.com/storytold/wordcraft` and enable the release secrets, and supply
the icon art).

## Parity by area

| Area | Status | Parity |
|---|---|---|
| Ribbon/menu catalog coverage | **390 / 431 (90.5%)** | measured (`cargo xtask parity` → [`docs/parity-checklist.md`](docs/parity-checklist.md)) |
| **Feature breadth** (weighted, incl. dialog options, styles, languages beyond the ribbon) | **~80%** | estimated |
| **Ready for real work** (full target) | **~60%** (55–63%) | estimated, additive weighted sum over the dimensions |
| **Mainstream practitioner** | **~55%** | estimated: weekly-work depth × discounts for interaction (×0.93), stability (×0.90), file exchange (×0.90) |
| **Essentials user** | **~63%** | estimated: core-feature depth × discounts for launch (×0.90), discoverability (×0.95), opening received files (×0.92) |
| Remaining to **beta** | **~150–230 h** of Opus 5.5 agent wall-clock (gaps #1–#7) | estimated |
| Remaining to **full parity** with Word desktop | **~750–1,250 h** (70–80% parallelizable) | estimated |
| Commands / tests / code | 431 commands · 852 tests + 10 property blocks · ~88,700 lines of Rust | measured (source) |
| Releases | v0.4.0 (2026-10-10): signed macOS universal DMG, Windows MSI x64/x86/arm64, Linux AppImage/deb/rpm/Flatpak x86_64 + aarch64, FreeBSD, web | measured |

Hours are calibrated from this repo's PRs (equations #191: ~9.7k lines in ~5 h; `.doc` reader
#145: ~4.5k lines in ~10 h; layout fidelity PRs run 5–10× slower per line). The earlier "≈120–150 h
to 100%" (2026-10-06) left out localization, multilingual proofing, ecosystem, hardware, PDF Reflow
and `.doc` writing, and priced the DOCX corpus at 10 h; see
[methodology](docs/target-app-parity.md#methodology). The three readiness numbers, their weights,
discounts and the user evidence (120 issues from outside users, 4 praise, 0 "switched from Word",
~21 open core-path bugs) are in
[readiness numbers](docs/target-app-parity.md#readiness-numbers-full-mainstream-practitioner-essentials-user).

| Audience | Ready | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | **~60%** | **~650–1,100 h** (70–80% parallelizes) | Breadth: charts/SmartArt/Draw, localization and proofing languages, East Asian and RTL typography, `.doc` writing, PDF Reflow, ecosystem; plus everything below |
| Mainstream practitioner | **~55%** | **~300–500 h** (~65% parallelizes) | DOCX real-world fidelity and preserving charts/SmartArt, pagination fidelity, formatting revisions, dialog depth, objects, stability |
| Essentials user | **~63%** | **~90–140 h** (~50% parallelizes) | Install/launch stability, opening files people send (corpus subset, chart fallbacks), native printing, picture handling, English spelling depth |

**Why alpha:** the core workflows (write, format, styles, lists, tables, references, review, mail
merge, open and save .docx) work end to end and ship as signed builds on every desktop platform
and the web. **Why not beta:** DOCX has never been tested on real-world files; charts, SmartArt and
OLE objects are dropped silently; pagination differs from Word (no Aptos-metric font, no column
balancing or footnote continuation); formatting revisions are lost; there is open field evidence of
a startup crash and several hangs; no native printing. Gaps #1–#7 in [`docs/gaps.md`](docs/gaps.md)
are the beta list. All six core workflows pass the
[alpha gate](docs/roadmap.md#alpha-gate-core-workflows) end to end on macOS, with save and reopen;
the two partial ones (real-world .docx, printing) complete, roughly.

## By dimension

| Dimension | Parity | Hours to full | Doc |
|---|---|---|---|
| Features (depth) | 68% | 210–335 | [target-app-parity.md](docs/target-app-parity.md#feature-areas) |
| UI/UX fidelity | 65% | 65–100 | [ui-parity.md](docs/ui-parity.md) · [typing-parity.md](docs/typing-parity.md) |
| File formats | 60% | 185–300 | [file-format-parity.md](docs/file-format-parity.md) |
| Layout and pagination fidelity | 55% | 90–145 | [layout-parity.md](docs/layout-parity.md) |
| Hardware | 40% | 20–40 | [hardware-parity.md](docs/hardware-parity.md) |
| Localization | 25% | 80–130 + native review | [localization-parity.md](docs/localization-parity.md) |
| Performance | 75% | 10–20 | [layout-parity.md](docs/layout-parity.md#performance) |
| Stability | 50% | 25–45 | [gaps.md](docs/gaps.md) (#3) |
| Platforms | 95% (beyond Word on Linux, BSD, web) | 5–10 | this page |
| Ecosystem and plugins | 15% | 40–80 + owner decisions | [gaps.md](docs/gaps.md) (#22) |
| AI features | 40% (beyond Word for external agents; no in-app assistant) | 15–30 | [gaps.md](docs/gaps.md) (#25) |

## Features

| Area | Parity | Hours |
|---|---|---|
| M0 | Skeleton + vertical slice (model, layout, render, engine, Word-style UI, CLI, MCP, web) | **done** |
| M1 | DOCX I/O | **done** (first version; corpus testing next) |
| M2 | Home tab complete | **done** |
| M3 | Insert tab | mostly done (charts/SmartArt/icons/3D missing) |
| M4 | Layout + Design tabs | **done** except column balancing |
| M5 | Tables | **done** (row splitting landed) |
| M6 | References | first version done; long footnote continuation landed |
| M7 | Review | **done** (margin balloons landed) |
| M8 | View | mostly done |
| M9 | Mailings | done (first version) |
| M10 | File/Backstage | mostly done; native printing missing |
| M11 | Draw + objects | text wrap and text boxes done; ink, rotation, grouping missing |
| M12 | Formats breadth (PDF, ODT, RTF, HTML, MD, TXT) | **done** (first versions) |
| M13 | Performance budgets | on track (1.4 ms relayout) |
| M14 | 1.0 polish, packaging, signing | pipeline written; waiting on remote and secrets |

## Recently landed
- M1: Preserve document-attached custom XML/bibliography data, properties, dependent relationships and content types with bounded graph traversal. Repeated round-trip, hostile-input and multi-page raster/Undo regressions added; independent Word verification remains pending.
- M5: Drag unmerged column borders and unsplit row borders to resize tables, with one undo step and Escape cancellation (#217, local fix).
- M7: Accept/Reject resolves tracked paragraph breaks; DOCX preserves their revision tags across saves (#229, local fix).
- M9: Select Recipients opens a CSV entry dialog; invalid input stays open for correction (#240, local fix).
- Right-to-left and Persian text: bidi reordering (UAX #9), Arabic-script shaping, right-to-left
  paragraphs (Left-to-Right / Right-to-Left Text Direction), visual arrow keys, `w:rtl`/`w:cs`/`w:szCs`
  /`w:bCs`/`w:iCs`/`w:rFonts w:cs`/`w:lang w:bidi` round-trip, Persian font substitutes.
- Ukrainian interface catalog: all existing 771 labels, Ukrainian locale detection and a persisted language choice (#85).
- Brazilian Portuguese interface catalog: all existing 771 labels, `pt-BR` locale detection and a persisted language choice (#127).
- Text wraps around floating pictures and shapes; text boxes lay out their own text.
- Table rows split across pages between lines (Can't Split honoured, header rows repeat).
- Drop caps; automatic hyphenation with Word's 0.25" hyphenation zone; soft hyphens.
- Page borders measured from the page edge, line numbers, vertical alignment, column separators.
- Comment balloons in a markup area beside each page, with leader lines to their anchors.

## Next
See [production audit](docs/production-audit.md): protect DOCX data and add durable crash recovery, then footnote continuation, column balancing, native printing and drawing depth. No independent Word 365 parity certification is available.

- M14: Native file dialogs run asynchronously; delayed responses are cancelled after document replacement (#94, PR #246 plus regression guard).

- M14: Release font inputs include Simplified Chinese; native UI can use installed CJK fallback faces (#241, PR #248).

- M8: Interface theme can follow the operating system, with persisted Light/Dark/System choices (#115, PR #249).

- M6: Mendeley tab imports RIS exports, searches references, inserts citations/bibliographies and refreshes them; source libraries survive DOCX saves. Cloud sign-in and Mendeley Cite field editing remain future work.

## Phase 2 — professional document recovery (2026-10-10)

- Arc 1: native immutable recovery archive foundation, complete editor/media/passthrough snapshots and exact source DOCX retention. Restoring requires Save As; incomplete writes are ignored and live sessions are protected by OS locks. Storage regression tests include abrupt process exit and corrupted archives. Automatic scheduling and the restore/discard/compare interface remain for arc 2. See [recovery storage contract](docs/recovery.md).

- Arc 2: periodic background snapshots (first modified frame, then 15 seconds), replacement/exit checkpoints, startup recovery interface, restore/discard/compare/original-package commands and six command-label translations. Tests include an actual process kill during a write, CRC corruption, restart DOCX/raster fidelity, scheduler timing and UI Save Changes/Cancel protection. Native platform APIs preserve WASM builds; browser recovery remains separate. See docs/recovery.md for bounded storage and preservation limits.
- Verified: both recovery arcs passed `cargo xtask ci` (fmt, Clippy with warnings denied, workspace tests, assets, layers and WASM). Final suite: **821 passing tests**, including **16 added recovery tests** across storage, engine commands, DOCX/raster restoration and UI. Headless recovery window rendered and visually inspected. No native Windows/macOS power-loss or independent Word parity verification was performed.

## DOCX compatibility follow-up from 199822f (2026-10-10)

- Fixed chart loss during open/edit/save: preserve chart drawing references and their byte-exact dependency graphs, including embedded XLSX, style XML and relationship files. Reuse the existing opaque object model and bounded custom XML graph traversal. Charts remain preservation-only placeholders with explicit File Info/automation warnings.
- Reject checked corrupt package entries, ambiguous part names, invalid chart dependencies and unsafe drawing namespaces instead of exporting lost data. Failed saves retain original files and document metadata. Recovery and Undo retain chart content.
- Added 10 regressions: synthetic manuscript/thesis/tracked report package cases, per-story relationships, hostile input, failure integrity and recovery/raster restoration. `cargo xtask ci` passed all six gates with **831 tests**. See [the compatibility contract](docs/docx-compatibility.md) for reproduction, preservation limits and the remaining audit backlog.
- Arbitrary DOCX losslessness and Microsoft Word 365 visual parity remain unverified. OLE, SmartArt, content controls, unmodeled styles/metadata and native Windows/macOS checks remain outstanding.

## Generic DOCX preservation from 6807535 (2026-10-10)

- Added bounded package-wide preservation with guarded main-body text splices. Original namespaces, extension attributes, SDT metadata/bindings, unknown styles/numbering/settings/metadata, SmartArt/OLE/ActiveX-related data, embedded and orphan parts survive tested saves without executing active content. Original relationship scopes/IDs and content types are retained.
- Strict XML/OPC graph checks reject malformed or missing dependencies, conflicting identifiers/types, escaping targets and oversized packages. Unsafe structural/formatting/object edits, protected-control edits and signature-breaking edits fail explicitly. Existing chart/Excel/custom XML, Undo, recovery and atomic-save mechanisms are reused.
- Guarded saves retain original core metadata/statistics; automatic revision/timestamp updates are deferred. Normal native documents keep ordinary editing. New conservative import budgets: 512 parts / 64 MiB total; XML/model inspection 16 MiB. See [the compatibility contract](docs/docx-compatibility.md) for operation and verification limits.
- Added 19 regressions, including all-part byte fidelity, duplicate protected/unprotected text, native editing, corrupt/oversized input, failed saves and recovery. All six `cargo xtask ci` gates passed with **850 tests** (19 new). GUI/CLI release builds passed; the release CLI preserved the synthetic package byte for byte, and the File Info warning was rendered and visually inspected. Independent Word rendering and native Windows/macOS verification remain outstanding.

## Pagination correctness from 207c442 (2026-10-10)

- Audited five priority defects: section finalization/odd-even starts, note continuation, multicolumn reservation/balancing, header/footer clearance and unbreakable content overflow. See [pagination audit and regression contract](docs/pagination.md).
- Fixed outgoing-page finalization before section geometry changes, exactly-once vertical alignment/footnote flushing, physical odd/even filler placement before numbering restarts and hostile page-number overflow. Preserved existing continuous/web flow and DOCX guarded editing restrictions.
- Added nine regression tests covering mixed portrait/landscape sections, short academic notes, centered body text, first-page headers, restarted numbering, multipage tables, Undo/recovery/save/reopen, exact raster self-comparison and preservation-safe failures. Generated portrait/landscape pages were visually inspected. All six CI gates passed with **859 tests**, including **nine new regressions**. GUI/CLI release builds passed.
- Phase 4B now covers long-note continuation, conservative final-page column balancing, resolved story clearance and oversized-content progress. Retroactive multicolumn note reflow, complex column transitions, overflow reporting, native printing and independent Word/Windows/macOS comparison remain follow-up work.


## Phase 4B pagination reliability (2026-10-10)

- Long footnotes split at line boundaries, reserve up to 40% of the page body, continue in order and emit continuation pages after body text ends.
- Equal-width multicolumn sections balance complete plain paragraphs on their final page while preserving order and line breaks. Pages containing floats, tables, forced breaks, borders or complex paragraph rules remain on normal flow.
- Body clearance now measures inherited/default/first/even headers and footers before pagination. It reserves the tallest selectable story across the section, favoring safe spacing; page-specific clearance is not yet optimized.
- Oversized inline objects remain whole and permit following text to progress. Existing table pagination continues to split rows where possible and places unsplittable rows/repeated headers oversized on an empty page without retry loops. This can extend beyond page bounds and is not clipped or automatically rescaled.
- Added regression cases for long-note continuity plus Undo/recovery/DOCX reopen, balanced paragraph order, tall first/even headers and footers, and an oversized inline image. Existing table regressions cover splittable and `cant_split` rows and oversized repeated headers.
- All six CI gates passed with **865 tests** (47 suites); GUI and CLI release builds succeeded. Internally verified on Linux only. Long-note continuation with complex note tables/objects and mixed section transitions, retroactive multicolumn footnote reflow, complex column balancing, overflow diagnostics/choices, native printing and independent Word/Windows/macOS comparison remain open. These checks do not establish Microsoft Word pagination parity.
