# CSL processor integration assessment

**Status:** No CSL processor is integrated. This audit is a dependency decision, not a claim of CSL support. Reviewed 2026-10-11.

## Decision

Do not add an automatic CSL 1.0.2 formatting path until the engine has a maintained, testable interface whose output can be compared against the CSL conformance fixtures. The available Rust choices do not currently meet that bar for arbitrary user styles. Keeping the existing WordCraft-managed formatter is safer than silently converting CSL styles into a different model or pinning an inactive processor as if it were a stable dependency.

## Candidates

| Candidate | Strength | Limitation relevant to WordCraft | Decision |
|---|---|---|---|
| [`citeproc-rs`](https://github.com/citation-style-language/citeproc-rs) | Native Rust implementation intended to process CSL, includes citation grouping, disambiguation and a CSL test harness. | Its README calls it work in progress and says some official CSL tests fail. The checked-out CSL project head is dated 2023-01-31; no stable published Rust processor package was identified. This is a high-risk dependency for user documents and the existing WASM/release matrix. | Do not integrate as the production formatter without a maintained release, current toolchain validation and a passing, reported conformance baseline. |
| [`citum-engine`](https://crates.io/crates/citum-engine) with [`citum-migrate`](https://crates.io/crates/citum-migrate) | Current pure-Rust engine, usable as a library; supports grouped citation processing, sorting, disambiguation, localized output and native HTML rendering. MIT/Apache-2.0. | Citum uses its own YAML style model, not CSL XML. The companion converter is a best-effort migration system; its own design documentation explains that CSL macros, conditional branches, empty-group suppression and type-specific layouts do not map mechanically. Integrating it would mean offering converted Citum styles, not general CSL 1.0.2. The migrator also brings a much larger dependency surface than a citation feature should add without an end-to-end test corpus. | Reassess for a separate, explicitly labeled Citum-style integration or for selected styles after output equivalence is independently demonstrated. Do not advertise arbitrary `.csl` compatibility. |

The official CSL project describes CSL styles as the source data for thousands of journal and publisher formats. A supported list of several familiar style names does not establish support for the full format. See the [CSL developer overview](https://citationstyles.org/developers/), [CSL 1.0.2 specification](https://docs.citationstyles.org/en/stable/specification.html), and [official styles repository](https://github.com/citation-style-language/styles).

## Current WordCraft behavior

- WordCraft keeps its existing local APA, MLA, Chicago and IEEE formatter for WordCraft-managed `CITATION` fields. These are limited formatters, not CSL style processing.
- BibTeX and CSL JSON remain source interchange formats. CSL JSON import/export does not imply support for CSL XML style files or the CSL processor behavior.
- External Zotero/Mendeley/EndNote fields are still not passed through a WordCraft citation formatter. Package/field preservation protections remain in force.
- The current document field model stores one cached result string per WordCraft citation field. It does not yet represent CSL rich text, citation item groups, note positions, or style-engine warnings in a way that can round-trip as editable fields. Those are integration prerequisites, not cosmetic details.

## Safe next gate

Before adopting a processor, pin a maintained release and run its full supported CSL fixture suite on the project’s supported Rust targets. Add a corpus containing numeric, author-date and note styles; multiple-item clusters; locators; sort/collapse; author/year disambiguation; locale terms; italics and links; DOI and page-range formatting; missing metadata; and malformed/unsupported constructs. Compare known expected outputs and expose unsupported input as a clear error or warning. Then implement a WordCraft adapter that preserves field markup and rich bibliography formatting, and prove Undo, recovery, DOCX save/reopen and PDF output. Until those checks pass, continue to label WordCraft’s four built-in styles as limited local formatters.
