//! Private roots, peer credentials, kernel leases and identity-owned sockets.
//!
//! Unix checks ownership and modes on every path and verifies peer credentials.
//! Windows roots and peers return `Unsupported` until owner and access-list
//! validation enforce the same private-user boundary.

use crate::{
    Error, Result,
    net::{Listener, Stream},
};
use std::{
    fs::{self, File, OpenOptions},
    io,
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
    /// Returns `Unsupported` on Windows, or identity lookup, missing-home,
    /// invalid relative path or path-length errors.
    pub fn for_user(relative: &Path, home: Home) -> Result<Self> {
        if cfg!(windows) {
            return Err(Error::Unsupported);
        }
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::PrivatePath);
        }
        Self::explicit(home_dir(home)?.join(relative))
    }

    /// Select an independent explicit root. Relative paths become absolute.
    ///
    /// # Errors
    /// Returns `Unsupported` on Windows, or I/O or path-length errors.
    /// Parent components and symlinks are refused.
    pub fn explicit(path: impl AsRef<Path>) -> Result<Self> {
        if cfg!(windows) {
            return Err(Error::Unsupported);
        }
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
        let path = simplify(resolved);
        if path
            .join("manager.sock")
            .as_os_str()
            .as_encoded_bytes()
            .len()
            >= SOCKET_PATH_LIMIT
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
                    match private_dir_builder().create(&path) {
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
        let file = match no_follow()
            .read(true)
            .write(true)
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

#[cfg(target_os = "linux")]
const SOCKET_PATH_LIMIT: usize = 108;
#[cfg(windows)]
const SOCKET_PATH_LIMIT: usize = 108;
#[cfg(all(unix, not(target_os = "linux")))]
const SOCKET_PATH_LIMIT: usize = 104;

/// Strip Windows' verbatim prefix from canonical paths: `AF_UNIX` addresses
/// cannot use it. Other platforms keep the path unchanged.
fn simplify(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str()
            && let Some(rest) = text.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest);
        }
    }
    path
}

#[cfg(unix)]
fn home_dir(home: Home) -> Result<PathBuf> {
    match home {
        Home::Account => Ok(nix::unistd::User::from_uid(nix::unistd::geteuid())?
            .ok_or(Error::PrivatePath)?
            .dir),
        Home::Environment => std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(Error::PrivatePath),
    }
}

/// Windows has no safe account-database lookup; the profile directory comes
/// from `USERPROFILE`, which Windows sets for every logon session.
#[cfg(windows)]
fn home_dir(home: Home) -> Result<PathBuf> {
    let variables: &[&str] = match home {
        Home::Account => &["USERPROFILE"],
        Home::Environment => &["HOME", "USERPROFILE"],
    };
    variables
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .ok_or(Error::PrivatePath)
}

fn private_dir_builder() -> fs::DirBuilder {
    #[allow(unused_mut, reason = "only Unix sets a mode")]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

fn no_follow() -> OpenOptions {
    #[allow(unused_mut, reason = "only Unix adds the no-follow flag")]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    options
}

#[cfg(unix)]
fn private(path: &Path, socket: bool) -> Result<fs::Metadata> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = fs::symlink_metadata(path)?;
    if meta.uid() != nix::unistd::geteuid().as_raw()
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

#[cfg(windows)]
fn private(_path: &Path, _socket: bool) -> Result<fs::Metadata> {
    Err(Error::Unsupported)
}

#[cfg(unix)]
fn private_file(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err(Error::PrivatePath);
    }
    Ok(())
}

#[cfg(windows)]
fn private_file(file: &File) -> Result<()> {
    if !file.metadata()?.is_file() {
        return Err(Error::PrivatePath);
    }
    Ok(())
}

/// The device and inode that identify a path across replacement. Windows
/// exposes no stable file identity safely, so there every path compares equal
/// and cleanup relies on the lease alone.
#[cfg(unix)]
fn identity(meta: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(windows)]
fn identity(_: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}

#[cfg(unix)]
fn is_owned_socket(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    meta.file_type().is_socket() && meta.uid() == nix::unistd::geteuid().as_raw()
}

#[cfg(windows)]
fn is_owned_socket(meta: &fs::Metadata) -> bool {
    !meta.is_dir() && !meta.file_type().is_symlink()
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
#[cfg(unix)]
pub fn check_peer(stream: &Stream) -> Result<()> {
    #[cfg(target_os = "linux")]
    let uid =
        nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)?.uid();
    #[cfg(not(target_os = "linux"))]
    let uid = nix::unistd::getpeereid(stream)?.0.as_raw();
    check_uid(uid, nix::unistd::geteuid().as_raw())
}

/// Refuse Windows peers until private-root access-list validation is implemented.
///
/// # Errors
/// Always returns `Unsupported` on Windows.
#[cfg(windows)]
pub fn check_peer(_stream: &Stream) -> Result<()> {
    Err(Error::Unsupported)
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
        let mut options = no_follow();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(root.path.join("manager.lock"))?;
        private_file(&file)?;
        let meta = private(root.path(), false)?;
        let identity = identity(&meta);
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
        if identity(&meta) != self.identity || identity(&anchor) != identity(&file) {
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
        let listener = Listener::bind(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        let meta = private(&path, true)?;
        listener.set_nonblocking(true)?;
        Ok(Endpoint {
            lease: self,
            listener,
            identity: identity(&meta),
            path,
            control,
        })
    }
}

/// A listener borrowing its lease, so the socket cannot outlive kernel exclusion.
pub struct Endpoint<'a> {
    lease: &'a Lease,
    listener: Listener,
    path: PathBuf,
    identity: (u64, u64),
    control: bool,
}
impl Endpoint<'_> {
    /// Access the nonblocking listener for an external runtime adapter.
    #[must_use]
    pub fn listener(&self) -> &Listener {
        &self.listener
    }
    /// The socket's pathname.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Remove only the socket identity bound here, while the lease is still held.
    ///
    /// # Errors
    /// Returns root/lease replacement or I/O errors. Foreign replacements are preserved.
    pub fn unlink(&self) -> Result<()> {
        self.lease.verify()?;
        match fs::symlink_metadata(&self.path) {
            Ok(meta) if identity(&meta) == self.identity && is_owned_socket(&meta) => {
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
/// Returns `Absent`, `Starting`, `Capacity`, private-path, credential or transport
/// errors. An ambiguous refused connection to a leased endpoint is `Starting`;
/// on macOS this includes a full backlog as well as a stale/closed listener.
pub fn connect(root: &Root, control: bool, timeout: Duration) -> Result<Stream> {
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
            if is_owned_socket(&metadata) && root.leased()? {
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

/// A local connect either completes or is refused at once; Windows offers no
/// safe nonblocking connect, so the timeout applies only on Unix.
#[cfg(windows)]
fn connect_bounded(path: &Path, timeout: Duration) -> io::Result<Stream> {
    if timeout.is_zero() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Stream::connect(path)
}

#[cfg(unix)]
fn connect_bounded(path: &Path, timeout: Duration) -> io::Result<Stream> {
    use nix::{
        poll::{PollFd, PollFlags, poll},
        sys::socket::{
            AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket,
            sockopt::SocketError,
        },
    };
    use std::os::fd::{AsFd, AsRawFd};
    if timeout.is_zero() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(io::ErrorKind::InvalidInput)?;
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
            wait_for_connect(deadline, |timeout| poll(&mut descriptors, timeout))?;
            let error = getsockopt(&fd, SocketError)?;
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
        }
        Err(e) => return Err(e.into()),
    }
    let stream = Stream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(unix)]
fn wait_for_connect(
    deadline: Instant,
    mut wait: impl FnMut(nix::poll::PollTimeout) -> nix::Result<i32>,
) -> io::Result<()> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let millis = i32::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX);
        let timeout =
            nix::poll::PollTimeout::try_from(millis).unwrap_or(nix::poll::PollTimeout::MAX);
        match wait(timeout) {
            Ok(0) | Err(nix::errno::Errno::EINTR) => {}
            Ok(_) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    //! Windows cannot select a root without an enforced privacy boundary.

    use super::*;

    #[test]
    fn windows_roots_refuse_before_accessing_paths_or_account_environment() {
        assert!(matches!(crate::supported(), Err(Error::Unsupported)));
        assert!(matches!(
            Root::explicit(r"C:\Acme\runtime"),
            Err(Error::Unsupported)
        ));
        for home in [Home::Account, Home::Environment] {
            assert!(matches!(
                Root::for_user(Path::new("Acme"), home),
                Err(Error::Unsupported)
            ));
        }
        for socket in [false, true] {
            assert!(matches!(
                private(Path::new(r"C:\Acme\runtime"), socket),
                Err(Error::Unsupported)
            ));
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! Interrupted poll behavior using a real socket and an injected kernel result.

    use super::*;
    use nix::{
        errno::Errno,
        poll::{PollFd, PollFlags, poll},
    };
    use std::os::fd::AsFd;

    #[test]
    fn interrupted_connect_poll_retries_the_same_socket() {
        let (stream, _peer) = Stream::pair().unwrap();
        let mut descriptors = [PollFd::new(stream.as_fd(), PollFlags::POLLOUT)];
        let mut calls = 0;
        let result = wait_for_connect(Instant::now() + Duration::from_secs(1), |timeout| {
            calls += 1;
            if calls == 1 {
                Err(Errno::EINTR)
            } else {
                poll(&mut descriptors, timeout)
            }
        });
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(calls, 2);
    }

    #[test]
    fn interruption_at_connect_deadline_does_not_renew_the_budget() {
        let (stream, _peer) = Stream::pair().unwrap();
        let mut descriptors = [PollFd::new(stream.as_fd(), PollFlags::POLLIN)];
        let mut calls = 0;
        let result = wait_for_connect(Instant::now() + Duration::from_millis(20), |timeout| {
            calls += 1;
            if calls == 1 {
                assert_eq!(poll(&mut descriptors, timeout)?, 0);
                Err(Errno::EINTR)
            } else {
                Ok(1)
            }
        });
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::TimedOut));
        assert_eq!(calls, 1);
    }
}
