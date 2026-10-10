# Production-readiness audit — 2026-10-10

Baseline: fork main `f3c890f`, incorporating upstream `004286e` and the previously verified fixes. This is a source audit plus synthetic regression evidence, not an independent Microsoft Word 365 compatibility certification. Catalog command coverage is not behavioral parity; the roadmap's historical percentages and time estimates are not validated measurements.

Read: AGENTS.md, README.md, ROADMAP.md, docs/parity.md, workspace manifests, the enforced layering table in xtask/src/layers.rs, and the relevant DOCX, layout, engine, desktop and UI code. The local plan/ directory and ../../craftrules checkout are absent; the documented roadmap fallback was used. The shared craftrules AGENTS.md was unavailable through GitHub (404).

## Five highest-impact deficiencies

| Priority | Deficiency and evidence | Next acceptance criterion |
|---|---|---|
| 1 | Silent DOCX preservation loss. read_drawing supports pictures and wps shapes, then returns None for unsupported graphicData; charts/diagrams/OLE and content-control bindings have no complete preservation path. Document.passthrough already exists, but only VBA parts were carried through; document-attached custom XML, including bibliography libraries, disappeared. The new synthetic custom XML regression failed on the baseline because the saved ZIP lacked item1.xml. | Preserve custom XML first (this arc); then preserve unsupported drawing markup plus its dependent parts without confusing preserved content with editable/rendered content. Build a licensed, consented real-world DOCX corpus and independent render comparisons. |
| 2 | Durable crash recovery is missing despite the old roadmap statement. file.recover lists Session.versions, and those documents live only in process memory. AutoSave covers a file explicitly saved in the current session; it provides no durable checkpoint for a never-saved document. save_path uses a fixed sibling .tmp filename and rename, without unique create_new, fsync or a documented cross-platform replacement protocol. | Durable per-document recovery snapshots including media/opaque parts, unique atomic writes, restart recovery, disk-failure tests and process-kill soak tests. Never substitute JSON snapshots that omit media/passthrough. |
| 3 | Pagination depth remains incomplete. flush_notes places every collected footnote on the current page as a single block; no continuation state exists. Section columns do not balance. Tall notes and multi-column academic papers therefore need focused cases. | Long-footnote continuation and balanced final columns, pinned with geometry tests and independent Word window comparisons using synthetic documents. |
| 4 | Native printing is absent. file.print queues the existing print UI; the documented workflow exports PDF. No platform printer selection, job submission, duplex/copies or print-failure lifecycle is implemented. | Platform adapters behind a common Rust interface using the existing layout/PDF output; job options, cancellation and failures tested on each OS. Retain PDF export. |
| 5 | Drawing/diagram depth is absent. insert.chart, insert.smartArt and arrange.group lack registry implementations; shape grouping and advanced transforms have no complete model/serialization path. Adding toolbar buttons alone would increase coverage without compatibility. | A document object/group model, renderer, hit testing, Undo and DOCX round trips before exposing chart/diagram/group commands. Original icons/assets only. |

## Working functionality to preserve

- Existing OMML import/export, 2D equation layout, equation editor, symbols and scientific input. Remaining equation line breaking is a separate task.
- Existing citations/styles, bibliography, figure/table captions, cross-references and Zotero desktop bridge. The fork's Mendeley RIS import/search/citation controls and DOCX source-library persistence already work; do not reimplement them. Proprietary Mendeley Cite fields and cloud sign-in are separate limitations.
- Existing paragraph/table pagination, floating pictures/text boxes, review commands, track-changes DOCX tags and paragraph-break resolution. Broad “done” labels do not establish depth or independent fidelity.
- Existing original ribbon/UI, font fallback, asynchronous file dialogs, table drag resizing, CLI/MCP/control dispatch and WASM. No UI or command architecture replacement is needed for this fix.

Architecture: pure Rust; geom L0; doc/fonts/proof L1; layout/docx/docbin/formats L2; render/pdf L3; engine L4; mcp/zotero L5; ui-egui L6; applications host platform services. The executable layering rules are authoritative (the older AGENTS table places proof differently). New user-facing behavior remains a registered command, with UI-only behavior in the UI layer.

## First compatibility arc: custom XML preservation

The new crates/docx/src/custom_xml.rs uses the existing Arc-backed Document.passthrough storage. It starts from the main document's implicit customXml relationships and traverses internal dependent relationships iteratively with a visited set. XML data, properties, and their relationship parts are copied byte-for-byte; original part locations and content types are retained. Main-document relationships are regenerated because these roots are implicit, with package-absolute targets. Strict relationship types and arbitrary legal part locations are supported. External dependent relationships are retained as bytes and never fetched; external custom XML roots are rejected as invalid.

Bounds: 512 preserved parts, 64 MiB preserved data, 128 KiB internal bookkeeping, 4096-byte type labels and 1024-byte part paths. Cycles and shared targets terminate and produce one part. Missing dependencies, malformed related relationship XML, generated-part collisions and exhausted budgets return errors instead of silently saving lost data. Bookkeeping is never written into the ZIP. Existing VBA handling stays separate.

Regression evidence:

- Baseline bibliography fixture fails with FileNotFound after saving.
- Synthetic bibliography data, datastore properties and relationship bytes survive an ordinary text edit and three successive saves in DOCX, DOTX and DOCM.
- Multiple roots, shared properties, cycles, external links, Strict relationship types and alternate part locations are tested.
- Missing/reserved parts, corrupt relationship XML, name collisions and malformed/oversized bookkeeping fail safely.
- Engine-level edit/Undo/save/reopen regression retains opaque bytes and compares page count and every raster pixel over a multi-page synthetic document. This is a self-comparison using WordCraft's renderer, not an independent Word oracle or a general visual-fidelity score.

Limitations: bibliography XML is preserved, not imported into WordCraft's editable source model or synchronized with edited citations. Charts, SmartArt, OLE, content-control bindings and other unknown markup are still not lossless. The whole ZIP is regenerated; only preserved custom XML part bytes are promised unchanged. Other export formats need their own preservation contracts. Malformed or oversized custom XML packages can now fail to open/save rather than open/save with silent loss. No native Word, Windows/macOS printer, recovery restart, or real-world licensed corpus comparison was performed in this arc.

## Backlog and modular expansion

Continue with unsupported-object preservation and durable recovery before new drawing features. Next pagination arcs: footnote continuation, column balancing and legacy VML float placement; review existing pending fixes before duplicating them. Follow with native printing, then object/group transforms, charts and diagrams; academic enhancements reuse the existing reference and equation models.

Optional services remain separate future modules: AI assistance accepts explicit selected text and proposes ordinary undoable commands; collaboration owns transport and conflict resolution outside layout; cloud synchronization owns authenticated storage and offline queues; durable version history/recovery owns snapshots and migrations. The core model, renderer, CLI/MCP and offline editing must work without any service. Do not introduce secrets, default uploads, service dependencies or claim these modules already exist.

Validation command: cargo xtask ci (format, Clippy with warnings denied, workspace tests, assets, layers, WASM). Focused reproduction and regression logs: /tmp/wordcraft-custom-xml-{baseline,tests,fixtures,visual}.log. Full cargo xtask ci passed all six steps, including 805 passing workspace tests. Eight regressions were added in this arc (three preservation/limit unit tests, four package compatibility tests, one multi-page raster/Undo test). Log: /tmp/wordcraft-production-docx-ci.log.

## Phase 2 follow-up: document recovery

The durable recovery deficiency is addressed by a separate immutable archive containing the editor model, media, preserved package data and exact opened Word package. Background snapshots and restart restore/discard/text comparison are implemented; existing AutoSave and in-memory Version History remain separate. Original source files are never written by recovery, and unsupported parts remain recoverable from the untouched original package. This does not solve ordinary DOCX export loss. See [the recovery contract and verification limits](recovery.md), including Linux process-kill tests and outstanding Windows/macOS power-loss verification.
