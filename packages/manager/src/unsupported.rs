//! Explicit unsupported endpoint and lease boundary for unimplemented platforms.

use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Requested source of the account home; platform resolution is not implemented.
#[derive(Debug, Clone, Copy)]
pub enum Home {
    Account,
    Environment,
}

/// Platform-neutral root handle. Constructors refuse until an adapter exists.
#[derive(Debug, Clone)]
pub struct Root {
    path: PathBuf,
}
impl Root {
    /// Resolve a per-user root.
    ///
    /// # Errors
    /// Always returns `Unsupported` on this platform.
    pub fn for_user(_relative: &Path, _home: Home) -> Result<Self> {
        Err(Error::Unsupported)
    }
    /// Select an explicit root.
    ///
    /// # Errors
    /// Always returns `Unsupported` on this platform.
    pub fn explicit(_path: impl AsRef<Path>) -> Result<Self> {
        Err(Error::Unsupported)
    }
    /// Read the selected root without I/O.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Prepare an owner-private root.
    ///
    /// # Errors
    /// Always returns `Unsupported` on this platform.
    pub fn prepare(&self) -> Result<()> {
        Err(Error::Unsupported)
    }
    /// Inspect kernel lease ownership.
    ///
    /// # Errors
    /// Always returns `Unsupported` on this platform.
    pub fn leased(&self) -> Result<bool> {
        Err(Error::Unsupported)
    }
}

/// A future platform lease guard; no pathname or PID is treated as authority.
pub struct Lease {
    _private: (),
}
impl Lease {
    /// Acquire a kernel-owned exclusive lease.
    ///
    /// # Errors
    /// Always returns `Unsupported` until the platform lock adapter exists.
    pub fn acquire(_root: &Root, _wait: std::time::Duration) -> Result<Self> {
        Err(Error::Unsupported)
    }
}
