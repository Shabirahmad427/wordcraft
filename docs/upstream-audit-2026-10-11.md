# Upstream WordCraft audit — 2026-10-11

Updated 2026-10-11 for the requested sync from fork commit
`f17dbb3e08c3534412f0f34d8b687f5ec707bf69`. Upstream `main` was fetched at
`2ffba93b5189410a415970be179dcf481092d756`, 19 commits beyond the prior common commit
`dba6e66d53733c2c0fd28355249d24eb3698187e`. The merge commit is `f3038b4` (full hash recorded
after validation); integration follow-up changes are kept as ordinary commits. Open PRs were
reviewed individually and not auto-merged. The protected `/home/shabir/software/wordcraft`
checkout and its `fix/failed-save-metadata` branch were not used.

The 19 upstream commits add WordArt/text effects and shape connectors, editable content controls,
chart creation, accessibility and dialogs, drawing/ink tools, document grid and RTL tables,
move tracking, section-specific note options and separators, linked text boxes, continuous-break
column balancing, spreadsheet mail-merge recipients, icons, enclosed characters and phonetic ruby
text. The fork's chart-preserving writer remains authoritative for imported package-backed charts;
new model-only chart creation uses upstream chart serialization. The merge needed compatibility
repairs because fork-owned layout, command and source models differ from current upstream types.

## Recently merged upstream PRs

| PR(s) | Status | Relevance and fork overlap | Risk | Action |
|---|---|---|---|---|
| [#639](https://github.com/storytold/wordcraft/pull/639) | Merged | Reusable CI/release workflows; changes release infrastructure, not Rust code | Medium: release workflows now call `storytold/craft-common-actions` at mutable `@main` references with `contents: write` | Integrated as upstream main requested. Before a release, review the external workflow at the exact run revision and consider immutable SHA pinning |
| [#484](https://github.com/storytold/wordcraft/pull/484), [#482](https://github.com/storytold/wordcraft/pull/482) | Merged | `.doc` import and TXT/RTF/ODT/HTML/LaTeX compatibility; complements DOCX preservation | Low–medium: legacy-format behavior has only internal regression evidence | Integrated; broaden real-file corpus testing later |
| [#487](https://github.com/storytold/wordcraft/pull/487) | Merged | Equation numbering, OMML control properties, Undo and LaTeX whitespace; overlaps fork guarded OMML editing | Medium: imported unsupported OMML remains guarded and cannot always be saved after edits | Integrated with a test adjusted to verify source preservation without weakening guarded export |
| [#388](https://github.com/storytold/wordcraft/pull/388) | Merged | Eight more bibliography styles; overlaps fork source import and external-manager field preservation | Low–medium: internal formatter behavior is not standards-suite or manager interoperability proof | Integrated; citation metadata and manager fields remain protected |
| [#478](https://github.com/storytold/wordcraft/pull/478) | Merged | Mail-merge rules and header/footer fields; adds engine field behavior | Medium: field results depend on document-specific semantics | Integrated and workspace tested |
| [#384](https://github.com/storytold/wordcraft/pull/384), [#378](https://github.com/storytold/wordcraft/pull/378) | Merged | Custom ribbon and keyboard shortcuts | Low: settings/UI integration only | Integrated |
| [#474](https://github.com/storytold/wordcraft/pull/474), [#360](https://github.com/storytold/wordcraft/pull/360), [#364](https://github.com/storytold/wordcraft/pull/364) | Merged | Group transforms and contour/wrap-side drawing layout; overlaps custom positioning and overflow diagnostics | Medium: geometry/layout varies by renderer and has no page-by-page Word comparison | Integrated; parity remains unverified |
| [#510](https://github.com/storytold/wordcraft/pull/510) | Merged | CLI handling for closed stdout; crash-resilience improvement | Low | Integrated; dedicated regression passes |
| [#494](https://github.com/storytold/wordcraft/pull/494), [#514](https://github.com/storytold/wordcraft/pull/514), upstream Russian translation and [#411](https://github.com/storytold/wordcraft/pull/411) | Merged | Localization/catalog work intersects fork-added recovery, Mendeley, chart and printing labels | Low | Integrated catalog union and added RU/TR translations for fork labels; localization tests pass |

Other upstream commits in the 25-commit range add common shortcuts, field-code commands, interface
size, spelling vocabulary, hyperlink selection behavior, and minor UI/documentation fixes. Upstream
version remains `0.5.0`; GitHub lists `v0.5.0` as the latest published release. This merge is newer
source, not a new published release.

## Recently closed without merge

| PR | Status | Relevance and overlap | Risk | Recommended action |
|---|---|---|---|---|
| [#596](https://github.com/storytold/wordcraft/pull/596) | Closed, unmerged | UI external clipboard paste; directly related to still-open issue #584 and existing clipboard handlers | Medium: clipboard regressions differ by platform | Reproduce the issue and review this patch before a separate fix |
| [#629](https://github.com/storytold/wordcraft/pull/629) | Closed, unmerged | Reads character spacing/font effects into the Font dialog; #552 remains open | Low–medium: dialog/model synchronization | Check why it closed and reproduce before reopening or reimplementing |
| [#559](https://github.com/storytold/wordcraft/pull/559) | Closed, unmerged | Full legacy `.doc` reader proposal; current upstream commits fix selected `.doc` import cases but do not imply complete reader coverage | High: binary format complexity and potential silent loss | Treat as separate format project; do not equate recent `.doc` fixes with full support |
| [#607](https://github.com/storytold/wordcraft/pull/607) | Closed, unmerged | Russian/French/German localization; Russian landed in a separate merged PR, while French/German remain absent | Low | Reuse only any missing Russian catalog work; plan French/German independently |
| [#597](https://github.com/storytold/wordcraft/pull/597) | Closed, unmerged | Earlier reusable-workflow proposal, superseded by merged [#639](https://github.com/storytold/wordcraft/pull/639) | Medium: same mutable shared-workflow dependency concern | No action beyond reviewing the final #639 workflow calls |

## Open PRs to revisit individually

| PR | Status | Relevance and overlap | Risk | Recommended action |
|---|---|---|---|---|
| [#621](https://github.com/storytold/wordcraft/pull/621) | Open, dirty at last GitHub review | Security fix for predictable temporary filenames/symlinks. The fork already has random exclusive sibling staging, atomic replacement, fsync and symlink-destination rejection in `wordcraft_engine::io::atomic_write`; UI no-path screenshot/render defaults were still unsafe and are now changed to return bytes in memory | Medium: platform-specific atomic replacement and filesystem race guarantees need native testing | Keep the fork's tested writer; review #621 only for any additional platform-specific defense not already present. Do not claim all filesystem threat models are covered |
| [#612](https://github.com/storytold/wordcraft/pull/612) | Open, clean | Custom XML backing-store preservation. Strong overlap with the fork's bounded package graph and bibliography stores | Medium: duplicate mechanisms could conflict on part naming/relationships | Compare fixtures and edge cases; import missing regression coverage or narrowly scoped gaps, not a parallel preservation pipeline |
| [#647](https://github.com/storytold/wordcraft/pull/647) | Open, not merged | Preserves omitted paragraph defaults; directly relevant to DOCX round-trip fidelity | Medium: interacts with normalized model defaults and guarded-save comparisons | Review against the merged fork's omitted-default snapshots before adopting; do not treat the PR as upstream behavior |
| [#617](https://github.com/storytold/wordcraft/pull/617) | Open, not merged | Preserves unstyled TOC result ranges; overlaps field/result retention | Medium | Reproduce on the integrated fork; adopt narrowly if it preserves field guards |
| [#626](https://github.com/storytold/wordcraft/pull/626) | Open, not merged | Theme color/font precedence improves DOCX rendering | Low–medium | Compare against fork theme/font substitution and add an import/render fixture before adoption |
| [#636](https://github.com/storytold/wordcraft/pull/636) | Open, unknown | Keeps active path and dirty flag correct on copy export; overlaps save metadata and recovery | Medium | Compare state-transition tests before porting |
| [#415](https://github.com/storytold/wordcraft/pull/415) | Open, dirty | Broad chart creation/editing for column/bar/line/pie/area/scatter; fork already has guarded embedded-workbook mappings and atomic edits for bar/line/area/single-series pie/doughnut | High: PR's literal-data charts do not synchronize existing embedded XLSX, which could bypass fork safety | Do not replace fork chart transactions; review only for UI/model ideas that do not mutate foreign charts unsafely |
| [#392](https://github.com/storytold/wordcraft/pull/392) | Open, dirty | Content-control model/editing; fork preserves SDTs and custom XML but blocks unsafe edits | High: editing bindings without synchronizing custom XML can lose data | Keep read-only guard until binding updates are validated end to end |
| [#353](https://github.com/storytold/wordcraft/pull/353), [#361](https://github.com/storytold/wordcraft/pull/361), [#390](https://github.com/storytold/wordcraft/pull/390) | Open; clean/clean/dirty | Continuous-break balancing and per-section footnote separators/options overlap fork pagination | Medium | Reproduce with mixed sections/notes; port only behavior not already covered and compare against existing safe fallback |
| [#366](https://github.com/storytold/wordcraft/pull/366) | Open, unknown | Complex-script formatting preservation overlaps RTL/complex-script DOCX support | Medium | Compare imported run properties and guarded package output before adoption |
| [#609](https://github.com/storytold/wordcraft/pull/609) | Open, unknown | Windows startup without shipped DXC DLLs; launcher reliability | Medium | Verify Windows package artifact and runtime before adopting |

The three DOCX PRs above remained open and had `merged=false` during the GitHub review; no patch
was merged solely because it was open or had passing checks. The audited patch descriptions are
not proof that their behavior is in upstream `main`.

`UNKNOWN`/`DIRTY` is GitHub's mergeability status, not a judgment that the work is invalid.

## Important open issues

| Issue | Status | Impact/relevance | Risk | Recommended action |
|---|---|---|---|---|
| [#533](https://github.com/storytold/wordcraft/issues/533) | Open | Windows x64 installer reportedly cannot accept typing while the portable build works | Critical usability/release regression | Reproduce the exact installer and portable artifacts on Windows before the next release |
| [#576](https://github.com/storytold/wordcraft/issues/576) | Open | Linux file dialogs fail on Labwc/Wayland and Void Linux, blocking open/save/import | High: core data-access workflows unavailable on affected systems | Reproduce with file-dialog backend and desktop portal diagnostics |
| [#589](https://github.com/storytold/wordcraft/issues/589) | Open | Comments disappear in print/export/convert; overlaps print fidelity | High for review workflows | Add comments to print/PDF/export fixtures and make omission explicit where unsupported |
| [#551](https://github.com/storytold/wordcraft/issues/551) | Open | Legacy `.doc` import drops inline/floating pictures; overlaps newly improved `.doc` importer | High: silent content loss | Add a synthetic regression and preserve picture data or surface an import warning |
| [#554](https://github.com/storytold/wordcraft/issues/554) | Open | 500+ page, media/table performance has not been measured | Medium–high for large documents | Run a bounded benchmark before changing pagination architecture |
| [#599](https://github.com/storytold/wordcraft/issues/599) | Open | Equation text cannot be partially selected with pointer/Shift+Arrow | Medium for academic editing | Add focused equation hit-testing and selection tests |
| [#584](https://github.com/storytold/wordcraft/issues/584) | Open | Ribbon/context-menu Paste ignores external clipboard while Ctrl+V works | Medium usability bug | Reproduce on Linux/Windows/macOS clipboard adapters |
| [#529](https://github.com/storytold/wordcraft/issues/529) | Open | RTL tables remain unsupported/incomplete | Medium for right-to-left documents | Add bidi table fixtures without changing guarded XML behavior |
| [#534](https://github.com/storytold/wordcraft/issues/534) | Open | Font, typesetting and paragraph discrepancies are broad and under-specified | Medium | Triage into reproducible independent reports |
| [#678](https://github.com/storytold/wordcraft/issues/678) | Open | Control `ui.render`, `ui.screenshot` and the MCP UI bridge used predictable `/tmp` image paths, enabling pre-created symlink/race overwrite attempts | High: could overwrite a file accessible to the app user | Fixed locally for no-path APIs by returning PNG bytes inline; explicit path writes use the atomic writer. Needs post-push hosted verification and native filesystem testing |
| [#679](https://github.com/storytold/wordcraft/issues/679) | Open | HTML/Markdown output and hyperlink import/click policy did not consistently reject non-web schemes, including newline-obfuscated JavaScript | High: exported links could be unsafe when opened in a browser | Fixed locally with shared scheme validation and regression tests for importer, exporter and click navigation; issue remains open upstream |
| [#685](https://github.com/storytold/wordcraft/issues/685) | Open | Created charts lack an embedded Excel workbook relationship/data sheet | Medium for edit-in-Excel workflow | Not part of this sync: imported charts retain the fork's safe chart/XLSX synchronization; newly created charts are still model/cache-backed and this report remains open |

## Integration verification

The merge commit is `f3038b4` (parents: fork `f17dbb3e08c3534412f0f34d8b687f5ec707bf69`,
upstream `2ffba93b5189410a415970be179dcf481092d756`). Full merge hash:
`f3038b4f64f481e8cdda03038824c20517e1d89f`. It integrates 19 upstream commits after the common
base `dba6e66d53733c2c0fd28355249d24eb3698187e`. The working tree's compatibility follow-up is
committed separately after the merge; no unresolved conflicts remain. The merge required manual
reconciliation in 22 paths, primarily chart serialization/commands, DOCX package preservation,
layout state, UI dispatch and locale catalogs.

### Integration follow-up and validation

- Security issue [#678](https://github.com/storytold/wordcraft/issues/678) is still **open upstream**. No-path control screenshots/rendering now return PNG bytes inline, and the MCP bridge no longer creates predictable temporary image files. Explicit file paths use the fork's hardened atomic writer. This mitigates the reported predictable-temp symlink path in tested code; it is not an independent security audit.
- Security issue [#679](https://github.com/storytold/wordcraft/issues/679) is still **open upstream**. HTML/Markdown export, HTML import and UI hyperlink following now share an allow-list for safe web/mail/fragment/relative destinations and reject unsafe schemes/control characters. This does not substitute for browser-specific URL security testing.
- DOCX PRs [#647](https://github.com/storytold/wordcraft/pull/647), [#626](https://github.com/storytold/wordcraft/pull/626) and [#617](https://github.com/storytold/wordcraft/pull/617) were rechecked on 2026-10-11 and remain **open with `merged=false`**. None was copied wholesale or represented as part of upstream main. Revisit them individually against the fork's omitted-default, theme and field-preservation fixtures.
- Chart issue [#685](https://github.com/storytold/wordcraft/issues/685) remains **open**. Existing package-backed editable charts still use the fork's mapped cache/XLSX atomic transactions; charts newly created by WordCraft still do not create embedded Excel workbooks. This synchronization did not implement that separate feature.
- Full `cargo xtask ci` passed: fmt, Clippy, workspace tests, asset attribution, dependency-layer checks and WASM checks. Workspace totals: **1,420 passed, 0 failed, 1 ignored**. Linux release builds passed for GUI and CLI. Release-mode cross-target checks passed for `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`; these are compile checks, not native runtime tests.
- The only raster difference discovered during integration was caused by canonical default note separator XML being read back as a custom separator. The reader now normalizes only the exact writer-generated default; styled/custom separator content remains retained. Added a DOCX round-trip test; academic, long-note and NextColumn raster regressions pass.
- Two additional merge regressions were corrected: text-box bodies are now laid out once for content and overflow reporting, and configured section-end endnotes are placed at their section boundary. UI dialog routing/dirty-state and command search were reconciled with the merged catalog. Russian/Turkish catalogs translate the merged command labels while intentionally retaining English fallback for other untranslated strings.

Hosted workflow verification for integration commit `838a7d1b50dadb76b0f274cea5a73d8be8edc3e2`:

| Workflow | Result at audit time | Run |
|---|---|---|
| FreeBSD 14 build + test | In progress (not failed or canceled) | [38190479182](https://github.com/Shabirahmad427/wordcraft/actions/runs/38190479182) |
| Windows ARM64 package | In progress (not failed or canceled) | [38190479170](https://github.com/Shabirahmad427/wordcraft/actions/runs/38190479170) |
| Packaging lint | Not triggered by this push; latest observed run succeeded on `863a82d` | [38166237852](https://github.com/Shabirahmad427/wordcraft/actions/runs/38166237852) |
| Release | Not triggered by this non-tag push | — |

The in-progress hosted jobs had not reported results when this audit was updated. Synthetic
fixtures and Linux rendering do not establish Microsoft Word/Excel interoperability, native
printer compatibility, or full platform runtime behavior.

## Follow-up upstream integration through `46d5e99` (2026-10-11)

After the first push, upstream `main` advanced from `2ffba93b5189410a415970be179dcf481092d756` to
`46d5e99b3e82828f95ec87da92b2e1fc1db03b78`: **20 additional commits** (39 commits total since
`dba6e66d53733c2c0fd28355249d24eb3698187e`). Version metadata remains 0.5.0; this is source
integration, not a new published release. A backup branch, `backup/pre-upstream-sync-46d5e99`,
points to the fork before this second merge. The merge joins fork commit
`2385c40af994e34f46f796519a0060447a2d29f6` with the upstream tip.

New upstream changes cover style updates and Clippy cleanup, Windows FXC shader compilation,
FreeBSD/Windows ARM64 CI, SmartArt types, Outline view, vertical section text, Options panes,
Templates and Add-ins Organizer, shape text-wrap controls, paragraph dialog fixes, Chinese font
names, accessibility, footnotes in cells and page-total fields, CJK font fallback, layout work
(including mirrored margins, tall headers and endnote continuation), and editing fixes. Compatible
functionality was integrated. The fork's layout implementation and data-preserving DOCX writer
remained authoritative where the upstream model differed; compatible vertical layout, page-total,
note and regression-test behavior was reconciled instead of replacing those subsystems.

There were **24 content-conflict files**. Resolution retained the fork's chart package/cache/XLSX
transaction path, chart formatting and edit UI, guarded DOCX package mutation, recovery and atomic
save logic, printing and pinned-workflow security. Upstream SmartArt/model-only chart serialization
and UI features were integrated alongside them. Tests exposed and the merge fixed three regressions:
Backstage Options now dispatches to the upstream multi-pane page, Estonian covers “Update fields
before printing”, and upstream Russian/Turkish command translations were added without replacing
existing fork translations. No open PR was merged automatically.

### Requested security and PR/issue review

- Issues [#678](https://github.com/storytold/wordcraft/issues/678) and
  [#679](https://github.com/storytold/wordcraft/issues/679) remain open upstream. The prior fork
  mitigations remain: no-path image APIs return bytes without predictable temporary files, and unsafe
  URL schemes/control characters are rejected at import/export/navigation boundaries. Regression
  tests cover these paths; this is not an independent security audit.
- DOCX PRs [#647](https://github.com/storytold/wordcraft/pull/647),
  [#626](https://github.com/storytold/wordcraft/pull/626), and
  [#617](https://github.com/storytold/wordcraft/pull/617) remain separate proposals and were not
  merged automatically. Their omission-default, theme-precedence and TOC-result cases still warrant
  fixture review; existing preservation safeguards were kept intact.
- Chart issue [#685](https://github.com/storytold/wordcraft/issues/685) remains open. Imported charts
  keep validated XLSX/cache synchronization; charts newly created from the document model still do
  not generate embedded workbooks. This merge did not start unrelated chart development.

### Verification for this merge

- `cargo xtask ci` passed all six gates: fmt, Clippy, workspace tests, asset attribution, dependency
  layers and WASM checks. The layout suite's 201 tests passed; Options, localization, DOCX, chart,
  recovery and pagination regressions passed.
- The complete workspace harness was rerun after CI and reported **1,496 passed, 0 failed, 1
  ignored** (including integration tests; doc-test targets contained no tests).
- Linux GUI and CLI release build passed. Release checks for `x86_64-pc-windows-gnu` and
  `aarch64-apple-darwin` passed. These are cross-compilation checks, not native GUI or printer runs.
- No Microsoft Word reference output or physical printer was available. Hosted FreeBSD/Windows ARM64
  results and the exact test count will be added after the verified merge commit is pushed. Packaging
  lint and release workflows are recorded separately because source pushes need not trigger them.

The merge commit is `14c9c6c04e1cd0e1e38ea4a3995d71a611c0084f`, with parents fork
`2385c40af994e34f46f796519a0060447a2d29f6` and upstream
`46d5e99b3e82828f95ec87da92b2e1fc1db03b78`. It was pushed normally to
`Shabirahmad427/wordcraft:main` (fast-forward from `2385c40`); no force push. A Linux release backup
was made before installation at `/home/shabir/.local/share/wordcraft/backups/2026-10-12-14c9c6c`.
The protected checkout and its `fix/failed-save-metadata` branch were not accessed or modified.

Hosted checks for the pushed merge:

| Workflow | Result | Run |
|---|---|---|
| Packaging lint | Success | [38192354469](https://github.com/Shabirahmad427/wordcraft/actions/runs/38192354469) |
| FreeBSD 14 build + test | In progress at last check; not a failure or cancellation | [38192443180](https://github.com/Shabirahmad427/wordcraft/actions/runs/38192443180) |
| Windows ARM64 package + install test | In progress at last check; not a failure or cancellation | [38192445712](https://github.com/Shabirahmad427/wordcraft/actions/runs/38192445712) |
| Release publishing | Not triggered; merge push is not a release tag | — |

Older runs canceled after a newer push are recorded as canceled, not failed. The run IDs above are
the explicit workflow-dispatch checks for this merge because those workflows run on a schedule,
release branch, packaging PR changes or manual dispatch, not ordinary `main` pushes.
