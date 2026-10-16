//! Engine save, Undo and durable recovery must carry guarded package state together.
#[path = "../../docx/tests/common/preservation.rs"]
mod fixtures;
use fixtures::{archive, fixture, replace, unpack};
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
