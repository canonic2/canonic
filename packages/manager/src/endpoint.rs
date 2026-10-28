//! Unix private roots, peer credentials, kernel leases and identity-owned sockets.

use crate::{Error, Result};
use nix::unistd::{User, geteuid};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

/// Where a default per-user root comes from.
#[derive(Debug, Clone, Copy)]
pub enum Home {
    Account,
    Environment,
}

/// Resolved root and fixed ordinary/control endpoint names. Resolution does no writes.
#[derive(Debug, Clone)]
pub struct Root {
    path: PathBuf,
}

impl Root {
    /// Resolve a default root beneath the account home, or explicitly opt into `HOME`.
    ///
    /// # Errors
    /// Returns identity lookup, missing-home, invalid relative path or path-length errors.
    pub fn for_user(relative: &Path, home: Home) -> Result<Self> {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::PrivatePath);
        }
        let home = match home {
            Home::Account => User::from_uid(geteuid())?.ok_or(Error::PrivatePath)?.dir,
            Home::Environment => std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or(Error::PrivatePath)?,
        };
        Self::explicit(home.join(relative))
    }

    /// Select an independent explicit root. Relative paths become absolute.
    ///
    /// # Errors
    /// Returns I/O or path-length errors. Parent components and symlinks are refused.
    pub fn explicit(path: impl AsRef<Path>) -> Result<Self> {
        let path = std::path::absolute(path)?;
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::PrivatePath);
        }
        // Resolve existing parent aliases such as the system's /tmp alias, but
        // leave the root itself for no-symlink validation during prepare.
        let mut resolved = path.clone();
        if let Some(parent) = path.parent() {
            for ancestor in parent.ancestors() {
                match fs::canonicalize(ancestor) {
                    Ok(base) => {
                        resolved = base.join(
                            path.strip_prefix(ancestor)
                                .map_err(|_| Error::PrivatePath)?,
                        );
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let path = resolved;
        #[cfg(target_os = "linux")]
        let limit = 108;
        #[cfg(not(target_os = "linux"))]
        let limit = 104;
        if path
            .join("manager.sock")
            .as_os_str()
            .as_encoded_bytes()
            .len()
            >= limit
        {
            return Err(Error::PathTooLong);
        }
        Ok(Self { path })
    }
    /// Root path, without creating it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Ordinary or reserved control socket path.
    #[must_use]
    pub fn socket(&self, control: bool) -> PathBuf {
        self.path.join(if control {
            "control.sock"
        } else {
            "manager.sock"
        })
    }

    /// Create missing directories with 0700 and validate the final private root.
    ///
    /// # Errors
    /// Returns I/O or `PrivatePath` for symlinks, foreign or permissive roots.
    pub fn prepare(&self) -> Result<()> {
        let mut path = PathBuf::new();
        for component in self.path.components() {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(Error::PrivatePath),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::DirBuilder::new().mode(0o700).create(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e.into()),
                    }
                    private(&path, false)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        private(&self.path, false)?;
        Ok(())
    }

    /// Inspect without creating a directory or lock file.
    ///
    /// # Errors
    /// Returns validation or I/O errors; false means no kernel lease holder.
    pub fn leased(&self) -> Result<bool> {
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(self.path.join("manager.lock"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        private_file(&file)?;
        match file.try_lock() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

fn private(path: &Path, socket: bool) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    if meta.uid() != geteuid().as_raw()
        || meta.mode() & 0o077 != 0
        || if socket {
            !meta.file_type().is_socket()
        } else {
            !meta.is_dir()
        }
    {
        return Err(Error::PrivatePath);
    }
    Ok(meta)
}
fn private_file(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != geteuid().as_raw()
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err(Error::PrivatePath);
    }
    Ok(())
}

/// Check a kernel-reported UID, also usable by platform adapters and forged-peer tests.
///
/// # Errors
/// Returns `Peer` unless the IDs are equal.
pub fn check_uid(peer: u32, owner: u32) -> Result<()> {
    if peer == owner {
        Ok(())
    } else {
        Err(Error::Peer)
    }
}

/// Verify every accepted or connected stream before reading a frame.
///
/// # Errors
/// Returns `Peer` or the credential system-call error.
pub fn check_peer(stream: &UnixStream) -> Result<()> {
    #[cfg(target_os = "linux")]
    let uid =
        nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)?.uid();
    #[cfg(not(target_os = "linux"))]
    let uid = nix::unistd::getpeereid(stream)?.0.as_raw();
    check_uid(uid, geteuid().as_raw())
}

/// Kernel-owned exclusive lock. The anchor is never unlinked or rewritten.
pub struct Lease {
    root: Root,
    file: File,
    identity: (u64, u64),
    bound: [std::sync::atomic::AtomicBool; 2],
}
impl Lease {
    /// Wait with capped exponential backoff for an exiting owner.
    ///
    /// # Errors
    /// Returns `AlreadyRunning` at the deadline; validation and I/O errors remain distinct.
    pub fn acquire(root: &Root, wait: Duration) -> Result<Self> {
        root.prepare()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(root.path.join("manager.lock"))?;
        private_file(&file)?;
        let meta = private(root.path(), false)?;
        let identity = (meta.dev(), meta.ino());
        let deadline = Instant::now() + wait;
        let mut delay = Duration::from_millis(2);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        root: root.clone(),
                        file,
                        identity,
                        bound: [const { std::sync::atomic::AtomicBool::new(false) }; 2],
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(
                        delay.min(deadline.saturating_duration_since(Instant::now())),
                    );
                    delay = (delay * 2).min(Duration::from_millis(100));
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(Error::AlreadyRunning),
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
    fn verify(&self) -> Result<()> {
        let meta = private(self.root.path(), false)?;
        let anchor = fs::symlink_metadata(self.root.path.join("manager.lock"))?;
        let file = self.file.metadata()?;
        if (meta.dev(), meta.ino()) != self.identity
            || (anchor.dev(), anchor.ino()) != (file.dev(), file.ino())
        {
            return Err(Error::PrivatePath);
        }
        Ok(())
    }

    /// Bind only while this lease is alive. Replace an owned stale socket only.
    ///
    /// # Errors
    /// Returns validation, socket or I/O errors. Non-sockets are preserved.
    pub fn bind(&self, control: bool) -> Result<Endpoint<'_>> {
        let slot = &self.bound[usize::from(control)];
        if slot.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::EndpointBound);
        }
        let result = self.bind_once(control);
        if result.is_err() {
            slot.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }

    fn bind_once(&self, control: bool) -> Result<Endpoint<'_>> {
        self.verify()?;
        let path = self.root.socket(control);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                private(&path, true)?;
                fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let meta = private(&path, true)?;
        listener.set_nonblocking(true)?;
        Ok(Endpoint {
            lease: self,
            listener,
            path,
            identity: (meta.dev(), meta.ino()),
            control,
        })
    }
}

/// A listener borrowing its lease, so the socket cannot outlive kernel exclusion.
pub struct Endpoint<'a> {
    lease: &'a Lease,
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
    control: bool,
}
impl Endpoint<'_> {
    /// Access the nonblocking listener for an external runtime adapter.
    #[must_use]
    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }
    /// Remove only the socket identity bound here, while the lease is still held.
    ///
    /// # Errors
    /// Returns root/lease replacement or I/O errors. Foreign replacements are preserved.
    pub fn unlink(&self) -> Result<()> {
        self.lease.verify()?;
        match fs::symlink_metadata(&self.path) {
            Ok(meta)
                if (meta.dev(), meta.ino()) == self.identity && meta.file_type().is_socket() =>
            {
                fs::remove_file(&self.path)?;
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
}
impl Drop for Endpoint<'_> {
    fn drop(&mut self) {
        let _ = self.unlink();
        self.lease.bound[usize::from(self.control)]
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Connect without creating any files, validating both pathname and kernel peer.
///
/// # Errors
/// Returns `Absent`, `Starting`, private-path, credential or transport errors.
pub fn connect(root: &Root, control: bool, timeout: Duration) -> Result<UnixStream> {
    match private(root.path(), false) {
        Ok(_) => {}
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Err(Error::Absent),
        Err(e) => return Err(e),
    }
    match private(&root.socket(control), true) {
        Ok(_) => {}
        Err(Error::PrivatePath) => {
            // bind publishes an owned socket before chmod restricts its mode.
            // A live lease proves startup may still be preparing that endpoint.
            // Do not connect until a later probe validates private permissions.
            let metadata = fs::symlink_metadata(root.socket(control))?;
            if metadata.file_type().is_socket()
                && metadata.uid() == geteuid().as_raw()
                && root.leased()?
            {
                return Err(Error::Starting);
            }
            return Err(Error::PrivatePath);
        }
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
            return Err(if root.leased()? {
                Error::Starting
            } else {
                Error::Absent
            });
        }
        Err(e) => return Err(e),
    }
    let stream = match connect_bounded(&root.socket(control), timeout) {
        Ok(stream) => stream,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return Err(Error::Capacity);
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            return Err(if root.leased()? {
                Error::Starting
            } else {
                Error::Absent
            });
        }
        Err(e) => return Err(e.into()),
    };
    check_peer(&stream)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

fn connect_bounded(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    use nix::{
        poll::{PollFd, PollFlags, PollTimeout, poll},
        sys::socket::{
            AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket,
            sockopt::SocketError,
        },
    };
    use std::os::fd::{AsFd, AsRawFd};
    if timeout.is_zero() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )?;
    nix::fcntl::fcntl(
        &fd,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    )?;
    nix::fcntl::fcntl(
        &fd,
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
    )?;
    let address = UnixAddr::new(path)?;
    match connect(fd.as_raw_fd(), &address) {
        Ok(()) => {}
        Err(nix::errno::Errno::EINPROGRESS) => {
            let mut descriptors = [PollFd::new(fd.as_fd(), PollFlags::POLLOUT)];
            if poll(
                &mut descriptors,
                PollTimeout::try_from(timeout).unwrap_or(PollTimeout::MAX),
            )? == 0
            {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let error = getsockopt(&fd, SocketError)?;
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
        }
        Err(e) => return Err(e.into()),
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}
