//! Import Mendeley RIS exports into the document's portable source library.
//! No desktop database or proprietary plugin is read.
use crate::{CmdError, CmdResult, CommandSpec, Session, p};
use serde_json::{Value, json};
use wordcraft_doc::Source;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCES: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("mendeley.import", "Import Mendeley Library", "Mendeley › Library", import)
            .params(r#"{"ris": string} or {"path": string}; imports a Mendeley RIS export, updates matching IDs"#),
        CommandSpec::new("mendeley.refresh", "Refresh", "Mendeley › Document", |s, _| {
            super::citations::update_citations(s)?;
            Ok(json!({"refreshed": true}))
        }),
    ]
}

fn import(s: &mut Session, v: &Value) -> CmdResult {
    let text = if let Some(text) = p::str(v, "ris") {
        if text.len() > MAX_BYTES {
            return Err(CmdError::Params("RIS export exceeds 8 MiB".into()));
        }
        text.to_string()
    } else if let Some(path) = p::str(v, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Read;
            let f = std::fs::File::open(path).map_err(|e| CmdError::Failed(e.to_string()))?;
            let mut text = String::new();
            f.take((MAX_BYTES + 1) as u64).read_to_string(&mut text).map_err(|e| CmdError::Failed(e.to_string()))?;
            if text.len() > MAX_BYTES {
                return Err(CmdError::Params("RIS export exceeds 8 MiB".into()));
            }
            text
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err(CmdError::Params("give `ris` text in the browser".into()));
        }
    } else {
        return Err(CmdError::Params("give `ris` text or an exported RIS `path`".into()));
    };
    let incoming = parse_ris(&text).map_err(CmdError::Params)?;
    let mut sources = s.doc.sources.clone();
    let mut added = 0;
    let mut updated = 0;
    for src in incoming {
        if let Some(existing) = sources.iter_mut().find(|x| x.tag == src.tag) {
            *existing = src;
            updated += 1;
        } else {
            sources.push(src);
            added += 1;
        }
    }
    if sources.len() > MAX_SOURCES {
        return Err(CmdError::Params("source library exceeds 10000 entries".into()));
    }
    s.doc.sources = sources;
    super::citations::update_citations(s)?;
    s.touch();
    Ok(json!({"added": added, "updated": updated, "total": s.doc.sources.len()}))
}

/// Parse records completely before changing the document. Reject incomplete exports.
fn parse_ris(text: &str) -> Result<Vec<Source>, String> {
    let mut out = Vec::new();
    let mut record: Option<Source> = None;
    let mut last = String::new();
    for (n, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let field = line.get(..5).filter(|prefix| prefix.as_bytes().get(2..5) == Some(b"  -"));
        let Some(prefix) = field else {
            if line.starts_with(char::is_whitespace) {
                let src = record.as_mut().ok_or_else(|| format!("RIS line {}: continuation outside a record", n + 1))?;
                append(src, &last, line.trim());
                continue;
            }
            return Err(format!("RIS line {}: expected a tagged field", n + 1));
        };
        let tag = prefix.get(..2).unwrap_or("");
        let value = line.get(5..).unwrap_or("").trim();
        match tag {
            "TY" => {
                if record.is_some() {
                    return Err("RIS record has no ER terminator".into());
                }
                let kind = match value {
                    "JOUR" | "EJOUR" => "article",
                    "BOOK" | "CHAP" => "book",
                    "RPRT" => "report",
                    "ELEC" => "website",
                    _ => "other",
                };
                record = Some(Source { kind: kind.into(), ..Default::default() });
            }
            "ER" => {
                let mut src = record.take().ok_or("RIS ER outside a record")?;
                if src.title.trim().is_empty() {
                    return Err("RIS source needs a title (TI or T1)".into());
                }
                if src.tag.is_empty() {
                    // Stable across reordering and reimport: do not use record position.
                    let identity = format!("{}\n{}\n{}", src.author, src.title, src.year);
                    src.tag = format!("Mendeley_{:016x}", hash(identity.as_bytes()));
                }
                if out.iter().any(|x: &Source| x.tag == src.tag) {
                    return Err(format!("duplicate RIS source ID {}", src.tag));
                }
                out.push(src);
                if out.len() > MAX_SOURCES {
                    return Err("RIS export exceeds 10000 entries".into());
                }
            }
            _ => {
                let src = record.as_mut().ok_or_else(|| format!("RIS line {}: field outside a record", n + 1))?;
                if tag == "ID" || tag == "DO" {
                    if src.tag.is_empty() || tag == "ID" {
                        src.tag = format!("Mendeley_{:016x}", hash(value.as_bytes()));
                    }
                } else if tag == "AU" || tag == "A1" {
                    if !src.author.is_empty() {
                        src.author.push_str("; ");
                    }
                    src.author.push_str(value);
                } else if tag == "PY" || tag == "Y1" {
                    src.year = value.split('/').next().unwrap_or("").to_string();
                } else if tag == "EP" {
                    if !src.pages.is_empty() && !value.is_empty() {
                        src.pages.push('–');
                    }
                    src.pages.push_str(value);
                } else {
                    append(src, tag, value);
                }
            }
        }
        last = tag.to_string();
    }
    if record.is_some() {
        return Err("RIS record has no ER terminator".into());
    }
    if out.is_empty() {
        return Err("no RIS references found; export the library as RIS in Mendeley".into());
    }
    Ok(out)
}

fn append(src: &mut Source, tag: &str, value: &str) {
    let dest = match tag {
        "TI" | "T1" => &mut src.title,
        "JO" | "JF" | "T2" => &mut src.journal,
        "PB" => &mut src.publisher,
        "CY" | "PP" => &mut src.city,
        "VL" => &mut src.volume,
        "SP" => &mut src.pages,
        "UR" => &mut src.url,
        _ => return,
    };
    if !dest.is_empty() {
        dest.push(' ');
    }
    dest.push_str(value);
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    const RIS: &str = "TY  - JOUR\nID  - abc-123\nAU  - Rivera, Alex\nAU  - Chen, Mei\nTI  - Shared Spaces\nPY  - 2021/01/01\nJO  - Open Research\nVL  - 7\nSP  - 12\nEP  - 19\nER  - \n";
    #[test]
    fn import_cite_save_reopen_refresh_and_undo() {
        let mut s = Session::new(wordcraft_doc::Document::new());
        assert_eq!(s.run("mendeley.import", &json!({"ris": RIS})).unwrap()["added"], 1);
        let src = s.doc.sources[0].clone();
        assert_eq!((&*src.author, &*src.year, &*src.pages), ("Rivera, Alex; Chen, Mei", "2021", "12–19"));
        s.run("references.citation", &json!({"tag": src.tag})).unwrap();
        s.run("references.bibliography", &json!({})).unwrap();
        let bytes = wordcraft_docx::write(&s.doc).unwrap();
        let doc = wordcraft_docx::read(&bytes).unwrap();
        assert_eq!(doc.sources, s.doc.sources);
        let mut s = Session::new(doc);
        let changed = RIS.replace("2021/01/01", "2025/01/01");
        assert_eq!(s.run("mendeley.import", &json!({"ris": changed})).unwrap()["updated"], 1);
        assert_eq!(s.doc.sources.len(), 1);
        assert!(s.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("2025"));
        s.run("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.sources[0].year, "2021");
    }
    #[test]
    fn malformed_import_is_atomic_and_hostile_input_is_safe() {
        let mut s = Session::new(wordcraft_doc::Document::new());
        for ris in [format!("{RIS}TY  - BOOK\nTI  - unfinished"), format!("{RIS}{RIS}"), "☃☃  - bad".into(), "ER  - \n".into()] {
            assert!(s.run("mendeley.import", &json!({"ris": ris})).is_err());
            assert!(s.doc.sources.is_empty());
            assert!(!s.dirty);
        }
        assert!(s.run("mendeley.import", &json!({"ris": "x".repeat(MAX_BYTES + 1)})).is_err());
    }
    #[test]
    fn stable_ids_unicode_continuations_and_missing_terminator() {
        let ris = "TY  - BOOK\nTI  - 文献\n      continued\nAU  - 王, 明\nPY  - 2024\nER  - \n";
        let a = parse_ris(ris).unwrap();
        assert_eq!(a[0].title, "文献 continued");
        assert_eq!(a, parse_ris(&format!("\u{feff}{ris}")).unwrap());
        assert!(parse_ris("TY  - BOOK\nTI  - book\n").is_err());
    }
}
