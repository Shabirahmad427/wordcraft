//! Engine save, Undo and durable recovery must carry guarded package state together.
#[path = "../../docx/tests/common/preservation.rs"]
mod fixtures;
use fixtures::{archive, fixture, replace, table_fixture, unpack};
use serde_json::json;
use wordcraft_engine::{
    Session,
    doc::{Document, StoryRef},
    recovery::legacy::{Snapshot, Store, unique_id},
};
#[test]
fn guarded_text_save_undo_and_recovery_preserve_inert_parts_and_bindings() {
    let directory = std::env::temp_dir().join(format!("wordcraft-generic-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.docx");
    let output = directory.join("edited.docx");
    let bytes = archive(&fixture());
    std::fs::write(&source, &bytes).unwrap();
    let mut session = Session::new(Document::new());
    let opened = session.run("file.open", &json!({"path":source})).unwrap();
    assert!(!opened["compatibilityWarnings"].as_array().unwrap().is_empty());
    let original = session.doc.clone();
    session.run("text.insert", &json!({"text":"Recovered edit: "})).unwrap();
    let edited = session.doc.clone();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, original);
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(session.doc, edited);
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut recovered);
    assert_eq!(recovered.doc, edited);
    assert_eq!(*recovered.source_package.as_ref().unwrap().bytes, bytes);
    let metadata = recovered.doc.core.clone();
    recovered.run("file.save", &json!({"path":output})).unwrap();
    assert_eq!(recovered.doc.core, metadata);
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    let saved = std::fs::read(&output).unwrap();
    let a = unpack(&bytes);
    let b = unpack(&saved);
    for (path, data) in &a {
        if path != "word/document.xml" {
            assert_eq!(b.get(path), Some(data), "changed {path}");
        }
    }
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path":output})).unwrap();
    assert_eq!(reopened.doc.plain_text(StoryRef::Body), recovered.doc.plain_text(StoryRef::Body));
    let before = recovered.layout();
    let after = reopened.layout();
    assert_eq!(before.pages.len(), after.pages.len());
    for (a, b) in before.pages.iter().zip(&after.pages) {
        let options = wordcraft_render::RenderOptions::default();
        assert_eq!(
            wordcraft_render::render_page(&recovered.doc, a, 1.0, &options).to_straight(),
            wordcraft_render::render_page(&reopened.doc, b, 1.0, &options).to_straight()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn guarded_table_format_across_paragraphs_is_undoable_and_round_trips() {
    use wordcraft_engine::doc::{Path, Pos};

    let directory = std::env::temp_dir().join(format!("wordcraft-guarded-table-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.docx");
    let output = directory.join("edited.docx");
    let bytes = table_fixture();
    std::fs::write(&source, &bytes).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source})).unwrap();
    let table_index = session.doc.body.iter().position(|block| block.as_table().is_some()).unwrap();
    let first = Pos { story: StoryRef::Body, path: Path(vec![table_index as u32, 0, 0, 0]), off: 2 };
    let second = Pos { story: StoryRef::Body, path: Path(vec![table_index as u32, 0, 0, 1]), off: 6 };
    session.run("select.range", &json!({"anchor": first, "focus": second})).unwrap();
    let before = session.doc.clone();
    session.run("format.bold", &json!({})).unwrap();
    let formatted = session.doc.clone();
    assert_ne!(formatted, before, "formatting changed the selected table text");
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, before, "Undo restores the imported table formatting");
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(session.doc, formatted, "Redo restores the table formatting");
    session.run("file.save", &json!({"path": output})).unwrap();

    let saved = std::fs::read(&output).unwrap();
    for (path, data) in unpack(&bytes) {
        if path != "word/document.xml" {
            assert_eq!(unpack(&saved).get(&path), Some(&data), "changed package part {path}");
        }
    }
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": output})).unwrap();
    let table = reopened.doc.body.iter().find_map(|block| block.as_table()).unwrap();
    let first = table.rows[0].cells[0].blocks[0].as_para().unwrap();
    let second = table.rows[0].cells[0].blocks[1].as_para().unwrap();
    assert!((2..9).all(|offset| first.props_of_char(offset).bold == Some(true)));
    assert!((0..6).all(|offset| second.props_of_char(offset).bold == Some(true)));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn guarded_header_and_footnote_edits_undo_redo_and_save_without_changing_fields() {
    use wordcraft_engine::doc::{InlineObject, PartKind, Path, Pos, SectionProps, para_block};
    let directory = std::env::temp_dir().join(format!("wordcraft-guarded-stories-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.docx");
    let output = directory.join("edited.docx");
    let mut d = Document::from_text("Body text");
    let mut header_para = wordcraft_engine::doc::Paragraph::with_text("Header label ", Default::default());
    header_para
        .insert_object(
            13,
            InlineObject::Field { instr: "PAGE".into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() },
            &Default::default(),
        )
        .unwrap();
    let header = d.add_part(PartKind::Header, vec![para_block(header_para)]);
    let mut footer_para = wordcraft_engine::doc::Paragraph::with_text("Page ", Default::default());
    footer_para
        .insert_object(
            5,
            InlineObject::Field { instr: "PAGE".into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() },
            &Default::default(),
        )
        .unwrap();
    let footer = d.add_part(PartKind::Footer, vec![para_block(footer_para)]);
    let note = d.add_part(PartKind::Footnote, vec![para_block(wordcraft_engine::doc::Paragraph::with_text("Note text", Default::default()))]);
    d.insert_object(
        &Pos::body(0, "Body text".len()),
        InlineObject::NoteRef { kind: wordcraft_engine::doc::para::NoteKind::Footnote, id: note, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    let mut section = SectionProps::default();
    section.headers.default = Some(header);
    section.footers.default = Some(footer);
    d.last_section = section;
    let mut files = unpack(&wordcraft_docx::write(&d).unwrap());
    replace(&mut files, "word/document.xml", "<w:document ", "<w:document xmlns:future=\"urn:test\" future:keep=\"yes\" ");
    let bytes = archive(&files);
    std::fs::write(&source, &bytes).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source})).unwrap();
    let part = |kind| *session.doc.parts.iter().find(|(_, p)| p.kind == kind).unwrap().0;
    let header_id = part(PartKind::Header);
    let note_id = part(PartKind::Footnote);
    let before = session.doc.clone();
    let header_pos = Pos { story: StoryRef::Part(header_id), path: Path::top(0), off: 0 };
    session.run("caret.set", &json!({"pos": header_pos})).unwrap();
    session.run("text.insert", &json!({"text":"Updated "})).unwrap();
    let after_header = session.doc.clone();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, before);
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(session.doc, after_header);
    let note_len = session.doc.para(StoryRef::Part(note_id), &Path::top(0)).unwrap().len();
    let note_pos = Pos { story: StoryRef::Part(note_id), path: Path::top(0), off: note_len };
    session.run("caret.set", &json!({"pos": note_pos})).unwrap();
    session.run("text.insert", &json!({"text":"Edited "})).unwrap();
    let edited = session.doc.clone();
    session.run("edit.undo", &json!({})).unwrap();
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(session.doc, edited);
    session.run("file.save", &json!({"path": output})).unwrap();
    let saved = std::fs::read(&output).unwrap();
    let old = unpack(&bytes);
    let new = unpack(&saved);
    assert_ne!(old.get("word/header1.xml"), new.get("word/header1.xml"));
    assert_ne!(old.get("word/footnotes.xml"), new.get("word/footnotes.xml"));
    assert_eq!(old.get("word/footer1.xml"), new.get("word/footer1.xml"));
    let saved_header = std::str::from_utf8(new.get("word/header1.xml").unwrap()).unwrap();
    assert!(saved_header.contains("fldChar") && saved_header.contains("PAGE"), "page field markup was not retained: {saved_header}");
    assert_eq!(old.get("word/_rels/document.xml.rels"), new.get("word/_rels/document.xml.rels"));
    for (path, data) in &old {
        if !["word/header1.xml", "word/footnotes.xml"].contains(&path.as_str()) {
            assert_eq!(new.get(path), Some(data), "unrelated part changed: {path}");
        }
    }
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": output})).unwrap();
    assert!(
        reopened
            .doc
            .parts
            .values()
            .any(|p| p.kind == PartKind::Header && p.blocks.iter().any(|b| b.as_para().is_some_and(|p| p.text.starts_with("Updated Header label "))))
    );
    assert!(
        reopened
            .doc
            .parts
            .values()
            .any(|p| p.kind == PartKind::Footnote && p.blocks.iter().any(|b| b.as_para().is_some_and(|p| p.text.ends_with("Note textEdited "))))
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn guarded_no_edit_save_restores_imported_zoom_and_round_trips_unchanged() {
    let directory = std::env::temp_dir().join(format!("wordcraft-guarded-zoom-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let output = directory.join("saved.docx");
    let mut parts = fixture();
    replace(&mut parts, "word/settings.xml", "</w:settings>", "<w:zoom w:percent=\"173\"/></w:settings>");
    let source = archive(&parts);
    let doc = wordcraft_engine::io::open_bytes("source.docx", &source).unwrap();
    assert!(wordcraft_docx::has_preserved_package(&doc));
    assert_eq!(doc.settings.zoom, Some(wordcraft_engine::doc::Zoom { percent: 173, fit: None }));
    let expected_text = doc.plain_text(StoryRef::Body);

    // The CLI builds a headless session directly from the imported model. That session must
    // inherit the DOCX view zoom so a no-edit save doesn't turn it into a different protected edit.
    let mut session = Session::new(doc);
    assert_eq!(session.view.zoom, 1.73);
    session.run("file.save", &json!({"path":output})).unwrap();

    let saved = std::fs::read(&output).unwrap();
    assert_eq!(saved, source, "a no-edit guarded save keeps every original package byte");
    let reopened = wordcraft_engine::io::open_bytes("saved.docx", &saved).unwrap();
    assert_eq!(reopened.plain_text(StoryRef::Body), expected_text);
    assert_eq!(reopened.settings.zoom, Some(wordcraft_engine::doc::Zoom { percent: 173, fit: None }));
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn unsupported_edit_failed_save_retains_original_path_dirty_state_and_recovery() {
    let directory = std::env::temp_dir().join(format!("wordcraft-generic-failed-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("original.docx");
    let bytes = archive(&fixture());
    std::fs::write(&path, &bytes).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path":path})).unwrap();
    session.run("select.all", &json!({})).unwrap();
    session.run("format.bold", &json!({})).unwrap();
    let before = session.doc.clone();
    let previous_path = session.path.clone();
    assert!(session.run("file.save", &json!({})).is_err());
    assert_eq!(session.doc, before);
    assert_eq!(session.path, previous_path);
    assert!(session.dirty);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut restored = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut restored);
    assert_eq!(restored.doc, before);
    assert_eq!(*restored.source_package.as_ref().unwrap().bytes, bytes);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn malformed_import_does_not_replace_the_open_document() {
    let directory = std::env::temp_dir().join(format!("wordcraft-generic-import-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("broken.docx");
    let mut files = fixture();
    files.remove("vendor/nested.bin");
    std::fs::write(&path, archive(&files)).unwrap();
    let mut session = Session::new(Document::from_text("Keep this unsaved writing"));
    session.run("text.insert", &json!({"text":"Existing edit: "})).unwrap();
    let before = session.doc.clone();
    assert!(session.run("file.open", &json!({"path":path})).is_err());
    assert_eq!(session.doc, before);
    assert!(session.dirty);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn forbidden_xml_text_failed_save_keeps_document_and_destination() {
    let directory = std::env::temp_dir().join(format!("wordcraft-invalid-xml-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("writing.docx");
    let original_bytes = b"existing destination";
    std::fs::write(&path, original_bytes).unwrap();
    let mut session = Session::new(Document::from_text("Before\u{1}after"));
    let before = session.doc.clone();
    let previous_path = session.path.clone();
    assert!(session.run("file.save", &json!({"path":path})).is_err());
    assert_eq!(session.doc, before);
    assert_eq!(session.path, previous_path);
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
    std::fs::remove_dir_all(directory).unwrap();
}
