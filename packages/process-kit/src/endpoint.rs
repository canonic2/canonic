//! Private roots, peer credentials, kernel leases and identity-owned sockets.
//!
//! Unix checks ownership and modes on every path and verifies peer credentials.
//! Windows checks each path's owner, protected access list and integrity label,
//! and verifies the peer process's token user.

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
    /// Returns identity lookup, missing-home, invalid relative path or
    /// path-length errors.
    pub fn for_user(relative: &Path, home: Home) -> Result<Self> {
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
    /// Returns I/O or path-length errors. Parent components and symlinks are
    /// refused, as are Windows roots that are not on a local drive letter.
    pub fn explicit(path: impl AsRef<Path>) -> Result<Self> {
        let path = std::path::absolute(path)?;
        if path.components().any(|c| matches!(c, Component::ParentDir)) || !local_prefix(&path) {
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
        // Resolution turns mapped network drives into UNC paths, so check again;
        // prepare and every privacy check also inspect the opened volume.
        if !local_prefix(&path) {
            return Err(Error::PrivatePath);
        }
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
                    match create_private_dir(&path) {
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

/// Windows roots must name a local drive letter: UNC, device and verbatim
/// forms are refused before they are opened. Unix paths have no prefix.
fn local_prefix(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(prefix.kind(), std::path::Prefix::Disk(_)),
        _ => cfg!(unix),
    }
}

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

#[cfg(windows)]
fn home_dir(home: Home) -> Result<PathBuf> {
    match home {
        Home::Account => Ok(crate::windows::profile_directory()?),
        Home::Environment => ["HOME", "USERPROFILE"]
            .iter()
            .find_map(std::env::var_os)
            .map(PathBuf::from)
            .ok_or(Error::PrivatePath),
    }
}

/// What identifies a path across replacement: device and inode on Unix,
/// volume serial number and 128-bit file ID on Windows.
#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(windows)]
type Identity = crate::windows::Identity;

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(windows)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    crate::windows::create_private_directory(path)
}

fn no_follow() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

#[cfg(unix)]
fn open_anchor(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    no_follow()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
fn open_anchor(path: &Path) -> io::Result<File> {
    crate::windows::open_anchor(path)
}

#[cfg(unix)]
fn private(path: &Path, socket: bool) -> Result<Identity> {
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
    Ok((meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn private(path: &Path, socket: bool) -> Result<Identity> {
    use crate::windows::Kind;
    let kind = if socket {
        Kind::Socket
    } else {
        Kind::Directory
    };
    crate::windows::private(path, kind)?.ok_or(Error::PrivatePath)
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
    if crate::windows::private_file(file)? {
        Ok(())
    } else {
        Err(Error::PrivatePath)
    }
}

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    file.metadata().map(|meta| (meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn file_identity(file: &File) -> io::Result<Identity> {
    crate::windows::file_identity(file)
}

#[cfg(unix)]
fn path_identity(path: &Path) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).map(|meta| (meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn path_identity(path: &Path) -> io::Result<Identity> {
    crate::windows::path_identity(path)
}

/// The identity of a socket this user owns at `path`, or `None` for any other
/// file. Windows sockets carry the creator's default owner until restricted,
/// so there the private root, validated first, establishes ownership.
#[cfg(unix)]
fn owned_socket(path: &Path) -> io::Result<Option<Identity>> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = fs::symlink_metadata(path)?;
    Ok(
        (meta.file_type().is_socket() && meta.uid() == nix::unistd::geteuid().as_raw())
            .then_some((meta.dev(), meta.ino())),
    )
}

#[cfg(windows)]
fn owned_socket(path: &Path) -> io::Result<Option<Identity>> {
    crate::windows::socket_identity(path)
}

/// Remove a socket whose identity the caller has just checked.
#[cfg(unix)]
fn remove_socket(path: &Path, _: Identity) -> io::Result<()> {
    fs::remove_file(path)
}

/// Remove a socket, rechecking its identity on the deleting handle. The name
/// disappears at once, so concurrent clients see it absent rather than denied.
#[cfg(windows)]
fn remove_socket(path: &Path, identity: Identity) -> io::Result<()> {
    crate::windows::remove_socket(path, identity)
}

/// Restrict a newly bound socket before publishing it.
#[cfg(unix)]
fn restrict_socket(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_socket(path: &Path) -> io::Result<()> {
    crate::windows::restrict_socket(path)
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

/// Verify every accepted or connected stream before reading a frame, by the
/// peer process's token user.
///
/// # Errors
/// Returns `Peer` when the peer process belongs to another user or cannot be
/// queried, or the socket error.
#[cfg(windows)]
pub fn check_peer(stream: &Stream) -> Result<()> {
    if crate::windows::peer_is_user(stream)? {
        Ok(())
    } else {
        Err(Error::Peer)
    }
}

/// Kernel-owned exclusive lock. The anchor is never unlinked or rewritten.
pub struct Lease {
    root: Root,
    file: File,
    identity: Identity,
    bound: [std::sync::atomic::AtomicBool; 2],
}
impl Lease {
    /// Wait with capped exponential backoff for an exiting owner.
    ///
    /// # Errors
    /// Returns `AlreadyRunning` at the deadline; validation and I/O errors remain distinct.
    pub fn acquire(root: &Root, wait: Duration) -> Result<Self> {
        root.prepare()?;
        let file = open_anchor(&root.path.join("manager.lock"))?;
        private_file(&file)?;
        let identity = private(root.path(), false)?;
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
        let root = private(self.root.path(), false)?;
        let anchor = path_identity(&self.root.path.join("manager.lock"))?;
        if root != self.identity || anchor != file_identity(&self.file)? {
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
                let identity = private(&path, true)?;
                remove_socket(&path, identity)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = Listener::bind(&path)?;
        restrict_socket(&path)?;
        let identity = private(&path, true)?;
        listener.set_nonblocking(true)?;
        Ok(Endpoint {
            lease: self,
            listener,
            identity,
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
    identity: Identity,
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
        match owned_socket(&self.path) {
            Ok(Some(identity)) if identity == self.identity => {
                remove_socket(&self.path, identity)?;
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
    if timeout.is_zero() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
    }
    let until = Instant::now() + timeout;
    match private_for_connect(root.path(), false, until) {
        Ok(_) => {}
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Err(Error::Absent),
        Err(e) => return Err(e),
    }
    match private_for_connect(&root.socket(control), true, until) {
        Ok(_) => {}
        Err(Error::PrivatePath) => {
            // bind publishes an owned socket before it is restricted. A live
            // lease proves startup may still be preparing that endpoint. Do not
            // connect until a later probe validates private permissions.
            if owned_socket_for_connect(&root.socket(control), until)?.is_some() && root.leased()? {
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
    let remaining = until.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Error::Capacity);
    }
    let stream = match connect_bounded(&root.socket(control), remaining) {
        Ok(stream) => stream,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) || queue_full(&e) =>
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

fn private_for_connect(path: &Path, socket: bool, until: Instant) -> Result<Identity> {
    #[cfg(windows)]
    {
        use crate::windows::Kind;
        let kind = if socket {
            Kind::Socket
        } else {
            Kind::Directory
        };
        crate::windows::private_until(path, kind, until)?.ok_or(Error::PrivatePath)
    }
    #[cfg(unix)]
    {
        let _ = until;
        private(path, socket)
    }
}

fn owned_socket_for_connect(path: &Path, until: Instant) -> io::Result<Option<Identity>> {
    #[cfg(windows)]
    {
        crate::windows::socket_identity_until(path, until)
    }
    #[cfg(unix)]
    {
        let _ = until;
        owned_socket(path)
    }
}

#[cfg(windows)]
fn connect_bounded(path: &Path, timeout: Duration) -> io::Result<Stream> {
    crate::windows::connect(path, timeout)
}

#[cfg(windows)]
fn queue_full(error: &io::Error) -> bool {
    crate::windows::queue_full(error)
}

#[cfg(unix)]
fn queue_full(_: &io::Error) -> bool {
    false
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
mod windows_roots {
    //! Root resolution on a Windows host.

    use super::*;

    #[test]
    fn network_roots_are_refused_and_local_roots_prepare_privately() {
        assert!(matches!(
            Root::explicit(r"\\localhost\C$\Acme\runtime"),
            Err(Error::PrivatePath)
        ));
        let temporary = tempfile::tempdir().unwrap();
        let root = Root::explicit(temporary.path().join("acme").join("runtime")).unwrap();
        root.prepare().unwrap();
        assert!(!root.leased().unwrap());
        let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
        assert!(root.leased().unwrap());
        let endpoint = lease.bind(false).unwrap();
        let client = connect(&root, false, Duration::from_secs(2)).unwrap();
        check_peer(&client).unwrap();
        drop(client);
        endpoint.unlink().unwrap();
        assert!(!root.socket(false).exists());
    }

    #[test]
    #[ignore = "mapping the administrative share needs an elevated session"]
    fn a_mapped_network_drive_root_is_refused_without_changes() {
        struct Mapping(String);
        impl Drop for Mapping {
            fn drop(&mut self) {
                let _ = std::process::Command::new("net")
                    .args(["use", &self.0, "/delete", "/y"])
                    .stdout(std::process::Stdio::null())
                    .status();
            }
        }
        let temporary = tempfile::tempdir().unwrap();
        let relative = temporary.path().strip_prefix(r"C:\").unwrap();
        // A letter can be free yet keep a remembered mapping, which refuses `net use`.
        let drive = ('M'..='Z')
            .rev()
            .map(|letter| format!("{letter}:"))
            .filter(|drive| !Path::new(&format!(r"{drive}\")).exists())
            .find(|drive| {
                std::process::Command::new("net")
                    .args(["use", drive, r"\\localhost\C$", "/persistent:no"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            })
            .expect("no drive letter could map the administrative share");
        let _mapping = Mapping(drive.clone());
        let path = Path::new(&format!(r"{drive}\")).join(relative).join("acme");
        assert!(path.parent().unwrap().is_dir(), "{}", path.display());
        let refused = Root::explicit(&path).and_then(|root| root.prepare());
        assert!(matches!(refused, Err(Error::PrivatePath)), "{refused:?}");
        assert!(!temporary.path().join("acme").exists());
    }

    #[test]
    fn an_existing_inherited_root_is_refused_without_repair() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("ordinary");
        fs::create_dir(&path).unwrap();
        let root = Root::explicit(&path).unwrap();
        assert!(matches!(root.prepare(), Err(Error::PrivatePath)));
        assert!(matches!(
            Lease::acquire(&root, Duration::ZERO),
            Err(Error::PrivatePath)
        ));
        assert!(!path.join("manager.lock").exists());
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
