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

fn mixed_format_fixture() -> Vec<u8> {
    let mut files = fixture();
    replace(
        &mut files,
        "word/document.xml",
        "Editable introduction α &amp; β",
        "Editable </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>introduction</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t xml:space=\"preserve\"> α &amp; β",
    );
    archive(&files)
}

fn picture_fixture() -> Vec<u8> {
    let mut files = fixture();
    let picture = r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><wp:extent cx="1270000" cy="635000"/><wp:docPr id="18" name="Synthetic picture"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:blipFill><a:blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="pictureRel"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
    replace(&mut files, "word/document.xml", "<w:sectPr>", &format!("{picture}<w:sectPr>"));
    replace(
        &mut files,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!(r#"<Relationship Id="pictureRel" Type="{OFFICE}image" Target="media/picture.png"/></Relationships>"#),
    );
    replace(&mut files, "[Content_Types].xml", "</Types>", r#"<Default Extension="png" ContentType="image/png"/></Types>"#);
    files.insert("word/media/picture.png".into(), b"synthetic image bytes".to_vec());
    archive(&files)
}

fn source_with_explicit_normal_style() -> Vec<u8> {
    let mut files = fixture();
    let source = String::from_utf8(files.get("word/document.xml").unwrap().clone()).unwrap();
    let text = source.find("Editable introduction α &amp; β").unwrap();
    let paragraph = source.get(..text).unwrap().rfind("<w:p>").unwrap();
    let before_text = source.get(paragraph..text).unwrap();
    let updated = if let Some(ppr_end) = before_text.find("</w:pPr>") {
        let insertion = paragraph + ppr_end;
        format!("{}<w:pStyle w:val=\"Normal\"/>{}", &source[..insertion], &source[insertion..])
    } else {
        let run = before_text.find("<w:r>").unwrap() + paragraph;
        format!("{}<w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr>{}", &source[..run], &source[run..])
    };
    files.insert("word/document.xml".into(), updated.into_bytes());
    archive(&files)
}

fn assert_only_document_xml_changed(source: &[u8], saved: &[u8]) {
    let before = unpack(source);
    let after = unpack(saved);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (path, bytes) in before {
        if path != "word/document.xml" {
            assert_eq!(after.get(&path), Some(&bytes), "changed package part {path}");
        }
    }
}

fn story_fixture() -> Vec<u8> {
    use wordcraft_doc::{PartKind, SectionProps, para_block};
    let mut d = Document::from_text("Body text");
    let header = d.add_part(PartKind::Header, vec![para_block(wordcraft_doc::Paragraph::with_text("Header label", Default::default()))]);
    let footer = d.add_part(PartKind::Footer, vec![para_block(wordcraft_doc::Paragraph::with_text("Page ", Default::default()))]);
    let note = d.add_part(PartKind::Footnote, vec![para_block(wordcraft_doc::Paragraph::with_text("Note text", Default::default()))]);
    d.insert_object(
        &Pos::body(0, "Body text".len()),
        wordcraft_doc::InlineObject::NoteRef { kind: wordcraft_doc::para::NoteKind::Footnote, id: note, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    let mut section = SectionProps::default();
    section.headers.default = Some(header);
    section.footers.default = Some(footer);
    d.last_section = section;
    let native = wordcraft_docx::write(&d).unwrap();
    let mut files = unpack(&native);
    replace(&mut files, "word/document.xml", "<w:document ", "<w:document xmlns:future=\"urn:test\" future:keep=\"yes\" ");
    let _ = header;
    archive(&files)
}

fn insert_part(doc: &mut Document, id: u32, text: &str) {
    let story = StoryRef::Part(id);
    let para = doc.para(story, &Path::top(0)).unwrap();
    let off = if para.text.starts_with('\u{fffc}') { '\u{fffc}'.len_utf8() } else { 0 };
    let props = para.props_of_char(off).clone();
    doc.insert_text(&Pos { story, path: Path::top(0), off }, text, &props).unwrap();
}

#[test]
fn guarded_header_and_footnote_text_edits_preserve_source_parts_and_reopen() {
    use wordcraft_doc::PartKind;
    let source = story_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let header = *doc.parts.iter().find(|(_, p)| p.kind == PartKind::Header).unwrap().0;
    let note = *doc.parts.iter().find(|(_, p)| p.kind == PartKind::Footnote).unwrap().0;
    insert_part(&mut doc, header, "Updated ");
    insert_part(&mut doc, note, "Edited ");
    let saved = wordcraft_docx::write(&doc).unwrap();
    let before = unpack(&source);
    let after = unpack(&saved);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    assert_ne!(before.get("word/header1.xml"), after.get("word/header1.xml"));
    assert_ne!(before.get("word/footnotes.xml"), after.get("word/footnotes.xml"));
    for (path, bytes) in before {
        if path != "word/header1.xml" && path != "word/footnotes.xml" {
            assert_eq!(after.get(&path), Some(&bytes), "unrelated part changed: {path}");
        }
    }
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert!(
        reopened
            .parts
            .values()
            .any(|p| p.kind == PartKind::Header && p.blocks.iter().any(|b| b.as_para().is_some_and(|p| p.text == "Updated Header label")))
    );
    assert!(
        reopened
            .parts
            .values()
            .any(|p| p.kind == PartKind::Footnote && p.blocks.iter().any(|b| b.as_para().is_some_and(|p| p.text.ends_with("Edited Note text"))))
    );
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
fn text_edit_inside_one_mixed_format_run_preserves_neighbor_runs() {
    let source = mixed_format_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let offset = "Edit".len();
    let props = doc.para(StoryRef::Body, &Path::top(0)).unwrap().props_at(offset).clone();
    doc.insert_text(&Pos::body(0, offset), "safe ", &props).unwrap();

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let para = reopened.para(StoryRef::Body, &Path::top(0)).unwrap();
    assert_eq!(para.text, "Editsafe able introduction α & β");
    assert_eq!(para.runs.get(1).map(|run| run.props.bold), Some(Some(true)));
    assert!(para.runs.iter().any(|run| run.props.italic == Some(true)));
}

#[test]
fn formatting_a_supported_run_preserves_its_source_markup_and_other_parts() {
    let source = archive(&fixture());
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let run = doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().runs.first_mut().unwrap();
    run.props.bold = Some(true);

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert_eq!(reopened.para(StoryRef::Body, &Path::top(0)).unwrap().props_of_char(0).bold, Some(true));
    let package = unpack(&saved);
    let xml = String::from_utf8(package.get("word/document.xml").unwrap().clone()).unwrap();
    assert!(xml.contains("<w:b/>"));
    assert!(xml.contains("future:extension=\"keep\""));
}

#[test]
fn adding_italic_to_one_mixed_format_run_keeps_existing_bold_and_neighbor_runs() {
    let source = mixed_format_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let run = doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().runs.get_mut(1).unwrap();
    run.props.italic = Some(true);

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let para = reopened.para(StoryRef::Body, &Path::top(0)).unwrap();
    assert_eq!(para.runs.get(1).map(|run| run.props.bold), Some(Some(true)));
    assert_eq!(para.runs.get(1).map(|run| run.props.italic), Some(Some(true)));
    let xml = String::from_utf8(unpack(&saved).get("word/document.xml").unwrap().clone()).unwrap();
    assert!(xml.contains("<w:b/>") && xml.contains("<w:i/>"), "mixed run formatting was not preserved: {xml}");
}

#[test]
fn formatting_a_selection_across_source_runs_splits_only_the_selected_text() {
    let source = mixed_format_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let paragraph = doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap();
    let from = 6;
    let to = 25;
    paragraph
        .format(from, to, &|props| {
            props.bold = Some(true);
            props.bold_cs = Some(true);
        })
        .unwrap();

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let paragraph = reopened.para(StoryRef::Body, &Path::top(0)).unwrap();
    assert_eq!(paragraph.text, "Editable introduction α & β");
    assert_ne!(paragraph.props_of_char(from.saturating_sub(1)).bold, Some(true));
    for offset in from..to {
        assert_eq!(paragraph.props_of_char(offset).bold, Some(true), "bold mismatch at character {offset}");
    }
    assert_ne!(paragraph.props_of_char(to).bold, Some(true));
}

#[test]
fn formatting_an_existing_mixed_run_boundary_retains_original_italic_and_bold() {
    let source = mixed_format_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let paragraph = doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap();
    paragraph.format(9, 21, &|props| props.italic = Some(true)).unwrap();

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let paragraph = reopened.para(StoryRef::Body, &Path::top(0)).unwrap();
    for offset in 9..21 {
        assert_eq!(paragraph.props_of_char(offset).bold, Some(true));
        assert_eq!(paragraph.props_of_char(offset).italic, Some(true));
    }
    assert_eq!(paragraph.props_of_char(8).italic, None);
    assert_eq!(paragraph.props_of_char(21).italic, Some(true), "the source's italic run remains intact");
}

#[test]
fn formatting_a_whole_paragraph_preserves_paragraph_mark_formatting() {
    let source = archive(&fixture());
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let paragraph = doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap();
    paragraph.mark.bold = Some(true);
    paragraph.mark.bold_cs = Some(true);

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let paragraph = reopened.para(StoryRef::Body, &Path::top(0)).unwrap();
    assert_eq!(paragraph.mark.bold, Some(true));
    assert_eq!(paragraph.mark.bold_cs, Some(true));
}

#[test]
fn changing_to_an_existing_paragraph_style_preserves_unknown_package_parts() {
    let source = source_with_explicit_normal_style();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    assert!(doc.styles.styles.iter().any(|style| style.id == "Heading1"));
    doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().props.style = Some("Heading1".into());

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert_eq!(reopened.para(StoryRef::Body, &Path::top(0)).unwrap().props.style.as_deref(), Some("Heading1"));
}

#[test]
fn guarded_picture_resize_updates_frame_and_picture_extents_only() {
    let source = picture_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    let mut found = false;
    for block in &mut doc.body {
        let wordcraft_doc::Block::Para(paragraph) = Arc::make_mut(block) else { continue };
        for object in &mut paragraph.objects {
            if let wordcraft_doc::InlineObject::Image { w, h, .. } = object {
                assert_eq!((*w, *h), (100.0, 50.0));
                *w = 144.0;
                *h = 72.0;
                found = true;
            }
        }
    }
    assert!(found, "fixture picture was not imported");

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let files = unpack(&saved);
    let xml = String::from_utf8(files.get("word/document.xml").unwrap().clone()).unwrap();
    assert!(xml.contains("<wp:extent cx=\"1828800\" cy=\"914400\"/>"), "{xml}");
    assert!(xml.contains("<a:ext cx=\"1828800\" cy=\"914400\"/>"), "{xml}");
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let dimensions = reopened
        .body
        .iter()
        .filter_map(|block| block.as_para())
        .flat_map(|paragraph| &paragraph.objects)
        .find_map(|object| if let wordcraft_doc::InlineObject::Image { w, h, .. } = object { Some((*w, *h)) } else { None });
    assert_eq!(dimensions, Some((144.0, 72.0)));
}

#[test]
fn guarded_picture_resize_rejects_mismatched_picture_shape_extent() {
    let mut files = unpack(&picture_fixture());
    replace(&mut files, "word/document.xml", "<a:ext cx=\"1270000\" cy=\"635000\"/>", "<a:ext cx=\"1270000\" cy=\"635001\"/>");
    let source = archive(&files);
    let mut doc = wordcraft_docx::read(&source).unwrap();
    for block in &mut doc.body {
        let wordcraft_doc::Block::Para(paragraph) = Arc::make_mut(block) else { continue };
        if let Some(wordcraft_doc::InlineObject::Image { w, h, .. }) = paragraph.objects.first_mut() {
            *w = 144.0;
            *h = 72.0;
            break;
        }
    }
    assert!(wordcraft_docx::write(&doc).is_err(), "mismatched picture geometry must stay guarded");
}

#[test]
fn guarded_table_cell_format_and_style_preserve_package_and_reopen() {
    let source = table_fixture();
    let mut doc = wordcraft_docx::read(&source).unwrap();
    assert!(doc.styles.styles.iter().any(|style| style.id == "Heading1"));
    let table_index = doc.body.iter().position(|block| block.as_table().is_some()).expect("synthetic table imported");
    let wordcraft_doc::Block::Table(table) = Arc::make_mut(&mut doc.body[table_index]) else { panic!("table index changed") };
    let wordcraft_doc::Block::Para(paragraph) = Arc::make_mut(&mut table.rows[0].cells[0].blocks[0]) else {
        panic!("first cell block is a paragraph")
    };
    paragraph.format(2, 9, &|props| props.bold = Some(true)).unwrap();
    let wordcraft_doc::Block::Para(second) = Arc::make_mut(&mut table.rows[0].cells[0].blocks[1]) else { panic!("second cell block is a paragraph") };
    second.props.style = Some("Heading1".into());

    let saved = wordcraft_docx::write(&doc).unwrap();
    assert_only_document_xml_changed(&source, &saved);
    let reopened = wordcraft_docx::read(&saved).unwrap();
    let table = reopened.body.iter().find_map(|block| block.as_table()).expect("saved table reopened");
    let first = table.rows[0].cells[0].blocks[0].as_para().unwrap();
    assert_eq!(first.text, "Cell alpha");
    for offset in 2..9 {
        assert_eq!(first.props_of_char(offset).bold, Some(true), "bold mismatch at cell offset {offset}");
    }
    assert_eq!(first.props_of_char(1).bold, None);
    assert_eq!(first.props_of_char(9).bold, None);
    let second = table.rows[0].cells[0].blocks[1].as_para().unwrap();
    assert_eq!(second.props.style.as_deref(), Some("Heading1"));
}

#[test]
fn guarded_table_structure_edits_remain_rejected() {
    let mut doc = wordcraft_docx::read(&table_fixture()).unwrap();
    let table_index = doc.body.iter().position(|block| block.as_table().is_some()).expect("synthetic table imported");
    let wordcraft_doc::Block::Table(table) = Arc::make_mut(&mut doc.body[table_index]) else { panic!("table index changed") };
    table.rows[0].cells[0].blocks.pop();
    assert!(wordcraft_docx::write(&doc).is_err(), "changing a table's paragraph structure must stay guarded");
}
#[test]
fn bound_control_edit_is_blocked_even_when_outside_text_is_identical() {
    let files = fixture();
    let mut doc = wordcraft_docx::read(&archive(&files)).unwrap();
    // Body 2 is an ordinary paragraph, body 3 is the bound SDT with the same text.
    assert_eq!(doc.para(StoryRef::Body, &Path::top(2)).unwrap().text, doc.para(StoryRef::Body, &Path::top(3)).unwrap().text.replace('\u{fffc}', ""));
    insert(&mut doc, 3, "Protected: ");
    let error = wordcraft_docx::write(&doc).unwrap_err().to_string();
    assert!(error.contains("protected block structure"), "{error}");
    let mut outside = wordcraft_docx::read(&archive(&files)).unwrap();
    insert(&mut outside, 2, "Outside: ");
    let saved = wordcraft_docx::write(&outside).unwrap();
    let reopened = wordcraft_docx::read(&saved).unwrap();
    assert_eq!(reopened.para(StoryRef::Body, &Path::top(3)).unwrap().text, "\u{fffc}Shared text\u{fffc}");
    assert!(reopened.para(StoryRef::Body, &Path::top(2)).unwrap().text.starts_with("Outside: "));
}
#[test]
fn unsupported_formatting_structure_settings_and_object_edits_fail_safely() {
    let source = archive(&fixture());
    for action in ["unsupported format", "delete paragraph", "settings", "binding text", "object", "style", "numbering", "metadata"] {
        let mut doc = wordcraft_docx::read(&source).unwrap();
        match action {
            "unsupported format" => {
                doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().runs.first_mut().unwrap().props.underline =
                    Some(wordcraft_doc::props::Underline::Single)
            }
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
fn unknown_style_markup_survives_safe_text_edits_and_rejects_style_rewrites() {
    let source = archive(&fixture());
    let mut doc = wordcraft_docx::read(&source).unwrap();
    insert(&mut doc, 0, "Safe text edit: ");
    let saved = wordcraft_docx::write(&doc).unwrap();
    let saved_files = unpack(&saved);
    let original_files = unpack(&source);
    assert_eq!(saved_files.get("word/styles.xml"), original_files.get("word/styles.xml"));
    assert!(String::from_utf8(saved_files.get("word/styles.xml").unwrap().clone()).unwrap().contains("future:unknownStyleProperty"));

    let mut edited = wordcraft_docx::read(&source).unwrap();
    edited.styles.get_mut("Normal").unwrap().chr.bold = Some(true);
    let error = wordcraft_docx::write(&edited).unwrap_err().to_string();
    assert!(error.contains("preserv") || error.contains("unsupported"), "{error}");
    assert_eq!(unpack(&source).get("word/styles.xml"), original_files.get("word/styles.xml"));
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
