//! The compiler's input file system: the native one, with the server's
//! generated modules (entries, hot clients, Astro renders, plugin modules)
//! laid over it in memory. Nothing is written inside the project.
use rspack_fs::{FileMetadata, FilePermissions, NativeFileSystem, ReadableFileSystem};
use rspack_paths::{Utf8Path, Utf8PathBuf};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub(crate) struct Overlay {
    native: Arc<NativeFileSystem>,
    files: Arc<RwLock<HashMap<Utf8PathBuf, Generated>>>,
    /// The last modification time handed out, so every write gets a later one.
    clock: Arc<AtomicU64>,
}

/// A generated module and when its content last changed.
///
/// Rspack's persistent cache trusts a file whose modification time matches
/// its snapshot without reading it. A fresh time on every changed write, and
/// after every restart, makes it compare content instead.
struct Generated {
    bytes: Arc<Vec<u8>>,
    modified_ms: u64,
}

impl fmt::Debug for Overlay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Overlay")
    }
}

impl Overlay {
    pub(crate) fn new() -> Self {
        Self {
            native: Arc::new(NativeFileSystem::new(false)),
            files: Arc::default(),
            clock: Arc::default(),
        }
    }

    /// Writes a generated module. Returns whether its content changed, so an
    /// unchanged rewrite starts no build and keeps its modification time.
    pub(crate) fn write(&self, file: &str, source: &str) -> bool {
        let mut files = self.files.write().expect("overlay lock");
        let path = Utf8PathBuf::from(file);
        if files
            .get(&path)
            .is_some_and(|existing| existing.bytes.as_slice() == source.as_bytes())
        {
            return false;
        }
        let generated = Generated {
            bytes: Arc::new(source.as_bytes().to_vec()),
            modified_ms: self.tick(),
        };
        files.insert(path, generated);
        true
    }

    /// The wall-clock time in milliseconds, later than any time handed out before.
    fn tick(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        let mut last = self.clock.load(Ordering::Relaxed);
        loop {
            let next = now.max(last + 1);
            match self
                .clock
                .compare_exchange_weak(last, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return next,
                Err(seen) => last = seen,
            }
        }
    }

    fn get(&self, path: &Utf8Path) -> Option<Arc<Vec<u8>>> {
        self.files
            .read()
            .expect("overlay lock")
            .get(path)
            .map(|generated| generated.bytes.clone())
    }

    fn metadata_of(&self, path: &Utf8Path) -> Option<FileMetadata> {
        let files = self.files.read().expect("overlay lock");
        if let Some(generated) = files.get(path) {
            return Some(FileMetadata {
                is_file: true,
                is_directory: false,
                is_symlink: false,
                atime_ms: generated.modified_ms,
                mtime_ms: generated.modified_ms,
                ctime_ms: generated.modified_ms,
                size: generated.bytes.len() as u64,
            });
        }
        // A folder that holds only generated modules changed with its latest one.
        files
            .iter()
            .filter(|(file, _)| file.starts_with(path))
            .map(|(_, generated)| generated.modified_ms)
            .max()
            .map(|modified_ms| FileMetadata {
                is_file: false,
                is_directory: true,
                is_symlink: false,
                atime_ms: modified_ms,
                mtime_ms: modified_ms,
                ctime_ms: modified_ms,
                size: 0,
            })
    }

    fn names_in(&self, dir: &Utf8Path) -> Vec<String> {
        self.files
            .read()
            .expect("overlay lock")
            .keys()
            .filter(|file| file.parent() == Some(dir))
            .filter_map(|file| file.file_name().map(str::to_owned))
            .collect()
    }
}

#[async_trait::async_trait]
impl ReadableFileSystem for Overlay {
    async fn read(&self, path: &Utf8Path) -> rspack_fs::Result<Vec<u8>> {
        match self.get(path) {
            Some(bytes) => Ok(bytes.as_ref().clone()),
            None => self.native.read(path).await,
        }
    }
    fn read_sync(&self, path: &Utf8Path) -> rspack_fs::Result<Vec<u8>> {
        match self.get(path) {
            Some(bytes) => Ok(bytes.as_ref().clone()),
            None => self.native.read_sync(path),
        }
    }
    async fn metadata(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.metadata_of(path) {
            Some(metadata) if metadata.is_file => Ok(metadata),
            overlaid => match self.native.metadata(path).await {
                Ok(metadata) => Ok(metadata),
                Err(error) => overlaid.ok_or(error),
            },
        }
    }
    fn metadata_sync(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.metadata_of(path) {
            Some(metadata) if metadata.is_file => Ok(metadata),
            overlaid => match self.native.metadata_sync(path) {
                Ok(metadata) => Ok(metadata),
                Err(error) => overlaid.ok_or(error),
            },
        }
    }
    async fn symlink_metadata(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.metadata_of(path) {
            Some(metadata) if metadata.is_file => Ok(metadata),
            overlaid => match self.native.symlink_metadata(path).await {
                Ok(metadata) => Ok(metadata),
                Err(error) => overlaid.ok_or(error),
            },
        }
    }
    async fn read_link(&self, path: &Utf8Path) -> rspack_fs::Result<Utf8PathBuf> {
        self.native.read_link(path).await
    }
    async fn canonicalize(&self, path: &Utf8Path) -> rspack_fs::Result<Utf8PathBuf> {
        if self.metadata_of(path).is_some() && self.native.metadata_sync(path).is_err() {
            return Ok(path.to_path_buf());
        }
        self.native.canonicalize(path).await
    }
    async fn read_dir(&self, dir: &Utf8Path) -> rspack_fs::Result<Vec<String>> {
        let mut names = self.native.read_dir(dir).await.unwrap_or_default();
        names.extend(self.names_in(dir));
        Ok(names)
    }
    fn read_dir_sync(&self, dir: &Utf8Path) -> rspack_fs::Result<Vec<String>> {
        let mut names = self.native.read_dir_sync(dir).unwrap_or_default();
        names.extend(self.names_in(dir));
        Ok(names)
    }
    async fn permissions(&self, path: &Utf8Path) -> rspack_fs::Result<Option<FilePermissions>> {
        if self.get(path).is_some() {
            return Ok(None);
        }
        self.native.permissions(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generated_modules_shadow_and_extend_the_native_tree() {
        let dir = std::env::temp_dir().join(format!("acme-overlay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("real.ts"), "export const real = 1;").unwrap();
        let overlay = Overlay::new();
        let root = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        let virtual_file = root.join(".workbench-rspack/entry.js");
        assert!(overlay.write(virtual_file.as_str(), "export {};"));
        assert!(
            !overlay.write(virtual_file.as_str(), "export {};"),
            "unchanged"
        );
        assert_eq!(overlay.read(&virtual_file).await.unwrap(), b"export {};");
        assert_eq!(
            overlay.read(&root.join("real.ts")).await.unwrap(),
            b"export const real = 1;"
        );
        assert!(
            overlay
                .metadata(&root.join(".workbench-rspack"))
                .await
                .unwrap()
                .is_directory
        );
        assert!(
            overlay
                .read_dir(&root)
                .await
                .unwrap()
                .contains(&"real.ts".to_owned())
        );
        assert_eq!(
            overlay
                .read_dir(&root.join(".workbench-rspack"))
                .await
                .unwrap(),
            vec!["entry.js".to_owned()]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_changed_module_gets_a_later_modification_time() {
        let file = Utf8Path::new("/acme/.workbench-rspack/entry.js");
        let modified = |overlay: &Overlay| {
            let metadata = overlay.metadata_sync(file).unwrap();
            assert_eq!(metadata.mtime_ms, metadata.ctime_ms);
            metadata.mtime_ms
        };
        let overlay = Overlay::new();
        overlay.write(file.as_str(), "export const a = 1;");
        let first = modified(&overlay);
        assert_ne!(first, 0, "a zero time would let a stale cache entry match");
        overlay.write(file.as_str(), "export const a = 1;");
        assert_eq!(
            modified(&overlay),
            first,
            "unchanged content keeps its time"
        );
        overlay.write(file.as_str(), "export const a = 2;");
        let second = modified(&overlay);
        assert!(second > first);
        assert_eq!(
            overlay
                .metadata_sync(Utf8Path::new("/acme/.workbench-rspack"))
                .unwrap()
                .mtime_ms,
            second
        );

        // A restarted server writes the same module again: a new time, so
        // the persistent cache compares its content rather than trusting it.
        let restarted = Overlay::new();
        restarted.write(file.as_str(), "export const a = 2;");
        assert_ne!(modified(&restarted), 0);
    }
}
