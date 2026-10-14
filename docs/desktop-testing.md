# Desktop GUI validation

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-14 · **Change:** Hosted installed-package matrix and one-off Windows timeout recorded · **Target:** WordCraft desktop on Linux, macOS and Windows

This document separates a live desktop window, egui event-driven tests, offscreen rendering and
hosted builds. A CI job that compiles or tests the GUI is not evidence that a person used a desktop
window on that platform.

## Phase 7T packaged application evidence

An Apple Silicon app copied from a locally generated DMG passed both opt-in native-window smokes
under isolated profiles. The document smoke also sent a saved DOCX to the running app via
LaunchServices Open With, confirming the packaged bundle's Apple document-event handoff. It
edited text and a table, used Undo/Redo, saved/reopened DOCX, exported PDF and exited through
`app.quit` with the eframe exit callback and key cleanup observed. No test profile wrote to the
developer's preferences or recovery data. This is a real window controlled through synthetic
events; it is not physical mouse or keyboard input. The [installation audit](native-installation.md)
records the DMG signature defect and correction. Hosted installed-package run 38502639179 passed
on Linux, macOS and Windows after a Windows retry. The first Windows attempt timed out on a control
response after text-event queuing and force-terminated the child after the test failed; the retry
passed normally. That transient remains unexplained. Phase 7S's source-built native smoke does not
substitute for installed-package checks.
A universal DMG also passed the same two smokes with its x86_64 GUI selected through
LaunchServices `open --arch x86_64` under Rosetta; this does not establish Intel Mac hardware
behavior.

The updated package workflow stages upstream v0.5.0 packages, verifies their checksums, and
tests upgrades to the current build with isolated preferences and recovery snapshots. It also adds
Windows control-request lifecycle logs from authenticated receipt through UI-thread dispatch and
TCP response. These checks have not yet run on hosted runners and are not runtime evidence. The
prior Windows timeout remains unexplained; its retry passed without reproducing the issue. New
package artifacts are retained for 90 days.

## Phase 7S current evidence

The local host is Apple Silicon macOS (`aarch64-apple-darwin`). WordCraft uses eframe 0.36.2 with
`wgpu`, `accesskit`, `wayland`, `x11`, `links` and `default_fonts`; the production app does not enable
eframe's `glow` feature.

The startup stall is reproducible when a raw executable is spawned by the non-interactive test
launcher. WordCraft and a minimal app built against the same eframe version both stop before the
app-creator callback with either wgpu or glow. A `sample` backtrace places the main thread in
`winit::run_on_demand` → `-[NSApplication run]` → AppKit's `nextEventMatchingMask` → CoreFoundation's
Mach-message wait. The app creator, first frame, control key and endpoint never appear. The
`Resumed` callback itself is inside winit and is not instrumented directly, but this stack is waiting
for an AppKit event before that callback can run. Since eframe creates the window and graphics
renderer after the first resume, the evidence places this stall before adapter/device selection,
surface setup and renderer creation; it does not implicate WordCraft or either graphics backend.
LaunchServices and HiServices messages were present in some attempts but were not shown to cause the
wait. The specific macOS environment condition behind the raw-launch behavior has not been isolated.

The minimal wgpu and glow probes both reached their creator, first frame, `on_exit` and normal
`run_native` return when launched from an interactive Terminal session. The native smoke now starts
WordCraft through LaunchServices using a temporary `.app` bundle. Two consecutive local native
smokes passed from Terminal: the first created a window and authenticated control endpoint, drove
real egui pointer/key/text events through a synthetic editing workflow (Unicode, bold, table,
Undo/Redo, DOCX save/reopen, PDF export and screenshot), then used `app.quit`; the second used an
isolated preferences-enabled profile, edited a dirty document, quit, and verified its recovery
snapshot. Both `open -W` waiters returned status 0 after the app emitted the eframe `on_exit`
marker and removed its control key. No WordCraft or eframe-probe process remained afterward. On
macOS the harness observes the LaunchServices waiter's exit status rather than a direct `waitpid`
status for the app; normal application shutdown is independently confirmed by `app.quit`, the
`on_exit` callback, key removal, the verified recovery snapshot, and process disappearance. Neither
smoke was counted as a pass after forced termination.

The no-preferences smoke sets `WORDCRAFT_NO_PREFS=1`, `HOME` and macOS `CFFIXED_USER_HOME` to a
fresh isolated profile. It asserts that no preferences, recovery store, logs, fonts or custom
dictionary are created there. The separate shutdown regression enables normal preferences only
inside another new temporary profile and confirms the latest unsaved text is recoverable. No
developer preferences, logs or recovery entries were accessed or removed.

Hosted Printing run [38476284508](https://github.com/Shabirahmad427/wordcraft/actions/runs/38476284508)
passed the complete native smoke matrix. Linux ran under Xvfb, Openbox and software GL; it reported
focused window/canvas, inserted Unicode and bold text, inserted a table and cell text, passed
Undo/Redo, completed DOCX/PDF checks, and exited with status 0 after the eframe exit callback.
Windows ran with its native window and DirectX 12 software adapter, completed the same editing,
table, Undo/Redo, DOCX/PDF and normal `app.quit` sequence, and exited with status 0 after the exit
callback. macOS Apple Silicon passed both isolated native-window tests; the `open -W` waiter returned
status 0 after `app.quit` and the eframe exit callback. All three separate isolated-profile recovery
shutdown tests also passed.

The earlier Linux and Windows table failures were harness geometry: fixed point `(166, 190)` fell
between the second-row grid cell's hit bounds on those runners. `ui.inspect` now reports the rendered
Insert › Table button and grid-cell bounds, and the test clicks their centers. Linux/Windows
reported `(166, 200)`; macOS reported `(166, 190)` because its popup was positioned higher. Windows
also exposed that the old immediate Undo assertion could run before the window processed the key;
the smoke now polls bounded document-state transitions for Undo/Redo and reports document, focus,
caret/selection, pending events and a screenshot on timeout. This validates the harness behavior;
it does not change shared editing logic.

Local `cargo xtask ci` passed all six gates and Apple Silicon release GUI/CLI builds plus version
commands passed. Native desktop evidence above is from the hosted GUI matrix; workspace egui tests
and offscreen screenshots remain headless coverage and are not counted as desktop interaction.
Phase 7S native smoke acceptance is met on this run; platform gaps outside this workflow remain in
the support matrix below.

The explicit macOS quit path now stops the recovery worker and writes its final checkpoint before
calling `NSApplication.terminate`, since AppKit can bypass eframe's exit callback. The isolated
recovery regression passed with the native smoke. The earlier separate release launch's reported
exit 143 remains unexplained; its signal sender cannot be identified from the available evidence.
Exit 101 from a prior `cargo test` run was the test runner reporting a failed startup assertion; it
was not the application exit status. A child that was SIGKILLed during earlier bounded diagnostics
is recorded as forced cleanup, never as a successful launch.

Local verification: `cargo xtask ci` passed all six gates, including workspace tests; the Apple
Silicon release GUI and CLI built and both `--version` commands passed. The local macOS native smoke
passes with the corrected Insert-tab click at `y=45`, followed by an assertion that Insert became
active. Hosted Printing run
[38467901780](https://github.com/Shabirahmad427/wordcraft/actions/runs/38467901780) tested the
first hosted corrections. Linux installed `libxkbcommon-x11-0`, reached the app creator, control
endpoint and first frame, and passed isolated recovery shutdown; its editing smoke then left the
document empty after the injected page click/text and the harness forcibly terminated the app.
Windows reached the first frame and typed/formatted text, but the tab state remained Home. The
original click used `y=30`, the boundary between WordCraft's title row and tab strip; the harness
then forced termination after the table assertion. The click moved to `y=45` with an Insert-tab
assertion, Linux focus diagnostics and a runner-temp artifact path. Those fixes were intermediate;
the later run [38476284508](https://github.com/Shabirahmad427/wordcraft/actions/runs/38476284508) passed
the complete Linux/Windows native workflows as recorded above. See
[native desktop platform support](platform-support.md) for the platform-by-platform evidence.

No physical mouse/keyboard input, native Open/Save panel interaction, system clipboard exchange or
Word 365 comparison is claimed. Control-channel UI events exercise the application window but are
not physical input.

## Phase 7R evidence

The checkout began at `0ff86c8`. On the local macOS host, the release app launched with its real
window and localhost control channel. It opened the normal profile's recovery chooser, which showed
existing recovery entries. I did not open, discard or modify those entries, so I stopped before
editing. This means there is no native-window save/reopen/PDF workflow result for this run. The
window screenshot was not inspected because it included the recovery chooser.

An isolated offscreen `ui_shot` run used a fresh blank synthetic document at 1440×900 logical points
and 2× rendering. Home, Insert, Layout, References, Review and View were captured and inspected;
the ribbon and Home style samples were visible. The screenshots were saved under `/private/tmp`
and are not repository fixtures. The existing `home_style_gallery_preview_contains_rendered_sample_ink`
pixel test separately exercises built-in and custom styles, missing-font fallback, light/dark
preview backgrounds and 1×/2× raster scale.

The egui event path (pointer, key and text events delivered to the running UI harness) exercised
Unicode text, Bold, Undo/Redo, the Insert tab, the Table menu, a 2×2 grid selection and cell typing.
This is not OS-level physical mouse/keyboard input. Before the correction, the table was inserted
but all four cells remained empty when text was typed immediately. The picker called `app.run`
directly and skipped the ribbon helper that requests canvas focus. The picker now calls
`run_from_ribbon`; `dialogs::tests::table_grid_selection_requests_document_focus` failed before
the change and passes after it. The same isolated UI event script then produced
`[["Alpha","Beta"],["Gamma","Delta"]]`; Cmd+Z removed `Delta` and Cmd+Shift+Z restored it.

## Phase 7R historical evidence

At the earlier Phase 7R checkout `0ff86c8`, launching through the normal macOS profile opened
the recovery chooser with existing entries, so that run stopped before editing. Phase 7S uses fresh
temporary profiles; the editing smoke disables preferences while the shutdown regression enables
them only inside its isolated profile. The raw-executable startup matrix remains a diagnostic of
the non-interactive launch condition, not a passing GUI smoke. The prior hosted runs below are
historical evidence.

### Area audit

| Ribbon area | Evidence | Result |
|---|---|---|
| Home: clipboard, fonts, paragraphs, styles | Offscreen screenshot and style-preview pixel regressions | Controls rendered; style samples were visible. No new defect observed in the screenshot. |
| Insert: tables, pictures, equations, charts | Offscreen screenshot; actual pointer path through Table menu and 2×2 grid | Menu and grid rendered. Table-grid focus defect reproduced and fixed. Picture/equation/chart insertion was not exercised in a live document. |
| Layout: margins, columns, breaks, page settings | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| References: citations, captions, footnotes | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| Review: editing and document tools | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| View: zoom, navigation, page display | Offscreen screenshot | Controls rendered; resize, scrollbar, zoom and navigation behavior were not exhaustively tested in a native window. |

No complete tooltip, keyboard-shortcut, 1×/2× desktop-window, resize, scrollbar, or screen-reader
audit was performed here. A 2× offscreen screenshot and a pixel test are not substitutes for those
checks. The existing ribbon group-fit regression remains part of the UI suite.

### Hosted results for code commit `270c7d8`

- [Printing run 38438412430](https://github.com/Shabirahmad427/wordcraft/actions/runs/38438412430) completed successfully on Ubuntu, macOS and Windows, including native app/UI tests on each runner and native clipboard adapter tests on macOS and Windows. The Linux clipboard test also passed under Xvfb.
- [Document regressions run 38438424704](https://github.com/Shabirahmad427/wordcraft/actions/runs/38438424704) completed successfully: DOCX/session tests on all three OSes, Windows and macOS native CLI smoke, Linux GUI/CLI release build and CLI smoke, visual regression, and advisory performance.
- Hosted runners execute OS-native tests and builds, but do not launch an interactive WordCraft desktop window or simulate a user's full editing session. The local native-window limitation above still applies.

## Synthetic Word 365 comparison protocol (not yet performed)

Use only documents written from scratch for this comparison. Do not open a personal document. Word
may be observed as a black box; do not inspect its bundle or commit any file Word produces.

1. In WordCraft, start a blank document and type a fixed synthetic sample: a title, one heading,
   two paragraphs with bold/italic and `naïve café — 東京`, plus a 2×2 table with short labels.
2. Save it to a temporary path such as `/private/tmp/wordcraft-wordcraft-reference.docx`.
3. Open that file in Microsoft Word 365. Tile the WordCraft and Word windows left/right using the
   macOS window controls. Compare page size, margins, line breaks, heading/body appearance, table
   dimensions and cell text. Record app versions, macOS version, display scale and installed fonts.
4. In Word, create the same synthetic content from scratch and save it to a separate temporary
   file. Open that file in WordCraft. Compare the same properties, edit text and a table cell in
   each app, then save/reopen and export a PDF from WordCraft.
5. Record differences and whether they are layout, font substitution, editing or package-round-trip
   differences. Do not use byte-identical PDF comparison as the pass criterion.
6. If screenshots are needed, capture each application window separately by window ID. Word
   screenshots belong only under the ignored `plan/word/screenshots/` directory. Do not capture
   the whole desktop, and do not commit the temporary DOCX/PDF files or files saved by Word.

This protocol has not been run. No Word 365 compatibility result is claimed.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-13 | minor | Located the raw-launch stall in AppKit's pre-resume event wait; verified LaunchServices native smokes, isolated profile safety, recovery shutdown and normal child status |
| 2026-10-13 | minor | Recorded hosted native smoke failures on Linux and Windows and enabled dependency installation plus isolated Windows failure diagnostics |
| 2026-10-13 | minor | Found Windows ribbon click hit the title/tab boundary; corrected it and added an active-tab check. Linux hosted startup is fixed, but first page text input remains unresolved |
| 2026-10-13 | minor | Confirmed Linux Xvfb had no OS/window focus; added Openbox and a focus readiness assertion. Confirmed Windows text editing passes but the later table picker click fails; added a picker screenshot for hosted diagnosis |
| 2026-10-13 | minor | Linux Openbox and picker target discovery passed its full native workflow; Windows now reaches table cell editing and exposed an immediate Undo assertion race, so bounded Undo/Redo state waits were added pending hosted validation |
| 2026-10-13 | minor | Printing run 38476284508 passed Linux, macOS and Windows native smoke workflows, including editing, table insertion, Undo/Redo and normal quit; recorded native versus headless evidence separately |
| 2026-10-13 | major | Recorded corrected DMG installation and packaged-app LaunchServices Open With, editing and quit evidence separately from source-built native smokes |
| 2026-10-14 | minor | Recorded the completed installed-package matrix, Linux Weston startup check and unexplained one-off Windows control timeout followed by a passing retry |
