# Clipboard security and resource limits

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7Q native adapter audit and external paste bounds.

## Data flow and bounds

Desktop clipboard transport uses `arboard` 3.6.1. WordCraft publishes a selected fragment as HTML with plain-text alternative in one adapter call. The app keeps the publisher context alive on its UI thread because X11 selection ownership ends when its last context drops unless a clipboard manager has already claimed the data. Paste reads plain text and HTML, and the shared UI passes them to the engine. The engine imports supported HTML through `wordcraft-formats`; it does not provide an image loader for clipboard HTML, so external image URLs are not fetched. Data-URI images remain supported within the HTML input cap.

The engine accepts external HTML and RTF only up to 16 MiB before parsing. Normal Paste, Keep Text Only, and Paste Special reject external plain text over 16 MiB before insertion. Oversized HTML in ordinary Paste is ignored as a rich representation, allowing a bounded plain-text alternative to be used. Oversized plain text is rejected without changing document content.

`arboard`'s HTML and text getters return allocated `String` values and have no caller-supplied size limit. Therefore these bounds protect parsing and document insertion, but do not prevent `arboard` from first allocating an oversized clipboard value. The same limitation applies to text delivered by egui's paste event before WordCraft receives it. RTF and Wayland encoded-image reads use bounded readers; the current HTML/text abstraction has no cross-platform bounded-read API. We have not added direct macOS, Windows, or X11 bindings solely to avoid that allocation.

The shared HTML importer is a tolerant tokenizer rather than a browser engine. It supports a documented subset, caps open-element nesting at 256, ignores scripts/styles and other active or non-content elements, drops unsafe hyperlink targets, ignores unsupported CSS, and does not load external resources in clipboard mode. Malformed markup is tolerated; malformed CF_HTML offsets fall back to its fragment text instead of slicing invalid boundaries. These rules do not amount to a general-purpose HTML sanitizer for rendering arbitrary web pages.

## Verification

Shared engine and format tests cover supported inline formatting, tables and spans, Unicode, malformed/tag-soup markup, unsafe links, skipped scripts/styles, CF_HTML offsets, no external image fetch, oversized HTML fallback, oversized text rejection for all Paste variants, and document preservation. UI tests verify rich-to-plain fallback when the native publisher returns an error.

An opt-in native adapter test runs only when `WORDCRAFT_NATIVE_CLIPBOARD_TEST=1`. The hosted Printing workflow enables it on disposable runners and validates native HTML/plain-text write/read plus shared parsing and insertion on Linux X11 (Xvfb), macOS NSPasteboard and Windows clipboard/CF_HTML. It is not a full GUI interaction or a Microsoft Word interoperability test. Local tests leave the developer clipboard untouched by default. The first hosted Linux and macOS runs caught test issues: X11 data was lost when the short-lived owner was dropped, and macOS HTML import included a trailing empty paragraph. The adapter now retains its X11 owner; the cross-platform assertion checks semantic content and structure without relying on a final empty paragraph.

The remaining limit is the pre-check allocation performed by native text/HTML getters. To remove it would require bounded platform-specific transport or an upstream API change and separate review of macOS pasteboard, Windows clipboard and both X11/Wayland implementations.
