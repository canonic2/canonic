//! Process-shared FIFO admission for the project writer lock.
use super::{Error, checked_file, open_file};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

const ADMISSION: &str = "admission.lock";

pub(super) struct Queue(PathBuf);

pub(super) struct Admission<'a> {
    queue: &'a Queue,
    _lock: File,
    first: Option<u64>,
    last: Option<u64>,
}

pub(super) struct Ticket {
    number: u64,
    // A process exit or cancelled request releases its place in the queue.
    lease: File,
}

impl Ticket {
    pub(super) fn number(&self) -> u64 {
        self.number
    }
}

impl Queue {
    pub(super) fn open(directory: &Path) -> Result<Self, Error> {
        let path = directory.join("writer-queue");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(Error::Invalid("writer queue must be a directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                super::super::private_directory(&path)?;
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(windows)]
        crate::windows_security::check_private(&path)?;
        Ok(Self(path))
    }

    pub(super) fn try_admit(
        &self,
        ticket: Option<&Ticket>,
    ) -> Result<Option<Admission<'_>>, Error> {
        let path = self.0.join(ADMISSION);
        let lock = open_file(&path, false)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        checked_file(&path, &lock)?;
        if let Some(ticket) = ticket {
            // An operator repair or move can invalidate a ticket's path while
            // its original inode remains locked. Never wait for an absent turn.
            checked_file(&self.0.join(ticket.number.to_string()), &ticket.lease)?;
        }
        let mut first: Option<u64> = None;
        let mut last: Option<u64> = None;
        for entry in fs::read_dir(&self.0)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(number) = name.to_str().and_then(|name| {
                name.parse::<u64>()
                    .ok()
                    .filter(|number| number.to_string() == name)
            }) else {
                // Only canonical numeric names belong to this queue.
                continue;
            };
            let path = entry.path();
            let lease = super::super::objects::read_file(&path)?;
            match lease.try_lock() {
                Ok(()) => {
                    drop(lease);
                    fs::remove_file(path)?;
                }
                Err(fs::TryLockError::WouldBlock) => {
                    first = Some(first.map_or(number, |first| first.min(number)));
                    last = Some(last.map_or(number, |last| last.max(number)));
                }
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        Ok(Some(Admission {
            queue: self,
            _lock: lock,
            first,
            last,
        }))
    }
}

impl Admission<'_> {
    pub(super) fn first(&self) -> Option<u64> {
        self.first
    }

    pub(super) fn join(&self) -> Result<Ticket, Error> {
        let number = self
            .last
            .map_or(Some(0), |last| last.checked_add(1))
            .ok_or(Error::Invalid("writer queue sequence exhausted"))?;
        let lease = open_file(&self.queue.0.join(number.to_string()), true)?;
        lease.lock()?;
        Ok(Ticket { number, lease })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stray_writer_queue_files_do_not_block_or_get_reclaimed() {
        let root = assert_fs::TempDir::new().unwrap();
        let queue = Queue::open(root.path()).unwrap();
        let stray = queue.0.join(".DS_Store");
        fs::write(&stray, "unowned data").unwrap();
        super::super::try_acquire(root.path())
            .unwrap()
            .release()
            .unwrap();
        assert_eq!(fs::read_to_string(stray).unwrap(), "unowned data");
    }

    #[cfg(unix)]
    #[test]
    fn replaced_writer_ticket_reports_invalid_state() {
        let root = assert_fs::TempDir::new().unwrap();
        let queue = Queue::open(root.path()).unwrap();
        let ticket = queue.try_admit(None).unwrap().unwrap().join().unwrap();
        let path = queue.0.join(ticket.number.to_string());
        fs::rename(&path, root.path().join("replacement-source")).unwrap();
        let _replacement = open_file(&path, true).unwrap();
        assert!(matches!(
            queue.try_admit(Some(&ticket)),
            Err(Error::Invalid(
                "authority lock must be private and unchanged"
            ))
        ));
    }
}
