//! Windows token, security-descriptor, file-identity, peer-process and socket
//! calls behind safe functions. This is the only Windows module that contains
//! `unsafe`: each block states the invariants it relies on, and every handle or
//! system allocation it obtains is released by an owning guard.
#![allow(
    unsafe_code,
    reason = "Win32 security, file and Winsock calls have no safe std interface"
)]

use crate::net::Stream;
mod launch;
pub(crate) use launch::spawn_detached;
use std::{
    ffi::{OsString, c_void},
    fs::{File, OpenOptions},
    io,
    mem::{MaybeUninit, size_of},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::{AsRawHandle, AsRawSocket, FromRawHandle, FromRawSocket, OwnedHandle, RawSocket},
    },
    path::{Path, PathBuf},
    ptr,
    sync::{Once, OnceLock},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    },
    Networking::WinSock::{
        AF_UNIX, FD_SET, FIONBIO, INVALID_SOCKET, MSG_PEEK, POLLRDNORM, SIO_AF_UNIX_GETPEERPID,
        SO_ERROR, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, SOCKET_ERROR, SOL_SOCKET, TIMEVAL,
        WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA, WSAEINVAL, WSAENOBUFS,
        WSAEWOULDBLOCK, WSAGetLastError, WSAIoctl, WSAPOLLFD, WSAPoll, WSASocketW, WSAStartup,
        connect as wsa_connect, getsockopt, ioctlsocket, recv, select,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetLengthSid,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
        GetSecurityDescriptorSacl, GetSidSubAuthority, GetSidSubAuthorityCount,
        GetTokenInformation, INHERITED_ACE, IsValidSid, LABEL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, SE_DACL_PRESENT, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, SYSTEM_MANDATORY_LABEL_ACE,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
        FILE_INFO_BY_HANDLE_CLASS, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo,
        FileDispositionInfoEx, FileIdInfo, FileStandardInfo, GetDriveTypeW,
        GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_ALWAYS, READ_CONTROL,
        SetFileInformationByHandle, VOLUME_NAME_DOS, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, IO_REPARSE_TAG_AF_UNIX,
            SECURITY_MANDATORY_MEDIUM_RID, SYSTEM_MANDATORY_LABEL_ACE_TYPE,
        },
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        WindowsProgramming::DRIVE_REMOTE,
    },
    UI::Shell::GetUserProfileDirectoryW,
};

/// Volume serial number and 128-bit file ID: stable across renames, distinct
/// after replacement, and valid on `ReFS` as well as NTFS.
pub(crate) type Identity = (u64, u128);

/// The object a private path must be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Directory,
    Socket,
}

/// An owned copy of a binary SID. Equal bytes are equal SIDs.
#[derive(PartialEq, Eq)]
struct Sid(Vec<u8>);

/// Frees one `LocalAlloc` allocation returned by the system.
struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a system call that allocates with
            // LocalAlloc and transfers ownership to the caller; only this guard
            // holds it, so it is freed exactly once.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Winsock's handle type for a stream. `RawSocket` and `SOCKET` are the same
/// width on every Windows target Rust supports.
#[allow(
    clippy::cast_possible_truncation,
    reason = "RawSocket and SOCKET share a width"
)]
fn socket(stream: &Stream) -> SOCKET {
    stream.as_raw_socket() as SOCKET
}

fn wsa_error() -> io::Error {
    // SAFETY: reads the calling thread's last Winsock error; no preconditions.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

/// Copy a SID out of a system buffer.
///
/// # Safety
/// `sid` must be null or point to a SID that stays valid during this call.
unsafe fn copy_sid(sid: PSID) -> Option<Sid> {
    // SAFETY: the caller guarantees `sid` is null or readable; IsValidSid
    // verifies the structure before GetLengthSid trusts its sub-authority count.
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return None;
    }
    // SAFETY: a valid SID is exactly GetLengthSid bytes long and readable.
    let length = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: as above; the bytes are copied before the caller's buffer ends.
    Some(Sid(unsafe {
        std::slice::from_raw_parts(sid.cast::<u8>(), length)
    }
    .to_vec()))
}

fn open_token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: `process` is an open process handle or the current-process
    // pseudo-handle; `token` receives a new handle on success.
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) })?;
    // SAFETY: OpenProcessToken succeeded, so `token` is an open handle that no
    // other owner holds.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

fn token_user(token: &OwnedHandle) -> io::Result<Sid> {
    let mut needed = 0_u32;
    // SAFETY: a null buffer of length zero only asks for the required size.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 storage gives the pointer alignment TOKEN_USER requires.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer is writable for at least `needed` bytes and aligned for
    // TOKEN_USER.
    check(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    })?;
    // SAFETY: TokenUser fills the buffer with a TOKEN_USER whose SID pointer
    // refers to bytes inside the same buffer, which is alive here.
    unsafe { copy_sid((*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid) }
        .ok_or_else(|| io::ErrorKind::InvalidData.into())
}

fn current_process() -> HANDLE {
    // SAFETY: returns the current-process pseudo-handle; it needs no closing.
    unsafe { GetCurrentProcess() }
}

/// The current process token's user SID: the effective user.
fn user() -> io::Result<&'static Sid> {
    static USER: OnceLock<Sid> = OnceLock::new();
    if let Some(user) = USER.get() {
        return Ok(user);
    }
    let sid = token_user(&open_token(current_process())?)?;
    Ok(USER.get_or_init(|| sid))
}

fn sid_string(sid: &Sid) -> io::Result<String> {
    let mut text = ptr::null_mut();
    // SAFETY: `sid` holds a validated SID; `text` receives a LocalAlloc'd
    // NUL-terminated string, owned by the guard below.
    check(unsafe { ConvertSidToStringSidW(sid.0.as_ptr().cast_mut().cast(), &raw mut text) })?;
    let _owned = Local(text.cast());
    // SAFETY: the conversion returned a NUL-terminated string that the guard
    // keeps alive until after this copy.
    unsafe { wide_string(text) }
}

/// Copy a NUL-terminated UTF-16 string returned by the system.
///
/// # Safety
/// `text` must point to a readable NUL-terminated UTF-16 string.
unsafe fn wide_string(text: *const u16) -> io::Result<String> {
    let mut length = 0;
    // SAFETY: every unit up to and including the terminator is readable.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` units precede the terminator and are readable.
    let units = unsafe { std::slice::from_raw_parts(text, length) };
    String::from_utf16(units).map_err(|_| io::ErrorKind::InvalidData.into())
}

/// The SID that follows the header and mask of an access-allowed or mandatory
/// label ACE. Taking its address by offset avoids assuming more alignment than
/// `GetAce` guarantees.
///
/// # Safety
/// `ace` must point to an ACE of one of those two types.
unsafe fn ace_sid(ace: *const ACE_HEADER) -> PSID {
    const _: () = assert!(
        std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart)
            == std::mem::offset_of!(SYSTEM_MANDATORY_LABEL_ACE, SidStart)
    );
    // SAFETY: the SID begins at this offset inside the ACE, by the caller's
    // contract on the ACE type.
    unsafe {
        ace.cast::<u8>()
            .add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart))
            .cast_mut()
            .cast()
    }
}

/// Profile directory of the current process token, independent of the
/// `USERPROFILE` and `HOME` variables.
pub(crate) fn profile_directory() -> io::Result<PathBuf> {
    let token = open_token(current_process())?;
    let mut length = 0_u32;
    // SAFETY: a null buffer only reports the required length in characters.
    unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), ptr::null_mut(), &raw mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0_u16; length as usize];
    // SAFETY: the buffer holds `length` UTF-16 units, as the call requires.
    check(unsafe {
        GetUserProfileDirectoryW(token.as_raw_handle(), buffer.as_mut_ptr(), &raw mut length)
    })?;
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Ok(PathBuf::from(OsString::from_wide(&buffer[..end])))
}

/// A self-relative descriptor parsed from SDDL, freed on drop.
struct Descriptor(Local);

/// Owner and protected DACL for the user only, with an explicit medium label
/// so neither an elevated creator's default owner nor a higher label applies.
fn private_descriptor(directory: bool) -> io::Result<Descriptor> {
    let sid = sid_string(user()?)?;
    let inherit = if directory { "OICI" } else { "" };
    let sddl = format!("O:{sid}D:P(A;{inherit};FA;;;{sid})S:(ML;{inherit};NW;;;ME)");
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    // SAFETY: `text` is NUL-terminated; on success `descriptor` receives a
    // LocalAlloc'd self-relative descriptor that the guard owns.
    check(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            ptr::null_mut(),
        )
    })?;
    Ok(Descriptor(Local(descriptor)))
}

impl Descriptor {
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: 0,
        }
    }
}

/// Create one directory with the private descriptor applied atomically.
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    let descriptor = private_descriptor(true)?;
    let attributes = descriptor.attributes();
    let path = wide(path);
    // SAFETY: `path` is NUL-terminated and `attributes` points to a descriptor
    // that outlives the call.
    check(unsafe { CreateDirectoryW(path.as_ptr(), &raw const attributes) })
}

/// Open or create the lease anchor without following reparse points. A new
/// anchor receives the private descriptor atomically; an existing one keeps
/// its descriptor for validation.
pub(crate) fn open_anchor(path: &Path) -> io::Result<File> {
    let descriptor = private_descriptor(false)?;
    let attributes = descriptor.attributes();
    let path = wide(path);
    // SAFETY: `path` is NUL-terminated, `attributes` outlives the call and the
    // template handle is null.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &raw const attributes,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW succeeded, so `handle` is an open file handle that no
    // other owner holds.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

/// Open a path itself, never a reparse target, for inspection.
fn open(path: &Path, write_security: bool) -> io::Result<File> {
    open_until(path, write_security, Instant::now() + DELETION_SETTLE)
}

fn open_until(path: &Path, write_security: bool, until: Instant) -> io::Result<File> {
    let mut access = READ_CONTROL | FILE_READ_ATTRIBUTES;
    if write_security {
        access |= WRITE_DAC | WRITE_OWNER;
    }
    let deadline = until.min(Instant::now() + DELETION_SETTLE);
    let mut delay = Duration::from_millis(1);
    loop {
        let opened = OpenOptions::new()
            .access_mode(access)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path);
        match opened {
            Err(error)
                if error.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok()
                    && settle(deadline, &mut delay) => {}
            other => return other,
        }
    }
}

/// How long to retry while another process finishes deleting a socket. A
/// file being deleted refuses opens with access denied, and a socket refuses
/// connects with an invalid-argument error, until the deleting handle closes;
/// the path then is absent or holds a replacement. The bound keeps a lasting
/// denial a prompt error.
const DELETION_SETTLE: Duration = Duration::from_millis(100);

/// Sleep with capped backoff before another attempt, unless `deadline` passed.
fn settle(deadline: Instant, delay: &mut Duration) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    std::thread::sleep((*delay).min(remaining));
    *delay = (*delay * 2).min(Duration::from_millis(10));
    true
}

/// Read one fixed-size information class.
///
/// # Safety
/// `T` must be the structure Windows returns for `class`.
unsafe fn information<T>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut value = MaybeUninit::<T>::zeroed();
    // SAFETY: the buffer is writable for size_of::<T>() bytes, matching `class`
    // by the caller's contract.
    check(unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            value.as_mut_ptr().cast(),
            u32::try_from(size_of::<T>()).unwrap_or(u32::MAX),
        )
    })?;
    // SAFETY: the call succeeded and initialized the structure; zeroed padding
    // is a valid value for these plain-data types.
    Ok(unsafe { value.assume_init() })
}

struct Info {
    identity: Identity,
    directory: bool,
    tag: Option<u32>,
    links: u32,
}

fn info(file: &File) -> io::Result<Info> {
    // SAFETY: each structure matches its information class.
    let id: FILE_ID_INFO = unsafe { information(file, FileIdInfo) }?;
    // SAFETY: as above.
    let tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { information(file, FileAttributeTagInfo) }?;
    // SAFETY: as above.
    let standard: FILE_STANDARD_INFO = unsafe { information(file, FileStandardInfo) }?;
    Ok(Info {
        identity: (
            id.VolumeSerialNumber,
            u128::from_le_bytes(id.FileId.Identifier),
        ),
        directory: tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        tag: (tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0).then_some(tag.ReparseTag),
        links: standard.NumberOfLinks,
    })
}

/// A local drive-letter volume, decided from the open handle so mapped drives
/// and other remote aliases resolve to their UNC form and are refused.
fn is_local(file: &File) -> io::Result<bool> {
    let mut buffer = vec![0_u16; 1024];
    loop {
        // SAFETY: the buffer is writable for its length in UTF-16 units.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < buffer.len() {
            buffer.truncate(length);
            break;
        }
        buffer.resize(length + 1, 0);
    }
    // A local result has the form \\?\X:\...
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let Some(rest) = buffer.strip_prefix(prefix.as_slice()) else {
        return Ok(false);
    };
    let [letter, colon, separator, ..] = *rest else {
        return Ok(false);
    };
    if !char::from_u32(u32::from(letter)).is_some_and(|c| c.is_ascii_alphabetic())
        || colon != u16::from(b':')
        || separator != u16::from(b'\\')
    {
        return Ok(false);
    }
    let root = [letter, colon, separator, 0];
    // SAFETY: `root` is a NUL-terminated drive root such as C:\.
    Ok(unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_REMOTE)
}

/// Visit each ACE header in an ACL; the visitor returns false to stop.
///
/// # Safety
/// `acl` must be null or point to an ACL that stays valid during this call.
unsafe fn aces(
    acl: *const ACL,
    mut visit: impl FnMut(*const ACE_HEADER) -> bool,
) -> io::Result<bool> {
    if acl.is_null() {
        return Ok(true);
    }
    let mut size = MaybeUninit::<ACL_SIZE_INFORMATION>::zeroed();
    // SAFETY: `acl` is valid by contract and the buffer matches the class.
    check(unsafe {
        GetAclInformation(
            acl,
            size.as_mut_ptr().cast(),
            u32::try_from(size_of::<ACL_SIZE_INFORMATION>()).unwrap_or(u32::MAX),
            AclSizeInformation,
        )
    })?;
    // SAFETY: GetAclInformation succeeded and initialized the structure.
    let count = unsafe { size.assume_init() }.AceCount;
    for index in 0..count {
        let mut ace = ptr::null_mut();
        // SAFETY: `index` is below the ACL's ACE count.
        check(unsafe { GetAce(acl, index, &raw mut ace) })?;
        if !visit(ace.cast_const().cast()) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Owner, protected user-only DACL and no label above medium.
fn is_private_descriptor(file: &File) -> io::Result<bool> {
    let user = user()?;
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut sacl = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open with READ_CONTROL; the out-pointers receive
    // pointers into one LocalAlloc'd descriptor, owned by the guard below.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            &raw mut owner,
            ptr::null_mut(),
            &raw mut dacl,
            &raw mut sacl,
            &raw mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(i32::MAX),
        ));
    }
    let _owned = Local(descriptor);
    // SAFETY: `owner` points into the descriptor the guard keeps alive.
    if unsafe { copy_sid(owner) }.as_ref() != Some(user) {
        return Ok(false);
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: `descriptor` is the valid descriptor returned above.
    check(unsafe {
        GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision)
    })?;
    if control & SE_DACL_PRESENT == 0 || control & SE_DACL_PROTECTED == 0 || dacl.is_null() {
        return Ok(false);
    }
    // SAFETY: `dacl` points into the live descriptor; each ACE pointer comes
    // from GetAce and its type determines the layout read.
    let dacl_private = unsafe {
        aces(dacl, |ace| {
            let header = *ace;
            if header.AceFlags & u8::try_from(INHERITED_ACE).unwrap_or(u8::MAX) != 0 {
                return false;
            }
            match u32::from(header.AceType) {
                ACCESS_DENIED_ACE_TYPE => true,
                ACCESS_ALLOWED_ACE_TYPE => copy_sid(ace_sid(ace)).as_ref() == Some(user),
                _ => false,
            }
        })
    }?;
    if !dacl_private {
        return Ok(false);
    }
    // SAFETY: `sacl` is null or points into the live descriptor; label ACEs
    // carry a SID whose last sub-authority is the integrity RID.
    unsafe {
        aces(sacl, |ace| {
            if u32::from((*ace).AceType) != SYSTEM_MANDATORY_LABEL_ACE_TYPE {
                return true;
            }
            let sid = ace_sid(ace);
            if IsValidSid(sid) == 0 {
                return false;
            }
            let count = u32::from(*GetSidSubAuthorityCount(sid));
            count > 0
                && i64::from(*GetSidSubAuthority(sid, count - 1))
                    <= i64::from(SECURITY_MANDATORY_MEDIUM_RID)
        })
    }
}

/// Validate a private directory or socket without following reparse points.
/// Returns its identity, or `None` when it is the wrong type, remote or not
/// private. Missing paths return the `NotFound` error.
pub(crate) fn private(path: &Path, kind: Kind) -> io::Result<Option<Identity>> {
    private_until(path, kind, Instant::now() + DELETION_SETTLE)
}

/// Validate a path without retrying deletion beyond the caller's deadline.
pub(crate) fn private_until(
    path: &Path,
    kind: Kind,
    until: Instant,
) -> io::Result<Option<Identity>> {
    let file = open_until(path, false, until)?;
    let info = info(&file)?;
    let shape = match kind {
        Kind::Directory => info.directory && info.tag.is_none(),
        Kind::Socket => info.tag == Some(IO_REPARSE_TAG_AF_UNIX),
    };
    Ok((shape && is_local(&file)? && is_private_descriptor(&file)?).then_some(info.identity))
}

/// Validate the open lease anchor: an ordinary single-link private file.
pub(crate) fn private_file(file: &File) -> io::Result<bool> {
    let info = info(file)?;
    Ok(!info.directory
        && info.tag.is_none()
        && info.links == 1
        && is_local(file)?
        && is_private_descriptor(file)?)
}

/// Identity of an open file.
pub(crate) fn file_identity(file: &File) -> io::Result<Identity> {
    info(file).map(|info| info.identity)
}

/// Identity of a path itself, without following reparse points.
pub(crate) fn path_identity(path: &Path) -> io::Result<Identity> {
    file_identity(&open(path, false)?)
}

/// Identity of an `AF_UNIX` socket file, or `None` for anything else. Callers
/// have validated the private root, which only the user can write into.
pub(crate) fn socket_identity(path: &Path) -> io::Result<Option<Identity>> {
    socket_identity_until(path, Instant::now() + DELETION_SETTLE)
}

/// Inspect a socket without retrying deletion beyond the caller's deadline.
pub(crate) fn socket_identity_until(path: &Path, until: Instant) -> io::Result<Option<Identity>> {
    let info = info(&open_until(path, false, until)?)?;
    Ok((info.tag == Some(IO_REPARSE_TAG_AF_UNIX)).then_some(info.identity))
}

/// Remove the socket at `path` when it still has `expected` identity, checked
/// on the same handle that deletes it. POSIX semantics remove the name at once
/// even while other processes hold the file open; an ordinary delete leaves it
/// pending until they close, and opening a pending file reports access denied.
pub(crate) fn remove_socket(path: &Path, expected: Identity) -> io::Result<()> {
    let file = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let info = info(&file)?;
    if info.tag != Some(IO_REPARSE_TAG_AF_UNIX) || info.identity != expected {
        return Ok(());
    }
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    // SAFETY: the handle has DELETE access, and the pointer and size describe
    // `disposition`, which outlives the call.
    check(unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            (&raw const disposition).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(u32::MAX),
        )
    })
}

/// Replace a bound socket's inherited descriptor and creator-default owner
/// with the private descriptor.
pub(crate) fn restrict_socket(path: &Path) -> io::Result<()> {
    let file = open(path, true)?;
    let descriptor = private_descriptor(false)?;
    let (mut owner, mut dacl, mut sacl) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    let (mut present, mut defaulted) = (0, 0);
    // SAFETY: the descriptor is valid; each call stores a pointer into it, and
    // the descriptor outlives SetSecurityInfo below.
    unsafe {
        check(GetSecurityDescriptorOwner(
            descriptor.0.0,
            &raw mut owner,
            &raw mut defaulted,
        ))?;
        check(GetSecurityDescriptorDacl(
            descriptor.0.0,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        ))?;
        check(GetSecurityDescriptorSacl(
            descriptor.0.0,
            &raw mut present,
            &raw mut sacl,
            &raw mut defaulted,
        ))?;
    }
    // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER, and the
    // owner, DACL and SACL point into the live descriptor.
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | LABEL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            ptr::null_mut(),
            dacl,
            sacl,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(i32::MAX),
        ));
    }
    Ok(())
}

/// Compare the connected peer process's token user with the effective user.
/// Returns false when the peer process or its token cannot be opened, as for
/// an elevated peer in another logon session seen from medium integrity.
pub(crate) fn peer_is_user(stream: &Stream) -> io::Result<bool> {
    let mut pid = 0_u32;
    let mut returned = 0_u32;
    // SAFETY: the socket is open; the output buffer is a u32, as this ioctl
    // requires, and no overlapped operation is used.
    let result = unsafe {
        WSAIoctl(
            socket(stream),
            SIO_AF_UNIX_GETPEERPID,
            ptr::null(),
            0,
            (&raw mut pid).cast(),
            u32::try_from(size_of::<u32>()).unwrap_or(u32::MAX),
            &raw mut returned,
            ptr::null_mut(),
            None,
        )
    };
    if result != 0 {
        return Err(wsa_error());
    }
    // SAFETY: opening a process by ID has no memory-safety preconditions.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Ok(false);
    }
    // SAFETY: OpenProcess succeeded, so `process` is an open handle we own.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    Ok(open_token(process.as_raw_handle())
        .and_then(|token| token_user(&token))
        .is_ok_and(|sid| is_user(&sid)))
}

/// Whether a peer's token user is the effective user; a lookup failure refuses.
fn is_user(peer: &Sid) -> bool {
    user().is_ok_and(|user| peer == user)
}

fn startup() {
    static START: Once = Once::new();
    START.call_once(|| {
        let mut data = MaybeUninit::<WSADATA>::zeroed();
        // SAFETY: requests Winsock 2.2 into a writable WSADATA. A failure
        // surfaces from the next socket call.
        unsafe { WSAStartup(0x202, data.as_mut_ptr()) };
    });
}

fn set_nonblocking(socket: SOCKET, nonblocking: bool) -> io::Result<()> {
    let mut value = u32::from(nonblocking);
    // SAFETY: the socket is open and FIONBIO takes a u32 argument.
    if unsafe { ioctlsocket(socket, FIONBIO, &raw mut value) } == 0 {
        Ok(())
    } else {
        Err(wsa_error())
    }
}

/// Connect a nonblocking socket and wait for completion against one absolute
/// deadline, so a pending connection never outlives the caller's budget.
pub(crate) fn connect(path: &Path, timeout: Duration) -> io::Result<Stream> {
    if timeout.is_zero() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(io::ErrorKind::InvalidInput)?;
    let bytes = path.to_str().ok_or(io::ErrorKind::InvalidInput)?.as_bytes();
    let mut address = SOCKADDR_UN {
        sun_family: AF_UNIX,
        sun_path: [0; 108],
    };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = i8::from_ne_bytes([*byte]);
    }
    let length = i32::try_from(size_of::<u16>() + bytes.len() + 1)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    startup();
    let settle_until = deadline.min(Instant::now() + DELETION_SETTLE);
    let mut delay = Duration::from_millis(1);
    loop {
        match connect_once(&address, length, deadline) {
            Err(error)
                if error.raw_os_error() == Some(WSAEINVAL) && settle(settle_until, &mut delay) => {}
            other => return other,
        }
    }
}

/// One connection attempt on a new socket.
fn connect_once(address: &SOCKADDR_UN, length: i32, deadline: Instant) -> io::Result<Stream> {
    // SAFETY: creates a new socket; the result is checked before use.
    let raw = unsafe {
        WSASocketW(
            i32::from(AF_UNIX),
            SOCK_STREAM,
            0,
            ptr::null(),
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if raw == INVALID_SOCKET {
        return Err(wsa_error());
    }
    // SAFETY: WSASocketW returned a new socket owned by nobody else; the
    // stream closes it on every return path below.
    let stream = unsafe { Stream::from_raw_socket(raw as RawSocket) };
    set_nonblocking(raw, true)?;
    // SAFETY: `address` is a valid SOCKADDR_UN and `length` covers the family,
    // the path and its terminator.
    let result = unsafe { wsa_connect(raw, ptr::from_ref(address).cast::<SOCKADDR>(), length) };
    if result == SOCKET_ERROR {
        // SAFETY: reads this thread's last Winsock error.
        let error = unsafe { WSAGetLastError() };
        if error != WSAEWOULDBLOCK {
            return Err(io::Error::from_raw_os_error(error));
        }
        wait_connected(raw, deadline)?;
    }
    set_nonblocking(raw, false)?;
    Ok(stream)
}

/// Windows refuses a connection to a listener whose queue is full with
/// `WSAENOBUFS`, where Unix reports a refusal or leaves the connect pending.
pub(crate) fn queue_full(error: &io::Error) -> bool {
    error.raw_os_error() == Some(WSAENOBUFS)
}

fn wait_connected(socket: SOCKET, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut write = FD_SET {
            fd_count: 1,
            fd_array: [0; 64],
        };
        write.fd_array[0] = socket;
        let mut except = FD_SET {
            fd_count: 1,
            fd_array: [0; 64],
        };
        except.fd_array[0] = socket;
        let timeout = TIMEVAL {
            tv_sec: i32::try_from(remaining.as_secs()).unwrap_or(i32::MAX),
            tv_usec: i32::try_from(remaining.subsec_micros()).unwrap_or(0).max(1),
        };
        // SAFETY: the sets each name one open socket and outlive the call.
        let ready = unsafe {
            select(
                0,
                ptr::null_mut(),
                &raw mut write,
                &raw mut except,
                &raw const timeout,
            )
        };
        if ready == SOCKET_ERROR {
            return Err(wsa_error());
        }
        if ready == 0 {
            continue;
        }
        if except.fd_count > 0 {
            let mut error = 0_i32;
            let mut length = i32::try_from(size_of::<i32>()).unwrap_or(4);
            // SAFETY: SO_ERROR writes one i32 into the provided buffer.
            let result = unsafe {
                getsockopt(
                    socket,
                    SOL_SOCKET,
                    SO_ERROR,
                    (&raw mut error).cast(),
                    &raw mut length,
                )
            };
            if result == SOCKET_ERROR {
                return Err(wsa_error());
            }
            return Err(io::Error::from_raw_os_error(error));
        }
        return Ok(());
    }
}

/// Wait at most `timeout` for the socket to become readable or closed.
pub(crate) fn wait_readable(stream: &Stream, timeout: Duration) -> io::Result<bool> {
    let millis = timeout.as_millis().saturating_add(u128::from(
        !timeout.subsec_nanos().is_multiple_of(1_000_000),
    ));
    let mut descriptor = WSAPOLLFD {
        fd: socket(stream),
        events: POLLRDNORM,
        revents: 0,
    };
    // SAFETY: one valid WSAPOLLFD for an open socket.
    let ready = unsafe {
        WSAPoll(
            &raw mut descriptor,
            1,
            i32::try_from(millis).unwrap_or(i32::MAX),
        )
    };
    if ready == SOCKET_ERROR {
        return Err(wsa_error());
    }
    Ok(ready > 0)
}

/// Peek one byte without consuming it: zero means orderly EOF.
pub(crate) fn peek(stream: &Stream) -> io::Result<usize> {
    let mut byte = 0_u8;
    // SAFETY: the buffer is one writable byte and MSG_PEEK leaves it queued.
    let received = unsafe { recv(socket(stream), &raw mut byte, 1, MSG_PEEK) };
    if received == SOCKET_ERROR {
        return Err(wsa_error());
    }
    Ok(usize::try_from(received).unwrap_or(0))
}

#[cfg(test)]
mod tests;
