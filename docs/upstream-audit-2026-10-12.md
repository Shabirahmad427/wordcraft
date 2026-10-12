# Upstream WordCraft audit — 2026-10-12

## Integration snapshot

- Fork starting point: `1d7daa4098e85c9cf928696623ba38868deb8e02`.
- Upstream remote: `storytold/wordcraft` `main`, commit `159b84f54c74e9e82fb3109b758ae7dd6f4e3ab0` (v0.6.1).
- Common base: `46d5e99b3e82828f95ec87da92b2e1fc1db03b78`.
- Upstream commits integrated: 181.
- Backup branch: `backup/pre-upstream-sync-2026-10-12-1d7daa4`.
- Merge commit: `d976573cd452dd821beb90466b7a2cdc7ae34e18` (181 upstream commits; follow-up fixes are in the delivery commit).
- No unresolved index conflicts remain. The merge has resolution differences in 57 files (`git show --remerge-diff`); overlapping areas included document I/O, save/printing, DOCX import/export, layout, citations, app services, and the chart-preservation writer. Follow-up compatibility edits restore fork behaviors while adapting to upstream APIs. The complete local CI suite passed after fixing a print-preview test that still set the removed legacy range state and adding the WASM browser-file error path required by password-protected open commands.
- Verification: all six `cargo xtask ci` gates passed (fmt, Clippy, workspace tests, assets, layers, WASM). A separate workspace rerun passed 2,079 tests with 0 failures and 2 ignored. The Linux GUI and CLI release build passed. Windows GNU GUI/CLI and macOS arm64 GUI/CLI cross-target checks passed. The macOS check exposed an obsolete call to a print-worker join function; it was removed because current print submission is synchronous. Installed Rust targets were Linux x86_64, Windows GNU x86_64, macOS arm64 and WASM. WASM checks emit existing non-fatal unused/dead-code warnings; Clippy with `-D warnings` passed.
- The installed application was not replaced: `~/.local/bin/wordcraft --version` remains `wordcraft 0.5.0 (dev)`. The validated source/build version is WordCraft `0.6.1`. No private DOCX files were opened or processed. The Word 365 benchmark remains blocked pending genuine, provenance-documented reference files and an available Word verification environment.

## Requested GitHub issue review

Statuses below were queried from `storytold/wordcraft` on 2026-10-12. “Affected” means the relevant code path exists in this fork; it does not mean the report has been reproduced.

### Stabilization follow-up from v0.6.1

- Upstream `main` was fetched again from the isolated stabilization checkout. It is still `159b84f54c74e9e82fb3109b758ae7dd6f4e3ab0` (v0.6.1); there are **0** upstream-only commits beyond the already integrated merge. No new issue/PR integration was available.
- **#965 reproduced** with a synthetic CLI sequence: after `insert.picture`, `text.newParagraph` and `text.insert` replaced the selected picture. Engine/script insertion now leaves the caret after the image by default; interactive insertion and clipboard paste explicitly keep the picture selected. The regression verifies the image remains in the model and after DOCX save/reopen.
- **#966 reproduced** with a synthetic document matching the report's keep-with-next heading, long body paragraph and widow control. The first release-PDF check exposed a missing paragraph-after gap in the fit calculation; the layout lookahead now reserves the next paragraph's two-line widow/orphan minimum plus the effective inter-paragraph gap. The updated regression checks the actual heading/body paragraph indices; a rebuilt CLI PDF now places both on page 2. Existing keep-with-next pagination tests still pass.
- **#916 UI symptom fixed locally:** `insert.crossReference` without a target now opens an in-app target picker for headings, bookmarks, figures, tables and equations; insertion still uses the engine's Word-compatible REF/PAGEREF path. The UI routing test and equation-caption renumbering/broken-target test pass. Arbitrary user-document `SEQ` behavior was not independently tested.
- **#898 not reproduced in the synthetic bridge path:** a Zotero citation inserted at document end remains field-balanced and unchanged after following text is typed and after DOCX round-trip. This does not verify the full Zotero desktop/plugin UI sequence, which remains untested.
- **#963 remains unresolved/unverified:** no attached source DOCX or minimal sequence is available. Existing synthetic regressions pass separately for image DOCX round-trip, text-box direction, and vertical table-cell direction, but these do not establish the reported combination or root cause.
- Chart-package byte preservation, workbook-backed error-bar edit/undo/recovery/round-trip, and academic section/pagination recovery integration tests passed after the stabilization edits. No private DOCX was read or used.

| Issue | Status and upstream fix | Fork overlap / risk | Recommended action |
|---|---|---|---|
| [#965](https://github.com/storytold/wordcraft/issues/965) — typing after `insert.picture` can remove the picture | Open; no upstream-main fix identified. | Reproduced with a synthetic CLI sequence. Engine default now places the caret after a scripted image; interactive/paste paths preserve selection. High data-loss risk; the synthetic regression passes through DOCX reopen. | Local mitigation implemented; verify hosted checks and seek maintainer confirmation of expected CLI/UI semantics. |
| [#963](https://github.com/storytold/wordcraft/issues/963) — images missing and vertical text boxes horizontal | Open; no attached DOCX or upstream-main fix. | Separate synthetic image and text-direction tests pass, but the reported combination/root cause cannot be reproduced without input. Potentially high content/layout risk. | Keep open; request a minimal legally shareable reproducer. No fix claimed. |
| [#898](https://github.com/storytold/wordcraft/issues/898) — Zotero field at document end is not closed correctly | Open; no upstream-main fix identified. | Synthetic bridge insertion at document end, following typing, field-balance checks and DOCX round-trip pass. Actual Zotero app/plugin sequence remains untested. High citation editability risk if confirmed. | No fix claimed; validate with Zotero desktop and an authorized reproducer. |
| [#966](https://github.com/storytold/wordcraft/issues/966) — keep-with-next heading left behind after widow/orphan reflow | Open. Related layout PR #933 is merged; the synthetic reported combination reproduced separately. | Layout lookahead reserves the two-line widow/orphan minimum and effective spacing gap. A rebuilt CLI PDF and regression both place heading and body together; existing keep-next tests pass. Medium visual/pagination risk. | Local fix implemented; verify hosted checks and request reporter confirmation. |
| [#916](https://github.com/storytold/wordcraft/issues/916) — cross-reference button opens no dialog; SEQ equation numbering reported broken | Open; no upstream-main fix identified. | Empty UI invocation previously had no form; it now opens a typed target picker and inserts through existing REF/PAGEREF engine paths. UI routing and equation-caption renumber tests pass. Arbitrary SEQ behavior remains unverified. | UI fix implemented; verify hosted checks and reproduce the SEQ report with a shareable fixture before claiming full resolution. |

## Requested pull request review

These PR statuses were checked individually. Only merged commits on upstream `main` were integrated; PR #943 was not merged or cherry-picked.

| PR | Upstream status | Upstream change / fork overlap | Action and risk |
|---|---|---|---|
| [#940](https://github.com/storytold/wordcraft/pull/940) | Merged as `5383d80f753fbd14e525c307fadf840900a541d4` | Empty continuous section mark uses no extra space; layout overlap. | Integrated with upstream `main`; verify in mixed-section pagination tests. Low merge risk. |
| [#935](https://github.com/storytold/wordcraft/pull/935) | Merged as `5aa8151a5394397fb4e16b3286b0e80b8cc13bb4` | Text frame placement and DOCX save; overlaps text-box preservation. | Integrated, then reconciled with fork text-box and guarded-save behavior. Medium compatibility risk. |
| [#934](https://github.com/storytold/wordcraft/pull/934) | Merged as `97ade895d7b644b2ddf59980a52884984d46e2e8` | Post-column-break width; overlaps custom column flow. | Integrated; verify section transitions and NextColumn. Medium pagination risk. |
| [#933](https://github.com/storytold/wordcraft/pull/933) | Merged as `fc52079f587abfb82a45970075bc92f0f6464c6c` | Keep-with-next when the kept paragraph itself does not fit. | Integrated; does not by itself resolve #966’s widow/orphan sequence. Medium pagination risk. |
| [#923](https://github.com/storytold/wordcraft/pull/923) | Merged as `681a57fc1a3b6ddbfdc61703befd326a5338c0f2` | Kerning and automatic paragraph spacing; model/layout overlap. | Integrated; verify line metrics and pagination fixtures. Medium layout risk. |
| [#943](https://github.com/storytold/wordcraft/pull/943) | Closed, not merged (`merged=false`) | Preserve `w:dirty` field flags; relevant to citation and field fidelity. | Not applied because it is not on upstream `main`; audit its patch and add a fork-native field regression before considering it. |

## Verification and hosted checks

- The latest checks visible on the fork before this merge were for `f052ddf9e1804b138ec251bf09b82cb30743d237`: FreeBSD and Windows ARM64 both succeeded. These do not validate this merge.
- Packaging lint last visible for the earlier integration commit `14c9c6c04e1cd0e1e38ea4a3995d71a611c0084f` succeeded. Its paired FreeBSD run was cancelled; Windows ARM64 succeeded. Cancellation is not a test failure.
- On pushed commit `7a1f763b4bc9ea4bf321e04e10066632b45c7194`, Packaging lint run `38287099819` succeeded. Printing run `38287099297`, FreeBSD run `38287743422`, and Windows ARM64 run `38287746843` were still in progress at the last query. The older FreeBSD cancellation at `14c9c6c` was a canceled run, not a failed build. Recheck the final pushed commit before treating these hosted checks as complete.
- No physical printer was available for device-level print verification. Microsoft Word/365 was not used; interoperability remains unverified.

## Remaining high-priority follow-up

Obtain safe repro documents for #963 and the full Zotero desktop/plugin flow in #898; verify #916's arbitrary SEQ-field report; and obtain reporter confirmation for #965/#966. Review PR #943 independently. Do not treat passing synthetic tests or merged upstream PR status as proof of Word 365 compatibility.
