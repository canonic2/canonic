//! Bounded object-directory admission and kernel-held reader pins.
mod checkpoint;
use super::{Error, private};
pub(in crate::review) use checkpoint::Checkpoint;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(in crate::review) const BYTE_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
pub(in crate::review) const RESERVE: u64 = 1024 * 1024 * 1024;
const FILE_LIMIT: usize = 4096;
fn full(kind: crate::review::CapacityKind, needed: usize, limit: usize) -> Error {
    Error::Capacity {
        kind,
        pending: 0,
        retained: needed,
        limit,
    }
}
pub(in crate::review) type Pins = Arc<Vec<File>>;
type SweepRow = (std::ffi::OsString, PathBuf, u64, Option<File>);

pub(in crate::review) struct Directory {
    path: PathBuf,
    _guard: File,
    bytes: u64,
    files: usize,
}

fn open(path: &Path, write: bool, create: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
        .create(create)
        .truncate(false);
    private(&mut options);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            nix::fcntl::OFlag::O_NOFOLLOW.bits() | nix::fcntl::OFlag::O_NONBLOCK.bits(),
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    checked(path, &file)?;
    Ok(file)
}
pub(in crate::review) fn read_file(path: &Path) -> Result<File, Error> {
    open(path, false, false)
}

/// Bounded, read-only diagnostics; unsafe neighbors are never followed or fixed.
pub(in crate::review) fn anomalies(path: &Path) -> Result<Vec<PathBuf>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(vec![path.into()]);
    }
    let mut problems = Vec::new();
    for entry in fs::read_dir(path)?.take(FILE_LIMIT + 1) {
        let path = entry?.path();
        if let Err(error) = read_file(&path)
            && !matches!(&error, Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            problems.push(path);
        }
        if problems.len() == 64 {
            break;
        }
    }
    Ok(problems)
}

/// Exact comparison with fixed working memory and no link-following open.
pub(in crate::review) fn same_bytes(path: &Path, bytes: &[u8]) -> Result<bool, Error> {
    let mut file = match read_file(path) {
        Ok(file) => file,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() != bytes.len() as u64 {
        return Ok(false);
    }
    let mut buffer = vec![0_u8; 64 * 1024];
    for chunk in bytes.chunks(buffer.len()) {
        file.read_exact(&mut buffer[..chunk.len()])?;
        if &buffer[..chunk.len()] != chunk {
            return Ok(false);
        }
    }
    Ok(true)
}

fn checked(path: &Path, file: &File) -> Result<(), Error> {
    let current = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    if !current.is_file() || current.file_type().is_symlink() || !held.is_file() {
        return Err(Error::Invalid(
            "object storage must contain owned regular files",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = fs::metadata(path.parent().ok_or(Error::Invalid("object parent"))?)?;
        if (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.uid() != parent.uid()
            || held.mode() & 0o077 != 0
        {
            return Err(Error::Invalid(
                "object ownership, privacy or identity differs",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if held.file_attributes() & 0x0400 != 0 {
            return Err(Error::Invalid("object cannot be a reparse point"));
        }
        crate::windows_security::check_private_file(file)?;
        let other = crate::windows_security::read_regular(path)?;
        let a = winapi_util::file::information(file)?;
        let b = winapi_util::file::information(&other)?;
        if a.number_of_links() != 1
            || (a.volume_serial_number(), a.file_index())
                != (b.volume_serial_number(), b.file_index())
        {
            return Err(Error::Invalid("object identity differs"));
        }
    }
    Ok(())
}

impl Directory {
    /// Callers verify/create the private directory before entering. The stable
    /// mutex serializes publication and admission, never ordinary evaluation.
    pub(in crate::review) fn enter(path: &Path) -> Result<Self, Error> {
        Self::enter_attempts(path, 20)
    }

    pub(in crate::review) fn try_enter(path: &Path) -> Result<Self, Error> {
        Self::enter_attempts(path, 1)
    }

    fn enter_attempts(path: &Path, attempts: usize) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("object directory cannot be a link"));
        }
        let guard = open(&path.join(".objects.lock"), true, true)?;
        let mut acquired = false;
        for _ in 0..attempts {
            match guard.try_lock() {
                Ok(()) => {
                    acquired = true;
                    break;
                }
                Err(fs::TryLockError::WouldBlock) => {
                    if attempts > 1 {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                }
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        if !acquired {
            return Err(Error::Locked);
        }
        checked(&path.join(".objects.lock"), &guard)?;
        Ok(Self {
            path: path.into(),
            _guard: guard,
            bytes: 0,
            files: 0,
        })
    }

    /// Scan at most `FILE_LIMIT+1` entries. Unknown/private files count toward
    /// admission but are never selected for deletion by an unrecognized name.
    #[cfg(test)]
    pub(in crate::review) fn sweep(
        &mut self,
        keep: &BTreeSet<String>,
        removable: impl Fn(&str) -> bool,
    ) -> Result<(), Error> {
        self.sweep_refresh(keep, removable, |_| Ok(()))
    }

    pub(in crate::review) fn sweep_refresh(
        &mut self,
        keep: &BTreeSet<String>,
        removable: impl Fn(&str) -> bool,
        mut refresh: impl FnMut(&mut BTreeSet<String>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        crate::diagnostics::trace::count("objects.sweeps", 1);
        let mut keep = keep.clone();
        for _ in 0..4 {
            if self.sweep_batch(&mut keep, &removable, &mut refresh)? {
                return Ok(());
            }
        }
        // Admission never uses a partial inventory. Known garbage was reclaimed
        // in bounded batches; retry can continue without manual deletion.
        Err(Error::CleanupPending)
    }

    fn sweep_batch(
        &mut self,
        keep: &mut BTreeSet<String>,
        removable: &impl Fn(&str) -> bool,
        refresh: &mut impl FnMut(&mut BTreeSet<String>) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let mut rows = Vec::new();
        for entry in fs::read_dir(&self.path)?.take(FILE_LIMIT + 1) {
            let entry = entry?;
            rows.push(entry);
        }
        let overflow = rows.len() > FILE_LIMIT;
        let mut candidates = Vec::with_capacity(64);
        let mut removed = false;
        self.bytes = 0;
        self.files = 0;
        for row in rows {
            let name = row.file_name();
            let path = row.path();
            let metadata = fs::symlink_metadata(&path)?;
            let delete = name.to_str().is_some_and(|name| {
                name != ".objects.lock" && !keep.contains(name) && removable(name)
            });
            let exclusive = if delete && metadata.is_file() && !metadata.file_type().is_symlink() {
                // An unsafe neighbor counts toward admission, but never becomes
                // a deletion candidate or prevents publication of another name.
                let file = open(&path, true, false).ok();
                file.filter(|file| file.try_lock().is_ok())
            } else {
                None
            };
            candidates.push((name, path, metadata.len(), exclusive));
            if candidates.len() == 64 {
                removed |= self.reclaim(&mut candidates, keep, refresh)?;
            }
        }
        removed |= self.reclaim(&mut candidates, keep, refresh)?;
        if overflow && !removed {
            return Err(full(
                crate::review::CapacityKind::StoredEntries,
                self.files,
                FILE_LIMIT,
            ));
        }
        Ok(!overflow)
    }

    fn reclaim(
        &mut self,
        candidates: &mut Vec<SweepRow>,
        keep: &mut BTreeSet<String>,
        refresh: &mut impl FnMut(&mut BTreeSet<String>) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        // A publisher may commit and release its reader pin while we acquire
        // these locks. Refresh durable roots only after every deletion candidate
        // is exclusively held: publication cannot acquire a new pin on them.
        if candidates.iter().any(|(_, _, _, file)| file.is_some()) {
            refresh(keep)?;
        }
        let mut removed = false;
        for (name, path, bytes, exclusive) in candidates.drain(..) {
            if let Some(file) = exclusive
                && name.to_str().is_some_and(|name| !keep.contains(name))
                && checked(&path, &file).is_ok()
                && fs::remove_file(&path).is_ok()
            {
                removed = true;
                continue;
            }
            self.bytes = self
                .bytes
                .checked_add(bytes)
                .ok_or(crate::policy::Error::Limit)?;
            self.files += 1;
        }
        Ok(removed)
    }

    pub(in crate::review) fn admit(
        &self,
        bytes: u64,
        files: usize,
        recovery: bool,
    ) -> Result<(), Error> {
        let limit = BYTE_LIMIT - if recovery { 0 } else { RESERVE };
        let needed = self.bytes.saturating_add(bytes);
        if needed > limit {
            return Err(full(
                crate::review::CapacityKind::StoredBytes,
                usize::try_from(needed).unwrap_or(usize::MAX),
                usize::try_from(limit).unwrap_or(usize::MAX),
            ));
        }
        let limit = FILE_LIMIT - if recovery { 0 } else { 8 };
        let needed = self.files.saturating_add(files);
        if needed > limit {
            return Err(full(
                crate::review::CapacityKind::StoredEntries,
                needed,
                limit,
            ));
        }
        Ok(())
    }

    pub(in crate::review) fn added(&mut self, bytes: u64) {
        self.bytes += bytes;
        self.files += 1;
    }

    pub(in crate::review) fn pins(&self, names: &BTreeSet<String>) -> Result<Pins, Error> {
        let mut files = Vec::with_capacity(names.len());
        for name in names {
            if Path::new(name).components().count() != 1 {
                return Err(Error::Invalid("object pin name"));
            }
            let path = self.path.join(name);
            let file = open(&path, false, false)?;
            file.try_lock_shared().map_err(|_| Error::Locked)?;
            checked(&path, &file)?;
            files.push(file);
        }
        Ok(Arc::new(files))
    }

    pub(in crate::review) fn unpinned(&self, name: &str) -> Result<Option<File>, Error> {
        let path = self.path.join(name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let file = open(&path, true, false)?;
        file.try_lock().map_err(|_| Error::Locked)?;
        checked(&path, &file)?;
        Ok(Some(file))
    }
}

#[cfg(test)]
mod tests;
