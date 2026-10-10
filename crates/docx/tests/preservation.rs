//! Package-wide preservation and safe-editing contracts for unsupported OOXML.
#[path = "common/preservation.rs"]
mod fixtures;
use fixtures::*;
use std::sync::Arc;
use wordcraft_doc::{Document, Path, Pos, StoryRef};
fn insert(doc: &mut Document, index: usize, text: &str) {
    let props = doc.para(StoryRef::Body, &Path::top(index)).unwrap().props_of_char(0).clone();
    doc.insert_text(&Pos::body(index, 0), text, &props).unwrap();
}
#[test]
fn original_package_and_inert_embedded_content_survive_noop_exactly() {
    let files = fixture();
    let bytes = archive(&files);
    if let Some(directory) = std::env::var_os("WORDCRAFT_COMPAT_CORPUS_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("unsupported-package.docx"), &bytes).unwrap();
    }
    let doc = wordcraft_docx::read(&bytes).unwrap();
    assert!(!wordcraft_docx::compatibility_warnings(&doc).is_empty());
    assert_eq!(wordcraft_docx::write(&doc).unwrap(), bytes);
}
#[test]
fn text_edits_preserve_every_other_part_and_unsupported_story_markup() {
    let source = fixture();
    let mut doc = wordcraft_docx::read(&archive(&source)).unwrap();
    for generation in 0..3 {
        insert(&mut doc, 0, &format!("Edit {generation}: "));
        let output = wordcraft_docx::write(&doc).unwrap();
        let files = unpack(&output);
        assert_eq!(files.len(), source.len());
        for (path, original) in &source {
            if path != "word/document.xml" {
                assert_eq!(files.get(path), Some(original), "changed {path}");
            }
        }
        let original = String::from_utf8(source.get("word/document.xml").unwrap().clone()).unwrap();
        let saved = String::from_utf8(files.get("word/document.xml").unwrap().clone()).unwrap();
        let original_tail = original.split("<!--keep original comment-->").nth(1).unwrap();
        let saved_tail = saved.split("<!--keep original comment-->").nth(1).unwrap();
        assert_eq!(saved_tail, original_tail);
        assert!(saved.contains("future:extension=\"keep\""));
        assert!(
            saved.contains("w:dataBinding")
                && saved.contains("w:sdtPr")
                && saved.contains("dgm:relIds")
                && saved.contains("o:OLEObject")
                && saved.contains("w:control")
        );
        doc = wordcraft_docx::read(&output).unwrap();
        assert!(doc.plain_text(StoryRef::Body).contains(&format!("Edit {generation}: ")));
    }
}
#[test]
fn bound_control_edit_is_blocked_even_when_outside_text_is_identical() {
    let files = fixture();
    let mut doc = wordcraft_docx::read(&archive(&files)).unwrap();
    // Body 2 is an ordinary paragraph, body 3 is the bound SDT with the same text.
    assert_eq!(doc.para(StoryRef::Body, &Path::top(2)).unwrap().text, doc.para(StoryRef::Body, &Path::top(3)).unwrap().text);
    insert(&mut doc, 3, "Protected: ");
    let error = wordcraft_docx::write(&doc).unwrap_err().to_string();
    assert!(error.contains("protected block structure"), "{error}");
    let mut outside = wordcraft_docx::read(&archive(&files)).unwrap();
    insert(&mut outside, 2, "Outside: ");
    let saved = wordcraft_docx::write(&outside).unwrap();
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert_eq!(reopened.para(StoryRef::Body, &Path::top(3)).unwrap().text, "Shared text");
    assert!(reopened.para(StoryRef::Body, &Path::top(2)).unwrap().text.starts_with("Outside: "));
}
#[test]
fn formatting_structure_settings_and_object_edits_fail_safely() {
    let source = archive(&fixture());
    for action in ["format", "delete paragraph", "settings", "binding text", "object", "style", "numbering", "metadata"] {
        let mut doc = wordcraft_docx::read(&source).unwrap();
        match action {
            "format" => doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().runs.first_mut().unwrap().props.bold = Some(true),
            "delete paragraph" => {
                doc.body.remove(1);
            }
            "settings" => doc.settings.track_changes = true,
            "binding text" => insert(&mut doc, 3, "Changed control: "),
            "object" => doc.para_mut(StoryRef::Body, &Path::top(4)).unwrap().objects.clear(),
            "style" => doc.styles.styles.clear(),
            "numbering" => doc.numbering.nums.clear(),
            _ => doc.core.title = "Changed title".into(),
        }
        assert!(wordcraft_docx::write(&doc).is_err(), "unsafe save accepted: {action}");
    }
}
#[test]
fn malformed_missing_conflicting_and_escaping_dependencies_are_rejected() {
    for case in [
        "missing binary",
        "missing nested",
        "mismatched XML",
        "duplicate relationship",
        "undefined diagram ID",
        "duplicate type",
        "unbound prefix",
        "DTD",
        "unsafe path",
        "orphan rel owner",
    ] {
        let mut files = fixture();
        match case {
            "missing binary" => {
                files.remove("word/activeX/activeX1.bin");
            }
            "missing nested" => {
                files.remove("vendor/nested.bin");
            }
            "mismatched XML" => {
                files.insert("word/diagrams/layout1.xml".into(), b"<layout><broken></layout>".to_vec());
            }
            "duplicate relationship" => replace(
                &mut files,
                "vendor/_rels/data.bin.rels",
                "</Relationships>",
                r#"<Relationship Id="nested" Type="urn:test" Target="data.bin"/></Relationships>"#,
            ),
            "undefined diagram ID" => replace(&mut files, "word/document.xml", "r:dm=\"diagramData\"", "r:dm=\"missing\""),
            "duplicate type" => {
                replace(&mut files, "[Content_Types].xml", "</Types>", r#"<Default Extension="bin" ContentType="application/xml"/></Types>"#)
            }
            "unbound prefix" => {
                files.insert("vendor/future.xml".into(), b"<unknown:future/>".to_vec());
            }
            "DTD" => {
                files.insert(
                    "vendor/future.xml".into(),
                    b"<!DOCTYPE data [<!ENTITY secret SYSTEM 'file:///never-read'>]><data>&secret;</data>".to_vec(),
                );
            }
            "unsafe path" => replace(&mut files, "vendor/_rels/data.bin.rels", "Target=\"nested.bin\"", "Target=\"../../outside.bin\""),
            _ => {
                files.insert("vendor/_rels/missing.bin.rels".into(), format!(r#"<Relationships xmlns="{REL}"/>"#).into_bytes());
            }
        }
        assert!(wordcraft_docx::read(&archive(&files)).is_err(), "unsafe import accepted: {case}");
    }
}
#[test]
fn missing_or_corrupted_preservation_state_cannot_disable_save_guards() {
    let original = wordcraft_docx::read(&archive(&fixture())).unwrap();
    for key in ["wordcraft:packageSource.v1", "wordcraft:packageBaseline.v1", "wordcraft:packageModel.v1"] {
        let mut missing = original.clone();
        missing.passthrough.remove(key);
        assert!(wordcraft_docx::write(&missing).is_err(), "missing {key}");
        let mut corrupt = original.clone();
        corrupt.passthrough.insert(key.into(), Arc::new(b"corrupt state".to_vec()));
        assert!(wordcraft_docx::write(&corrupt).is_err(), "corrupt {key}");
    }
    let mut malformed = original.clone();
    let mut baseline = unpack(malformed.passthrough.get("wordcraft:packageBaseline.v1").unwrap());
    baseline.insert("word/document.xml".into(), b"<malformed".to_vec());
    malformed.passthrough.insert("wordcraft:packageBaseline.v1".into(), Arc::new(archive(&baseline)));
    assert!(wordcraft_docx::write(&malformed).is_err());
}
#[test]
fn signature_is_retained_and_mutation_is_refused() {
    let mut files = fixture();
    files.insert("_xmlsignatures/sig1.xml".into(), b"<syntheticSignature xmlns='urn:wordcraft:synthetic:signature'/>".to_vec());
    let source = archive(&files);
    let mut doc = wordcraft_docx::read(&source).unwrap();
    assert_eq!(wordcraft_docx::write(&doc).unwrap(), source);
    insert(&mut doc, 0, "Signature-breaking change: ");
    assert!(wordcraft_docx::write(&doc).unwrap_err().to_string().contains("digital signature"));
}
#[test]
fn flavor_conversion_preserves_content_types_extensions_and_macro_guard() {
    let files = fixture();
    let doc = wordcraft_docx::read(&archive(&files)).unwrap();
    for flavor in [wordcraft_docx::Flavor::Template, wordcraft_docx::Flavor::MacroDocument, wordcraft_docx::Flavor::MacroTemplate] {
        let output = wordcraft_docx::write_as(&doc, flavor).unwrap();
        let saved = unpack(&output);
        for (path, bytes) in &files {
            if path != "[Content_Types].xml" {
                assert_eq!(saved.get(path), Some(bytes));
            }
        }
        assert!(String::from_utf8(saved.get("[Content_Types].xml").unwrap().clone()).unwrap().contains("future:metadata=\"preserve\""));
        let back = wordcraft_docx::read(&output).unwrap();
        assert_eq!(wordcraft_docx::write_as(&back, flavor).unwrap(), output);
        if flavor.macros() {
            assert!(wordcraft_docx::write(&back).is_err());
        }
    }
}
#[test]
fn strict_namespace_and_alternate_main_part_location_remain_unchanged() {
    let mut files = fixture();
    replace(
        &mut files,
        "word/document.xml",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        "http://purl.oclc.org/ooxml/wordprocessingml/main",
    );
    let main = files.remove("word/document.xml").unwrap();
    let rels = files.remove("word/_rels/document.xml.rels").unwrap();
    files.insert("word/main.xml".into(), main);
    files.insert("word/_rels/main.xml.rels".into(), rels);
    replace(&mut files, "_rels/.rels", "word/document.xml", "word/main.xml");
    replace(&mut files, "[Content_Types].xml", "/word/document.xml", "/word/main.xml");
    let mut doc = wordcraft_docx::read(&archive(&files)).unwrap();
    insert(&mut doc, 0, "Strict edit: ");
    let saved = unpack(&wordcraft_docx::write(&doc).unwrap());
    assert!(String::from_utf8(saved.get("word/main.xml").unwrap().clone()).unwrap().contains("http://purl.oclc.org/ooxml/wordprocessingml/main"));
    assert!(!saved.contains_key("word/document.xml"));
    assert_eq!(saved.get("word/_rels/main.xml.rels"), files.get("word/_rels/main.xml.rels"));
    wordcraft_docx::read(&archive(&saved)).unwrap();
}
#[test]
fn utf16_source_is_preserved_and_text_edits_are_explicitly_rejected() {
    let mut files = fixture();
    let xml = String::from_utf8(files.get("word/document.xml").unwrap().clone()).unwrap().replace("UTF-8", "UTF-16");
    let mut encoded = vec![0xff, 0xfe];
    encoded.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    files.insert("word/document.xml".into(), encoded);
    let bytes = archive(&files);
    let mut doc = wordcraft_docx::read(&bytes).unwrap();
    assert_eq!(wordcraft_docx::write(&doc).unwrap(), bytes);
    insert(&mut doc, 0, "Edit: ");
    assert!(wordcraft_docx::write(&doc).unwrap_err().to_string().contains("UTF-16"));
}

#[test]
fn native_documents_keep_normal_formatting_and_structure_editing() {
    let source = wordcraft_docx::write(&Document::from_text("Native title\nNative body")).unwrap();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    assert!(!wordcraft_docx::has_preserved_package(&doc), "fully reproducible native packages should keep ordinary editing");
    doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().runs.first_mut().unwrap().props.bold = Some(true);
    doc.body.push(wordcraft_doc::para_block(wordcraft_doc::Paragraph::with_text("New paragraph", Default::default())));
    let saved = wordcraft_docx::write(&doc).unwrap();
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert_eq!(reopened.body.len(), 3);
    assert_eq!(reopened.para(StoryRef::Body, &Path::top(0)).unwrap().runs.first().unwrap().props.bold, Some(true));
}
#[test]
fn namespace_declarations_comments_and_each_unknown_property_force_preservation() {
    let native = unpack(&wordcraft_docx::write(&Document::from_text("Editable native paragraph")).unwrap());
    for (path, before, after) in [
        ("word/document.xml", "<w:document ", "<w:document xmlns:unused=\"urn:wordcraft:future-unused\" "),
        ("word/document.xml", "</w:body>", "<!--unknown retained comment--></w:body>"),
        ("word/document.xml", "</w:body>", "<?wordcraft retain='exact'?></w:body>"),
        ("word/styles.xml", "</w:styles>", "<future:styles xmlns:future='urn:unknown' keep='yes'/></w:styles>"),
        ("word/settings.xml", "</w:settings>", "<future:settings xmlns:future='urn:unknown' keep='yes'/></w:settings>"),
        ("docProps/core.xml", "</cp:coreProperties>", "<future:metadata xmlns:future='urn:unknown' keep='yes'/></cp:coreProperties>"),
    ] {
        let mut files = native.clone();
        replace(&mut files, path, before, after);
        let bytes = archive(&files);
        let mut doc = wordcraft_docx::read(&bytes).unwrap();
        assert!(wordcraft_docx::has_preserved_package(&doc), "{path}: {after}");
        insert(&mut doc, 0, "Edit: ");
        let saved = unpack(&wordcraft_docx::write(&doc).unwrap());
        for (name, data) in &files {
            if name != "word/document.xml" {
                assert_eq!(saved.get(name), Some(data), "{name}");
            }
        }
        if path == "word/document.xml" {
            assert!(String::from_utf8(saved.get(path).unwrap().clone()).unwrap().contains(after));
        }
    }
}
#[test]
fn case_insensitive_main_target_still_patches_the_correct_source_part() {
    let mut files = fixture();
    replace(&mut files, "_rels/.rels", "word/document.xml", "WORD/document.xml");
    let mut doc = wordcraft_docx::read(&archive(&files)).unwrap();
    insert(&mut doc, 0, "Case-safe edit: ");
    let bytes = wordcraft_docx::write(&doc).unwrap();
    let reopened = wordcraft_docx::read(&bytes).unwrap();
    assert!(reopened.plain_text(StoryRef::Body).starts_with("Case-safe edit: "));
    let saved = unpack(&bytes);
    assert_eq!(saved.get("_rels/.rels"), files.get("_rels/.rels"));
}
#[test]
fn package_part_count_and_declared_zip_size_are_bounded() {
    let mut files = fixture();
    for index in 0..513 {
        files.insert(format!("vendor/excess{index}.bin"), vec![1]);
    }
    assert!(wordcraft_docx::read(&archive(&files)).unwrap_err().to_string().contains("512 parts"));
    let mut bytes = archive(&fixture());
    let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    let oversized = (65u32 * 1024 * 1024).to_le_bytes();
    bytes.get_mut(central + 24..central + 28).unwrap().copy_from_slice(&oversized);
    let error = wordcraft_docx::read(&bytes).unwrap_err().to_string();
    assert!(error.contains("declares") && error.contains("bytes"), "{error}");
}
