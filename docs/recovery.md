# Document recovery

## Storage foundation (Phase 2, arc 1)

Recovery archives (`.wcr`) are separate from saved documents. They contain the serialized editor model, embedded media, preserved package parts, selection and bibliography style. Opening a DOCX/DOCM/DOTX/DOTM retains its exact source package in memory; recovery archives include those original bytes, including unsupported markup and relationships. Restoring creates a modified, unnamed working copy requiring Save As. Exporting the original package refuses to overwrite an existing file.

Each archive is immutable. A writer creates a unique private staging file, writes a CRC-protected ZIP, synchronizes the file, then renames it into place. Unix additionally synchronizes the directory. Incomplete `.partial` files are ignored. A failed write cannot replace the preceding snapshot. OS file locks identify live writers and release on process death; another session cannot discard their snapshots. Retention prunes only the current writer's versions of the same document after a successful write.

Read limits: 512 MiB archive/expanded payload, 16 MiB model JSON, 1 MiB metadata, 2048 ZIP entries, 4096 directory entries, JSON depth 64 and 250,000 values. Duplicate ZIP entries, invalid attachment paths/keys, mismatched sizes and unsupported metadata versions fail visibly. Nothing is extracted by ZIP entry path. Recovery directories/files use owner-only permissions on Unix; Windows uses the user's settings-directory ACLs.

This foundation is native Rust and uses the existing workspace ZIP dependency. Automatic scheduling and the recovery dialog follow in arc 2. Existing AutoSave and in-memory Version History are unchanged. This is not independent Microsoft Word parity verification. Linux process-exit and interrupted-write regressions do not establish Windows/macOS power-loss guarantees. Unsupported DOCX elements are retained in the original package; they are not made editable or rendered by recovery. Ordinary DOCX export still has the limitations described in `production-audit.md`.

Tests: `cargo test -p wordcraft-engine recovery::tests`. The subprocess regression exits without Rust destructors after committing an unsaved snapshot and leaving an incomplete staging file; reopening the store verifies lock release and restored content. Other tests cover archive fidelity, corrupted ZIPs, bounded hostile JSON, failed writes, immutable previous snapshots, per-document retention, live-session protection and original-copy overwrite refusal.
