use std::path::PathBuf;
use wordcraft_doc::{Document, StoryRef};
use wordcraft_engine::io;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wordcraft-save-integration-{}-{}",
            std::process::id(),
            wordcraft_engine::recovery::legacy::unique_id()
        ));
        std::fs::create_dir(&path).expect("scratch directory");
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample(text: &str) -> Document {
    Document::from_text(text)
}

#[cfg(unix)]
#[test]
fn docx_save_does_not_follow_legacy_temp_symlink_or_overwrite_original_neighbor() {
    let scratch = Scratch::new();
    let document = scratch.path("paper.docx");
    let neighbor = scratch.path("neighbor");
    let legacy_tmp = scratch.path("paper.docx.tmp");
    let original = io::save_bytes("paper.docx", &sample("original document")).expect("original package");
    std::fs::write(&document, &original).expect("write original");
    std::fs::write(&neighbor, b"neighbor remains private").expect("neighbor");
    std::os::unix::fs::symlink(&neighbor, &legacy_tmp).expect("legacy symlink");

    io::save_path(&document, &sample("saved document")).expect("safe save");

    let reopened = io::open_path(&document).expect("reopen committed DOCX");
    assert_eq!(reopened.plain_text(StoryRef::Body), "saved document");
    assert_eq!(std::fs::read(&neighbor).expect("neighbor bytes"), b"neighbor remains private");
    assert_eq!(std::fs::read_link(&legacy_tmp).expect("legacy link remains"), neighbor);
}

#[test]
fn failed_save_keeps_original_bytes_and_leaves_no_save_temporary_file() {
    let scratch = Scratch::new();
    let original_path = scratch.path("original.docx");
    let original = io::save_bytes("original.docx", &sample("still original")).expect("original package");
    std::fs::write(&original_path, &original).expect("write original");
    let blocked_path = scratch.path("blocked.docx");
    std::fs::create_dir(&blocked_path).expect("blocked destination");

    assert!(io::save_path(&blocked_path, &sample("must not be written")).is_err());

    assert_eq!(std::fs::read(original_path).expect("original bytes"), original);
    let names = std::fs::read_dir(&scratch.0)
        .expect("read directory")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(!names.iter().any(|name| name.starts_with(".wordcraft-save-")));
}
