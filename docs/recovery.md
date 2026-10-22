# Document recovery

WordCraft maintains two compatible local recovery formats: fork-created immutable `.wcr` archives
and upstream's worker-written periodic document snapshots. Existing `.wcr` files remain in place
and are listed by `recovery.archiveList`; automatic snapshots use `file.recover`.

The desktop app creates a recovery snapshot on the first modified frame, then at most every 15 seconds while new edits exist. Serialization/compression and disk writes run on one background worker, with at most one periodic snapshot in flight. Recovery is independent of the AutoSave switch and covers documents that have never been saved. It writes only to a separate recovery directory, never to the original document. A checkpoint before document replacement/Close and a final checkpoint on orderly exit retain the latest unsaved state. These boundaries may wait for an in-flight write. A failed recovery write cannot prevent an explicit save to a healthy destination.

On launch, available versions appear in **Recover Unsaved Documents**. Reopen the window through **File → Info → Recover Unsaved Documents** or command search. Select a timestamped version, then:

- **Restore Recovery Version** opens a modified, unnamed working copy requiring Save As. Unsaved changes in the current document get the existing Save/Don't Save/Cancel prompt. The recovery archive remains available.
- **Discard Recovery Version** asks for confirmation before deleting that archive. Versions belonging to a live application session are protected by an OS file lock. Other archives and saved files remain untouched.
- **Compare Recovery Versions** shows the recovered body text beside the current document or another recovery version. The UI previews the first 8,000 characters. This is a text comparison, not a visual or tracked-changes comparison.
- **Save Original Package** exports the exact source DOCX/DOCM/DOTX/DOTM bytes, including unsupported elements and relationships. The destination must not exist. The original filename extension is suggested. This contains the opened source, not subsequent edits.

Three recent versions per document are retained for the current application session, after a new snapshot has committed. Other sessions' or crashed documents are retained until explicitly discarded. Normal exit retains unsaved recovery archives too; a listed version does not necessarily mean its application crashed. Close the recovery window to defer a decision. Recovery is local and is not uploaded.

## Locations and controls

| Platform | Recovery directory |
|---|---|
| Linux/BSD | `$XDG_CONFIG_HOME/wordcraft/recovery`, otherwise `~/.config/wordcraft/recovery` |
| macOS | `~/Library/Application Support/WordCraft/recovery` |
| Windows | `%APPDATA%\WordCraft\recovery` |

`WORDCRAFT_NO_PREFS` disables preferences and automatic recovery storage for isolated agent/test runs. Failure to initialize recovery is shown in the status bar and logged. Snapshot errors are reported; previous committed versions remain available. `file.recover`, `recovery.restore`, `recovery.discard`, `recovery.compare` and `recovery.original` also work through CLI/MCP. A headless session must supply `directory` explicitly; desktop MCP uses the application's configured directory. Discard via an API is explicit and does not open a confirmation dialog.

## Preservation and atomic storage

Recovery archives (`.wcr`) contain the complete serialized editor model, embedded media, preserved package parts, selection and bibliography style. The two model fields excluded from ordinary JSON serialization (`media` and `passthrough`) are stored as separate archive entries. Citation sources, custom properties, equations, headers/footers, document settings and any validated chart/workbook transaction baseline stay in the model. Opening a Word package retains its exact source bytes; those are archived separately too. A chart edit updates the model's guarded package parts, not the saved file or the original-package archive, so recovery can restore the current chart and still export the exact originally opened DOCX. Recovery never regenerates a DOCX merely to checkpoint it, so unsupported parts cannot disappear through the recovery writer.

Each archive is immutable. A writer creates a unique private staging file, writes a CRC-protected ZIP, synchronizes the file, then renames it into place. Unix additionally synchronizes the directory. Incomplete `.partial` files are ignored and failed writes cannot replace preceding snapshots. An OS lock identifies each live writer and releases on process death. Retention uses monotonic sequence numbers, so a backwards wall-clock adjustment cannot delete newer versions. The original-package export synchronizes a sibling staging file and publishes it with an atomic, non-replacing hard link, then removes staging; a filesystem without hard-link support returns an error.

Read limits: 512 MiB archive/expanded payload, 16 MiB model JSON, 1 MiB metadata, 2048 ZIP entries, 4096 directory entries, JSON depth 64 and 250,000 values. Metadata text, attachment keys and entry names are bounded. Duplicate ZIP entries, invalid attachment names/keys, mismatched sizes, CRC failures and unsupported metadata versions return errors. Corrupt archives stay on disk for explicit discard; a failed restore leaves the current document intact. No ZIP entry is extracted by its path. Directories/files use owner-only Unix permissions; Windows uses the user's settings-directory ACLs. Recoveries contain document content and should be treated like saved documents.

## Verification and limits

`cargo test -p wordcraft-engine recovery` covers abrupt process exit, a child process killed during publication, interrupted writes, damaged payload CRC, corrupt ZIP listing/discard, structural limits, immutable previous snapshots, live-session protection, retention and command failure isolation. A synthetic DOCX regression checks exact original bytes (including unknown drawing data), bibliography custom XML, source properties, edited text, pagination and every rendered page pixel after restart. UI tests exercise periodic scheduling, replacement/exit checkpoints, failure followed by successful manual save, startup listing, comparison, restore with Save Changes/Cancel, and explicit corrupt-version discard. `cargo xtask ci` runs the repository's six required checks. `ui_shot` accepts `recoveryDirectory` on its first script line for isolated UI screenshots.

Limits: edits since the last successful snapshot can be lost in a crash; the nominal interval is 15 seconds plus write time. There is no keystroke journal, Undo-stack persistence, automatic salvage of partial ZIPs, encrypted recovery, cloud/version-history service or browser persistence. Large or structurally excessive documents fail recovery visibly; manual save remains available. Comparing full formatting, objects and notes requires visual review. Unsupported DOCX elements remain available in the original package but are not made editable or merged into the recovered edited model's DOCX export. Ordinary export retains the limitations in `production-audit.md`.

The Phase 7J failure-injection suite exercises interrupted staging, process termination during snapshot publication, CRC/archive corruption, failed restore isolation, concurrent native saves and unsupported OOXML preservation. A new regression injects directory-sync failure after a recovery archive has been renamed into its immutable committed name: the operation now returns success and logs reduced durability, matching document save behavior. Returning failure after commit could trigger a duplicate retry even though the recovery version was already present.

These deterministic tests exercise the same Rust path on Linux, macOS and Windows in the Document Regressions workflow. They simulate storage exhaustion and replacement denial in controlled directories; they do not fill a real filesystem or alter real ACLs/permissions. The process-termination tests kill a child at a controlled publication boundary; they do not test power removal, kernel panic, sudden hardware failure, every filesystem's write ordering, network filesystems or physical disks. Windows has no directory-fsync implementation in this std-only path. Edits since the last successful snapshot can still be lost (nominally up to 15 seconds plus write time). No Microsoft Word 365 parity claim is made.

Verified CI result for these two arcs: all six checks passed; 821 workspace tests passed (16 added recovery tests). The headless screenshot was rendered and inspected without committing a document image asset. CI logs: `/tmp/wordcraft-recovery-storage-ci.log` and `/tmp/wordcraft-recovery-ui-ci.log`.

## Guarded DOCX packages

Package-wide DOCX preservation state is captured alongside the editor model and media. Restoration retains unknown package data and the exact independently stored original ZIP. Supported text edits can be saved to a new DOCX; unsupported edits remain recoverable but saving DOCX returns an explicit error. The original package can still be exported separately. See [the guarded editing contract](docx-compatibility.md) for budgets and operation limits.
## Automatic desktop snapshots

Desktop WordCraft periodically keeps a local copy of unsaved work, including new drafts.
File › Recover lists available copies. The page opens at startup when copies are found,
unless a document was explicitly created or opened before the startup scan completed.
In particular, a window launched for New/Open keeps the requested document visible;
older copies remain available through File › Recover. Recovery uses the regular interface
palette so its labels and buttons stay readable in light and dark themes.
Restore opens an unsaved document with no file path. Save As chooses where to keep it.
Recovery never writes to the original document. AutoSave's permission to write to an earlier
file is cleared on restore.

Continue without restoring leaves copies available. Discard asks for confirmation and deletes
the selected copy, including its retained generation. It does not delete the editing document.
After discarding that document's copy, another copy is made only after a further edit.
A successful save removes that document's recovery copy on the next worker cycle while it
remains open. Copies left after switching documents can still be explicitly discarded.

Documents with a password do not get plaintext recovery copies. Applying a password waits for
any pending snapshot, removes the current document's recovery generations, and fails if cleanup
fails. Encrypted recovery is not implemented. Documents opened with a password keep recovery
disabled for that session. Removing the password allows future recovery again.

## Storage and timing

Copies live in `recovery/` under the app's profile folder. `WORDCRAFT_PROFILE_DIR` selects a
separate profile for isolated runs, including preferences, logs and control keys.
`WORDCRAFT_NO_PREFS` disables recovery as well as preferences. Headless engine/CLI sessions
and the browser do not create recovery files implicitly.

The desktop host checks for a dirty document every frame but snapshots at most once per ten
seconds, and only when its revision changes. It clones the shared document model for a bounded
worker queue. Encoding, file I/O, full-document validation and discovery run on the worker.
An unchanged document does not schedule repeated recovery repaints. The most recent edits can
be lost if a crash happens before the next snapshot or while its write is still pending.

Version 1 snapshots wrap the native JSON document with an id, document name, timestamp and
monotonic generation. Native JSON now includes media bytes, opaque preserved parts, and
chart/SmartArt/OLE source markup and relationships. Media and preserved parts are written as one
base64 string each. Older JSON without these fields, or with their bytes as arrays of numbers (as
written before base64), still opens.
Paragraph cache revisions are transient and regenerated on load; undo history and selection are
not part of the document snapshot. Recovery preserves the current in-memory model. It cannot
recover content already lost during import or make later export more faithful than the exporter.

Each generation is written exclusively to a temporary file, flushed and synchronized, decoded
and compared with the source document, then renamed to a new generation. The last valid
snapshot is retained until its successor is valid. Discovery falls back to an older valid copy
if the newest is corrupt and reports unreadable files. Abandoned temporary files are cleaned
only after acquiring that record's lock. Native file locks prevent other running instances from
restoring or deleting a live record. Unix folders/files use modes 0700/0600; Windows uses the
profile's inherited permissions. Directory synchronization runs on Unix; Windows synchronizes
the file but does not yet explicitly synchronize directory metadata. Power-loss durability on
Windows is therefore unverified.

Limits are 256 MiB of encoded JSON per snapshot, 2 GiB of snapshot files per profile, 128 records
and 16 generations per record during discovery. Ordinarily only two valid generations remain.
Copies are retained until saved or explicitly discarded; there is no automatic age-based deletion.
Over-limit writes report an error and retain the previous valid copy. A failure caused by the
snapshot itself (the size limit, the folder or disk budget, a value the format cannot keep) is
not retried until the document changes; other failures, such as a lock or I/O error, are tried
again at the next interval. Base64 takes 4/3 of a part's size (a 27 MiB picture makes a 36 MiB
copy), and DOCX compresses, so a copy can still be larger than the DOCX it came from. This first
version has a fixed
interval, English fallback for new labels, and no cloud, version-history or browser recovery.

## Commands and verification

`file.recover={}` reads cached `{enabled,busy,documents,error}`. Each document has `id`, `name`
and `savedAt`. `{"refresh":true}`, `{"restore":"id"}` and `{"discard":"id"}` queue worker
operations. Poll until `busy` is false; inspect `error` for failure. A restore is cancelled if the
document id or revision changes while it loads. Programmatic restores do not open dialogs;
user restores over dirty work use the existing Save Changes prompt.
`ui.recoveryDiscard={"id":"id"}` opens confirmation; `{"answer":"discard"}` or
`{"answer":"cancel"}` answers it.

Focused tests are `cargo test -p wordcraft-engine recovery`,
`cargo test -p wordcraft-engine --test recovery_format`, and
`cargo test -p wordcraft-ui-egui recovery`. They cover lifecycle, preservation, original-file
protection, failed writes, corruption, locks, password cleanup and the actual recovery buttons.
The contribution also uses a native SIGKILL/restart replay in temporary profiles for both new
drafts and edited existing files. It uses synthetic inputs and hashes the original before and
after. No Microsoft Word operation is needed for this reliability test.

## Maintenance validation

The 2026-10-12 rebase retains main’s editable graphic models, native font setup, current
options module, fixed-width Backstage and filename-aware Save As. The SIGKILL/restart
observations above belong to the original contribution, not a rerun on this branch. Current
validation executes the focused lifecycle and preservation tests in temporary profiles and
all repository CI gates; no daily-driver recovery folder or native app is operated.

The desktop host chooses the private recovery profile. Recovery commands accept known
snapshot IDs, never file paths or a profile override. This internal store is separate from
the automation roots that constrain opening and saving user documents.
