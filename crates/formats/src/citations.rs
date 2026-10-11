//! Bibliographic interchange helpers. These handle portable records only; CSL style processing
//! is separate and is not implied by importing/exporting CSL JSON.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use wordcraft_doc::Source;

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SOURCES: usize = 10_000;

/// Import BibTeX entries, retaining every field as source metadata so unsupported entry fields
/// remain exportable. Macro expansion and concatenation are rejected rather than guessed.
pub fn import_bibtex(text: &str) -> Result<Vec<Source>, String> {
    check_size(text.len())?;
    let entries = bib_entries(text)?;
    let mut out = Vec::new();
    let mut tags = BTreeSet::new();
    for (kind, key, fields) in entries {
        if out.len() >= MAX_SOURCES {
            return Err(format!("BibTeX export exceeds {MAX_SOURCES} entries"));
        }
        let tag = if key.is_empty() { stable_tag(&fields) } else { key.clone() };
        if !tags.insert(tag.clone()) {
            return Err(format!("duplicate BibTeX citation key `{tag}`"));
        }
        let mut source = Source { tag, kind: normalize_bib_kind(&kind).into(), ..Default::default() };
        for (field, value) in &fields {
            match field.as_str() {
                "author" => source.author = bib_authors(value),
                "title" => source.title = value.clone(),
                "year" | "date" => source.year = value.get(..4).unwrap_or(value).to_string(),
                "publisher" => source.publisher = value.clone(),
                "address" | "location" => source.city = value.clone(),
                "journal" | "journaltitle" | "booktitle" | "container-title" => source.journal = value.clone(),
                "volume" => source.volume = value.clone(),
                "pages" | "page" => source.pages = value.clone(),
                "url" => source.url = value.clone(),
                "doi" => source.doi = value.clone(),
                "isbn" => source.isbn = value.clone(),
                "number" | "issue" => source.issue = value.clone(),
                "edition" => source.edition = value.clone(),
                _ => {}
            }
        }
        if source.title.trim().is_empty() {
            return Err(format!("BibTeX entry `{}` has no title", source.tag));
        }
        source.metadata = fields.into_iter().map(|(key, value)| (key, Value::String(value))).collect();
        source.metadata.insert("_wordcraftFormat".into(), json!("bibtex"));
        if let Some(raw) = source.metadata.get("author").cloned() {
            source.metadata.insert("_wordcraftAuthorSnapshot".into(), json!(source.author));
            source.metadata.insert("_wordcraftRawAuthor".into(), raw);
        }
        out.push(source);
    }
    if out.is_empty() {
        return Err("no BibTeX entries found".into());
    }
    Ok(out)
}

/// Export source records as BibTeX, retaining extra imported fields where present.
pub fn export_bibtex(sources: &[Source]) -> Result<String, String> {
    check_count(sources.len())?;
    let mut out = String::new();
    for source in sources {
        if source.title.trim().is_empty() {
            return Err(format!("source `{}` has no title", source.tag));
        }
        let key = bib_key(&source.tag);
        let kind = bib_kind(&source.kind);
        let mut fields: BTreeMap<String, String> = source
            .metadata
            .iter()
            .filter(|(key, _)| !key.starts_with("_wordcraft"))
            .filter_map(|(key, value)| value.as_str().map(|value| (key.to_ascii_lowercase(), value.to_string())))
            .collect();
        let raw_author = source.metadata.get("_wordcraftFormat").and_then(Value::as_str) == Some("bibtex")
            && source.metadata.get("_wordcraftAuthorSnapshot").and_then(Value::as_str) == Some(source.author.as_str());
        fields.insert("title".into(), source.title.clone());
        fields.insert("year".into(), source.year.clone());
        if !source.author.is_empty() && !raw_author {
            fields.insert("author".into(), source.author.split("; ").collect::<Vec<_>>().join(" and "));
        } else if let Some(raw) = source.metadata.get("_wordcraftRawAuthor").and_then(Value::as_str) {
            fields.insert("author".into(), raw.into());
        }
        set_if_nonempty(&mut fields, "publisher", &source.publisher);
        set_if_nonempty(&mut fields, "address", &source.city);
        set_if_nonempty(&mut fields, "journal", &source.journal);
        set_if_nonempty(&mut fields, "volume", &source.volume);
        set_if_nonempty(&mut fields, "pages", &source.pages);
        set_if_nonempty(&mut fields, "url", &source.url);
        set_if_nonempty(&mut fields, "doi", &source.doi);
        set_if_nonempty(&mut fields, "isbn", &source.isbn);
        set_if_nonempty(&mut fields, "number", &source.issue);
        set_if_nonempty(&mut fields, "edition", &source.edition);
        out.push_str(&format!("@{kind}{{{key},\n"));
        for (name, value) in fields {
            if name.starts_with('_') || value.is_empty() {
                continue;
            }
            out.push_str(&format!("  {name} = {{{value}}},\n"));
        }
        out.push_str("}\n\n");
    }
    Ok(out)
}

/// Import CSL JSON (a record or an array of records). Unknown properties and original creator
/// structures are retained in `Source::metadata` and can be written back unchanged.
pub fn import_csl_json(text: &str) -> Result<Vec<Source>, String> {
    check_size(text.len())?;
    let root: Value = serde_json::from_str(text).map_err(|e| format!("invalid CSL JSON: {e}"))?;
    let values: Vec<&Value> = match &root {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![&root],
        _ => return Err("CSL JSON must be an object or an array of objects".into()),
    };
    check_count(values.len())?;
    let mut tags = BTreeSet::new();
    let known: BTreeSet<&str> = [
        "id",
        "type",
        "title",
        "author",
        "issued",
        "publisher",
        "publisher-place",
        "container-title",
        "volume",
        "issue",
        "page",
        "URL",
        "DOI",
        "ISBN",
        "edition",
    ]
    .into_iter()
    .collect();
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let object = value.as_object().ok_or("CSL JSON entries must be objects")?;
        let title = text_value(object.get("title"));
        if title.trim().is_empty() {
            return Err("CSL JSON entry needs a title".into());
        }
        let raw_author = object.get("author").cloned().unwrap_or(Value::Null);
        let author = csl_author(&raw_author)?;
        let year = csl_year(object.get("issued"));
        let suggested_id = object.get("id").and_then(Value::as_str).filter(|id| !id.trim().is_empty()).map(str::to_string);
        let tag = suggested_id.unwrap_or_else(|| stable_identity_tag(&author, &title, &year));
        if !tags.insert(tag.clone()) {
            return Err(format!("duplicate CSL source id `{tag}`"));
        }
        let mut source = Source {
            tag,
            kind: text_value(object.get("type")),
            author,
            title,
            year,
            publisher: text_value(object.get("publisher")),
            city: text_value(object.get("publisher-place")),
            journal: text_value(object.get("container-title")),
            volume: text_value(object.get("volume")),
            pages: text_value(object.get("page")),
            url: text_value(object.get("URL")),
            doi: text_value(object.get("DOI")),
            isbn: text_value(object.get("ISBN")),
            issue: text_value(object.get("issue")),
            edition: text_value(object.get("edition")),
            ..Default::default()
        };
        source.metadata = object.iter().filter(|(key, _)| !known.contains(key.as_str())).map(|(key, value)| (key.clone(), value.clone())).collect();
        source.metadata.insert("_wordcraftFormat".into(), json!("csl-json"));
        if !raw_author.is_null() {
            source.metadata.insert("author".into(), raw_author);
            source.metadata.insert("_wordcraftAuthorSnapshot".into(), json!(source.author));
        }
        if let Some(issued) = object.get("issued") {
            source.metadata.insert("issued".into(), issued.clone());
            source.metadata.insert("_wordcraftYearSnapshot".into(), json!(source.year));
        }
        out.push(source);
    }
    if out.is_empty() {
        return Err("no CSL JSON records found".into());
    }
    Ok(out)
}

/// Export CSL JSON while retaining unmodeled fields and creator/date structures from import.
pub fn export_csl_json(sources: &[Source]) -> Result<String, String> {
    check_count(sources.len())?;
    let values = sources.iter().map(csl_record).collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string_pretty(&values).map_err(|e| format!("could not encode CSL JSON: {e}"))
}

fn csl_record(source: &Source) -> Result<Value, String> {
    if source.title.trim().is_empty() {
        return Err(format!("source `{}` has no title", source.tag));
    }
    let mut object: Map<String, Value> = source
        .metadata
        .iter()
        .filter(|(key, _)| {
            !key.starts_with("_wordcraft")
                && !matches!(
                    key.as_str(),
                    "author"
                        | "year"
                        | "title"
                        | "journal"
                        | "volume"
                        | "pages"
                        | "url"
                        | "doi"
                        | "isbn"
                        | "issue"
                        | "edition"
                        | "publisher"
                        | "address"
                )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    object.insert("id".into(), json!(source.tag));
    object.insert("type".into(), json!(if source.kind.is_empty() { "article-journal" } else { &source.kind }));
    object.insert("title".into(), json!(source.title));
    let imported_author_matches = source.metadata.get("_wordcraftFormat").and_then(Value::as_str) == Some("csl-json")
        && source.metadata.get("_wordcraftAuthorSnapshot").and_then(Value::as_str) == Some(source.author.as_str());
    let author = if imported_author_matches {
        source.metadata.get("author").cloned().unwrap_or_else(|| csl_authors(&source.author))
    } else {
        csl_authors(&source.author)
    };
    if !author.is_null() {
        object.insert("author".into(), author);
    }
    let preserve_issued = source.metadata.get("_wordcraftYearSnapshot").and_then(Value::as_str) == Some(source.year.as_str());
    if !preserve_issued {
        if source.year.is_empty() {
            object.remove("issued");
        } else if let Ok(year) = source.year.parse::<i32>() {
            object.insert("issued".into(), json!({"date-parts": [[year]]}));
        } else {
            object.insert("issued".into(), json!({"raw": source.year}));
        }
    }
    for (key, value) in [
        ("publisher", &source.publisher),
        ("publisher-place", &source.city),
        ("container-title", &source.journal),
        ("volume", &source.volume),
        ("issue", &source.issue),
        ("page", &source.pages),
        ("URL", &source.url),
        ("DOI", &source.doi),
        ("ISBN", &source.isbn),
        ("edition", &source.edition),
    ] {
        if !value.is_empty() {
            object.insert(key.into(), json!(value));
        }
    }
    Ok(Value::Object(object))
}

fn csl_author(value: &Value) -> Result<String, String> {
    let Some(items) = value.as_array() else {
        return if value.is_null() { Ok(String::new()) } else { Err("CSL `author` must be an array".into()) };
    };
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        let person = item.as_object().ok_or("CSL author entries must be objects")?;
        let family = person.get("family").and_then(Value::as_str).unwrap_or("");
        let given = person.get("given").and_then(Value::as_str).unwrap_or("");
        let literal = person.get("literal").and_then(Value::as_str).unwrap_or("");
        let name =
            if !family.is_empty() { if given.is_empty() { family.to_string() } else { format!("{family}, {given}") } } else { literal.to_string() };
        if !name.is_empty() {
            names.push(name);
        }
    }
    Ok(names.join("; "))
}

fn csl_authors(author: &str) -> Value {
    let names: Vec<_> = author
        .split(';')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            if let Some((family, given)) = name.split_once(',') {
                json!({"family": family.trim(), "given": given.trim()})
            } else {
                json!({"literal": name})
            }
        })
        .collect();
    if names.is_empty() { Value::Null } else { Value::Array(names) }
}

fn csl_year(issued: Option<&Value>) -> String {
    issued
        .and_then(|value| value.pointer("/date-parts/0/0").or_else(|| value.get("raw")))
        .map(|year| match year {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn text_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn check_size(size: usize) -> Result<(), String> {
    if size > MAX_BYTES { Err(format!("citation export exceeds {} MiB", MAX_BYTES / (1024 * 1024))) } else { Ok(()) }
}

fn check_count(count: usize) -> Result<(), String> {
    if count > MAX_SOURCES { Err(format!("source library exceeds {MAX_SOURCES} entries")) } else { Ok(()) }
}

fn stable_tag(fields: &BTreeMap<String, String>) -> String {
    stable_identity_tag(
        fields.get("author").map(String::as_str).unwrap_or(""),
        fields.get("title").map(String::as_str).unwrap_or(""),
        fields.get("year").map(String::as_str).unwrap_or(""),
    )
}

fn stable_identity_tag(author: &str, title: &str, year: &str) -> String {
    let identity = format!("{author}\n{title}\n{year}");
    format!("source_{:016x}", hash(identity.as_bytes()))
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

fn normalize_bib_kind(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "book" | "mvbook" => "book",
        "inbook" | "incollection" | "inproceedings" | "conference" => "article",
        "article" | "article-journal" => "article",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "misc" | "webpage" => "other",
        _ => "other",
    }
}

fn bib_kind(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "book" => "book",
        "article" => "article",
        "thesis" => "phdthesis",
        "report" => "techreport",
        _ => "misc",
    }
}

fn bib_authors(value: &str) -> String {
    value
        .split(" and ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            if name.contains(',') {
                name.to_string()
            } else if let Some((given, family)) = name.rsplit_once(' ') {
                format!("{}, {}", family.trim(), given.trim())
            } else {
                name.into()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn bib_key(value: &str) -> String {
    let key: String = value.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.')).take(128).collect();
    if key.is_empty() { "source".into() } else { key }
}

fn set_if_nonempty(fields: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if !value.is_empty() {
        fields.insert(key.into(), value.into());
    }
}

fn bib_entries(input: &str) -> Result<Vec<(String, String, BTreeMap<String, String>)>, String> {
    let bytes = input.as_bytes();
    let mut cursor = 0;
    let mut entries = Vec::new();
    while cursor < bytes.len() {
        let Some(relative) = bytes.get(cursor..).and_then(|rest| rest.iter().position(|byte| *byte == b'@')) else { break };
        cursor += relative + 1;
        let type_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
            cursor += 1;
        }
        let kind = input.get(type_start..cursor).ok_or("invalid BibTeX entry type")?.to_ascii_lowercase();
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let opener = *bytes.get(cursor).ok_or("BibTeX entry is missing an opening brace")?;
        if opener != b'{' && opener != b'(' {
            return Err("BibTeX entry needs `{` or `(`".into());
        }
        cursor += 1;
        let close = if opener == b'{' { b'}' } else { b')' };
        let start = cursor;
        let mut depth = 1usize;
        let mut quoted = false;
        let mut escaped = false;
        while let Some(byte) = bytes.get(cursor).copied() {
            if escaped {
                escaped = false;
                cursor += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                cursor += 1;
                continue;
            }
            if byte == b'"' {
                quoted = !quoted;
                cursor += 1;
                continue;
            }
            if !quoted {
                if byte == b'{' {
                    depth = depth.saturating_add(1);
                }
                if byte == close {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
            }
            cursor += 1;
        }
        let body = input.get(start..cursor).ok_or("malformed BibTeX entry")?;
        cursor = cursor.saturating_add(1);
        if kind == "comment" || kind == "preamble" {
            continue;
        }
        if kind == "string" {
            return Err("BibTeX `@string` macros are not supported; expand them in the export first".into());
        }
        let (key, fields) = parse_bib_fields(body)?;
        entries.push((kind, key, fields));
    }
    Ok(entries)
}

fn parse_bib_fields(body: &str) -> Result<(String, BTreeMap<String, String>), String> {
    let bytes = body.as_bytes();
    let mut cursor = 0;
    let key_start = 0;
    while bytes.get(cursor).is_some_and(|byte| *byte != b',') {
        cursor += 1;
    }
    let key = body.get(key_start..cursor).unwrap_or("").trim().to_string();
    if bytes.get(cursor) != Some(&b',') {
        return Ok((key, BTreeMap::new()));
    }
    cursor += 1;
    let mut fields = BTreeMap::new();
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',') {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let name_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-') {
            cursor += 1;
        }
        let name = body.get(name_start..cursor).unwrap_or("").trim().to_ascii_lowercase();
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if name.is_empty() || bytes.get(cursor) != Some(&b'=') {
            return Err("malformed BibTeX field (expected `name = value`)".into());
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let value_start = cursor;
        let mut brace_depth = 0usize;
        let mut quote = false;
        let mut escaped = false;
        while let Some(byte) = bytes.get(cursor).copied() {
            if escaped {
                escaped = false;
                cursor += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                cursor += 1;
                continue;
            }
            if byte == b'"' {
                quote = !quote;
                cursor += 1;
                continue;
            }
            if !quote {
                if byte == b'{' {
                    brace_depth = brace_depth.saturating_add(1);
                }
                if byte == b'}' && brace_depth > 0 {
                    brace_depth -= 1;
                }
                if byte == b',' && brace_depth == 0 {
                    break;
                }
                if byte == b'#' && brace_depth == 0 {
                    return Err("BibTeX value concatenation (`#`) is unsupported".into());
                }
            }
            cursor += 1;
        }
        let raw = body.get(value_start..cursor).ok_or("malformed BibTeX field value")?.trim();
        let value = unwrap_bib_value(raw)?;
        if fields.insert(name.clone(), value).is_some() {
            return Err(format!("duplicate BibTeX field `{name}`"));
        }
        if bytes.get(cursor) == Some(&b',') {
            cursor += 1;
        }
    }
    Ok((key, fields))
}

fn unwrap_bib_value(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}') {
        return Ok(raw.get(1..raw.len() - 1).unwrap_or("").to_string());
    }
    if bytes.len() >= 2 && bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"') {
        return Ok(raw.get(1..raw.len() - 1).unwrap_or("").replace("\\\"", "\""));
    }
    if raw.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err(format!("BibTeX bare macro `{raw}` is unsupported; use a braced or quoted value"));
    }
    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibtex_unicode_extra_fields_and_round_trip() {
        let input = "@article{Rivera24, author={Rivera, Alex and 王, 明}, title={文献 {OOXML}}, year={2024}, journal={Open Data}, doi={10.5555/demo}, keywords={DOCX, XML}, note={preserve me}}";
        let sources = import_bibtex(input).unwrap();
        assert_eq!(sources[0].title, "文献 {OOXML}");
        assert_eq!(sources[0].author, "Rivera, Alex; 王, 明");
        assert_eq!(sources[0].doi, "10.5555/demo");
        assert_eq!(sources[0].metadata.get("note").and_then(Value::as_str), Some("preserve me"));
        let exported = export_bibtex(&sources).unwrap();
        let reopened = import_bibtex(&exported).unwrap();
        assert_eq!(reopened[0].title, sources[0].title);
        assert_eq!(reopened[0].metadata.get("keywords"), sources[0].metadata.get("keywords"));
    }

    #[test]
    fn csl_json_preserves_unicode_unknown_fields_creators_and_dates() {
        let input = r#"[{"id":"smith-2020","type":"article-journal","title":"科学の記録","author":[{"family":"O'Neil","given":"A. B.","ORCID":"0000-0001"}],"issued":{"date-parts":[[2020,5,7]]},"container-title":"Journal","DOI":"10.1000/xyz","custom-field":{"x":1}}]"#;
        let sources = import_csl_json(input).unwrap();
        assert_eq!(sources[0].author, "O'Neil, A. B.");
        assert_eq!(sources[0].year, "2020");
        assert_eq!(sources[0].doi, "10.1000/xyz");
        let output = export_csl_json(&sources).unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value[0]["author"][0]["ORCID"], "0000-0001");
        assert_eq!(value[0]["issued"]["date-parts"][0][1], 5);
        assert_eq!(value[0]["custom-field"]["x"], 1);
        assert_eq!(import_csl_json(&output).unwrap(), sources);
    }

    #[test]
    fn malformed_ambiguous_and_oversized_imports_fail_explicitly() {
        assert!(import_bibtex("@article{x, title=macro}").is_err());
        assert!(import_bibtex("@article{x, title={a} # {b}}").is_err());
        assert!(import_bibtex("@article{x, title={a}, title={b}}").is_err());
        assert!(import_csl_json("{not json}").is_err());
        assert!(import_csl_json("[{},{}]").is_err());
        assert!(import_csl_json(&" ".repeat(MAX_BYTES + 1)).is_err());
    }
}
