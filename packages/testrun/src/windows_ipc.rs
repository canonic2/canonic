//! Ephemeral roots for Windows private side-channel listeners.
use canonic_process_kit::{
    Error, Result,
    endpoint::{Lease, Root, RootIdentity},
};
use std::{fs, io, path::PathBuf, time::Duration};

/// Bound Windows nonblocking side-channel retries without a fixed-rate idle poll.
#[derive(Default)]
pub(crate) struct IdleBackoff(u64);

impl IdleBackoff {
    pub(crate) fn next_delay(&mut self) -> Duration {
        let delay = self.0.max(10);
        self.0 = (delay * 2).min(100);
        Duration::from_millis(delay)
    }

    pub(crate) fn reset(&mut self) {
        self.0 = 0;
    }
}

pub(crate) struct ScratchRoot {
    root: Root,
    identity: RootIdentity,
}

impl ScratchRoot {
    pub(crate) fn create() -> Result<Self> {
        let name = format!("tr-{}", uuid::Uuid::new_v4().simple());
        let root = Root::explicit(std::env::temp_dir().join(name))?;
        let identity = root.create()?;
        Ok(Self { root, identity })
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.root.socket(false)
    }

    pub(crate) fn acquire(&self) -> Result<Lease> {
        Lease::acquire(&self.root, Duration::ZERO)
    }

    pub(crate) fn close(&self) -> Result<()> {
        let path = self.root.path();
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if self.root.identity()? != self.identity {
            return Err(Error::PrivatePath);
        }
        // Only the original lease owner unlinks the socket. If it was
        // replaced or is still in use, preserve the root and its anchor.
        match fs::symlink_metadata(self.path()) {
            Ok(_) => return Err(Error::PrivatePath),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let anchor = path.join("manager.lock");
        match fs::symlink_metadata(&anchor) {
            Ok(_) => {
                let lease = self.acquire()?;
                lease.verify()?;
                fs::remove_file(&anchor)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::remove_dir(path)?;
        Ok(())
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
