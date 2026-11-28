//! Explicit source selection is snapshotted before tracing, never discovered from paths in events.
use super::Options;
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeSet,
    io::{Cursor, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Semaphore, time::Instant};

pub(crate) const MAX_FILES: usize = 16;
pub(crate) const MAX_FILE: usize = 256 * 1024;
pub(crate) const MAX_TOTAL: usize = 2 * 1024 * 1024;
static READERS: Semaphore = Semaphore::const_new(4);

pub(super) fn validate(options: &Options) -> Result<(), &'static str> {
    if options.sources == options.source_files.is_empty()
        || options.source_files.len() > MAX_FILES
        || options
            .source_files
            .iter()
            .any(|path| path.len() > 4096 || path.contains('\0') || !Path::new(path).is_absolute())
        || options.source_files.iter().collect::<BTreeSet<_>>().len() != options.source_files.len()
    {
        return Err("TRACE_SOURCE_OPTIONS");
    }
    Ok(())
}
#[derive(Default)]
pub(crate) struct Sources {
    entries: Vec<(String, Vec<u8>)>,
}
impl std::fmt::Debug for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraceSources")
            .field("files", &self.entries.len())
            .finish()
    }
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn checkpoint(cancel: &AtomicBool, deadline: Instant) -> Result<(), &'static str> {
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        Err("TRACE_SOURCE_DEADLINE")
    } else {
        Ok(())
    }
}
impl Sources {
    pub(crate) async fn prepare(
        options: &Options,
        deadline: Instant,
    ) -> Result<Self, &'static str> {
        validate(options)?;
        if !options.sources {
            return Ok(Self::default());
        }
        let cancel = Cancel(Arc::new(AtomicBool::new(false)));
        let cancelled = cancel.0.clone();
        let paths = options.source_files.clone();
        let permit = tokio::time::timeout_at(deadline, READERS.acquire())
            .await
            .map_err(|_| "TRACE_SOURCE_DEADLINE")?
            .map_err(|_| "TRACE_SOURCE_IO")?;
        let work = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut entries = Vec::new();
            let mut total = 0;
            for path in paths {
                checkpoint(&cancelled, deadline)?;
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(0x0020_0000);
                }
                let mut file = options.open(&path).map_err(|_| "TRACE_SOURCE_IO")?;
                let before = file.metadata().map_err(|_| "TRACE_SOURCE_IO")?;
                if !before.is_file() || before.file_type().is_symlink() {
                    return Err("TRACE_SOURCE_NOT_REGULAR");
                }
                let max = MAX_FILE.min(MAX_TOTAL - total);
                if before.len() > max as u64 {
                    return Err("TRACE_SOURCE_FILE_BOUNDS");
                }
                let mut bytes = Vec::with_capacity(before.len() as usize);
                let mut buffer = [0; 16384];
                loop {
                    checkpoint(&cancelled, deadline)?;
                    let n = file.read(&mut buffer).map_err(|_| "TRACE_SOURCE_IO")?;
                    if n == 0 {
                        break;
                    }
                    if bytes.len() + n > max {
                        return Err("TRACE_SOURCE_FILE_BOUNDS");
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let after = file.metadata().map_err(|_| "TRACE_SOURCE_IO")?;
                if after.len() != bytes.len() as u64
                    || before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                {
                    return Err("TRACE_SOURCE_CHANGED");
                }
                total += bytes.len();
                let extension = Path::new(&path)
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|s| format!(".{s}"))
                    .unwrap_or_default();
                entries.push((
                    format!("src/{:x}{extension}", Sha1::digest(path.as_bytes())),
                    bytes,
                ));
            }
            checkpoint(&cancelled, deadline)?;
            Ok(Self { entries })
        });
        tokio::time::timeout_at(deadline, work)
            .await
            .map_err(|_| "TRACE_SOURCE_DEADLINE")?
            .map_err(|_| "TRACE_SOURCE_WORKER_FAILED")?
    }
    pub(crate) fn count(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn append(&self, bytes: Vec<u8>, max: usize) -> Result<Vec<u8>, &'static str> {
        if self.entries.is_empty() {
            return Ok(bytes);
        }
        {
            let mut archive =
                zip::ZipArchive::new(Cursor::new(&bytes)).map_err(|_| "TRACE_ARCHIVE_PROTOCOL")?;
            for (name, _) in &self.entries {
                if archive.by_name(name).is_ok() {
                    return Err("TRACE_SOURCE_ARCHIVE_CONFLICT");
                }
            }
        }
        let mut writer =
            zip::ZipWriter::new_append(Cursor::new(bytes)).map_err(|_| "TRACE_ARCHIVE_PROTOCOL")?;
        for (name, bytes) in &self.entries {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .map_err(|_| "TRACE_ARCHIVE_PROTOCOL")?;
            writer
                .write_all(bytes)
                .map_err(|_| "TRACE_ARCHIVE_PROTOCOL")?;
        }
        let bytes = writer
            .finish()
            .map_err(|_| "TRACE_ARCHIVE_PROTOCOL")?
            .into_inner();
        if bytes.len() > max {
            return Err("TRACE_ARCHIVE_BOUNDS");
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn options(path: &Path) -> Options {
        Options {
            sources: true,
            source_files: vec![path.to_str().unwrap().into()],
            ..Default::default()
        }
    }
    fn archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new_stream(Vec::new());
        writer
            .start_file("trace.trace", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"Acme original trace").unwrap();
        writer
            .start_file("trace.network", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }
    #[tokio::test]
    async fn selected_sources_are_immutable_private_and_preserve_existing_zip_entries() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Acme.spec.ts");
        std::fs::write(&path, b"Acme selected source").unwrap();
        let options = options(&path);
        let sources = Sources::prepare(&options, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(options.summary()["sourceFileCount"], 1);
        assert!(!options.summary().to_string().contains("Acme.spec.ts"));
        assert!(!format!("{sources:?}").contains("Acme"));
        let bytes = sources.append(archive(), usize::MAX).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
        let name = format!(
            "src/{:x}.ts",
            Sha1::digest(path.to_str().unwrap().as_bytes())
        );
        let mut text = String::new();
        zip.by_name(&name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "Acme selected source");
        text.clear();
        zip.by_name("trace.trace")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "Acme original trace");
        assert!(zip.by_name("trace.network").is_ok());
        assert_eq!(
            sources.append(bytes, usize::MAX).unwrap_err(),
            "TRACE_SOURCE_ARCHIVE_CONFLICT"
        );
        assert_eq!(
            sources.append(archive(), 1).unwrap_err(),
            "TRACE_ARCHIVE_BOUNDS"
        );
    }
    #[tokio::test]
    async fn invalid_or_expired_source_selection_refuses_before_trace_setup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Acme.js");
        std::fs::write(&path, vec![0; MAX_FILE + 1]).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            Sources::prepare(&options(&path), until).await.unwrap_err(),
            "TRACE_SOURCE_FILE_BOUNDS"
        );
        assert_eq!(
            Sources::prepare(&options(root.path()), until)
                .await
                .unwrap_err(),
            "TRACE_SOURCE_NOT_REGULAR"
        );
        assert_eq!(
            Sources::prepare(&options(&path), Instant::now())
                .await
                .unwrap_err(),
            "TRACE_SOURCE_DEADLINE"
        );
        #[cfg(unix)]
        {
            let link = root.path().join("link.js");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(
                Sources::prepare(&options(&link), until).await.unwrap_err(),
                "TRACE_SOURCE_IO"
            );
        }
        let mut value = options(&path);
        value.sources = false;
        assert_eq!(value.validate(false), Err("TRACE_SOURCE_OPTIONS"));
        value.sources = true;
        assert_eq!(
            value.validate(true),
            Err("TRACE_ARCHIVE_OPTIONS_UNSUPPORTED")
        );
        value.source_files = vec!["relative.js".into()];
        assert_eq!(value.validate(false), Err("TRACE_SOURCE_OPTIONS"));
        value.source_files = vec![path.to_str().unwrap().into(); 2];
        assert_eq!(value.validate(false), Err("TRACE_SOURCE_OPTIONS"));
        assert_eq!(
            Options {
                live: true,
                ..Default::default()
            }
            .validate(true),
            Err("TRACE_ARCHIVE_OPTIONS_UNSUPPORTED")
        );
    }
    #[tokio::test]
    async fn aggregate_source_snapshot_budget_is_independent_of_each_file_bound() {
        let root = tempfile::tempdir().unwrap();
        let mut options = Options {
            sources: true,
            ..Default::default()
        };
        for index in 0..=MAX_TOTAL / MAX_FILE {
            let path = root.path().join(format!("Acme-{index}.ts"));
            std::fs::write(&path, vec![b'x'; MAX_FILE]).unwrap();
            options.source_files.push(path.to_str().unwrap().into());
        }
        assert!(options.validate(false).is_ok());
        assert_eq!(
            Sources::prepare(&options, Instant::now() + Duration::from_secs(5))
                .await
                .unwrap_err(),
            "TRACE_SOURCE_FILE_BOUNDS"
        );
        assert_eq!(
            Options {
                sources: true,
                ..Default::default()
            }
            .validate(false),
            Err("TRACE_SOURCE_OPTIONS")
        );
    }
}
