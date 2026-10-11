//! Atomic native file replacement through an exclusively owned sibling temporary file.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(super) fn write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "refusing to replace a symbolic link"));
        }
        Ok(metadata) if metadata.is_file() => Some(metadata),
        Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "save destination is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut temp = Temporary::create(destination, metadata.is_some())?;
    {
        let file = temp.file.as_mut().ok_or_else(|| io::Error::other("save temporary file is closed"))?;
        file.write_all(bytes)?;
        if let Some(metadata) = metadata {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
    }
    drop(temp.file.take());
    fs::rename(&temp.path, destination)?;
    temp.committed = true;
    if let Err(error) = sync_parent(destination) {
        // The rename is already committed and cannot be rolled back without risking data loss.
        // Report reduced crash durability, but don't tell the caller that the save failed.
        log::warn!("saved {}, but could not synchronize its containing directory: {error}", destination.display());
    }
    Ok(())
}

fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

struct Temporary {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl Temporary {
    fn create(destination: &Path, _replacing: bool) -> io::Result<Self> {
        Self::create_with(destination, _replacing, || {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
            Ok(random)
        })
    }

    fn create_with(destination: &Path, _replacing: bool, mut nonce: impl FnMut() -> io::Result<[u8; 16]>) -> io::Result<Self> {
        if destination.file_name().is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "save destination needs a file name"));
        }
        for _ in 0..128 {
            let random = nonce()?;
            let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            let path = destination.with_file_name(format!(".wordcraft-save-{suffix}.tmp"));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                // Keep replacement contents private until all bytes are staged. New files use
                // the normal 0666-and-umask policy, matching ordinary file creation.
                if _replacing {
                    options.mode(0o600);
                }
            }
            match options.open(&path) {
                Ok(file) => return Ok(Self { path, file: Some(file), committed: false }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not create a unique save temporary file"))
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            // Remove only the path this save successfully created. A cleanup failure does not
            // justify touching the original destination or a neighbouring file.
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let mut random = [0_u8; 8];
            getrandom::fill(&mut random).expect("random scratch suffix");
            let path = std::env::temp_dir().join(format!("wordcraft-safe-save-{}-{:x?}", std::process::id(), random));
            fs::create_dir(&path).expect("scratch directory");
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn names(&self) -> Vec<String> {
            let mut names = fs::read_dir(&self.0)
                .expect("read scratch")
                .map(|entry| entry.expect("scratch entry").file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>();
            names.sort();
            names
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn malicious_legacy_temp_symlink_is_never_followed_or_removed() {
        #[cfg(unix)]
        {
            let scratch = Scratch::new();
            let destination = scratch.path("document.docx");
            let neighbor = scratch.path("neighbor");
            let old_temp = scratch.path("document.docx.tmp");
            fs::write(&destination, b"original").expect("original");
            fs::write(&neighbor, b"neighbor").expect("neighbor");
            std::os::unix::fs::symlink(&neighbor, &old_temp).expect("symlink");
            write(&destination, b"replacement").expect("safe save");
            assert_eq!(fs::read(&destination).expect("destination bytes"), b"replacement");
            assert_eq!(fs::read(&neighbor).expect("neighbor bytes"), b"neighbor");
            assert_eq!(fs::read_link(old_temp).expect("retained link"), neighbor);
        }
    }

    #[test]
    fn legacy_temp_file_collision_and_directory_are_untouched() {
        let scratch = Scratch::new();
        let destination = scratch.path("document.docx");
        let collision = scratch.path("document.docx.tmp");
        fs::write(&destination, b"original").expect("original");
        fs::write(&collision, b"keep").expect("collision");
        write(&destination, b"replacement").expect("safe save");
        assert_eq!(fs::read(&collision).expect("collision bytes"), b"keep");
        assert_eq!(fs::read(&destination).expect("destination bytes"), b"replacement");

        let second = scratch.path("directory-target.docx");
        fs::create_dir(&second).expect("destination directory");
        assert!(write(&second, b"no").is_err());
        assert!(second.is_dir());
    }

    #[test]
    fn random_name_collision_retries_without_truncating_the_existing_entry() {
        let scratch = Scratch::new();
        let destination = scratch.path("document.docx");
        let collision = scratch.path(format!(".wordcraft-save-{}.tmp", "00".repeat(16)).as_str());
        fs::write(&collision, b"keep collision").expect("collision");
        let mut calls = 0;
        let temp = Temporary::create_with(&destination, false, || {
            calls += 1;
            Ok([if calls == 1 { 0 } else { 1 }; 16])
        })
        .expect("second candidate succeeds");
        assert_eq!(calls, 2);
        assert_eq!(fs::read(collision).expect("collision bytes"), b"keep collision");
        drop(temp);
        assert_eq!(scratch.names(), [".wordcraft-save-00000000000000000000000000000000.tmp"]);
    }

    #[test]
    fn preparation_failure_preserves_original_and_cleans_only_owned_temp() {
        let scratch = Scratch::new();
        let destination = scratch.path("document.docx");
        fs::write(&destination, b"original").expect("original");
        let result = (|| {
            let mut temp = Temporary::create(&destination, true)?;
            temp.file.as_mut().ok_or_else(|| io::Error::other("closed"))?.write_all(b"partial")?;
            Err::<(), _>(io::Error::other("injected interruption"))
        })();
        assert!(result.is_err());
        assert_eq!(fs::read(&destination).expect("original bytes"), b"original");
        assert_eq!(scratch.names(), ["document.docx"]);
    }

    #[test]
    fn concurrent_writes_never_share_staging_files_or_leave_partial_content() {
        let scratch = Scratch::new();
        let destination = scratch.path("document.docx");
        fs::write(&destination, b"original").expect("original");
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for byte in *b"ab" {
            let barrier = Arc::clone(&barrier);
            let destination = destination.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                write(&destination, &vec![byte; 256 * 1024]).expect("concurrent save");
                byte
            }));
        }
        barrier.wait();
        let winners = workers.into_iter().map(|worker| worker.join().expect("save thread")).collect::<Vec<_>>();
        let bytes = fs::read(destination).expect("committed file");
        assert_eq!(bytes.len(), 256 * 1024);
        assert!(winners.iter().any(|byte| bytes.iter().all(|saved| saved == byte)));
        assert_eq!(scratch.names(), ["document.docx"]);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_existing_mode_and_refuses_destination_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let destination = scratch.path("document.docx");
        let target = scratch.path("target");
        fs::write(&destination, b"original").expect("original");
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o640)).expect("mode");
        write(&destination, b"updated").expect("save");
        assert_eq!(fs::metadata(&destination).expect("metadata").permissions().mode() & 0o777, 0o640);

        fs::write(&target, b"target").expect("target");
        let link = scratch.path("link.docx");
        std::os::unix::fs::symlink(&target, &link).expect("destination symlink");
        assert!(write(&link, b"unsafe").is_err());
        assert_eq!(fs::read(target).expect("target bytes"), b"target");
        assert!(link.is_symlink());
    }
}
