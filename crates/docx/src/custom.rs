//! Custom document properties (`docProps/custom.xml`, ECMA-376 Part 1 §22.3). Citation
//! managers keep their document preferences here (Zotero: `ZOTERO_PREF_1`…), so they must
//! survive a round trip intact.

use wordcraft_doc::CustomProp;

use crate::xml::{El, MAX_DEPTH, Node, W, esc};

/// The format identifier every custom property carries (§22.3.2.2).
const FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";
/// Most properties we keep from one file.
const MAX_PROPS: usize = 10_000;

/// Simple variant types kept as text.
const SIMPLE: &[&str] = &[
    "lpwstr", "lpstr", "bstr", "i1", "i2", "i4", "i8", "int", "ui1", "ui2", "ui4", "ui8", "uint", "r4", "r8", "decimal", "bool", "date", "filetime",
    "cy", "error", "clsid",
];

/// Properties of a parsed `docProps/custom.xml` root.
pub fn read(root: &El) -> Vec<CustomProp> {
    let mut out = Vec::new();
    for p in root.els().filter(|e| e.local() == "property").take(MAX_PROPS) {
        let Some(name) = p.attr("name").filter(|n| !n.is_empty()) else { continue };
        let Some(v) = p.els().next() else { continue };
        let kind = v.local();
        let prop = if SIMPLE.contains(&kind) {
            CustomProp { name: name.to_string(), kind: kind.to_string(), value: v.text() }
        } else {
            let mut xml = String::new();
            el_xml(v, &mut xml, 0);
            CustomProp { name: name.to_string(), kind: "raw".into(), value: xml }
        };
        out.push(prop);
    }
    out
}

/// `docProps/custom.xml` for `props`.
pub fn write(props: &[CustomProp]) -> Vec<u8> {
    let mut w = W::new();
    w.open(
        "Properties",
        &[
            ("xmlns", "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties"),
            ("xmlns:vt", "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"),
        ],
    );
    // Property ids start at 2 (§22.3.2.2).
    for (pid, p) in (2u32..).zip(props.iter().filter(|p| !p.name.is_empty())) {
        if p.kind == "raw" && !raw_is_wellformed(&p.value) {
            continue;
        }
        w.open("property", &[("fmtid", FMTID), ("pid", &pid.to_string()), ("name", &p.name)]);
        if p.kind == "raw" {
            w.raw(&p.value);
        } else {
            let kind = if SIMPLE.contains(&p.kind.as_str()) { p.kind.as_str() } else { "lpwstr" };
            w.leaf(&format!("vt:{kind}"), &[], &p.value);
        }
        w.close("property");
    }
    w.close("Properties");
    w.into_bytes()
}

/// A raw value is written only if it parses inside a `vt`-namespaced wrapper.
fn raw_is_wellformed(xml: &str) -> bool {
    let doc = format!(r#"<x xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">{xml}</x>"#);
    crate::xml::parse(doc.as_bytes()).is_ok()
}

/// Serialise an element with its canonical prefixes.
fn el_xml(e: &El, out: &mut String, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        if k.starts_with("xmlns") {
            continue;
        }
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&esc(v));
        out.push('"');
    }
    if e.kids.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for k in &e.kids {
        match k {
            Node::El(c) => el_xml(c, out, depth + 1),
            Node::Text(t) => out.push_str(&esc(t)),
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

/// Deserialize WordCraft's source library with a cap during parsing, before a hostile array
/// could allocate an unbounded number of source objects.
pub(crate) fn read_sources(value: &str) -> Result<Vec<wordcraft_doc::Source>, serde_json::Error> {
    struct Sources;
    impl<'de> serde::de::Visitor<'de> for Sources {
        type Value = Vec<wordcraft_doc::Source>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most 10000 bibliography sources")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut sources = Vec::new();
            while let Some(source) = seq.next_element::<wordcraft_doc::Source>()? {
                if sources.len() >= 10000 {
                    return Err(serde::de::Error::custom("source library exceeds 10000 entries"));
                }
                sources.push(source);
            }
            Ok(sources)
        }
    }
    use serde::Deserializer;
    let mut deserializer = serde_json::Deserializer::from_str(value);
    let sources = (&mut deserializer).deserialize_seq(Sources)?;
    deserializer.end()?;
    Ok(sources)
}

#[cfg(test)]
mod source_tests {
    #[test]
    fn oversized_and_malformed_source_arrays_are_rejected() {
        assert!(super::read_sources(&format!("[{}]", vec!["{}"; 10001].join(","))).is_err());
        assert!(super::read_sources("[{\"title\":\"book\"}] trailing").is_err());
        assert!(super::read_sources("{}").is_err());
    }
}
