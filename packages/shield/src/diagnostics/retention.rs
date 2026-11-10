//! Startup-only admission for optional logs. Live writers reserve their full
//! per-process allowance; kernel leases distinguish them from completed logs.
use super::LIMIT;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const FILES: usize = 32;
const BYTES: u64 = FILES as u64 * LIMIT;
const DIRECTORY_ENTRIES: usize = 4096;

fn unavailable() -> io::Error {
    io::Error::other("diagnostic retention capacity unavailable")
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            nix::fcntl::OFlag::O_NOFOLLOW.bits() | nix::fcntl::OFlag::O_NONBLOCK.bits(),
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(7);
    }
    options
}

fn checked(path: &Path, file: &File) -> io::Result<()> {
    let current = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    if !current.is_file() || current.file_type().is_symlink() || !held.is_file() {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.uid() != rustix::process::geteuid().as_raw()
            || held.mode() & 0o077 != 0
        {
            return Err(unavailable());
        }
    }
    #[cfg(windows)]
    {
        let a = winapi_util::file::information(file)?;
        let other = crate::windows_security::inspect_regular(path)?;
        let b = winapi_util::file::information(&other)?;
        if a.number_of_links() != 1
            || (a.volume_serial_number(), a.file_index())
                != (b.volume_serial_number(), b.file_index())
        {
            return Err(unavailable());
        }
        crate::windows_security::check_private_file(file)?;
    }
    Ok(())
}

fn owned_name(name: &str) -> bool {
    name.strip_prefix("shield-")
        .and_then(|name| name.strip_suffix(".jsonl"))
        .and_then(|name| name.split_once('-'))
        .is_some_and(|(time, pid)| {
            !time.is_empty()
                && !pid.is_empty()
                && time.bytes().all(|b| b.is_ascii_digit())
                && pid.bytes().all(|b| b.is_ascii_digit())
        })
}

pub(super) fn create(directory: &Path) -> io::Result<(PathBuf, File)> {
    create_with_limits(directory, FILES, BYTES)
}

fn create_with_limits(
    directory: &Path,
    max_files: usize,
    max_bytes: u64,
) -> io::Result<(PathBuf, File)> {
    let lock_path = directory.join(".shield-log.lock");
    let lock = match options().create_new(true).open(&lock_path) {
        Ok(file) => {
            #[cfg(windows)]
            crate::windows_security::make_private_file(&file)?;
            file
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => options().open(&lock_path)?,
        Err(error) => return Err(error),
    };
    checked(&lock_path, &lock)?;
    lock.try_lock().map_err(|_| unavailable())?;
    checked(&lock_path, &lock)?;
    let mut files = 0usize;
    let mut bytes = LIMIT;
    let mut completed = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= DIRECTORY_ENTRIES {
            return Err(unavailable());
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| owned_name(name)) else {
            continue;
        };
        let path = entry.path();
        // Unrecognized or unsafe files are never cleanup candidates.
        let file = options().open(&path)?;
        checked(&path, &file)?;
        files += 1;
        match file.try_lock() {
            Ok(()) => {
                let len = file.metadata()?.len();
                bytes = bytes.saturating_add(len);
                let timestamp = name[7..].split('-').next().unwrap_or_default();
                completed.push((
                    timestamp.parse::<u128>().map_err(io::Error::other)?,
                    path,
                    file,
                    len,
                ));
            }
            Err(TryLockError::WouldBlock) => {
                bytes = bytes.saturating_add(LIMIT.max(file.metadata()?.len()));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    completed.sort_by_key(|(timestamp, ..)| *timestamp);
    for (_, path, file, len) in completed {
        if files < max_files && bytes <= max_bytes {
            break;
        }
        checked(&path, &file)?;
        fs::remove_file(path)?;
        files -= 1;
        bytes -= len;
    }
    if files >= max_files || bytes > max_bytes {
        return Err(unavailable());
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = directory.join(format!("shield-{timestamp}-{}.jsonl", std::process::id()));
    let file = options().create_new(true).open(&path)?;
    #[cfg(windows)]
    crate::windows_security::make_private_file(&file)?;
    file.try_lock().map_err(|_| unavailable())?;
    checked(&path, &file)?;
    Ok((path, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_preserves_active_logs_and_reclaims_oldest_completed_logs() {
        let root = assert_fs::TempDir::new().unwrap();
        let (old, old_file) = create_with_limits(root.path(), 2, 2 * LIMIT).unwrap();
        drop(old_file);
        let (live, live_file) = create_with_limits(root.path(), 2, 2 * LIMIT).unwrap();
        let (new, _new_file) = create_with_limits(root.path(), 2, 2 * LIMIT).unwrap();
        assert!(!old.exists());
        assert!(live.exists());
        assert!(new.exists());
        assert!(create_with_limits(root.path(), 2, 2 * LIMIT).is_err());
        assert!(live.exists());
        drop(live_file);
        assert!(create_with_limits(root.path(), 2, 2 * LIMIT).is_ok());
    }

    #[test]
    fn admission_accounts_bytes_and_preserves_foreign_files() {
        let root = assert_fs::TempDir::new().unwrap();
        let foreign = root.path().join("unrelated.jsonl");
        fs::write(&foreign, "preserve").unwrap();
        let (old, file) = create_with_limits(root.path(), 8, LIMIT).unwrap();
        file.set_len(1).unwrap();
        drop(file);
        let (_new, _file) = create_with_limits(root.path(), 8, LIMIT).unwrap();
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(foreign).unwrap(), "preserve");
    }

    #[test]
    fn busy_directory_admission_stops_without_waiting_or_changing_live_logs() {
        let root = assert_fs::TempDir::new().unwrap();
        let (path, _file) = create(root.path()).unwrap();
        let lock = options()
            .open(root.path().join(".shield-log.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(create(root.path()).is_err());
        assert!(path.exists());
        drop(lock);
        assert!(create(root.path()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn private_log_in_shared_parent_is_owned_by_the_process_user() {
        use std::os::unix::fs::MetadataExt as _;
        let root = assert_fs::TempDir::new().unwrap();
        let path = root.path().parent().unwrap().join(format!(
            "shield-retention-fixture-{}",
            root.path().file_name().unwrap().to_str().unwrap()
        ));
        let file = options().create_new(true).open(&path).unwrap();
        assert_eq!(
            file.metadata().unwrap().uid(),
            rustix::process::geteuid().as_raw()
        );
        let result = checked(&path, &file);
        drop(file);
        fs::remove_file(path).unwrap();
        assert!(
            result.is_ok(),
            "a private log does not inherit its parent's owner"
        );
    }

    #[cfg(unix)]
    #[test]
    fn admission_refuses_symlink_candidates_without_touching_the_target() {
        let root = assert_fs::TempDir::new().unwrap();
        let target = root.path().join("foreign");
        fs::write(&target, "preserve").unwrap();
        std::os::unix::fs::symlink(&target, root.path().join("shield-1-1.jsonl")).unwrap();
        assert!(create(root.path()).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
    }
}
