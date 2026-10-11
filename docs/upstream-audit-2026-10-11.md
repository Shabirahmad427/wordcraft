# Upstream WordCraft audit — 2026-10-11

Compared fork commit `25f4056c14506d35c90b548f5bcaaf085375df8a` with upstream `main` at
`dba6e66d53733c2c0fd28355249d24eb3698187e`. The common base is `e4b7d155f15a7784197ba34eeea0b54490d70eb2`;
the merge includes 25 upstream commits. This audit covers recent merged work and open PRs/issues
that intersect DOCX integrity, recovery, printing, equations, charts, layout or security. Open PRs
were not merged.

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
| [#621](https://github.com/storytold/wordcraft/pull/621) | Open, dirty | Protects native saves against predictable-temp collisions and symlink redirection. Direct overlap with fork atomic save; current `crates/engine/src/io.rs` still uses a predictable sibling `.tmp` and `std::fs::write` | High: original or neighboring files can be overwritten in hostile filesystem conditions | Prioritize a dedicated security review and port only after adapting tests to the fork's save metadata/recovery semantics; do not treat current atomic rename as sufficient protection |
| [#612](https://github.com/storytold/wordcraft/pull/612) | Open, clean | Custom XML backing-store preservation. Strong overlap with the fork's bounded package graph and bibliography stores | Medium: duplicate mechanisms could conflict on part naming/relationships | Compare fixtures and edge cases; import missing regression coverage or narrowly scoped gaps, not a parallel preservation pipeline |
| [#647](https://github.com/storytold/wordcraft/pull/647) | Open, clean | Preserves omitted paragraph defaults; directly relevant to DOCX round-trip fidelity | Medium: interacts with normalized model defaults and guarded-save comparisons | Review against fork preservation tests and retain any missing defaults behavior |
| [#617](https://github.com/storytold/wordcraft/pull/617) | Open, unknown | Preserves unstyled TOC result ranges; overlaps field/result retention | Medium | Reproduce on fork; adopt if it preserves existing field guards |
| [#626](https://github.com/storytold/wordcraft/pull/626) | Open, clean | Theme color/font precedence improves DOCX rendering | Low–medium | Review against fork theme/font substitution and add an import/render fixture |
| [#636](https://github.com/storytold/wordcraft/pull/636) | Open, unknown | Keeps active path and dirty flag correct on copy export; overlaps save metadata and recovery | Medium | Compare state-transition tests before porting |
| [#415](https://github.com/storytold/wordcraft/pull/415) | Open, dirty | Broad chart creation/editing for column/bar/line/pie/area/scatter; fork already has guarded embedded-workbook mappings and atomic edits for bar/line/area/single-series pie/doughnut | High: PR's literal-data charts do not synchronize existing embedded XLSX, which could bypass fork safety | Do not replace fork chart transactions; review only for UI/model ideas that do not mutate foreign charts unsafely |
| [#392](https://github.com/storytold/wordcraft/pull/392) | Open, dirty | Content-control model/editing; fork preserves SDTs and custom XML but blocks unsafe edits | High: editing bindings without synchronizing custom XML can lose data | Keep read-only guard until binding updates are validated end to end |
| [#353](https://github.com/storytold/wordcraft/pull/353), [#361](https://github.com/storytold/wordcraft/pull/361), [#390](https://github.com/storytold/wordcraft/pull/390) | Open; clean/clean/dirty | Continuous-break balancing and per-section footnote separators/options overlap fork pagination | Medium | Reproduce with mixed sections/notes; port only behavior not already covered and compare against existing safe fallback |
| [#366](https://github.com/storytold/wordcraft/pull/366) | Open, unknown | Complex-script formatting preservation overlaps RTL/complex-script DOCX support | Medium | Compare imported run properties and guarded package output before adoption |
| [#609](https://github.com/storytold/wordcraft/pull/609) | Open, unknown | Windows startup without shipped DXC DLLs; launcher reliability | Medium | Verify Windows package artifact and runtime before adopting |

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

## Integration verification

The merge commit is `f01498726e1a2421f72a8a21ebff646573f74d42` (parents: fork
`25f4056c14506d35c90b548f5bcaaf085375df8a`, upstream
`dba6e66d53733c2c0fd28355249d24eb3698187e`). `cargo xtask ci` passed all six gates, and the
workspace suite passed 1,266 tests with one ignored. Linux GUI/CLI release builds and release-mode
Windows GNU and macOS Apple Silicon cross-target checks passed. These checks do not establish
physical-printer, native installer or Microsoft Word/Excel compatibility.
