//! Bounded reads, private directories and atomic private writes shared by the
//! filesystem adapters.
//!
//! Every file the library writes is private to the user on Unix (0600, in
//! directories it creates as 0700) and appears complete or not at all.

use std::{
    fs::{self, File, FileType},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Outcome of a bounded read of a path expected to be a regular file.
pub(crate) enum Bounded {
    /// The whole file, within the limit.
    Bytes(Vec<u8>),
    /// The path names something other than a regular file.
    NotFile,
    /// The file is longer than the limit.
    TooLarge,
}

/// Reads a whole regular file, refusing anything longer than `max_bytes`.
///
/// Every caller passes a path whose final component it has already checked
/// or canonicalized. On Unix that component is opened without following a
/// link, so a link swapped in after the check is refused as
/// [`Bounded::NotFile`]; directory components are still followed. The file
/// is also opened without blocking, so a FIFO or device in place of the file
/// is refused instead of waiting for a writer.
pub(crate) fn read_bounded(path: &Path, max_bytes: u64) -> io::Result<Bounded> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_NOFOLLOW);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        #[cfg(unix)]
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => return Ok(Bounded::NotFile),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(Bounded::NotFile);
    }
    if metadata.len() > max_bytes {
        return Ok(Bounded::TooLarge);
    }
    let mut bytes = Vec::new();
    // Read one byte past the limit to notice a file that grew after `metadata`.
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Ok(Bounded::TooLarge);
    }
    Ok(Bounded::Bytes(bytes))
}

/// The type of the entry at `path` without following a final symbolic link,
/// or `None` when nothing is there.
pub(crate) fn kind(path: &Path) -> io::Result<Option<FileType>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Creates `path` and missing parents, private to the user on Unix.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// A temporary path in `directory`, unique to this process and call.
pub(crate) fn temporary_path(directory: &Path, stem: &str) -> PathBuf {
    directory.join(format!(
        ".{stem}.{}.{}.tmp",
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Whether a committed replacement is known to survive a crash.
#[derive(Debug)]
pub(crate) enum Commit {
    /// The rename was made durable.
    Durable,
    /// The rename happened, but making it durable failed; the new contents
    /// are in place and may revert to the old ones after a crash.
    NotDurable(io::Error),
}

/// Writes `bytes` to `temporary`, which must not exist, flushes it with
/// [`sync`], renames it over `target` and, on Unix, flushes the directory the
/// same way so the rename survives a crash.
///
/// An `Err` means nothing was committed: `target` is unchanged. If a step
/// before the rename fails after the temporary file was created, that file is
/// removed; an existing file at `temporary` is never touched. Once the rename
/// succeeds the result is `Ok`, reporting whether it is durable; there is no
/// rollback.
pub(crate) fn replace_private(temporary: &Path, target: &Path, bytes: &[u8]) -> io::Result<Commit> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temporary)?;
    let synced = file.write_all(bytes).and_then(|()| sync(&file));
    drop(file);
    let written = synced.and_then(|()| fs::rename(temporary, target));
    if let Err(error) = written {
        // Best effort: the temporary file belongs to this write alone.
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    Ok(match sync_directory(target) {
        Ok(()) => Commit::Durable,
        Err(error) => Commit::NotDurable(error),
    })
}

/// Flushes a file, or a directory opened as one, through the filesystem so a
/// crash cannot lose it or reorder it after a later rename. On macOS this is
/// `fsync`, not the `F_FULLFSYNC` that `File::sync_all` issues there: that
/// also empties the drive's write cache and waits behind every other
/// process's flush. Elsewhere it is `File::sync_all`.
fn sync(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        rustix::fs::fsync(file).map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

/// Syncs the directory holding `path`, making a rename into it durable.
fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        sync(&File::open(directory)?)
    }
    #[cfg(not(unix))]
    {
        // Directories cannot be opened for syncing; the rename itself is atomic.
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{TempDir, prelude::*};

    #[test]
    fn bounded_reads_return_whole_files_or_refuse() {
        let root = TempDir::new().unwrap();
        root.child("file").write_str("acme").unwrap();
        assert!(
            matches!(read_bounded(&root.path().join("file"), 4).unwrap(), Bounded::Bytes(b) if b == b"acme")
        );
        assert!(matches!(
            read_bounded(&root.path().join("file"), 3).unwrap(),
            Bounded::TooLarge
        ));
        assert!(matches!(
            read_bounded(root.path(), 3).unwrap(),
            Bounded::NotFile
        ));
        assert!(read_bounded(&root.path().join("missing"), 3).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_file_is_refused() {
        let root = TempDir::new().unwrap();
        root.child("target").write_str("acme").unwrap();
        let link = root.path().join("file");
        std::os::unix::fs::symlink(root.path().join("target"), &link).unwrap();
        assert!(matches!(
            read_bounded(&link, 1024).unwrap(),
            Bounded::NotFile
        ));
    }

    #[test]
    fn unbounded_limits_do_not_overflow() {
        let root = TempDir::new().unwrap();
        root.child("file").write_str("acme").unwrap();
        assert!(
            matches!(read_bounded(&root.path().join("file"), u64::MAX).unwrap(), Bounded::Bytes(b) if b == b"acme")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_in_place_of_a_file_is_refused_without_blocking() {
        let root = TempDir::new().unwrap();
        let fifo = root.path().join("catalog.json");
        // A blocking open of a FIFO with no writer would never return.
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        assert!(matches!(
            read_bounded(&fifo, 1024).unwrap(),
            Bounded::NotFile
        ));
    }

    #[test]
    fn private_writes_replace_atomically_and_clean_up() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("a/b");
        create_private_dir(&dir).unwrap();
        let target = dir.join("file");
        assert!(matches!(
            replace_private(&temporary_path(&dir, "file"), &target, b"one").unwrap(),
            Commit::Durable
        ));
        replace_private(&temporary_path(&dir, "file"), &target, b"two").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary files remain"
        );
        let occupied = dir.join("occupied");
        fs::write(&occupied, "x").unwrap();
        assert!(replace_private(&occupied, &target, b"three").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert_eq!(
            fs::read(&occupied).unwrap(),
            b"x",
            "an existing file is never removed"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&target), 0o600);
        }
    }

    #[test]
    fn kinds_distinguish_missing_entries_and_links() {
        let root = TempDir::new().unwrap();
        assert!(kind(&root.path().join("missing")).unwrap().is_none());
        assert!(kind(root.path()).unwrap().unwrap().is_dir());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path(), root.path().join("link")).unwrap();
            assert!(
                kind(&root.path().join("link"))
                    .unwrap()
                    .unwrap()
                    .is_symlink()
            );
        }
    }
}
