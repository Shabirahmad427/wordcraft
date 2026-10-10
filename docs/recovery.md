# Document recovery

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

Recovery archives (`.wcr`) contain the complete serialized editor model, embedded media, preserved package parts, selection and bibliography style. The two model fields excluded from ordinary JSON serialization (`media` and `passthrough`) are stored as separate archive entries. Citation sources, custom properties, equations, headers/footers and document settings stay in the model. Opening a Word package retains its exact source bytes; those are archived too. Recovery never regenerates a DOCX merely to checkpoint it, so unsupported original parts cannot disappear through the recovery writer.

Each archive is immutable. A writer creates a unique private staging file, writes a CRC-protected ZIP, synchronizes the file, then renames it into place. Unix additionally synchronizes the directory. Incomplete `.partial` files are ignored and failed writes cannot replace preceding snapshots. An OS lock identifies each live writer and releases on process death. Retention uses monotonic sequence numbers, so a backwards wall-clock adjustment cannot delete newer versions. The original-package export synchronizes a sibling staging file and publishes it with an atomic, non-replacing hard link, then removes staging; a filesystem without hard-link support returns an error.

Read limits: 512 MiB archive/expanded payload, 16 MiB model JSON, 1 MiB metadata, 2048 ZIP entries, 4096 directory entries, JSON depth 64 and 250,000 values. Metadata text, attachment keys and entry names are bounded. Duplicate ZIP entries, invalid attachment names/keys, mismatched sizes, CRC failures and unsupported metadata versions return errors. Corrupt archives stay on disk for explicit discard; a failed restore leaves the current document intact. No ZIP entry is extracted by its path. Directories/files use owner-only Unix permissions; Windows uses the user's settings-directory ACLs. Recoveries contain document content and should be treated like saved documents.

## Verification and limits

`cargo test -p wordcraft-engine recovery` covers abrupt process exit, a child process killed during publication, interrupted writes, damaged payload CRC, corrupt ZIP listing/discard, structural limits, immutable previous snapshots, live-session protection, retention and command failure isolation. A synthetic DOCX regression checks exact original bytes (including unknown drawing data), bibliography custom XML, source properties, edited text, pagination and every rendered page pixel after restart. UI tests exercise periodic scheduling, replacement/exit checkpoints, failure followed by successful manual save, startup listing, comparison, restore with Save Changes/Cancel, and explicit corrupt-version discard. `cargo xtask ci` runs the repository's six required checks. `ui_shot` accepts `recoveryDirectory` on its first script line for isolated UI screenshots.

Limits: edits since the last successful snapshot can be lost in a crash; the nominal interval is 15 seconds plus write time. There is no keystroke journal, Undo-stack persistence, automatic salvage of partial ZIPs, encrypted recovery, cloud/version-history service or browser persistence. Large or structurally excessive documents fail recovery visibly; manual save remains available. Comparing full formatting, objects and notes requires visual review. Unsupported DOCX elements remain available in the original package but are not made editable or merged into the recovered edited model's DOCX export. Ordinary export retains the limitations in `production-audit.md`.

Linux native tests and a headless UI inspection have been performed. Portable Rust APIs and existing platform directory rules support macOS and Windows, but real Windows/macOS crash/power-loss tests and independent Microsoft Word compatibility verification remain outstanding. Windows has no directory-fsync implementation in this std-only path; filesystem/hardware behavior can affect power-loss durability. No Microsoft Word 365 parity claim is made.

Verified CI result for these two arcs: all six checks passed; 821 workspace tests passed (16 added recovery tests). The headless screenshot was rendered and inspected without committing a document image asset. CI logs: `/tmp/wordcraft-recovery-storage-ci.log` and `/tmp/wordcraft-recovery-ui-ci.log`.
