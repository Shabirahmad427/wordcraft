# Mendeley

WordCraft imports Mendeley library exports without an account connection. In Mendeley Reference Manager, use **File → Export All → RIS**, then open WordCraft’s **Mendeley** tab and **Import / Insert Citation**. Choose the RIS file on desktop, or paste its text on desktop/web. Search imported references and click **Insert Citation** at the current caret. The dialog offers APA, MLA, Chicago and IEEE styles; **Insert Bibliography** and **Refresh** use the same document source library as References.

Reimporting entries with the same RIS ID updates them and refreshes citations and bibliography. Without an ID, DOI is used for identity; without either, identity is derived from author/title/year, so changing these creates a new entry. Import is one undo step. Incomplete records, missing titles, duplicate IDs, exports above 8 MiB and libraries above 10,000 entries are rejected before source changes.

Commands (CLI, MCP, control channel):

- `mendeley.import`: `{"path":"/path/library.ris"}` (desktop) or `{"ris":"TY  - JOUR\n…"}` (desktop/web).
- `references.sources`: lists document sources; imported tags begin `Mendeley_`.
- `references.citation`: `{"tag":"Mendeley_…"}`.
- `references.citationStyle`: `{"style":"APA"}` (also MLA, Chicago, IEEE).
- `references.bibliography`: optional `title`.
- `mendeley.refresh`: updates generated citations and bibliography.

The source library is stored in the WordCraft.Sources.v1 DOCX custom property; citations use CITATION fields and bibliography uses BIBLIOGRAPHY fields. This preserves refreshable sources when reopened in WordCraft. It does not claim interoperability with Mendeley Cite’s proprietary fields, cloud synchronization, or all CSL styles. Attachments and fields outside WordCraft’s source model are not imported.

Public documentation: [Mendeley export guide](https://www.elsevier.support/mendeley/answer/how-can-i-export-my-library), [API registration](https://dev.mendeley.com/reference/topics/application_registration.html). Cloud integration needs a registered application and user authorization; credentials are not bundled.
