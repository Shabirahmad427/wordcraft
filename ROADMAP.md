# WordCraft roadmap

## Phase 7I — shared layout and PDF regression (2026-10-12)

- Added synthetic DOCX import/layout/PDF assertions for paper and margins, page count, extracted text, images and fonts, table/notes/equation/column/vertical-text preservation, plus a saved-edit PDF check. The suite parses PDFs with an independent reader and avoids binary cross-OS comparisons. Coverage and limits: [cross-platform layout regressions](docs/cross-platform-layout-regressions.md).
- Reproduced an off-page header image with no clipping diagnostic, then fixed shared `layout` overflow analysis to inspect floating objects in header/footer and other non-body stories. The minimal test failed before the fix and passes afterward.
- The hosted Document regressions matrix now includes layout and PDF tests and native CLI smokes on Linux, macOS and Windows. All six local `cargo xtask ci` gates passed (2,101 tests passed, 2 ignored); native Apple Silicon GUI/CLI release builds and both version smokes passed. New hosted Linux/Windows results remain pending; no Word 365, Windows ARM64 runtime or macOS Intel runtime claim follows from this change.

## Cross-platform shared-core audit (2026-10-12)

- Audited the shared Rust document path and hosted regression coverage from fork commit `9d023894`; the five ranked remaining gaps are in [the cross-platform audit](docs/cross-platform-audit-2026-10-12.md). The Word 365 reference corpus still has zero verified pairs, so no new application-level compatibility claim is made.
- DOCX export now validates every completed package and rejects XML-forbidden model characters instead of silently dropping them. Synthetic regression covers all four Word package flavors. Document Regressions now requests shared DOCX/engine tests on Linux, macOS and Windows and a Linux GUI/CLI release build; hosted results remain to be verified.
- The native macOS gate exposed a font-sensitive two-column footnote test fixture; its anchor was moved within column two and the test passes with both bundled-only and system-font resolution. No layout algorithm or parity claim changed.
- Hosted [Document Regressions run 38320170798](https://github.com/Shabirahmad427/wordcraft/actions/runs/38320170798) passed all six jobs for `3a480b9`, including native shared-core suites on Ubuntu/macOS/Windows and Linux GUI/CLI release build and CLI smoke. [Packaging lint 38320171307](https://github.com/Shabirahmad427/wordcraft/actions/runs/38320171307) passed. No Word 365 application comparison or Windows ARM64/macOS Intel runtime result was obtained in this arc.

## Phase 7E — visual regression and performance evidence (2026-10-12)

- Added bounded PNG comparison, complete numbered-page set comparison, page geometry/count assertions, changed-pixel bounds and red-highlighted diagnostic images. The reviewed two-page baseline is a synthetic academic document rendered with bundled open fonts; comparison reports never overwrite expected images.
- Added repeatable 45-page and 133-page synthetic academic workloads with three-run median/MAD summaries for generation, load/layout, all-page rasterization and PDF export, plus peak process RSS. CI retains diagnostics and performance artifacts without timing-based failure thresholds. See [performance benchmark methodology and results](docs/performance-benchmarks.md).
- Final validation: `cargo xtask ci` passed all six gates (fmt, Clippy, tests, assets, layers and WASM); the final workspace run passed **2,097 tests, 0 failed, 2 ignored**. Linux GUI/CLI release builds and Windows GNU/macOS ARM64 release compile checks passed. The first CI rerun caught two Clippy issues (fixed); the next caught an incomplete performance-test fixture (fixed); an added advisory-regression test initially needed an explicit Rust type (fixed); the final full rerun passed. No missing native library or unresolved cross-symbol error remained.
- Visual tests remain synthetic and deterministic within their configured environment; no Word-derived baseline exists. Word 365 reference comparisons remain blocked (zero consented, provenance-verified files). Issue #963 remains open and unverified: the reporter's source file and exact steps are unavailable; generated UI/CLI fixtures continue to pass without establishing the reporter's root cause.
- Phase 7E.2 adds complete numbered-page comparisons with strict page-count/dimension checks, bounded per-pixel tolerances, changed-pixel bounding boxes and reviewable diff PNGs. Checked-in Linux goldens cover a two-page synthetic academic layout; `WORDCRAFT_FONT_DIRS` isolates these runs to bundled open fonts. Baselines are never changed by tests.
- Performance reporting now rasterizes every page in the 45-page and larger 133-page synthetic documents and records median, MAD, min/max and peak RSS with an environment manifest. CI stores raw and summarized artifacts but never fails on timing. `cargo xtask perf compare` highlights ≥25% deltas and refuses to call runs comparable when OS, toolchain, processor count, workload scale/repeats or font setup differ.
- Printing workflow run [38310906468](https://github.com/Shabirahmad427/wordcraft/actions/runs/38310906468) for base commit `f71d779` completed successfully on Linux, Windows and macOS, including the Linux browser-compatibility step. The new regression workflow later passed on the 7E.2 head; cross-platform applicability is recorded below.
- The committed goldens matched both pages exactly within the explicit policy. Bundled-font performance runs measured 45 pages (2,568 words) and 133 pages (7,600 words); local medians and spread are recorded in the benchmark report. Both Windows GNU and Apple Silicon CLI release compile checks passed. No native Windows/macOS runtime or physical printer test was performed.

## Phase 7G — Word 365 interoperability readiness check (2026-10-12)

- Verified the fetched fork `main` at `9336c2008da5a9009bfaa066d3f917a2c416fbda`. All six local `cargo xtask ci` gates passed (2,097 passed, 0 failed, 2 ignored); Linux GUI/CLI release builds and Windows GNU/macOS ARM64 release-mode CLI checks passed.
- Hosted Document Regressions (run 38316357991) and Packaging lint (38316358241) passed on `9336c20`. The latest available Printing success (38310906468) is on `f71d779`; FreeBSD (38306533331) and Windows ARM64 (38306533411) passed on `6535bb2`. Those workflows were not triggered by the later regression-only changes, so these results are not attributed to `9336c20`.
- Genuine Microsoft Word-generated DOCX/PDF corpus remains unavailable and the benchmark is blocked: no source documents were accessed or processed. The provenance manifest and package fingerprint/allowlist protocol are ready, but Word repair warnings, editability and reference-PDF visual comparisons remain untested. Details: [Word 365 interoperability report](docs/word365-interoperability.md).

## v0.6.1 stabilization — integrity issue follow-up (2026-10-12)

- Reproduced issue #965 with a synthetic CLI sequence and changed image insertion so scripted commands leave the caret after the inserted picture; interactive insertion and clipboard paste continue to select it. Regression verifies the picture survives follow-up typing and DOCX save/reopen.
- Reproduced issue #966 with a synthetic keep-with-next heading and widow-controlled long paragraph. Release-PDF verification exposed a missing paragraph-after gap in the fit calculation; the lookahead now reserves the two-line minimum plus the effective paragraph gap. A rebuilt CLI PDF and regression verify the heading and body begin on the same page.
- Fixed issue #916's empty cross-reference UI route by opening a target picker for headings, bookmarks, figures, tables and equations. The form routes insertion through the existing REF/PAGEREF engine command. The arbitrary SEQ field report is not independently verified.
- Added a Zotero bridge boundary regression for #898; the synthetic field remains balanced through following typing and DOCX round-trip, but the actual desktop plugin sequence remains unverified. Issue #963 remains unresolved pending a safe reproducer; standalone synthetic image and text-direction tests pass.
- Added real-process CLI and app-open regressions using synthetic DOCX files. The CLI path edits, saves, reopens, rasterizes and exports a multipage document; inline and floating images are asserted in actual PNG pixels, and the original DOCX bytes stay unchanged. The UI `file.open` path renders an embedded image and vertical text box; the vertical word's raster bounds and layout direction are checked, and its page PNG matches `file.exportPng` byte for byte. GUI visual review used the headless app window on this synthetic fixture. These tests pass but do not reproduce the reporter's unavailable source document, so upstream #963 remains unresolved/unverified.
- The opening-path audit found that GUI launch arguments, File → Open, recent/drop/picker opens use `file.open` and `open_path_with_source`; CLI used `open_path` and discarded `SourcePackage`. CLI now uses the source-preserving loader, with a regression checking exact original package bytes. GUI and CLI use the same DOCX importer; page rendering and PDF export use the same layout but distinct raster/vector output paths.
- All six `cargo xtask ci` gates passed. The exact `cargo test --workspace -- --quiet` recount is **2,086 passed, 0 failed, 2 ignored across 61 suites**. The same invocation at starting commit `f1da26b` gave **2,083 passed, 0 failed, 2 ignored across 60 suites**; the increase is three discovered tests (the CLI pipeline test at `aa3846f`, plus two regressions here). The earlier 1,991 figure was a partial-log counting error, not a Cargo failure or target/feature difference. Linux GUI/CLI release builds and release-mode `cargo check` for Windows GNU/macOS Apple Silicon passed. Native Windows/macOS linking is unavailable because MinGW's DLL tools and the Apple SDK/linker are absent. Hosted checks on `aa3846fac2ee264edd1fed9bb26f30e8c40f8367`: Printing and Packaging lint passed, Windows ARM64 passed, FreeBSD was still running at last check. Upstream main remains at `159b84f54c74e9e82fb3109b758ae7dd6f4e3ab0`. No Word 365 compatibility claim is made.
- Installed Linux binaries were rebuilt from code commit `6535bb260c8e150ae0669f6be61dd57cac4ba588`: `~/.local/bin/wordcraft` reports `0.6.1 (6535bb26)` and `~/.local/bin/wordcraft-cli` reports `0.6.1`. The replaced binaries are backed up at `~/.local/bin/backups/6535bb2/wordcraft` and `~/.local/bin/backups/6535bb2/wordcraft-cli`. The latest main commit is a documentation-only interoperability note after that code commit.
- Hosted checks for code commit `6535bb260c8e150ae0669f6be61dd57cac4ba588`: Printing run `38306437369`, Packaging lint run `38306533734`, and Windows ARM64 run `38306533411` completed successfully. FreeBSD run `38306533331` remained in progress at 2026-10-12 21:04 UTC; it was not marked failed or canceled. The later documentation commits change no code and did not trigger additional hosted checks.
- Detailed issue-by-issue evidence and remaining gaps: [October 12 upstream audit](docs/upstream-audit-2026-10-12.md).

## Upstream synchronization and integrity audit (2026-10-12)

- Merged upstream `storytold/wordcraft` main at `159b84f54c74e9e82fb3109b758ae7dd6f4e3ab0` (v0.6.1), 181 commits after common base `46d5e99b3e82828f95ec87da92b2e1fc1db03b78`, into fork commit `1d7daa4098e85c9cf928696623ba38868deb8e02`. Backup branch: `backup/pre-upstream-sync-2026-10-12-1d7daa4`; merge commit: `d976573cd452dd821beb90466b7a2cdc7ae34e18`.
- Reconciled upstream APIs with fork save/recovery, custom XML, chart package preservation, printing and layout features. Added tests for merged package validation and bibliography/custom XML name collisions; restored native ink preservation and rejected over-budget markup safely. Verification and 57 merge-resolution file details are in [the October 12 upstream audit](docs/upstream-audit-2026-10-12.md).
- Issues #965, #963, #898, #966 and #916 remain open upstream. #933/#934/#935/#940/#923 are merged and included; #943 is closed unmerged and was not applied. Local CI and Linux release builds pass; hosted Packaging lint passed, with FreeBSD, Windows ARM64 and printing still running at last query. The installed binary remains `0.5.0 (dev)`; the validated source build is `0.6.1`. The issue/PR review, hosted run IDs and remaining gaps are in [the October 12 upstream audit](docs/upstream-audit-2026-10-12.md). Do not claim Word 365 compatibility.
- The genuine Word 365 benchmark remains blocked. No private DOCX files were opened or processed.

**Stage: alpha** · next: beta, ~15 points (60% → 75% ready for real work) and ~150–230 h away

## Word 365 interoperability benchmark (2026-10-12)

- Added `cargo xtask interop inspect` and `cargo xtask interop compare` for bounded DOCX package fingerprints and explicit per-case changed-part allowlists. The contract flags removed, added, or changed package parts outside the manifest; it reports hashes and package metadata without claiming semantic validity.
- Added a reproducible case manifest template and protocol in [the interoperability report](docs/word365-interoperability.md). It separates synthetic fixtures from Word-generated references and records a feature/evidence matrix and defect severity scheme.
- The first execution attempt is **blocked**: no user-selected DOCX files have verified provenance and permission for processing; Word and Wine are unavailable in this Linux environment. The manifest has five blocked document classes, with no benchmark results reported. Word visual/editability/repair checks remain pending. Existing internal fixtures are synthetic only. Hosted FreeBSD/Windows ARM64 checks and independent visual comparisons are not yet verified for this benchmark commit.

## GitHub Actions hardening (2026-10-11)

- Replaced the four externally called reusable workflows with local, reviewable snapshots based on `craft-common-actions@8f081ff5db115de44313c9db09415c359bf2ca01`; all nested action references are now full immutable SHAs. Removed the nested `install-linux-deps@main` path and kept its package setup while calling the pinned, checksum-verifying nfpm action directly.
- Changed workflow permissions to read-only by default. Only the release-publishing job receives `contents: write`, and only on the `release` branch. Pull-request and scheduled workflows do not receive release secrets or write permissions.
- Default nfpm, Trunk and actionlint downloads have pinned SHA-256 checks. Rust `stable`, FreeBSD's rustup bootstrap, WiX/NuGet package bytes, OS repositories and hosted runner images remain mutable dependencies; see [security notes](docs/save-security.md). The source review is focused, not an independent supply-chain audit.
- PyYAML parsing, actionlint 1.7.12, full-SHA existence checks, all six `cargo xtask ci` gates, Linux GUI/CLI release builds, and release-mode Windows GNU/macOS target checks passed. GitHub Packaging lint passed on the fork after fixing its ShellCheck finding. FreeBSD and Windows ARM64 runs on the same commit are still in progress; native release publishing was not triggered. This update has no Rust-source changes.

## Error-bar editing (2026-10-12)

- Extended chart inspection and the existing `chart.editData` transaction to edit only already-present fixed magnitudes and asymmetric custom numeric literals (`c:numLit`) with complete, exact per-point coverage. Scatter X/Y definitions are independent. Formula references, unsupported value types, duplicate/malformed definitions and mismatched arrays remain read-only with diagnostics.
- The Chart Design dialog stages edits locally and routes Apply through the single chart transaction; Cancel makes no command call. Error-bar-only edits splice numeric values in chart XML without changing embedded XLSX values, chart caches, direction, caps, axis identifiers or unrelated XML. The engine's existing package replacement provides one Undo/redo action and recovery/save/reopen compatibility; preview/PDF refresh uses the current shared chart renderer.
- Added synthetic mapper/transaction tests and UI request tests, plus engine coverage for rendering, Undo/redo, recovery, save/reopen and PDF export. Microsoft Word/Excel interoperability and custom formula-range mapping remain unverified/outstanding.

## Upstream integration and security reports #678/#679 (2026-10-11)

- Integrated upstream `main` at `2ffba93b5189410a415970be179dcf481092d756` (19 commits after the previous common commit) into fork base `f17dbb3e08c3534412f0f34d8b687f5ec707bf69`. The merge preserves chart package transactions, recovery, DOCX preservation and fork workflow hardening; compatibility follow-ups retain package-backed chart serialization while enabling upstream's model-only chart creation, linked text boxes and document-grid behavior.
- Security report #678: no-path control rendering and screenshots now return PNG bytes inline; MCP UI tools consume them directly and no longer create/read fixed temporary paths. Explicit-path writes continue through the atomic writer.
- Security report #679: HTML/Markdown import/export and Ctrl/Command-click navigation use shared scheme validation. Unsafe executable/local schemes, including control-character-obfuscated JavaScript, are dropped as links while visible text remains.
- Added regression tests for unsafe URL schemes, import/export behavior, inline screenshot output and the MCP bridge's no-path calls. Local validation and hosted check results are recorded in [the current upstream audit](docs/upstream-audit-2026-10-11.md) after completion. Upstream issues #678/#679 remain open; their status is independent of fork mitigation. Issue #685 (Excel workbooks for newly created charts) remains out of scope and unresolved.
- Open PRs #647, #626 and #617 remain unmerged; they were not auto-applied. No Microsoft Word/Excel interoperability claim is made.

## Issue #685 — editable workbooks for inserted charts (2026-10-11)

- Newly inserted charts now serialize chart caches as worksheet references and add a matching embedded XLSX worksheet with series names, category labels and numeric values. Scatter charts store each series' paired X/Y datasets in separate columns. Complete finite datasets must pass the existing validated mapper before writing; sparse datasets keep blank workbook cells and aligned caches, and the existing mapper explicitly classifies them read-only.
- After save and reopen, generated charts retain their package-backed chart/workbook relationship and expose the existing Chart Design → Edit Chart Data workflow. Edits still flow only through `chart.editData` and its atomic synchronized cache/workbook transaction. Package-backed imported charts remain on their guarded preservation path.
- Nameless series, empty datasets, invalid scatter X values and multi-series pie/doughnut input fail explicitly during chart writing rather than creating malformed relationships. Sparse/missing numeric values remain renderable and package-backed but are intentionally read-only under the existing numeric-only edit contract. Synthetic round-trip and mapper tests cover all current chart families; independent Microsoft Word/Excel application verification remains outstanding.

## Chart-axis scaling and number formats (2026-10-11)

- Extended the existing `chart.editData` transaction and Chart Design dialog with Auto/Manual minimum and maximum bounds and safe General, integer, decimal, percentage and scientific number formats. The chart parser already used these OOXML fields in its shared preview/PDF renderer; axis-only edits now feed that same path and do not change embedded XLSX data.
- Controls are exposed only for uniquely resolved numeric value axes. Scatter charts distinguish X and Y; category axes are explicitly read-only. Logarithmic, reversed, malformed, duplicate and ambiguous axes reject edits. Unsupported existing custom number formats are preserved and not rewritten. Explicit major tick intervals remain unsupported because layout does not yet model `c:majorUnit` accurately.
- Synthetic package, UI, engine, Undo/redo, recovery and save/reopen regressions were added. `cargo xtask ci` passed all six gates; Linux GUI/CLI release builds passed. The prior base commit `8aa01db` has successful hosted FreeBSD and Windows ARM64 checks; this new commit will receive its own hosted runs after push. No independent Word/Excel comparison was available; this is not a claim of application parity.

## Chart-series formatting (2026-10-11)

- Extended the same `chart.editData` command and Chart Design dialog with safe RGB series colors for validated bar/column, line, area, scatter, pie and doughnut structures, plus explicit line-width edits for line/scatter series. Scatter marker fills are editable when represented by one unambiguous simple RGB fill; pie/doughnut colors are editable only when `varyColors` is explicitly off. Changes splice only existing color/width attributes, preserve other OOXML formatting, and leave chart caches and embedded XLSX parts unchanged. Preview and PDF use the existing chart renderer.
- Theme/gradient/transformed/implicit colors, ambiguous styles, varied pie slices and lines without one explicit width remain read-only. At this point, data labels were not yet represented end-to-end; later added below. Error bars have no rendering model. Synthetic package, family coverage, UI, rendering, engine Undo/redo, recovery, save/reopen and PDF tests were added. No independent Word/Excel comparison is available.

## Scatter marker formatting (2026-10-11)

- Added safe scatter marker editing to the existing Chart Design → Edit Chart Data workflow: explicit symbol, size and independent simple RGB fill. Supported symbols are circle, dot, square, triangle, diamond, star, plus, x, dash and none. Marker changes use the validated atomic chart transaction and are one Undo/recovery operation; they preserve unrelated chart XML and leave chart caches and embedded XLSX bytes unchanged.
- New scatter charts now serialize explicit circle/5 pt marker settings. Imported charts without per-series markers keep a renderer fallback based on `c:scatterStyle`; WordCraft does not invent OOXML marker nodes when editing those imported charts. Unsupported or ambiguous marker structures remain read-only. Rendering uses vector marker geometry shared by preview and PDF.
- `cargo xtask ci` passed all six gates. Standalone workspace tests passed **1502 tests, 0 failures, 1 ignored**; Linux GUI/CLI release builds and release checks for Windows GNU and macOS Apple Silicon passed. Hosted FreeBSD and Windows ARM64 runs both succeeded on `4b6f0f743fa4635caf8ff70a5d4780f33dad9f81`, the latest available hosted run during this audit; they do not include the marker changes from this task. Error bars remain unaudited beyond noting the absent rendering model; data labels, line dash editing and vector-PDF text verification remain future work. No independent Microsoft Word/Excel reference comparison was available.

## Data-label editing and rendering (2026-10-12)

- Extended the existing chart snapshot, `chart.editData` transaction and Chart Design dialog to edit explicit per-series value/category/series-name visibility and supported label positions for bar/column, line, area, pie, doughnut and scatter charts. The transaction changes only existing simple label `@val` attributes; it does not add label XML or alter chart caches or embedded XLSX data. Rendering uses cached values/categories and the same vector display list for preview and PDF.
- Per-point custom labels, deleted labels, duplicate/unknown data-label structures and unsupported/missing properties remain preserved and read-only. Existing label styling, custom number-format and separator XML survive unchanged; preview currently uses default text styling/separation, and dense labels may clip. Only synthetic fixtures were available; no independent Word/Excel reference output was tested. Dash patterns and error bars remain unsupported.

## Upstream synchronization (2026-10-11)

- Integrated upstream `storytold/wordcraft` main through `dba6e66` (25 commits since shared base `e4b7d15`). The update adds configurable ribbon/Quick Access Toolbar and keyboard shortcuts, field-code commands, object contour/wrap-side behavior, drawing group/rotation fixes, equation and citation fixes, `.doc` and text-format import/export fixes, mail-merge fields, spelling vocabulary, interface sizing, Russian/Turkish localization, and CLI closed-output handling.
- The merge retains fork-specific DOCX package guards and custom XML preservation, recovery, atomic saves, pagination, print adapters, academic editing, and chart/XLSX mapping and transactions. The validated editable chart subset now also includes simple scatter datasets with embedded workbook X/Y ranges; more complex scatter structures remain read-only.
- Upstream's open PRs were reviewed but not merged. Synthetic tests and local builds are not independent Microsoft Word/Excel compatibility verification.
- Merge commit: `f01498726e1a2421f72a8a21ebff646573f74d42`. `cargo xtask ci`: all six gates passed; workspace tests: **1,266 passed, 0 failed, 1 ignored**. Linux GUI/CLI release builds and release-mode `x86_64-pc-windows-gnu` / `aarch64-apple-darwin` compile checks passed. See [the upstream audit](docs/upstream-audit-2026-10-11.md) for open PRs/issues and residual risk.

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-11 · **Change:** added safe chart series colors and line widths · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

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
| Long footnotes that continue onto later pages | landed (40% page-area cap; continuation pages keep remaining lines visible). Multicolumn pagination now reserves note space page-wide and reflows earlier columns. Equivalent in-memory and DOCX-reopened notes match pixels when the in-memory story includes its OOXML self-reference mark. Word behavior, page-specific note styles and custom continuation separators remain unverified |
| Balanced multicolumn layout | safe final-page paragraph balancing landed; complex-content and multi-page balancing remain open |
| Print preview and desktop printing | shared-layout preview; Linux/macOS use CUPS, Windows PrintTicket/Windows.Data.Pdf adapter compile-checks; no physical printer verified, Windows app-level cancellation unavailable |
| Signed builds for macOS, Windows, Linux, FreeBSD and web | pipeline written; **needs the GitHub remote and release secrets from the owner** |
| Real app icon / mascot art | placeholder; **needs owner art** |

So: alpha is roughly **85% of the way**, with about **20–25 hours** of agent work left, plus two
owner actions (push to `github.com/storytold/wordcraft` and enable the release secrets, and supply
  the icon art).

## Parity by area

| Area | Status | Parity |
|---|---|---|
| Ribbon/menu catalog coverage | **487 / 502 (97.0%)** | measured (`cargo xtask parity` → [`docs/parity-checklist.md`](docs/parity-checklist.md)) |
| **Feature breadth** (weighted, incl. dialog options, styles, languages beyond the ribbon) | **~80%** | estimated |
| **Ready for real work** (full target) | **~60%** (55–63%) | estimated, additive weighted sum over the dimensions |
| **Mainstream practitioner** | **~55%** | estimated: weekly-work depth × discounts for interaction (×0.93), stability (×0.90), file exchange (×0.90) |
| **Essentials user** | **~63%** | estimated: core-feature depth × discounts for launch (×0.90), discoverability (×0.95), opening received files (×0.92) |
| Remaining to **beta** | **~150–230 h** of Opus 5.5 agent wall-clock (gaps #1–#7) | estimated |
| Remaining to **full parity** with Word desktop | **~750–1,250 h** (70–80% parallelizable) | estimated |
| Commands / tests / code | 433 commands · 855 tests + 10 property blocks · ~88,700 lines of Rust | measured (source) |
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
and the web. **Why not beta:** DOCX has not been independently tested on a real-world corpus;
unsupported package parts are guarded and preserved, but several features lack rendering or editing
depth; pagination differs from Word (no Aptos-metric font or complex column balancing); formatting
revisions remain incomplete; there is open field evidence of a startup crash and several hangs;
desktop printing relies on the system PDF viewer instead of WordCraft printer controls. Gaps #1–#7
in [`docs/gaps.md`](docs/gaps.md)
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
| Localization | 27% | 75–120 + native review | [localization-parity.md](docs/localization-parity.md) |
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
| M3 | Insert tab | mostly done (chart/SmartArt rendering is partial; icons/3D missing) |
| M4 | Layout + Design tabs | **done**; safe text-only column balancing added, complex-content fallback still open |
| M5 | Tables | **done** (row splitting landed) |
| M6 | References | first version done; long footnote continuation landed |
| M7 | Review | **done** (margin balloons landed) |
| M8 | View | mostly done |
| M9 | Mailings | done (first version) |
| M10 | File/Backstage | mostly done; native print preview and Linux CUPS submission landed; Windows native controls remain missing |
| M11 | Draw + objects | text wrap and text boxes done; ink, rotation, grouping missing |
| M12 | Formats breadth (PDF, ODT, RTF, HTML, MD, TXT) | **done** (first versions) |
| M13 | Performance budgets | on track (1.4 ms relayout) |
| M14 | 1.0 polish, packaging, signing | pipeline written; waiting on remote and secrets |

## Recently landed
- Phase 6C scientific error bars: fixed magnitudes and exact-length custom `numLit` arrays remain editable. Existing custom `numRef` ranges are editable only when the mapper validates contiguous single-column A1 literal numeric cells, point count/order, and exact agreement with `c:numCache`. `chart.editData` stages workbook-cell and error-cache updates together, revalidates the completed package, and preserves unrelated DOCX/XLSX parts. Scatter X/Y errors are independently editable. Engine integration covers preview refresh, Undo/redo, recovery, save/reopen and PDF; formula cells, external/missing sheets, malformed/overlapping ranges and cache mismatches remain read-only. Synthetic regressions only; no Word/Excel reference output was available.
- Phase 6C.5C axis-title rendering: the DOCX chart importer resolves unambiguous physical Cartesian axes from chart `axId` references and axis `axPos`, reads supported rich-text defaults (font, size, color, bold and alignment), and draws horizontal and rotated vertical axis titles for bar/column, horizontal bar, line, area and scatter charts. Layout reserves space around tick/category labels and all legend placements; long titles are clipped with an ellipsis inside the chart bounds. Rotated chart text uses the shared display-list transform, so raster and PDF output share the same path. Pie/doughnut, combined/secondary/ambiguous axes, unsupported rich styling and arbitrary title rotations use no axis-title drawing or a documented default-style/orientation fallback; source OOXML remains intact. Synthetic parser/layout/raster/PDF, immediate-edit-refresh, Undo/redo and DOCX save/reopen tests pass. No Word reference rendering was available.
- Chart title formatting increment: the existing chart edit transaction and Chart Design data dialog can update existing single-run rich-text chart titles and physical horizontal/vertical axis titles for validated Cartesian chart families. It splices only the title text node, preserving run/title formatting, axis IDs and other chart XML; the embedded XLSX remains byte-identical for formatting-only edits. Pie/doughnut expose no axis controls. Missing, multi-run, ambiguous-axis or otherwise unsupported title structures stay read-only; adding/removing titles is not supported. Engine tests cover Apply, Undo/redo, rerender and DOCX save/reopen using synthetic fixtures. Axis-title preview rendering was added in Phase 6C.5C. No independent Word/Excel comparison was available.
- Chart formatting increment: the existing Edit Chart Data dialog can reposition a supported chart's explicitly declared legend to left/right/top/bottom. It uses the existing validated `chart.editData` transaction and shares Undo/redo with data changes; formatting-only edits leave XLSX bytes unchanged. The chart formatter tests cover all seven validated chart kinds and reject absent/unsupported position attributes. Chart/axis titles and axis scaling/number formats are supported as documented above. Series colors and explicit line widths are supported only for safe existing OOXML styles; markers, data labels and constrained fixed/literal error-bar values are editable as documented, while dash editing and formula-driven/statistical error bars remain unsupported. Replaced ShellCheck-flagged `ls` file listings in Linux packaging scripts with bounded `find` listings. Synthetic verification only; no independent Word/Excel comparison.
- Native save hardening: document serialization completes before disk writes; native DOCX packages are parsed from the generated bytes before replacement. Saves, PNG exports, control screenshots and desktop preferences use random, exclusive sibling files, flush/sync before replacement, preserve existing basic permissions, refuse symlink destinations, and clean up only the staging file they created. Recovery already uses private `create_new` files and immutable publication; printing and control keys use exclusive private files. GitHub reusable workflows are pinned to the reviewed `craft-common-actions` commit `8f081ff5`; no mutable action refs remain in this repository's workflow call sites. Linux runtime tests pass; Windows/macOS behavior and nested actions inside the shared workflows still need independent review. See [save and workflow security notes](docs/save-security.md). This does not constitute an independent security audit.
- Phase 6C chart expansion: the validated mapper and atomic edit transaction support simple horizontal bar charts (`barDir="bar"`), line, area, single-series pie and single-series doughnut charts alongside vertical columns (`barDir="col"`). The later Phase 6C.3 extension adds scatter datasets; only synthetic OOXML was tested, and Word/Excel interoperability is unverified. Multi-series pie/doughnut remain read-only.
- Phase 6C.2 editor integration: `chart.editData` updates validated embedded column charts as one engine Undo/redo step; recovery, atomic saving, save/reopen and immediate cached-chart rerender are covered. The UI now provides Chart Design → Edit Chart Data and a chart context-menu action. `chart.inspectData` supplies read-only validated values; unsupported charts open with an explicit reason, and Cancel leaves editor state untouched. The spreadsheet-like form edits names, categories and finite values through the engine command. Phase 6C.3 extends the same dialog with paired X/Y values for supported scatter series. The dialog submits its original validated snapshot so concurrent edits are rejected as stale. Only synthetic OOXML fixtures were tested; Word/Excel interoperability is unverified.
- Phase 6C.3 scatter editing: simple embedded scatter charts with one or more series are editable when each series name, X range and Y range resolves to distinct cells in the same worksheet, each X/Y range is a contiguous single-column numeric range of equal nonzero length, and chart caches exactly match workbook values. The dialog displays paired X/Y numeric columns. Formula cells, external links, noncontiguous/malformed ranges, overlapping cells, mismatched caches, extension markup and unsupported package structures remain read-only. The atomic transaction updates X/Y cache points and exact workbook cells together, then re-maps the output before returning it. Mapper, transaction, UI serialization, and engine tests cover multi-series mapping, rejection, render refresh, Undo/redo, recovery, DOCX save/reopen and unrelated-part preservation. Verification used synthetic fixtures; Microsoft Word/Excel interoperability is unverified. `cargo xtask ci` passed all six gates, workspace tests passed (**1,280 passed, 0 failed, 1 ignored**), Linux GUI/CLI release builds passed, and release-mode Windows GNU/macOS target checks passed.
- Phase 6C.2 chart-data audit: standard bar/column, line, area, pie, doughnut and scatter charts render cached values; chart model values and embedded XLSX parts remain read-only. File Info/open/save warnings now distinguish rendered cached-data charts from opaque placeholder charts. Added a DOCX save regression verifying that the chart part and workbook remain unchanged. Safe editing still needs a model for series/range formulas and a guarded package-part patch path; formulas, shared strings, multiple worksheets and external data are not modified. See [DOCX compatibility](docs/docx-compatibility.md).
- Phase 6C.2 follow-up audit from `13135e9`: editing remained disabled until the validated series-to-cell mapper, atomic package transaction and engine command were in place. The dialog now consumes that command boundary and does not patch DOCX/XLSX data itself. Unsupported chart structures remain read-only and are not guessed.
- Phase 6C.2 read-only mapping foundation: added a strict internal mapper for simple embedded column charts. It resolves workbook/worksheet relationships and contiguous single-column A1 ranges; maps series names, repeated/shared or inline category labels, and numeric values to exact sheet parts/cells; and verifies cache count, order, type and value. Formula cells, external relationships, unsafe/missing parts, unsupported chart/range forms and mismatches are rejected with a diagnostic. This phase is read-only: no package bytes, chart caches, workbooks or editor state are mutated, and chart editing remains unimplemented. Five mapping unit tests and an existing preservation integration test cover the subset; no Excel/Word interoperability test was available.
- Phase 6C.2 transaction foundation: added an internal, immutable-input chart package transaction bound to the original DOCX SHA-256 and a freshly revalidated mapping. It splices only mapped chart-cache and XLSX cell/shared-string text, then validates both nested and outer packages and rechecks cache/workbook consistency before returning output. Inline strings, safe shared strings, multiple series, category labels and numeric values are covered; stale plans, formula cells, external data, cache mismatches, conflicting edits, rich shared strings and unmapped shared-string aliases fail without output. Nine focused mapper/transaction unit tests cover successful updates and rejection cases, including byte-level preservation of unrelated uncompressed parts. No UI, editor/Undo/recovery integration or independent Excel/Word testing is included.
- Phase 6C audit: table construction/editing, cell formatting, resizing, merge/split, captions, repeated headers, row splitting and floating-image wrapping already exist. Charts render cached values and SmartArt reuses saved drawings, but neither is data-editable; embedded workbook data is preserved and is not synchronized with WordCraft edits. Fixed split-cell allocation for horizontal spans and reject vertical-merge splits that cannot be safely updated row-by-row. Four regressions cover grid/content distribution, DOCX save/reopen and safe rejection. Chart data editing, SmartArt editing, captions refresh automation and complex floating-object positioning remain open. See [DOCX compatibility](docs/docx-compatibility.md).
- M6: WordCraft-managed citation insertion refreshes existing bibliographies; source replacement refreshes matching citation fields and generated lists; deleting an actively cited source is rejected. The Update Citations and Bibliography command refreshes after manual citation deletion/reordering. Unresolved native tags produce an actionable status while retaining their cached result. Regression coverage confirms an external Zotero ADDIN field and preference property remain unchanged during native style/source refresh. See [citation compatibility](docs/citation-compatibility.md). This is synthetic internal verification only; no Zotero, Mendeley, CSL processor or Microsoft Word application was used.
- M6B: Source records now retain DOI, ISBN, issue, edition and opaque imported metadata. Added bounded BibTeX/CSL JSON interchange, a searchable source manager with editable metadata and import/export controls, and RIS DOI mapping. Format and engine regression tests cover Unicode and metadata round-tripping, malformed/oversized input handling, source merge, refresh and Undo. Direct interoperability with Zotero, Mendeley, CSL processors and Microsoft Word remains unverified.
- M6C audit: Compared the Rust CSL processor and current Citum Rust engine/migration path. Neither is a safe drop-in production CSL 1.0.2 formatter for this build: citeproc-rs is still documented as WIP with incomplete suite results and an inactive upstream; Citum uses a distinct style model and explicitly best-effort CSL migration. No formatter dependency or style file was added. Keep the four built-in styles labeled as limited local formatters; see [CSL processor assessment](docs/csl-engine-evaluation.md). Next gate is selecting a maintained processor with a measurable conformance baseline and a rich-field/document adapter.
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
- Upstream v0.4.0 adds chart and SmartArt rendering, password-protected DOCX open/save, drawing rotation/flipping, and equation support. Unsupported package content remains guarded and preserved when editing cannot safely reproduce it.
- Table rows split across pages between lines (Can't Split honoured, header rows repeat).
- Drop caps; automatic hyphenation with Word's 0.25" hyphenation zone; soft hyphens.
- Page borders measured from the page edge, line numbers, vertical alignment, column separators.
- Comment balloons in a markup area beside each page, with leader lines to their anchors.

## Next
See [production audit](docs/production-audit.md): protect DOCX data and add durable crash recovery, then complex-content column balancing, native printing and drawing depth. No independent Word 365 parity certification is available.

- M14: Native file dialogs run asynchronously; delayed responses are cancelled after document replacement (#94, PR #246 plus regression guard).

- M14: Release font inputs include Simplified Chinese; native UI can use installed CJK fallback faces (#241, PR #248).

- M8: Interface theme can follow the operating system, with persisted Light/Dark/System choices (#115, PR #249).

- Phase 4C step 1: layout reports horizontal/vertical content-boundary overflow and text-box clipping by page, element and severity. The desktop status bar keeps a localized warning visible, identifies the first affected element and jumps to its page. Oversized objects remain intact; no automatic resize or move occurs. See [pagination audit](docs/pagination.md).

- M6: Mendeley tab imports RIS exports, searches references, inserts citations/bibliographies and refreshes them; source libraries survive DOCX saves. Cloud sign-in and Mendeley Cite field editing remain future work.
- Phase 6A: equation import marks unmodeled OMML and blocks edits that would discard it; supported equation structures remain editable. See [equation compatibility](docs/equation-compatibility.md); Microsoft Word comparison remains unavailable.
- Phase 6A.2: automatic equation numbers no longer leak the `#` marker into DOCX output; expected sequence numbers restore editable marks and right-aligned layout on reopen. Equation-caption cross-references update through `SEQ`/`REF` fields, and broken references clear stale cached results. OMML-number references, stable equation IDs and section-based numbering remain gaps.
- Phase 6A.2: automatic equation numbers no longer leak the `#` marker into DOCX output; expected sequence numbers restore editable marks and right-aligned layout on reopen. Equation-caption cross-references update through `SEQ`/`REF` fields, and broken references clear stale cached results. OMML-number references, stable equation IDs and section-based numbering remain gaps.

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

- Audited five priority defects: section finalization/odd-even starts, note continuation, multicolumn reservation, header/footer clearance and unbreakable content overflow. See [pagination audit and regression contract](docs/pagination.md).
- Fixed outgoing-page finalization before section geometry changes, exactly-once vertical alignment/footnote flushing, physical odd/even filler placement before numbering restarts and hostile page-number overflow. Preserved existing continuous/web flow and DOCX guarded editing restrictions.
- Added nine regression tests covering mixed portrait/landscape sections, short academic notes, centered body text, first-page headers, restarted numbering, multipage tables, Undo/recovery/save/reopen, exact raster self-comparison and preservation-safe failures. Generated portrait/landscape pages were visually inspected. All six CI gates passed with **859 tests**, including **nine new regressions**. GUI/CLI release builds passed.
- Phase 4B covers long-note continuation and oversized-content progress, but the audit found no column-balancing pass and no complete header/footer clearance reservation in the merged implementation. Retroactive multicolumn note reflow, `NextColumn` starts, endnote continuation, complex column balancing, native printing and independent Word/Windows/macOS comparison remain open.


## Phase 4B pagination reliability (2026-10-10)

- Long footnotes split at line boundaries, reserve up to 40% of the page body, continue in order and emit continuation pages after body text ends.
- At the Phase 4B baseline, multicolumn text flowed through configured columns and explicit breaks, but notes discovered in a later column did not reflow earlier columns. Phase 4C.3 below adds that page-wide reservation pass. Column balancing remains unimplemented.
- Headers and footers render with inheritance and first/even selection. Pagination now reserves clearance for the tallest applicable story variant, which can leave extra whitespace when the selected header or footer is shorter. Independent Word comparison remains open.
- Oversized inline objects remain whole and permit following text to progress. Existing table pagination continues to split rows where possible and places unsplittable rows/repeated headers oversized on an empty page without retry loops. This can extend beyond page bounds and is not clipped or automatically rescaled.
- Added regression cases for long-note continuity plus Undo/recovery/DOCX reopen and oversized inline progress. Existing table regressions cover splittable and `cant_split` rows and oversized repeated headers; exploratory section tests are not wired into the crate test suite and are not counted as verified coverage.

## Phase 4C audit and overflow diagnostics (2026-10-10)

- Re-audited the merge against the earlier Phase 4B claims. Upstream chart rendering uses cached chart data; SmartArt rendering uses its saved drawing rather than recalculating the diagram; password-protected DOCX open/save is implemented. Drawing rotation/flipping and UI changes are present. These implementations have synthetic/package regression coverage, not independent Word visual verification.
- Recovery storage and restore, guarded package editing, chart dependency preservation, Undo, failed-save protection, and academic DOCX save/reopen remain covered by workspace tests. Desktop Print exports the current layout to PDF and opens a system viewer; printer selection, ranges, duplex, scaling, and a dedicated print preview are not implemented. Web printing delegates to the browser.
- Phase 4B documentation overstated merged column balancing and header/footer clearance: the audited layout has no balancing pass, and pagination does not reserve all inherited/even headers or tall footers. Those are recorded as open gaps in [pagination audit](docs/pagination.md).
- Implemented `DocLayout::overflow_pages` for body content extending below the available body rectangle and a persistent localized status-bar warning. Oversized images, floating shapes and unsplittable table rows remain intact and have focused regression tests. Horizontal overflow and text-box clipping are not detected.
- Validation: all six `cargo xtask ci` gates passed; the workspace lists 1,076 test cases. GUI and CLI release builds passed on Linux. A separate recount attempt hit sandbox TCP bind restrictions in desktop control-server tests. No Microsoft Word reference outputs or native Windows/macOS verification were available.
- Phase 4B validation baseline: all six CI gates passed with **865 tests** (47 suites), and GUI/CLI release builds succeeded on Linux. Complex note objects and mixed section transitions, retroactive multicolumn footnote reflow, complex column balancing, overflow choices, native printing and independent Word/Windows/macOS comparison remained open at that baseline. These checks do not establish Microsoft Word pagination parity.

## Phase 4C.3 — footnote rendering consistency and cross-column reflow (2026-10-10)

- The DOCX raster difference came from a synthetic in-memory footnote story that omitted its own display mark. The DOCX writer correctly serializes a `w:footnoteRef` in that story, and the importer keeps it as the note's leading reference object. The fixture now models that mark before the first render; its strict per-page raster comparison passes after reopen. This validates equivalent model/DOCX behavior and does not hide the marker.
- Print layout now repeats with page-level footnote reservations when a note appears in a multicolumn section. Each pass reserves the note area across every column on its physical page, moving earlier body lines before placing the note. A bounded high-water fallback keeps note anchors that move between pages from reintroducing overlap; that fallback can leave extra blank space in an unusual nonconverging document.
- A layout regression verifies a note anchored in column two reflows column one, with each body and note text byte present once and body lines above the separator. The existing end-to-end test continues through Undo, recovery, DOCX save/reopen, raster equality and PDF export.
- Document-end endnotes now flow across columns/pages after the body. Paragraphs split at line boundaries, retain their existing number/reference and order, and continue with a separator while respecting available body space above footer and footnote areas. Indivisible objects remain intact; an oversized item may overflow an otherwise empty column/page so pagination progresses without data loss.
- Added focused layout and engine regressions for multi-page line coverage/order, body clearance, Undo/redo, recovery restore, DOCX save/reopen raster equality and PDF export. `w:numFmt` remains global and continuous numbering follows reference order; the document model has no start/restart or endnote-position properties, so section-end placement and continuous/section/page restart selection are not implemented. Custom separator markup remains unmodeled.
- Internal Linux verification only. No Microsoft Word-produced reference output was available or used. Footnote tables/objects, custom continuation separator fidelity, section-end endnotes, configurable restart numbering, mixed-geometry note transitions and column balancing remain open; Word 365 pagination parity is not claimed.

## Phase 4C.4 — safe multicolumn balancing (2026-10-10)

- At continuous section endings and document end, balance a final page when its body consists of complete top-level paragraphs only. Two/three columns, equal and unequal widths, section spacing, header/footer body geometry and paragraph keep/widow constraints are respected. Paragraph groups are assigned in reading order, and every existing shaped line is checked against its destination width before the page changes.
- Pages containing notes, tables, floats/objects, partial paragraphs, page-dependent fields, explicit breaks, mixed-section content or insufficient destination width keep their stable original flow. `DocLayout::column_balance_skipped` exposes these pages and the status bar reports the fallback. Notes continue to use their existing page-wide reservations and endnote flow.
- Added regressions for 2/3 columns, unequal widths, continuous single-column transitions, note/table/shape fallback, and balanced save/reopen with Undo/redo, recovery, exact raster comparison and PDF export. Complex paragraphs that would need width-specific rebreaking and multi-page section balancing remain unsupported. No Microsoft Word reference output was available; this is not Word pagination parity.

## Phase 4C.5 — horizontal overflow and text-box clipping diagnostics (2026-10-10)

- Layout now exposes typed, page-specific horizontal and vertical diagnostics with severity and offending bounds. It checks body paragraphs against the active column, table extents against the body, table-cell text against allocated cell bounds, inline images and floating objects against their intended area, and text-box content before its existing fit pass. Horizontal text-box overflow is a warning; vertical content removed by the fit pass is marked clipped.
- Page/margin-positioned floating objects are compared with the physical page edge, so artwork intentionally extending into margins or covering the full page is not flagged as body overflow. Off-page objects remain visible as warnings. The UI keeps a localized status warning; its tooltip names the first element and axis, preserves the content assurance and jumps to the affected page.
- No object is moved, resized or discarded. Text-box clipping remains the existing rendering behavior but is now explicitly reported. Diagnostics are recomputed after edits, Undo/redo, recovery and DOCX reopen; a PDF export and raster self-comparison regression passed. Stroke/shadow extents, note tables, and some deeply nested or unsupported drawing geometry need further audit. No Word reference output was available, and no pagination parity claim is made.

## Upstream integration — WordCraft 0.5.0 source (2026-10-10)

- Merged all **15 upstream commits** after shared base `79b6f33`, through `e4b7d15`. This includes formatting revisions; DOCX chart, SmartArt and OLE save support; freeform pen/pencil/highlighter drawing and table drawing; font search; Columns, Symbol, Field, Tabs, Borders and Shading dialogs; Estonian localization; and overflow/crash fixes.
- Updated `quick-xml` from 0.38 to 0.41 for the upstream namespace-declaration denial-of-service fix. The fork’s preservation scanner now uses 0.41 namespace resolution and normalized attribute decoding. It continues to preserve extension attributes, content controls, custom XML, embedded data and unknown package parts through its guarded-save rules.
- Upstream main’s manifest and crate metadata say **0.5.0**, but the upstream GitHub Releases page still identifies **v0.4.0** as the latest published release. The merged build is therefore a tested source build, not a claim that upstream has published a 0.5.0 binary release.
- Merge conflict resolutions retained both sides’ tests and UI translations. DOCX scanner state now carries both package-wide preservation and the upstream embedded-part manifest; layout rendering retains upstream freeform geometry together with the fork’s overflow diagnostics and shape story references.
- Merge validation: all six `cargo xtask ci` gates passed (fmt, Clippy, workspace tests, assets, layers, WASM); 1,137 tests passed, none failed (one ignored). Linux GUI and CLI release builds passed. A chart recovery regression found during the full suite is fixed by serializing the inert embedded drawing source and relationship references in recovery snapshots. The upstream Estonian catalog was completed for the new command labels.
- Integration merge: `30b06020b20667bdc607dd6410eba97138e60fcd` (two-parent merge of fork `8086c22` and upstream `e4b7d15`). Upstream base tip: `e4b7d15`; 15 upstream commits integrated from shared base `79b6f33`. No Microsoft Word reference output was used. Verification is Linux-only; no Windows/macOS native build or print-driver test was run.

## Print preview and CUPS submission (based on `6e2d3f7`, 2026-10-10)

- File › Print renders a horizontal page preview from the same output `DocLayout` used to create the selected-page PDF. Range selection, overflow warnings and preview pagination therefore share one layout result. Printing leaves document contents and undo state unchanged.
- Linux desktop integrates CUPS queues, copies/collation, discovered media and duplex modes, orientation/scaling requests, and cancellation for queued jobs. Errors and absent queues leave the PDF viewer fallback available. The web retains browser printing.
- macOS is wired to the CUPS command interface but was not built or tested on macOS; Windows keeps the existing PDF-viewer flow without WordCraft printer controls. No physical printer or Microsoft Word comparison was available.
- Focused tests cover page ranges, multipage selected-PDF preview, native option/cancel handoff and CUPS parsing/argument construction. All six `cargo xtask ci` gates pass; the workspace reports **1,141 passed, 0 failed, 1 ignored** across 49 suites. Linux GUI and CLI release builds pass. No physical printer, macOS build or Windows printer integration was available for verification.

## Cross-platform print adapters (based on `40a85a6`, 2026-10-10)

- The platform-independent print backend contract now drives Linux/macOS CUPS and a Windows PrintTicket/Windows.Data.Pdf adapter. The WordCraft Print page provides printer selection and driver settings while PDF generation remains shared with preview.
- Windows supports driver-advertised media and duplex plus validated copies, collation, orientation and fit-to-page settings. Its adapter compile-check passed for `x86_64-pc-windows-gnu`; macOS CUPS compile-check passed for `aarch64-apple-darwin`.
- Windows submission is synchronous and its wrapper exposes no cancellable job ID; cancellation is available through the system print queue. The macOS path uses CUPS, not AppKit `NSPrintPanel`. No native platform runtime or physical printer was available, so behavior remains unverified on actual devices.
- Linux/macOS CUPS and Windows driver-specific options may differ. No independent Word print comparison was performed.
- Final validation: all six `cargo xtask ci` gates passed; workspace tests report **1,143 passed, 0 failed, 1 ignored across 49 suites**. Linux GUI/CLI release build passed. Windows and macOS release-mode `cargo check` passed for the available GNU and Apple Silicon targets; cross-target binaries were not linked or run.

## Upstream synchronization — WordCraft 0.5.0 source (2026-10-11)

- Merged upstream `main` at `2ffba93b5189410a415970be179dcf481092d756`, 19 commits after shared base `dba6e66d53733c2c0fd28355249d24eb3698187e`, into fork commit `f17dbb3e08c3534412f0f34d8b687f5ec707bf69`. Merge commit: `f3038b4f64f481e8cdda03038824c20517e1d89f`. The compatibility follow-up is a separate commit. The backup branch `backup/pre-upstream-sync-f17dbb3` remains available.
- Preserved fork-specific chart data/cache/XLSX transactions and formatting, guarded DOCX package editing, recovery, atomic saves, pagination, printing and workflow pinning. Reconciled model-only upstream chart serialization without replacing package-backed chart writes.
- Integrated upstream drawing/WordArt, content-control UI, chart creation, document grid/RTL table, section note options, linked text boxes, move tracking, accessible controls and other v0.5.0 source changes. Security issues #678 and #679 received local fixes; PRs #647/#626/#617 remain open upstream and were not merged. Chart issue #685 remains open: new charts still lack embedded Excel data files.
- Fixed integration findings for default note-separator round-trip pagination, section-end endnotes, duplicate text-box layout work, dialog dirty-state routing, and canonical command-search ranking. Unsupported DOCX structures remain behind existing guarded-edit failures.
- Validation: all six `cargo xtask ci` gates pass; **1,420 workspace tests passed, 0 failed, 1 ignored**. Linux GUI and CLI release builds pass. Release-mode Windows GNU and Apple Silicon compile checks pass; native runs, physical printing and Microsoft Word/Excel reference comparisons were unavailable. Version reports `0.5.0` (dev for GUI; `0.5.0` CLI).
- Installed GUI/CLI binaries were backed up at `/home/shabir/.local/share/wordcraft/backups/2026-10-11-838a7d1` before installing the validated 0.5.0 Linux builds. At audit time, hosted FreeBSD and Windows ARM64 jobs for `838a7d1` were still in progress; packaging lint did not trigger for this push (the latest observed run on `863a82d` succeeded). See [the detailed upstream audit](docs/upstream-audit-2026-10-11.md).

## Upstream synchronization follow-up (2026-10-11)

- Integrated the next 20 upstream commits through `46d5e99b3e82828f95ec87da92b2e1fc1db03b78` using a second normal merge from the 2ffba93 integration, preserving prior custom chart transactions/formatting, DOCX package guards, atomic recovery/save behavior, printing, and CI hardening. A pre-merge backup branch is retained. No open PR was auto-merged.
- Upstream additions include SmartArt types, Outline view, vertical section text, file Options panes, template/add-in Organizer, shape-wrap controls, accessibility updates, CJK font fallback, section geometry/layout work, and note/page-total behavior. Compatible changes were reconciled with fork-specific layout and DOCX models; where the upstream layout refactor could not safely be combined wholesale, the fork layout engine was retained and compatible fixes/tests were ported.
- Integration verification: `cargo xtask ci` passed all six gates, including the full workspace and WASM checks; the workspace reports **1,496 passed, 0 failed, 1 ignored**. Linux GUI/CLI release build and available Windows GNU/macOS Apple Silicon release-mode compile checks passed. Three merge regressions found in tests (Options dispatch and localization coverage) were corrected. Hosted workflows and installation are tracked separately in the detailed audit.
- Upstream audit details, conflict notes, issue/PR status and hosted run results are tracked in [docs/upstream-audit-2026-10-11.md](docs/upstream-audit-2026-10-11.md). No independent Microsoft Word rendering comparison or native printer/platform runtime test is implied.
