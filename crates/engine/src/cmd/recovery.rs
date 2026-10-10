//! Recovery commands are also callable through CLI/MCP, with an explicit directory.
#[cfg(not(target_arch = "wasm32"))]
use crate::p;
use crate::{CmdError, CmdResult, CommandSpec, Session};
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("recovery.restore", "Restore Recovery Version", "File › Info", restore)
            .params(r#"{"id": string, "directory"?: string}"#)
            .pure(),
        CommandSpec::new("recovery.discard", "Discard Recovery Version", "File › Info", discard)
            .params(r#"{"id": string, "directory"?: string}"#)
            .pure(),
        CommandSpec::new("recovery.compare", "Compare Recovery Versions", "File › Info", compare)
            .params(r#"{"id": string, "other"?: string (default: current document), "directory"?: string}"#)
            .pure(),
        CommandSpec::new("recovery.original", "Save Original Package", "File › Info", original)
            .params(r#"{"id": string, "path": string (must not exist), "directory"?: string}"#)
            .pure(),
    ]
}
#[cfg(not(target_arch = "wasm32"))]
fn store(s: &Session, v: &Value) -> Result<crate::recovery::Store, CmdError> {
    let dir = p::str(v, "directory")
        .map(std::path::PathBuf::from)
        .or_else(|| s.recovery_dir.clone())
        .ok_or_else(|| CmdError::Params("recovery is not configured; supply `directory`".into()))?;
    crate::recovery::Store::new(dir).map_err(CmdError::Failed)
}
pub fn list(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        serde_json::to_value(store(s, v)?.list().map_err(CmdError::Failed)?).map_err(|e| CmdError::Failed(e.to_string()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, v);
        unavailable()
    }
}
fn restore(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let snapshot = store(s, v)?.load(p::req_str(v, "id")?).map_err(CmdError::Failed)?;
        snapshot.restore(s);
        Ok(json!({"restored": true, "saveAsRequired": true}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, v);
        unavailable()
    }
}
fn discard(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        store(s, v)?.discard(p::req_str(v, "id")?).map_err(CmdError::Failed)?;
        Ok(json!({"discarded": true}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, v);
        unavailable()
    }
}
fn compare(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let store = store(s, v)?;
        let left = store.load(p::req_str(v, "id")?).map_err(CmdError::Failed)?.doc;
        let right = if let Some(id) = p::str(v, "other") { store.load(id).map_err(CmdError::Failed)?.doc } else { s.doc.clone() };
        let same_model = serde_json::to_value(&left).map_err(|e| CmdError::Failed(e.to_string()))?
            == serde_json::to_value(&right).map_err(|e| CmdError::Failed(e.to_string()))?;
        Ok(json!({"left": left.plain_text(wordcraft_doc::StoryRef::Body), "right": right.plain_text(wordcraft_doc::StoryRef::Body),
            "sameDocument": same_model && left.media == right.media && left.passthrough == right.passthrough,
            "comparison": "Body text preview; formatting and attachments are included in sameDocument, not a visual diff."}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, v);
        unavailable()
    }
}
fn original(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        store(s, v)?.export_original(p::req_str(v, "id")?, std::path::Path::new(p::req_str(v, "path")?)).map_err(CmdError::Failed)?;
        Ok(json!({"exported": true}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, v);
        unavailable()
    }
}
#[cfg(target_arch = "wasm32")]
fn unavailable() -> CmdResult {
    Err(CmdError::Failed("disk recovery is available in the desktop application".into()))
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::recovery::{Snapshot, Store, unique_id};
    use wordcraft_doc::{Document, StoryRef};
    fn setup() -> (Session, Store, String) {
        let directory = std::env::temp_dir().join(format!("wordcraft-recovery-commands-{}", unique_id()));
        let store = Store::new(directory.clone()).unwrap();
        let mut session = Session::new(Document::from_text("Recovered research"));
        session.recovery_dir = Some(directory);
        let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
        (session, store, id)
    }
    #[test]
    fn commands_list_compare_restore_and_discard_without_rewriting_original() {
        let (mut session, _store, id) = setup();
        session.run("text.insert", &json!({"text": "Current revision "})).unwrap();
        let before = serde_json::to_value(&session.doc).unwrap();
        let rev = session.rev();
        let undo = session.undo_labels();
        let listing = session.run("file.recover", &json!({})).unwrap();
        assert_eq!(listing[0]["id"], id);
        let compared = session.run("recovery.compare", &json!({"id": id})).unwrap();
        assert_eq!(compared["sameDocument"], false);
        assert_eq!(compared["left"], "Recovered research");
        assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
        assert_eq!(session.rev(), rev);
        assert_eq!(session.undo_labels(), undo);
        let same = session.run("recovery.compare", &json!({"id": id, "other": id})).unwrap();
        assert_eq!(same["sameDocument"], true);
        session.run("recovery.restore", &json!({"id": id})).unwrap();
        assert_eq!(session.doc.plain_text(StoryRef::Body), "Recovered research");
        assert!(session.path.is_none() && session.dirty);
        assert!(session.recovery_dir.is_some());
        session.run("recovery.discard", &json!({"id": id})).unwrap();
        assert!(session.run("file.recover", &json!({})).unwrap().as_array().unwrap().is_empty());
        std::fs::remove_dir_all(session.recovery_dir.unwrap()).unwrap();
    }
    #[test]
    fn failed_restore_and_compare_leave_document_and_history_unchanged() {
        let (mut session, _, _) = setup();
        session.run("text.insert", &json!({"text": "Keep me"})).unwrap();
        let before = serde_json::to_value(&session.doc).unwrap();
        let undo = session.undo_labels();
        let doc_id = session.document_id();
        for id in ["recovery.restore", "recovery.compare", "recovery.discard", "recovery.original"] {
            assert!(session.run(id, &json!({"id": "../../original", "path": "/no/output"})).is_err());
            assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
            assert_eq!(session.undo_labels(), undo);
            assert_eq!(session.document_id(), doc_id);
        }
        std::fs::remove_dir_all(session.recovery_dir.unwrap()).unwrap();
    }
}
