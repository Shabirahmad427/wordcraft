//! Durable, immutable recovery archives. Originals are never rewritten. Archives contain
//! editor JSON plus media, opaque package parts and (when available) the exact source DOCX.
//! No ZIP entry is extracted to disk; all lookups use manifest-controlled numeric names.
use crate::{Selection, Session, SourcePackage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};
use wordcraft_doc::Document;

const MAX_FILE: u64 = 512 << 20;
const MAX_MODEL: u64 = 16 << 20;
const MAX_META: u64 = 1 << 20;
const MAX_ENTRIES: usize = 2048;
const MAX_SCAN: usize = 4096;
static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn unique_id() -> String {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{time:x}-{:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 160 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn io_error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone)]
pub struct Snapshot {
    pub doc: Document,
    pub selection: Selection,
    pub bib_style: String,
    pub source: Option<SourcePackage>,
    pub original_path: Option<String>,
    pub title: String,
}
impl Snapshot {
    pub fn capture(s: &Session) -> Self {
        Self {
            doc: s.doc.clone(),
            selection: s.sel.clone(),
            bib_style: s.bib_style.clone(),
            source: s.source_package.clone(),
            original_path: s.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            title: if s.doc.core.title.is_empty() {
                s.path.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Unsaved document".into())
            } else {
                s.doc.core.title.clone()
            },
        }
    }
    /// Restore into a dirty working copy: Save As is required; never AutoSave to the original.
    pub fn restore(self, session: &mut Session) {
        session.set_document(self.doc);
        session.source_package = self.source;
        session.path = None;
        session.dirty = true;
        session.sel = self.selection;
        session.clamp_selection();
        session.bib_style = self.bib_style;
    }
}
#[derive(Serialize, Deserialize)]
struct Blob {
    key: String,
    entry: String,
    size: u64,
}
#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u32,
    id: String,
    writer: String,
    document: u64,
    timestamp: String,
    title: String,
    original_path: Option<String>,
    selection: Selection,
    bib_style: String,
    media: Vec<Blob>,
    opaque: Vec<Blob>,
    source_name: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub timestamp: String,
    pub original_path: Option<String>,
    pub has_original: bool,
    pub original_name: Option<String>,
    pub active: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}
impl Store {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(io_error)?;
        if std::fs::symlink_metadata(&dir).map_err(io_error)?.file_type().is_symlink() {
            return Err("recovery directory must not be a symlink".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        }
        Ok(Self { dir })
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("invalid recovery ID".into());
        }
        Ok(self.dir.join(format!("{id}.wcr")))
    }
    /// An OS file lock identifies a live writer; the OS releases it even on process death.
    pub fn lease(&self, writer: &str) -> Result<File, String> {
        if !valid_id(writer) {
            return Err("invalid recovery writer ID".into());
        }
        let file = private_file(&self.dir.join(format!("{writer}.lock")))?;
        file.try_lock().map_err(io_error)?;
        Ok(file)
    }
    fn active(&self, writer: &str) -> Result<bool, String> {
        if !valid_id(writer) {
            return Err("invalid recovery writer ID".into());
        }
        let path = self.dir.join(format!("{writer}.lock"));
        if !path.exists() {
            return Ok(false);
        }
        if !std::fs::symlink_metadata(&path).map_err(io_error)?.is_file() {
            return Err("recovery lease is not a regular file".into());
        }
        let file = OpenOptions::new().read(true).write(true).open(&path).map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(e) => Err(io_error(e)),
        }
    }
    pub fn save(&self, writer: &str, document: u64, snapshot: &Snapshot) -> Result<String, String> {
        if !valid_id(writer) {
            return Err("invalid writer ID".into());
        }
        let id = format!("{writer}-{:016x}", NEXT.fetch_add(1, Ordering::Relaxed));
        if snapshot.doc.media.len().saturating_add(snapshot.doc.passthrough.len()).saturating_add(3) > MAX_ENTRIES {
            return Err("too many recovery attachments".into());
        }
        let model = json_bytes(&snapshot.doc, MAX_MODEL)?;
        check_json(&model)?;
        let media: Vec<_> = snapshot
            .doc
            .media
            .iter()
            .enumerate()
            .map(|(i, (key, data))| Blob { key: key.clone(), entry: format!("media/{i}"), size: data.len() as u64 })
            .collect();
        let opaque: Vec<_> = snapshot
            .doc
            .passthrough
            .iter()
            .enumerate()
            .map(|(i, (key, data))| Blob { key: key.clone(), entry: format!("opaque/{i}"), size: data.len() as u64 })
            .collect();
        if media.len() + opaque.len() + 3 > MAX_ENTRIES {
            return Err("too many recovery attachments".into());
        }
        let mut total = model.len() as u64;
        for blob in media.iter().chain(&opaque) {
            total = total.checked_add(blob.size).ok_or("recovery size overflow")?;
        }
        total = total.checked_add(snapshot.source.as_ref().map_or(0, |s| s.bytes.len() as u64)).ok_or("recovery size overflow")?;
        if total > MAX_FILE {
            return Err("recovery snapshot exceeds 512 MiB".into());
        }
        let metadata = Metadata {
            version: 1,
            id: id.clone(),
            writer: writer.into(),
            document,
            timestamp: {
                let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
                format!("{}.{:09}Z", crate::cmd::iso_from_unix_secs(time.as_secs()).trim_end_matches('Z'), time.subsec_nanos())
            },
            title: snapshot.title.clone(),
            original_path: snapshot.original_path.clone(),
            selection: snapshot.selection.clone(),
            bib_style: snapshot.bib_style.clone(),
            media,
            opaque,
            source_name: snapshot.source.as_ref().map(|s| s.name.clone()),
        };
        if metadata.title.len() > 4096
            || metadata.original_path.as_ref().is_some_and(|p| p.len() > 4096)
            || metadata.source_name.as_ref().is_some_and(|p| p.len() > 4096)
            || metadata.bib_style.len() > 1024
        {
            return Err("recovery metadata text exceeds limits".into());
        }
        if metadata.media.iter().chain(&metadata.opaque).any(|b| b.key.len() > 4096) {
            return Err("recovery attachment key exceeds limits".into());
        }
        let meta = json_bytes(&metadata, MAX_META)?;
        check_json(&meta)?;
        self.publish(&id, |file| {
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
                zip.start_file(name, opts).map_err(io_error)?;
                zip.write_all(bytes).map_err(io_error)
            };
            put("metadata.json", &meta)?;
            put("document.json", &model)?;
            for blob in &metadata.media {
                put(&blob.entry, snapshot.doc.media.get(&blob.key).ok_or("missing media")?)?;
            }
            for blob in &metadata.opaque {
                put(&blob.entry, snapshot.doc.passthrough.get(&blob.key).ok_or("missing opaque part")?)?;
            }
            if let Some(source) = &snapshot.source {
                put("original.package", &source.bytes)?;
            }
            let file = zip.finish().map_err(io_error)?;
            if file.metadata().map_err(io_error)?.len() > MAX_FILE {
                return Err("compressed snapshot exceeds 512 MiB".into());
            }
            Ok(())
        })?;
        Ok(id)
    }
    /// Unique staging file + file fsync + rename to an immutable name. A killed writer leaves
    /// only .partial; listing ignores it. A failed write never replaces an earlier snapshot.
    fn publish(&self, id: &str, write: impl FnOnce(&mut File) -> Result<(), String>) -> Result<(), String> {
        let final_path = self.path(id)?;
        let partial = final_path.with_extension("partial");
        let mut file = private_file(&partial)?;
        let result = (|| {
            write(&mut file)?;
            file.sync_all().map_err(io_error)?;
            drop(file);
            if final_path.exists() {
                return Err("recovery ID already exists".into());
            }
            std::fs::rename(&partial, &final_path).map_err(io_error)?;
            sync_dir(&self.dir)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        result
    }
    fn metadata(&self, id: &str) -> Result<(Metadata, zip::ZipArchive<File>), String> {
        let file = open_regular(&self.path(id)?)?;
        if file.metadata().map_err(io_error)?.len() > MAX_FILE {
            return Err("recovery file exceeds 512 MiB".into());
        }
        let mut zip = zip::ZipArchive::new(file).map_err(io_error)?;
        if zip.len() > MAX_ENTRIES {
            return Err("too many recovery entries".into());
        }
        let mut names = BTreeSet::new();
        for i in 0..zip.len() {
            let entry = zip.by_index(i).map_err(io_error)?;
            let name = entry.name();
            if name.len() > 128 {
                return Err("recovery entry name too long".into());
            }
            if !names.insert(name.to_string()) {
                return Err("duplicate ZIP entry".into());
            }
        }
        let bytes = zip_bytes(&mut zip, "metadata.json", MAX_META)?;
        check_json(&bytes)?;
        let meta: Metadata = serde_json::from_slice(&bytes).map_err(io_error)?;
        if meta.version != 1 || meta.id != id || !valid_id(&meta.writer) || id.rsplit_once('-').map(|(w, _)| w) != Some(meta.writer.as_str()) {
            return Err("unsupported or inconsistent recovery metadata".into());
        }
        if meta.title.len() > 4096
            || meta.original_path.as_ref().is_some_and(|p| p.len() > 4096)
            || meta.source_name.as_ref().is_some_and(|p| p.len() > 4096)
            || meta.timestamp.len() > 64
            || meta.bib_style.len() > 1024
        {
            return Err("recovery metadata text exceeds limits".into());
        }
        if meta.media.len() + meta.opaque.len() + 3 > MAX_ENTRIES {
            return Err("too many recovery attachments".into());
        }
        Ok((meta, zip))
    }
    pub fn list(&self) -> Result<Vec<Entry>, String> {
        let mut entries = Vec::new();
        let mut scanned = 0;
        for entry in std::fs::read_dir(&self.dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            scanned += 1;
            if scanned > MAX_SCAN {
                return Err("recovery directory exceeds 4096 entries; archive or discard old versions".into());
            }
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "wcr") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).filter(|id| valid_id(id)) else { continue };
            let row = match self.metadata(id) {
                Ok((m, _)) => Entry {
                    id: id.into(),
                    title: m.title,
                    timestamp: m.timestamp,
                    original_path: m.original_path,
                    has_original: m.source_name.is_some(),
                    original_name: m.source_name,
                    active: self.active(&m.writer).unwrap_or(true),
                    error: None,
                },
                Err(error) => Entry {
                    id: id.into(),
                    title: "Unreadable recovery file".into(),
                    timestamp: String::new(),
                    original_path: None,
                    has_original: false,
                    original_name: None,
                    active: id.rsplit_once('-').is_some_and(|(writer, _)| self.active(writer).unwrap_or(true)),
                    error: Some(error),
                },
            };
            entries.push(row);
        }
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        Ok(entries)
    }
    pub fn load(&self, id: &str) -> Result<Snapshot, String> {
        let (meta, mut zip) = self.metadata(id)?;
        let model = zip_bytes(&mut zip, "document.json", MAX_MODEL)?;
        check_json(&model)?;
        let mut doc: Document = serde_json::from_slice(&model).map_err(io_error)?;
        let mut total = model.len() as u64;
        for (blobs, prefix, map) in [(&meta.media, "media/", &mut doc.media), (&meta.opaque, "opaque/", &mut doc.passthrough)] {
            let mut keys = BTreeSet::new();
            let mut names = BTreeSet::new();
            for blob in blobs {
                if blob.key.len() > 4096
                    || !keys.insert(&blob.key)
                    || !names.insert(&blob.entry)
                    || !blob.entry.strip_prefix(prefix).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                {
                    return Err("invalid recovery attachment".into());
                }
                total = total.checked_add(blob.size).ok_or("recovery size overflow")?;
                if total > MAX_FILE {
                    return Err("recovery contents exceed 512 MiB".into());
                }
                let data = zip_bytes(&mut zip, &blob.entry, blob.size.min(MAX_FILE))?;
                if data.len() as u64 != blob.size {
                    return Err("recovery attachment size mismatch".into());
                }
                map.insert(blob.key.clone(), Arc::new(data));
            }
        }
        let source = match meta.source_name {
            Some(name) => {
                let data = zip_bytes(&mut zip, "original.package", MAX_FILE.saturating_sub(total))?;
                Some(SourcePackage { name, bytes: Arc::new(data) })
            }
            None => None,
        };
        doc.ensure_nonempty();
        Ok(Snapshot { doc, selection: meta.selection, bib_style: meta.bib_style, source, original_path: meta.original_path, title: meta.title })
    }
    pub fn discard(&self, id: &str) -> Result<(), String> {
        // Derive the lease from the file name as well, so a corrupt archive can still be removed.
        let writer = id.rsplit_once('-').map(|(writer, _)| writer).ok_or("invalid recovery ID")?;
        if self.active(writer)? {
            return Err("this recovery version belongs to an active session".into());
        }
        std::fs::remove_file(self.path(id)?).map_err(io_error)?;
        if !self.list()?.iter().any(|r| r.id.rsplit_once('-').map(|(w, _)| w) == Some(writer)) {
            let _ = self.release_lease(writer);
        }
        sync_dir(&self.dir)
    }
    /// The worker may prune only its own successfully committed versions. Other sessions' or
    /// crashed documents are retained until the user explicitly discards them.
    pub fn prune_own(&self, writer: &str, document: u64, keep: usize) -> Result<(), String> {
        let mut ids = Vec::new();
        for row in self.list()? {
            if let Ok((m, _)) = self.metadata(&row.id)
                && m.writer == writer
                && m.document == document
            {
                ids.push(row.id);
            }
        }
        // Sequence numbers are fixed-width and monotonic within a writer. Retention must
        // remain correct even if the wall clock is moved backwards.
        ids.sort_by(|a, b| b.cmp(a));
        for id in ids.into_iter().skip(keep.max(1)) {
            std::fs::remove_file(self.path(&id)?).map_err(io_error)?;
        }
        sync_dir(&self.dir)
    }
    /// Called only after releasing our lease; old archives remain available.
    pub fn release_lease(&self, writer: &str) -> Result<(), String> {
        if !valid_id(writer) || self.active(writer)? {
            return Err("cannot release an active recovery lease".into());
        }
        std::fs::remove_file(self.dir.join(format!("{writer}.lock"))).map_err(io_error)?;
        sync_dir(&self.dir)
    }
    pub fn export_original(&self, id: &str, target: &Path) -> Result<(), String> {
        let snapshot = self.load(id)?;
        let source = snapshot.source.ok_or("no original package in this recovery version")?;
        // Publish a fully synchronized copy without ever replacing an existing destination.
        // A hard link in the same directory provides an atomic no-replace operation on Unix
        // and Windows/NTFS. Unsupported filesystems return an error, leaving originals alone.
        let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let partial = parent.join(format!(".wordcraft-original-{}.partial", unique_id()));
        let mut file = private_file(&partial)?;
        let result = (|| {
            file.write_all(&source.bytes).and_then(|_| file.sync_all()).map_err(io_error)?;
            drop(file);
            std::fs::hard_link(&partial, target).map_err(io_error)?;
            sync_dir(parent)
        })();
        let _ = std::fs::remove_file(&partial);
        result
    }
}
fn private_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(io_error)
}
fn open_regular(path: &Path) -> Result<File, String> {
    if !std::fs::symlink_metadata(path).map_err(io_error)?.is_file() {
        return Err("recovery entry is not a regular file".into());
    }
    File::open(path).map_err(io_error)
}
fn sync_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(dir).and_then(|f| f.sync_all()).map_err(io_error)?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}
fn zip_bytes(zip: &mut zip::ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>, String> {
    let file = zip.by_name(name).map_err(io_error)?;
    if file.size() > limit {
        return Err(format!("{name} exceeds recovery size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > limit {
        return Err(format!("{name} exceeds recovery size limit"));
    }
    Ok(bytes)
}
fn json_bytes(value: &impl Serialize, limit: u64) -> Result<Vec<u8>, String> {
    struct Capped {
        bytes: Vec<u8>,
        limit: u64,
    }
    impl Write for Capped {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() as u64 > self.limit.saturating_sub(self.bytes.len() as u64) {
                return Err(std::io::Error::other("recovery JSON exceeds size limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Capped { bytes: Vec::new(), limit };
    serde_json::to_writer(&mut writer, value).map_err(io_error)?;
    Ok(writer.bytes)
}
/// Count JSON values before deserializing model structs, bounding hostile array allocations.
fn check_json(bytes: &[u8]) -> Result<(), String> {
    use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
    struct Check<'a> {
        left: &'a mut usize,
        depth: usize,
    }
    impl<'de> DeserializeSeed<'de> for Check<'_> {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
            if *self.left == 0 || self.depth > 64 {
                return Err(serde::de::Error::custom("recovery JSON structure limit"));
            }
            *self.left -= 1;
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Check<'_> {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded recovery JSON")
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
            Ok(())
        }
        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
            Ok(())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
            while a.next_element_seed(Check { left: self.left, depth: self.depth + 1 })?.is_some() {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
            while a.next_key::<IgnoredAny>()?.is_some() {
                a.next_value_seed(Check { left: self.left, depth: self.depth + 1 })?;
            }
            Ok(())
        }
    }
    let mut left = 250_000;
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    Check { left: &mut left, depth: 0 }.deserialize(&mut parser).map_err(io_error)?;
    parser.end().map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Store {
        Store::new(std::env::temp_dir().join(format!("wordcraft-recovery-{}", unique_id()))).unwrap()
    }
    fn snapshot() -> Snapshot {
        let mut session = Session::new(Document::new());
        session.run("text.insert", &serde_json::json!({"text": "Recovered science α β"})).unwrap();
        session.doc.media.insert("image.png".into(), Arc::new(vec![1, 2, 3, 4]));
        session.doc.passthrough.insert("customXml/item1.xml".into(), Arc::new(b"<bibliography/>".to_vec()));
        session.doc.set_custom_prop("wordcraft.sources", "citation library");
        session
            .run(
                "mendeley.import",
                &serde_json::json!({"ris": "TY  - BOOK\nID  - recovery-source\nTI  - Recovery science\nAU  - Rivera, Alex\nPY  - 2024\nER  - \n"}),
            )
            .unwrap();
        session.source_package =
            Some(SourcePackage { name: "paper.docx".into(), bytes: Arc::new(b"exact source with unsupported relationships".to_vec()) });
        session.path = Some(PathBuf::from("paper.docx"));
        Snapshot::capture(&session)
    }
    #[test]
    fn restores_model_media_opaque_library_selection_and_exact_original() {
        let store = store();
        let original = snapshot();
        let id = store.save(&unique_id(), 1, &original).unwrap();
        let loaded = Store::new(store.dir.clone()).unwrap().load(&id).unwrap();
        assert_eq!(serde_json::to_value(&original.doc).unwrap(), serde_json::to_value(&loaded.doc).unwrap());
        assert_eq!(original.doc.media, loaded.doc.media);
        assert_eq!(original.doc.passthrough, loaded.doc.passthrough);
        assert_eq!(original.source.as_ref().unwrap().bytes, loaded.source.as_ref().unwrap().bytes);
        assert_eq!(serde_json::to_value(&original.selection).unwrap(), serde_json::to_value(&loaded.selection).unwrap());
        let mut session = Session::new(Document::new());
        loaded.restore(&mut session);
        assert!(session.dirty && session.path.is_none());
        assert!(session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("science"));
        let copy = store.dir.join("original.docx");
        store.export_original(&id, &copy).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), *original.source.unwrap().bytes);
        assert!(store.export_original(&id, &copy).is_err());
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn interrupted_write_and_corruption_preserve_previous_snapshot() {
        let store = store();
        let id = store.save(&unique_id(), 1, &snapshot()).unwrap();
        let before = std::fs::read(store.path(&id).unwrap()).unwrap();
        assert!(
            store
                .publish("a-b-c-d", |f| {
                    f.write_all(b"partial").unwrap();
                    Err("simulated disk failure".into())
                })
                .is_err()
        );
        std::fs::write(store.dir.join("abandoned.partial"), b"incomplete").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(std::fs::read(store.path(&id).unwrap()).unwrap(), before);
        let corrupt = "a-b-c-e";
        std::fs::write(store.path(corrupt).unwrap(), b"truncated ZIP").unwrap();
        assert!(store.load(corrupt).is_err());
        let row = store.list().unwrap().into_iter().find(|e| e.id == corrupt).unwrap();
        assert!(row.error.is_some() && !row.active);
        store.discard(corrupt).unwrap();
        assert!(store.load(&id).is_ok());
        assert!(store.load("../../original.docx").is_err());
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn live_writer_is_protected_and_retention_is_per_document() {
        let store = store();
        let writer = unique_id();
        let lease = store.lease(&writer).unwrap();
        let other = store.save(&unique_id(), 1, &snapshot()).unwrap();
        let second_doc = store.save(&writer, 2, &snapshot()).unwrap();
        let mut created = Vec::new();
        for _ in 0..20 {
            created.push(store.save(&writer, 1, &snapshot()).unwrap());
        }
        store.prune_own(&writer, 1, 3).unwrap();
        let rows = store.list().unwrap();
        assert_eq!(rows.len(), 5);
        assert!(created.iter().rev().take(3).all(|id| rows.iter().any(|r| &r.id == id)));
        assert!(rows.iter().any(|r| r.id == other));
        assert!(rows.iter().any(|r| r.id == second_doc));
        assert!(store.discard(&second_doc).is_err());
        drop(lease);
        store.release_lease(&writer).unwrap();
        store.discard(&second_doc).unwrap();
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn hostile_json_is_bounded_and_existing_archive_is_immutable() {
        assert!(check_json(format!("{}0{}", "[".repeat(70), "]".repeat(70)).as_bytes()).is_err());
        assert!(check_json(format!("[{}0]", "0,".repeat(250_001)).as_bytes()).is_err());
        assert!(json_bytes(&vec![0; 100], 10).is_err());
        let store = store();
        let id = store.save(&unique_id(), 1, &snapshot()).unwrap();
        let before = std::fs::read(store.path(&id).unwrap()).unwrap();
        assert!(store.publish(&id, |f| f.write_all(b"replacement").map_err(io_error)).is_err());
        assert_eq!(std::fs::read(store.path(&id).unwrap()).unwrap(), before);
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn crash_child() {
        let Some(path) = std::env::var_os("WORDCRAFT_RECOVERY_CRASH_TEST") else { return };
        let store = Store::new(PathBuf::from(path)).unwrap();
        let writer = unique_id();
        let _lease = store.lease(&writer).unwrap();
        store.save(&writer, 1, &snapshot()).unwrap();
        let staging_id = format!("{writer}-deadbeef");
        let _ = store.publish(&staging_id, |file| {
            file.write_all(b"incomplete ZIP write").unwrap();
            file.sync_all().unwrap();
            if std::env::var_os("WORDCRAFT_RECOVERY_KILL_TEST").is_some() {
                std::fs::write(store.dir.join("ready"), b"ready").unwrap();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            std::process::exit(73);
        });
    }
    #[test]
    fn process_death_releases_lease_and_recovers_unsaved_document() {
        let store = store();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::crash_child", "--nocapture"])
            .env("WORDCRAFT_RECOVERY_CRASH_TEST", &store.dir)
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(73));
        let rows = Store::new(store.dir.clone()).unwrap().list().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows.first().unwrap().active);
        assert!(store.load(&rows.first().unwrap().id).unwrap().doc.plain_text(wordcraft_doc::StoryRef::Body).contains("science"));
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn killed_process_during_publication_retains_last_committed_snapshot() {
        let store = store();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::crash_child", "--nocapture"])
            .env("WORDCRAFT_RECOVERY_CRASH_TEST", &store.dir)
            .env("WORDCRAFT_RECOVERY_KILL_TEST", "1")
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !store.dir.join("ready").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ready = store.dir.join("ready").exists();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(ready, "child reached interrupted write");
        let rows = store.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows.first().unwrap().active);
        assert!(store.load(&rows.first().unwrap().id).is_ok());
        assert!(std::fs::read_dir(&store.dir).unwrap().any(|e| e.unwrap().path().extension().is_some_and(|e| e == "partial")));
        std::fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn damaged_payload_fails_crc_without_removing_any_version() {
        let store = store();
        let id = store.save(&unique_id(), 1, &snapshot()).unwrap();
        let path = store.path(&id).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let offset = zip.by_name("document.json").unwrap().data_start();
        drop(zip);
        let mut bytes = std::fs::read(&path).unwrap();
        let byte = bytes.get_mut(usize::try_from(offset).unwrap()).unwrap();
        *byte ^= 0xff;
        std::fs::write(&path, bytes).unwrap();
        assert!(store.load(&id).is_err());
        assert!(path.exists());
        store.discard(&id).unwrap();
        std::fs::remove_dir_all(store.dir).unwrap();
    }
}
