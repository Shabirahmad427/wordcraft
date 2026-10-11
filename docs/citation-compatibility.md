# Citation and bibliography compatibility

**Status:** citation-manager package preservation and WordCraft-managed refresh are covered by internal synthetic regressions; direct Zotero, Mendeley, EndNote, CSL processor and Microsoft Word testing has not been performed. Last reviewed: 2026-10-10.

## Current behavior

- The editor inserts WordCraft-managed `CITATION` fields from its source library and renders four built-in styles (APA, MLA, Chicago and IEEE). These are local formatters, not a CSL processor; style names do not imply full style-guide compliance.
- A generated bibliography refreshes after a WordCraft citation is inserted and when a source is added, replaced or removed. Native `CITATION` fields in the body and secondary stories (including notes) refresh when their source metadata or style changes. After manually deleting or reordering citations, run References → Update Citations and Bibliography to refresh cached results and generated lists. Removing a source still used by a WordCraft `CITATION` field is rejected with a clear instruction. Missing source tags preserve the existing visible field result and set an actionable status message.
- Zotero, Mendeley and other citation-manager complex fields are represented as field ranges with their cached, formatted results. Custom document properties and reachable custom XML/package data are preserved by the existing DOCX package preservation path. WordCraft-managed field refresh operates only on `CITATION` instructions; it does not rewrite `ADDIN` field instructions, result runs, or custom metadata.
- WordCraft source data is stored in its own `WordCraft.Sources.v1` custom property. Imported external bibliography data and relationships are retained as opaque package data; WordCraft does not synchronize edits back into an external manager's JSON/XML metadata.
- Mendeley import accepts RIS exports, preserves the fields mapped by the current `Source` model, and uses stable generated IDs when an export has no ID. There is no Mendeley cloud sign-in or database access.
- BibTeX import/export and CSL JSON import/export are not implemented. The four local formatters do not parse arbitrary CSL styles. DOI and other metadata absent from the `Source` model cannot be edited by WordCraft's source manager; the original external package data remains preserved where the DOCX guard applies.

## Safe workflow and limits

Edit native citations through WordCraft's source library and citation fields. External `ADDIN` citations remain editable by their originating application only to the extent that application recognizes the preserved DOCX fields and package metadata; this has not been verified with those applications. Do not replace an external citation with a WordCraft citation unless that conversion is intentional. If an external field's visible result is stale, WordCraft does not claim it can refresh it.

The DOCX regressions use hand-authored synthetic OOXML and test field ranges, formatted cached results, custom properties, source metadata, and package preservation through WordCraft. They do not establish interoperability certification. Save behavior for guarded packages is described in [DOCX compatibility](docx-compatibility.md); complex edits that cannot preserve the original markup fail safely rather than being silently flattened.
