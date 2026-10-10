//! Desktop recovery scheduling and the command-driven recovery dialog.
use crate::WordApp;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Serialize)]
pub struct DialogState {
    pub entries: Vec<Value>,
    pub selected: String,
    pub other: String,
    pub error: String,
    pub left: String,
    pub right: String,
    pub compared: bool,
    pub discard_pending: bool,
}
impl DialogState {
    pub fn refresh(&mut self, app: &mut WordApp) {
        match app.execute("file.recover", json!({})) {
            Ok(Value::Array(entries)) => {
                self.entries = entries;
                self.error.clear();
            }
            Ok(_) => self.error = "Unexpected recovery listing".into(),
            Err(error) => self.error = error,
        }
    }
}

pub fn show(app: &mut WordApp, ui: &mut egui::Ui, state: &mut DialogState) -> bool {
    ui.set_min_width(640.0);
    ui.label("Recovery versions are kept separately from your saved files.");
    ui.label("Restore opens a working copy. Save Original Package preserves unsupported DOCX content.");
    if ui.button(crate::tl!("Refresh")).clicked() {
        state.refresh(app);
    }
    egui::ScrollArea::vertical().max_height(210.0).show(ui, |ui| {
        if state.entries.is_empty() {
            ui.label("No recovery versions available.");
        }
        for entry in &state.entries {
            let id = entry["id"].as_str().unwrap_or("");
            let title = entry["title"].as_str().unwrap_or("Recovery version");
            let time = entry["timestamp"].as_str().unwrap_or("");
            if ui.selectable_label(state.selected == id, format!("{title}  ·  {time}")).clicked() {
                state.selected = id.into();
                state.discard_pending = false;
            }
            if let Some(path) = entry["original_path"].as_str() {
                ui.small(path);
            }
            if entry["active"].as_bool() == Some(true) {
                ui.small("Active session — discard unavailable");
            }
            if let Some(error) = entry["error"].as_str() {
                ui.colored_label(egui::Color32::DARK_RED, error);
            }
            ui.separator();
        }
    });
    let entry = state.entries.iter().find(|v| v["id"].as_str() == Some(&state.selected));
    let readable = entry.is_some_and(|v| v["error"].is_null());
    let discardable = entry.is_some_and(|v| v["active"].as_bool() == Some(false));
    let original = readable && entry.is_some_and(|v| v["has_original"].as_bool() == Some(true));
    let original_extension = entry
        .and_then(|v| v["original_name"].as_str())
        .and_then(|name| std::path::Path::new(name).extension())
        .and_then(|s| s.to_str())
        .unwrap_or("docx")
        .to_string();
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(readable, egui::Button::new(crate::tl!("Restore Recovery Version"))).clicked() {
            match app.run("recovery.restore", json!({"id": state.selected})) {
                Ok(_) => close = true,
                Err(e) => state.error = e,
            }
        }
        if ui.add_enabled(discardable, egui::Button::new(crate::tl!("Discard Recovery Version"))).clicked() {
            state.discard_pending = true;
        }
        if ui.add_enabled(original, egui::Button::new(crate::tl!("Save Original Package"))).clicked() {
            let _ = app.ask_file(
                crate::FileDialogRequest::Save { name: format!("original-recovered.{original_extension}") },
                crate::file_dialogs::AfterPick::RecoveryOriginal { id: state.selected.clone() },
            );
        }
    });
    if state.discard_pending {
        ui.label("Permanently discard this recovery version? Your saved files are unaffected.");
        ui.horizontal(|ui| {
            if ui.button("Discard this version").clicked() {
                match app.execute("recovery.discard", json!({"id": state.selected})) {
                    Ok(_) => {
                        state.discard_pending = false;
                        state.selected.clear();
                        state.refresh(app);
                    }
                    Err(e) => state.error = e,
                }
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                state.discard_pending = false;
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label("Compare with:");
        egui::ComboBox::from_id_salt("recovery-other")
            .selected_text(if state.other.is_empty() { "Current document" } else { "Selected recovery version" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.other, String::new(), "Current document");
                for e in &state.entries {
                    if e["error"].is_null() {
                        ui.selectable_value(
                            &mut state.other,
                            e["id"].as_str().unwrap_or("").into(),
                            format!("{} · {}", e["title"].as_str().unwrap_or(""), e["timestamp"].as_str().unwrap_or("")),
                        );
                    }
                }
            });
        if ui.add_enabled(readable, egui::Button::new(crate::tl!("Compare Recovery Versions"))).clicked() {
            let mut params = json!({"id": state.selected});
            if !state.other.is_empty() {
                params["other"] = json!(state.other);
            }
            match app.execute("recovery.compare", params) {
                Ok(value) => {
                    state.left = value["left"].as_str().unwrap_or("").chars().take(8000).collect();
                    state.right = value["right"].as_str().unwrap_or("").chars().take(8000).collect();
                    state.compared = true;
                    state.error.clear();
                }
                Err(e) => state.error = e,
            }
        }
    });
    if state.compared {
        ui.small("Body text preview (first 8,000 characters). Formatting, notes and objects need visual review.");
        ui.columns(2, |cols| {
            for (col, text) in cols.iter_mut().zip([&state.left, &state.right]) {
                egui::ScrollArea::vertical().max_height(150.0).show(col, |ui| {
                    let mut view = text.as_str();
                    ui.add(egui::TextEdit::multiline(&mut view).desired_width(f32::INFINITY).desired_rows(6));
                });
            }
        });
    }
    if !state.error.is_empty() {
        ui.colored_label(egui::Color32::DARK_RED, &state.error);
    }
    if ui.button(crate::tl!("Close")).clicked() {
        close = true;
    }
    close
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{
        fs::File,
        sync::mpsc,
        thread::JoinHandle,
        time::{Duration, Instant},
    };
    use wordcraft_engine::recovery::{Snapshot, Store, unique_id};

    struct Job {
        snapshot: Snapshot,
        document: u64,
        revision: u64,
        ctx: Option<egui::Context>,
    }
    pub(crate) struct Controller {
        store: Store,
        writer: String,
        lease: Option<File>,
        sender: Option<mpsc::SyncSender<Job>>,
        results: mpsc::Receiver<Result<(u64, u64), String>>,
        worker: Option<JoinHandle<()>>,
        busy: bool,
        last_started: Option<Instant>,
        last_saved: Option<(u64, u64)>,
    }
    impl Controller {
        fn new(store: Store) -> Result<Self, String> {
            let writer = unique_id();
            let lease = store.lease(&writer)?;
            let (sender, receiver) = mpsc::sync_channel::<Job>(1);
            let (results_tx, results) = mpsc::channel();
            let worker_store = store.clone();
            let worker_writer = writer.clone();
            let worker = std::thread::Builder::new()
                .name("wordcraft-recovery".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        let result = worker_store
                            .save(&worker_writer, job.document, &job.snapshot)
                            .and_then(|_| worker_store.prune_own(&worker_writer, job.document, 3))
                            .map(|_| (job.document, job.revision));
                        let _ = results_tx.send(result);
                        if let Some(ctx) = job.ctx {
                            ctx.request_repaint();
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(Self {
                store,
                writer,
                lease: Some(lease),
                sender: Some(sender),
                results,
                worker: Some(worker),
                busy: false,
                last_started: None,
                last_saved: None,
            })
        }
        fn checkpoint(&mut self, session: &wordcraft_engine::Session) -> Result<(), String> {
            // Wait for the one in-flight write before a document boundary. Retention and
            // capture order must remain serial even when a user opens/closes during a write.
            if self.busy {
                self.busy = false;
                if let Ok(Ok(key)) = self.results.recv() {
                    self.last_saved = Some(key);
                }
            }
            let key = (session.document_id(), session.rev());
            if !session.dirty || self.last_saved == Some(key) {
                return Ok(());
            }
            self.store.save(&self.writer, key.0, &Snapshot::capture(session))?;
            self.store.prune_own(&self.writer, key.0, 3)?;
            self.last_saved = Some(key);
            Ok(())
        }
        fn stop(&mut self) {
            self.sender.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    impl Drop for Controller {
        fn drop(&mut self) {
            self.stop();
            self.lease.take();
            let _ = self.store.release_lease(&self.writer);
        }
    }
    impl WordApp {
        pub fn enable_recovery(&mut self, directory: std::path::PathBuf) -> Result<(), String> {
            if self.recovery.is_some() {
                return Err("recovery is already configured".into());
            }
            let store = Store::new(directory.clone())?;
            let entries = store.list()?;
            self.recovery = Some(Controller::new(store)?);
            self.session.recovery_dir = Some(directory);
            if !entries.is_empty() {
                let mut state = DialogState::default();
                state.refresh(self);
                self.dialog = Some(crate::dialogs::Dialog::Recovery { state });
            }
            Ok(())
        }
        pub(crate) fn recovery_checkpoint(&mut self) -> Result<(), String> {
            if let Some(controller) = &mut self.recovery {
                controller.checkpoint(&self.session)?;
            }
            Ok(())
        }
        pub fn finish_recovery(&mut self) {
            if let Some(controller) = &mut self.recovery {
                controller.stop();
            }
            if let Err(e) = self.recovery_checkpoint() {
                log::error!("Final recovery snapshot failed: {e}");
            }
            self.recovery.take();
        }
        pub(crate) fn recovery_tick(&mut self, ctx: &egui::Context) {
            let Some(controller) = &mut self.recovery else { return };
            let mut error = None;
            loop {
                match controller.results.try_recv() {
                    Ok(result) => {
                        controller.busy = false;
                        match result {
                            Ok(key) => controller.last_saved = Some(key),
                            Err(e) => error = Some(e),
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        controller.busy = false;
                        error = Some("Recovery worker stopped unexpectedly".into());
                        break;
                    }
                }
            }
            let key = (self.session.document_id(), self.session.rev());
            let due = controller.last_started.is_none_or(|t| t.elapsed() >= Duration::from_secs(15));
            if self.session.dirty && controller.last_saved != Some(key) && !controller.busy && due {
                let job = Job { snapshot: Snapshot::capture(&self.session), document: key.0, revision: key.1, ctx: Some(ctx.clone()) };
                controller.last_started = Some(Instant::now());
                match controller
                    .sender
                    .as_ref()
                    .ok_or("Recovery worker is stopped")
                    .and_then(|tx| tx.try_send(job).map_err(|_| "Recovery worker unavailable"))
                {
                    Ok(()) => controller.busy = true,
                    Err(e) => error = Some(e.into()),
                }
            }
            if self.session.dirty || controller.busy {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
            if let Some(e) = error {
                log::error!("Recovery snapshot failed: {e}");
                self.status(format!("Recovery snapshot failed: {e}. Save your document."));
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use wordcraft_doc::{Document, StoryRef};
        fn app() -> WordApp {
            WordApp::new(wordcraft_engine::Session::new(Document::new()), Default::default())
        }
        fn directory() -> std::path::PathBuf {
            std::env::temp_dir().join(format!("wordcraft-recovery-ui-{}", unique_id()))
        }
        fn settle(app: &mut WordApp) {
            let c = app.recovery.as_mut().unwrap();
            let key = c.results.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            c.last_saved = Some(key);
            c.busy = false;
        }
        #[test]
        fn periodic_snapshots_cover_unsaved_edits_without_overwriting_opened_file() {
            let directory = directory();
            std::fs::create_dir_all(&directory).unwrap();
            let original_path = directory.join("original.docx");
            let original = wordcraft_engine::io::save_bytes("original.docx", &Document::from_text("Original file")).unwrap();
            std::fs::write(&original_path, &original).unwrap();
            let mut a = app();
            a.enable_recovery(directory.join("recovery")).unwrap();
            a.execute("file.open", json!({"path": original_path})).unwrap();
            a.execute("text.insert", json!({"text": "First modification "})).unwrap();
            a.recovery_tick(&egui::Context::default());
            settle(&mut a);
            a.execute("text.insert", json!({"text": "Second modification "})).unwrap();
            a.recovery_tick(&egui::Context::default());
            assert!(!a.recovery.as_ref().unwrap().busy, "interval has not elapsed");
            a.recovery.as_mut().unwrap().last_started = Instant::now().checked_sub(Duration::from_secs(16));
            a.recovery_tick(&egui::Context::default());
            settle(&mut a);
            assert_eq!(std::fs::read(&original_path).unwrap(), original);
            let store = Store::new(directory.join("recovery")).unwrap();
            assert_eq!(store.list().unwrap().len(), 2);
            assert!(store.list().unwrap().iter().all(|e| e.active));
            a.finish_recovery();
            let mut restarted = app();
            restarted.enable_recovery(directory.join("recovery")).unwrap();
            assert!(matches!(restarted.dialog, Some(crate::dialogs::Dialog::Recovery { .. })));
            let row = store.list().unwrap().into_iter().find(|e| !e.active).unwrap();
            restarted.execute("recovery.restore", json!({"id": row.id})).unwrap();
            assert!(restarted.session.dirty && restarted.session.path.is_none());
            assert!(!restarted.autosaves());
            assert!(restarted.session.doc.plain_text(StoryRef::Body).contains("Second modification"));
            restarted.finish_recovery();
            std::fs::remove_dir_all(directory).unwrap();
        }
        #[test]
        fn document_switch_and_exit_checkpoint_latest_unsaved_edits() {
            let directory = directory();
            let mut a = app();
            a.enable_recovery(directory.clone()).unwrap();
            a.execute("text.insert", json!({"text": "First never-saved document"})).unwrap();
            a.execute("file.new", json!({})).unwrap();
            a.execute("text.insert", json!({"text": "Second never-saved document"})).unwrap();
            a.finish_recovery();
            let store = Store::new(directory.clone()).unwrap();
            let rows = store.list().unwrap();
            assert_eq!(rows.len(), 2);
            let recovered: Vec<_> = rows.iter().map(|e| store.load(&e.id).unwrap().doc.plain_text(StoryRef::Body)).collect();
            assert!(recovered.iter().any(|s| s.contains("First")));
            assert!(recovered.iter().any(|s| s.contains("Second")));
            std::fs::remove_dir_all(directory).unwrap();
        }
        #[test]
        fn recovery_failure_does_not_prevent_explicit_save() {
            let directory = directory();
            let mut a = app();
            a.enable_recovery(directory.clone()).unwrap();
            a.execute("text.insert", json!({"text": "Save despite recovery failure"})).unwrap();
            // Deliberately invalid recovery attachment key; an ordinary DOCX can still save.
            a.session.doc.media.insert("a".repeat(4097), std::sync::Arc::new(Vec::new()));
            let target = directory.join("saved.docx");
            assert_eq!(a.execute("file.save", json!({"path": target})).unwrap()["saved"], true);
            assert!(target.exists());
            a.finish_recovery();
            std::fs::remove_dir_all(directory).unwrap();
        }
        #[test]
        fn recovery_dialog_compare_and_restore_keep_save_changes_guard() {
            use egui_kittest::kittest::Queryable;
            let directory = directory();
            let store = Store::new(directory.clone()).unwrap();
            let source = wordcraft_engine::Session::new(Document::from_text("Recovered draft"));
            let id = store.save(&unique_id(), 1, &Snapshot::capture(&source)).unwrap();
            let mut a = app();
            a.enable_recovery(directory.clone()).unwrap();
            a.execute("text.insert", json!({"text": "Current unsaved writing"})).unwrap();
            if let Some(crate::dialogs::Dialog::Recovery { state }) = &mut a.dialog {
                state.selected = id.clone();
            }
            let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
                |ui, a: &mut WordApp| {
                    a.logic(ui.ctx());
                    a.ui(ui);
                },
                a,
            );
            h.get_by_label("Compare Recovery Versions").click();
            h.run();
            assert!(h.state().session.doc.plain_text(StoryRef::Body).contains("Current unsaved"));
            assert!(matches!(&h.state().dialog, Some(crate::dialogs::Dialog::Recovery { state }) if state.compared));
            h.get_by_label("Restore Recovery Version").click();
            h.run();
            assert!(matches!(h.state().dialog, Some(crate::dialogs::Dialog::SaveChanges { .. })));
            h.get_by_label("Cancel").click();
            h.run();
            assert!(h.state().session.doc.plain_text(StoryRef::Body).contains("Current unsaved"));
            h.state_mut().run("file.recover", json!({})).unwrap();
            if let Some(crate::dialogs::Dialog::Recovery { state }) = &mut h.state_mut().dialog {
                state.selected = id;
            }
            h.run();
            h.get_by_label("Restore Recovery Version").click();
            h.run();
            h.get_by_label("Don't Save").click();
            h.run();
            assert_eq!(h.state().session.doc.plain_text(StoryRef::Body), "Recovered draft");
            assert!(h.state().session.dirty && h.state().session.path.is_none());
            assert!(store.list().unwrap().len() >= 2, "current writing checkpointed before restore");
            h.state_mut().finish_recovery();
            drop(h);
            std::fs::remove_dir_all(directory).unwrap();
        }
        #[test]
        fn corrupt_recovery_can_be_explicitly_discarded_from_dialog() {
            use egui_kittest::kittest::Queryable;
            let directory = directory();
            let _store = Store::new(directory.clone()).unwrap();
            let id = format!("{}-a", unique_id());
            let path = directory.join(format!("{id}.wcr"));
            std::fs::write(&path, b"interrupted/corrupt archive").unwrap();
            let mut a = app();
            a.enable_recovery(directory.clone()).unwrap();
            if let Some(crate::dialogs::Dialog::Recovery { state }) = &mut a.dialog {
                state.selected = id;
            }
            let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
                |ui, a: &mut WordApp| {
                    a.logic(ui.ctx());
                    a.ui(ui);
                },
                a,
            );
            h.get_by_label("Discard Recovery Version").click();
            h.run();
            assert!(path.exists());
            h.get_by_label("Discard this version").click();
            h.run();
            assert!(!path.exists());
            h.state_mut().finish_recovery();
            drop(h);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::Controller;
