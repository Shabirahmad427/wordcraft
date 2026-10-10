//! Strict, namespace-aware XML inspection with offsets into the untouched source bytes.
//! Used only for preservation validation and narrowly scoped text splices, never execution.
use crate::DocxError;
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use std::{collections::BTreeMap, ops::Range};

pub(crate) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(crate) const WS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const RS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
pub(crate) const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(crate) const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MAX_NODES: usize = 200_000;
const MAX_XML: usize = 16 * 1024 * 1024;

pub(crate) fn error(message: impl Into<String>) -> DocxError {
    DocxError::Preservation(message.into())
}
#[derive(Debug)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<usize>,
    pub text: String,
    pub span: Range<usize>,
    pub content: Range<usize>,
    pub empty: bool,
}
impl Element {
    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.name == qualified(namespace, local)
    }
    pub fn word(&self, local: &str) -> bool {
        self.is(W, local) || self.is(WS, local)
    }
}
pub(crate) fn qualified(namespace: &str, local: &str) -> String {
    format!("{{{namespace}}}{local}")
}
fn name(namespace: ResolveResult<'_>, local: &[u8]) -> Result<String, DocxError> {
    let local = std::str::from_utf8(local).map_err(|e| error(e.to_string()))?;
    match namespace {
        ResolveResult::Bound(uri) => Ok(qualified(std::str::from_utf8(uri.as_ref()).map_err(|e| error(e.to_string()))?, local)),
        ResolveResult::Unbound => Ok(qualified("", local)),
        ResolveResult::Unknown(_) => Err(error("unbound XML namespace prefix")),
    }
}
/// UTF-16 is checked without lossy decoding. Offsets then refer to the transcoded XML;
/// edits to such a part are refused, while unchanged parts retain their original bytes.
pub(crate) fn utf8(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, DocxError> {
    if bytes.len() > MAX_XML {
        return Err(error("XML preservation inspection exceeds 16 MiB"));
    }
    let (rest, little) = match bytes.get(..2) {
        Some([0xff, 0xfe]) => (bytes.get(2..).ok_or_else(|| error("UTF-16 input"))?, true),
        Some([0xfe, 0xff]) => (bytes.get(2..).ok_or_else(|| error("UTF-16 input"))?, false),
        _ => return Ok(std::borrow::Cow::Borrowed(bytes)),
    };
    if rest.len() % 2 != 0 {
        return Err(error("incomplete UTF-16 code unit"));
    }
    let units: Vec<_> = rest.as_chunks::<2>().0.iter().copied().map(|b| if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }).collect();
    let text = String::from_utf16(&units).map_err(|e| error(e.to_string()))?;
    if text.len() > MAX_XML {
        return Err(error("transcoded XML exceeds 16 MiB"));
    }
    Ok(std::borrow::Cow::Owned(text.into_bytes()))
}
fn valid_char(c: char) -> bool {
    (c >= '\u{20}' || matches!(c, '\t' | '\n' | '\r')) && !matches!(c, '\u{fffe}' | '\u{ffff}')
}
fn check_chars(text: &str) -> Result<(), DocxError> {
    if text.chars().all(valid_char) { Ok(()) } else { Err(error("forbidden XML character")) }
}
pub(crate) fn inspect(bytes: &[u8]) -> Result<Vec<Element>, DocxError> {
    let bytes = utf8(bytes)?;
    check_chars(std::str::from_utf8(&bytes).map_err(|e| error(e.to_string()))?)?;
    let was_utf16 = matches!(bytes, std::borrow::Cow::Owned(_));
    let mut reader = NsReader::from_reader(bytes.as_ref());
    reader.config_mut().check_end_names = true;
    let mut nodes: Vec<Element> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut roots = 0usize;
    loop {
        let begin = reader.buffer_position() as usize;
        let (namespace, event) = reader.read_resolved_event().map_err(|e| error(format!("malformed XML: {e}")))?;
        let element_name = match &event {
            Event::Start(element) | Event::Empty(element) => name(namespace, element.local_name().as_ref())?,
            _ => String::new(),
        };
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                if nodes.len() >= MAX_NODES || stack.len() >= 256 {
                    return Err(DocxError::Limit("XML preservation structure limit".into()));
                }
                let mut attrs = BTreeMap::new();
                for attr in element.attributes() {
                    let attr = attr.map_err(|e| error(e.to_string()))?;
                    if attr.key.as_ref() == b"xmlns" || attr.key.as_ref().starts_with(b"xmlns:") {
                        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| error(e.to_string()))?;
                        let value = attr.unescape_value().map_err(|e| error(e.to_string()))?.into_owned();
                        check_chars(&value)?;
                        attrs.insert(format!("namespace:{key}"), value);
                        continue;
                    }
                    let (namespace, local) = reader.resolve_attribute(attr.key);
                    let key = name(namespace, local.as_ref())?;
                    let value = attr.unescape_value().map_err(|e| error(e.to_string()))?.into_owned();
                    check_chars(&value)?;
                    if attrs.insert(key, value).is_some() {
                        return Err(error("duplicate expanded XML attribute"));
                    }
                }
                let index = nodes.len();
                if let Some(parent) = stack.last().and_then(|i| nodes.get_mut(*i)) {
                    parent.children.push(index);
                } else {
                    roots += 1;
                    if roots > 1 {
                        return Err(error("multiple XML roots"));
                    }
                }
                let empty = matches!(event, Event::Empty(_));
                nodes.push(Element {
                    name: element_name,
                    attrs,
                    children: Vec::new(),
                    text: String::new(),
                    span: begin..end,
                    content: end..end,
                    empty,
                });
                if !empty {
                    stack.push(index);
                }
            }
            Event::End(_) => {
                let index = stack.pop().ok_or_else(|| error("unbalanced XML end tag"))?;
                let node = nodes.get_mut(index).ok_or_else(|| error("XML inspection index"))?;
                node.span.end = end;
                node.content.end = begin;
            }
            Event::Text(ref text) => {
                let text = text.decode().map_err(|e| error(e.to_string()))?;
                if let Some(node) = stack.last().and_then(|i| nodes.get_mut(*i)) {
                    node.text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err(error("text outside XML root"));
                }
            }
            Event::CData(ref text) => {
                let text = text.decode().map_err(|e| error(e.to_string()))?;
                let node = stack.last().and_then(|i| nodes.get_mut(*i)).ok_or_else(|| error("CDATA outside XML root"))?;
                node.text.push_str(&text);
            }
            Event::GeneralRef(ref reference) => {
                let value = match reference.resolve_char_ref().map_err(|e| error(e.to_string()))? {
                    Some(c) => c.to_string(),
                    None => {
                        let reference = reference.decode().map_err(|e| error(e.to_string()))?;
                        quick_xml::escape::resolve_predefined_entity(&reference).ok_or_else(|| error("unsupported XML entity"))?.to_string()
                    }
                };
                let node = stack.last().and_then(|i| nodes.get_mut(*i)).ok_or_else(|| error("entity outside XML root"))?;
                check_chars(&value)?;
                node.text.push_str(&value);
            }
            Event::Decl(ref declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|e| error(e.to_string()))?;
                    let supported = encoding.eq_ignore_ascii_case(b"UTF-8")
                        || (was_utf16 && [b"UTF-16".as_slice(), b"UTF-16LE", b"UTF-16BE"].iter().any(|e| encoding.eq_ignore_ascii_case(e)));
                    if !supported {
                        return Err(error("unsupported or mismatched preserved XML encoding"));
                    }
                }
            }
            Event::DocType(_) => return Err(error("DTD declarations are not allowed in preserved OOXML")),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() || roots != 1 {
        return Err(error("incomplete XML document"));
    }
    Ok(nodes)
}
pub(crate) fn equivalent(left: &[u8], right: &[u8]) -> Result<bool, DocxError> {
    if left == right {
        return Ok(true);
    }
    // Comments, processing instructions and declaration details can carry information
    // outside the modeled element tree. A lexical change there requires guarded retention.
    let has_misc = |bytes: &[u8]| {
        bytes.windows(4).any(|b| b == b"<!--")
            || bytes.windows(2).enumerate().any(|(index, b)| b == b"<?" && bytes.get(index..index.saturating_add(5)) != Some(b"<?xml"))
    };
    let declaration = |bytes: &[u8]| {
        bytes
            .windows(5)
            .position(|b| b == b"<?xml")
            .and_then(|start| {
                bytes.get(start..).and_then(|rest| rest.windows(2).position(|b| b == b"?>").and_then(|end| rest.get(..end.saturating_add(2))))
            })
            .map(|d| d.to_vec())
    };
    if has_misc(left) || has_misc(right) || declaration(left) != declaration(right) {
        return Ok(false);
    }
    let left = inspect(left)?;
    let right = inspect(right)?;
    Ok(left.len() == right.len()
        && left.iter().zip(&right).all(|(a, b)| a.name == b.name && a.attrs == b.attrs && a.children == b.children && a.text == b.text))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_aliases_duplicate_attributes_and_incomplete_xml_are_checked() {
        let nodes = inspect(r#"<x:root xmlns:x="urn:future"><x:text attr="&amp;">α &amp; β</x:text></x:root>"#.as_bytes()).unwrap();
        assert!(nodes.first().unwrap().is("urn:future", "root"));
        let text = nodes.get(1).unwrap();
        assert_eq!(text.text, "α & β");
        for bytes in [
            b"<a><b></a>".as_slice(),
            b"<future:a/>",
            b"<a x='1' x='2'/>",
            b"<!DOCTYPE a><a/>",
            b"<a/><b/>",
            b"<a>",
            b"<a>\0</a>",
            b"<a>&#0;</a>",
            b"<a x='&#0;'/>",
            b"<?xml version='1.0' encoding='ISO-8859-1'?><a/>",
        ] {
            assert!(inspect(bytes).is_err(), "{bytes:?}");
        }
    }
    #[test]
    fn xml_depth_and_utf16_corruption_are_bounded() {
        let deep = format!("{}{}", "<a>".repeat(257), "</a>".repeat(257));
        assert!(matches!(inspect(deep.as_bytes()), Err(DocxError::Limit(_))));
        assert!(inspect(&[0xff, 0xfe, b'<']).is_err());
        assert!(inspect(&[0xff, 0xfe, 0, 0xd8]).is_err());
    }
}
