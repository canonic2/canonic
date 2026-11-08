//! Crash-safe writer ownership with exclusion of older marker-only writers.
use super::{Checkpoint, Error, private, writer_running};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

pub(super) struct Lock {
    file: File,
    marker: Option<PathBuf>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Keep the kernel lock held until marker removal is complete. Its
        // persistent inode is never unlinked, including on error or unwind.
        if let Some(path) = self.marker.take() {
            let _ = fs::remove_dir(path);
        }
    }
}

impl Lock {
    pub(super) fn release(mut self) -> io::Result<()> {
        if let Some(path) = &self.marker {
            fs::remove_dir(path)?;
        }
        self.marker = None;
        self.file.unlock()
    }
}

pub(super) fn acquire(directory: &Path, checkpoint: Checkpoint<'_>) -> Result<Lock, Error> {
    checkpoint()?;
    let path = directory.join("writer.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    private(&mut options);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(&path)?;
    checked_file(&path, &file)?;
    let marker = directory.join("state.lock");
    for _ in 0..20 {
        checkpoint()?;
        match file.try_lock() {
            Ok(()) => {
                checked_file(&path, &file)?;
                if enter_marker(&marker)? {
                    return Ok(Lock {
                        file,
                        marker: Some(marker),
                    });
                }
                file.unlock()?;
            }
            Err(fs::TryLockError::WouldBlock) => {}
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(Error::Locked)
}

/// A directory atomically distinguishes new writers from old PID files, even
/// if a process dies immediately after creating its marker. Older binaries'
/// create-new file operation also refuses an existing directory. All new
/// writers hold writer.lock, so a leftover directory can be reused immediately.
fn enter_marker(path: &Path) -> Result<bool, Error> {
    let builder = &mut fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => return Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(Error::Invalid("authority marker cannot be a link"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0400 != 0 {
            return Err(Error::Invalid("authority marker cannot be a reparse point"));
        }
    }
    if metadata.is_dir() {
        if fs::read_dir(path)?.next().transpose()?.is_some() {
            return Err(Error::Invalid("authority marker directory must be empty"));
        }
        return Ok(true);
    }
    // A legacy writer holds no kernel lease. Only a bounded, valid PID whose
    // exit can be established permits reclamation. Unknown ownership stays busy.
    if !metadata.is_file() || metadata.len() > 32 {
        return Ok(false);
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut text = String::new();
    (&mut file).take(33).read_to_string(&mut text)?;
    let Some(pid) = text
        .strip_suffix('\n')
        .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|text| text.parse::<u32>().ok())
        .filter(|pid| *pid != 0)
    else {
        return Ok(false);
    };
    if writer_running(pid) {
        return Ok(false);
    }
    checked_file(path, &file)?;
    // Windows must close the old handle before deletion and creation of the
    // directory at this name; otherwise the file can remain delete-pending.
    drop(file);
    // No legacy writer can acquire the path while this dead owner's marker
    // exists. If one wins the subsequent create race, leave its marker alone.
    fs::remove_file(path)?;
    match builder.create(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Lock files are private regular files with one link in the owned review directory.
/// Check the held handle against the namespace before trusting its ownership.
fn checked_file(path: &Path, file: &File) -> Result<(), Error> {
    let current = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    if !current.is_file() || current.file_type().is_symlink() || !held.is_file() {
        return Err(Error::Invalid("authority lock must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = fs::metadata(
            path.parent()
                .ok_or(Error::Invalid("authority lock parent"))?,
        )?;
        if (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.mode() & 0o077 != 0
            || held.uid() != parent.uid()
        {
            return Err(Error::Invalid(
                "authority lock must be private and unchanged",
            ));
        }
    }
    #[cfg(windows)]
    {
        crate::windows_security::check_private(path)?;
        let current = crate::windows_security::read_regular(path)?;
        let current = winapi_util::file::information(&current)?;
        let held = winapi_util::file::information(file)?;
        if held.number_of_links() != 1
            || (current.volume_serial_number(), current.file_index())
                != (held.volume_serial_number(), held.file_index())
        {
            return Err(Error::Invalid(
                "authority lock must be private and unchanged",
            ));
        }
    }
    Ok(())
}
