//! Windows host checks for private descriptors, reparse refusal, file identity,
//! peer identity, bounded connect and owner-channel waits. Descriptors are read
//! back as SDDL, independently of the ACE walk that validation uses.

use super::*;
use crate::net::Listener;
use std::{io::Write, process::Command};
use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;

fn sddl(path: &Path) -> String {
    let file = open(path, false).unwrap();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let information =
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    // SAFETY: the handle has READ_CONTROL; the descriptor is owned by the guard.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            information,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    assert_eq!(status, 0);
    let _descriptor = Local(descriptor);
    let mut text = ptr::null_mut();
    // SAFETY: the descriptor is valid; the string is owned by the guard below.
    check(unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            information,
            &raw mut text,
            ptr::null_mut(),
        )
    })
    .unwrap();
    let _text = Local(text.cast());
    // SAFETY: the conversion returned a NUL-terminated string kept alive above.
    unsafe { wide_string(text) }.unwrap()
}

/// The DACL section's entries, such as `(A;OICI;FA;;;S-1-5-...)`.
fn dacl_entries(sddl: &str) -> Vec<String> {
    let dacl = sddl.split("D:").nth(1).unwrap();
    let dacl = dacl.split("S:").next().unwrap();
    dacl.split('(')
        .skip(1)
        .map(|entry| format!("({}", entry.trim_end_matches(|c| c != ')')))
        .collect()
}

fn user_sid() -> String {
    sid_string(user().unwrap()).unwrap()
}

#[test]
fn created_directories_and_anchors_have_one_protected_user_entry_and_the_user_owner() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("acme");
    create_private_directory(&directory).unwrap();
    let sid = user_sid();
    let text = sddl(&directory);
    assert!(text.starts_with(&format!("O:{sid}")), "{text}");
    assert!(text.contains("D:P"), "{text}");
    assert_eq!(
        dacl_entries(&text),
        [format!("(A;OICI;FA;;;{sid})")],
        "{text}"
    );
    assert!(!text.contains(";HI)") && !text.contains(";SI)"), "{text}");
    assert!(private(&directory, Kind::Directory).unwrap().is_some());

    let anchor = open_anchor(&directory.join("manager.lock")).unwrap();
    assert!(private_file(&anchor).unwrap());
    let text = sddl(&directory.join("manager.lock"));
    assert!(text.starts_with(&format!("O:{sid}")), "{text}");
    assert_eq!(dacl_entries(&text), [format!("(A;;FA;;;{sid})")], "{text}");
}

#[test]
fn inherited_permissive_and_reparse_roots_are_refused_without_changes() {
    let temporary = tempfile::tempdir().unwrap();
    let ordinary = temporary.path().join("ordinary");
    std::fs::create_dir(&ordinary).unwrap();
    let before = sddl(&ordinary);
    assert!(private(&ordinary, Kind::Directory).unwrap().is_none());
    assert_eq!(sddl(&ordinary), before);

    let target = temporary.path().join("target");
    create_private_directory(&target).unwrap();
    let junction = temporary.path().join("junction");
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(private(&target, Kind::Directory).unwrap().is_some());
    assert!(private(&junction, Kind::Directory).unwrap().is_none());

    let file = temporary.path().join("manager.sock");
    std::fs::write(&file, b"acme").unwrap();
    assert!(private(&file, Kind::Socket).unwrap().is_none());
    assert!(socket_identity(&file).unwrap().is_none());
    assert_eq!(std::fs::read(&file).unwrap(), b"acme");
}

#[test]
#[ignore = "raising an object's label above medium needs an elevated session"]
fn an_existing_object_labelled_above_medium_is_refused_without_changes() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("acme");
    create_private_directory(&directory).unwrap();
    let status = Command::new("icacls")
        .arg(&directory)
        .args(["/setintegritylevel", "H"])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let before = sddl(&directory);
    assert!(before.contains(";HI)"), "{before}");
    assert!(private(&directory, Kind::Directory).unwrap().is_none());
    assert_eq!(sddl(&directory), before);
}

#[test]
fn restricted_sockets_are_private_and_identity_changes_on_replacement() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("acme");
    create_private_directory(&root).unwrap();
    let path = root.join("manager.sock");
    let listener = Listener::bind(&path).unwrap();
    restrict_socket(&path).unwrap();
    let sid = user_sid();
    let text = sddl(&path);
    assert!(text.starts_with(&format!("O:{sid}")), "{text}");
    assert!(text.contains("D:P"), "{text}");
    assert_eq!(dacl_entries(&text), [format!("(A;;FA;;;{sid})")], "{text}");
    let first = private(&path, Kind::Socket).unwrap().unwrap();
    assert_eq!(socket_identity(&path).unwrap(), Some(first));
    assert_eq!(path_identity(&path).unwrap(), first);

    drop(listener);
    std::fs::remove_file(&path).unwrap();
    let _replacement = Listener::bind(&path).unwrap();
    restrict_socket(&path).unwrap();
    assert_ne!(private(&path, Kind::Socket).unwrap().unwrap(), first);
}

#[test]
fn same_user_peers_pass_in_both_directions_and_connect_is_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("acme");
    create_private_directory(&root).unwrap();
    let path = root.join("manager.sock");
    let listener = Listener::bind(&path).unwrap();
    restrict_socket(&path).unwrap();
    let client = connect(&path, Duration::from_secs(2)).unwrap();
    let (server, _) = listener.accept().unwrap();
    assert!(peer_is_user(&client).unwrap());
    assert!(peer_is_user(&server).unwrap());

    let absent = root.join("absent.sock");
    let started = Instant::now();
    let error = connect(&absent, Duration::from_millis(200)).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2), "{error}");
    assert!(
        matches!(connect(&path, Duration::ZERO), Err(e) if e.kind() == io::ErrorKind::InvalidInput)
    );
}

#[test]
fn a_forged_sid_for_another_account_fails_the_peer_comparison() {
    let user = user().unwrap();
    assert!(is_user(user));
    // The same domain with the final relative ID changed names another account.
    let mut forged = user.0.clone();
    let last = forged.len() - 4;
    let rid = u32::from_le_bytes(forged[last..].try_into().unwrap());
    forged[last..].copy_from_slice(&rid.wrapping_add(1).to_le_bytes());
    assert!(!is_user(&Sid(forged)));
}

#[test]
fn pending_socket_deletion_respects_connection_and_readiness_budgets() {
    use crate::{client, endpoint};
    use windows_sys::Win32::Storage::FileSystem::{FILE_DISPOSITION_INFO, FileDispositionInfo};

    let temporary = tempfile::tempdir().unwrap();
    let root = endpoint::Root::explicit(temporary.path().join("acme")).unwrap();
    let lease = endpoint::Lease::acquire(&root, Duration::ZERO).unwrap();
    let _endpoint = lease.bind(true).unwrap();
    let path = root.socket(true);
    let pending = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .unwrap();
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: 1 };
    // SAFETY: the handle has DELETE access; the pointer and size describe
    // `disposition`, which outlives the call. Keeping the handle holds deletion pending.
    check(unsafe {
        SetFileInformationByHandle(
            pending.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).unwrap(),
        )
    })
    .unwrap();
    // Establish a lasting access denial, rather than relying on a deletion race.
    assert_eq!(
        OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap_err()
            .raw_os_error(),
        i32::try_from(ERROR_ACCESS_DENIED).ok()
    );
    let budget = Duration::from_millis(5);
    let started = Instant::now();
    let result = endpoint::connect(&root, true, budget);
    assert!(
        matches!(result, Err(crate::Error::Io(e)) if e.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok())
    );
    assert!(started.elapsed() < DELETION_SETTLE);
    let started = Instant::now();
    let result = client::connect(
        &root,
        None,
        &client::Options {
            readiness: budget,
            ..client::Options::default()
        },
    );
    assert!(matches!(result, Err(crate::Error::Starting)));
    assert!(started.elapsed() < DELETION_SETTLE);
}

#[test]
fn owner_waits_time_out_report_data_and_observe_eof_without_consuming() {
    let (stream, mut peer) = Stream::pair().unwrap();
    assert!(!wait_readable(&stream, Duration::ZERO).unwrap());
    assert!(!wait_readable(&stream, Duration::from_millis(20)).unwrap());
    peer.write_all(b"x").unwrap();
    assert!(wait_readable(&stream, Duration::from_secs(2)).unwrap());
    assert_eq!(peek(&stream).unwrap(), 1);
    assert_eq!(peek(&stream).unwrap(), 1);

    let (stream, peer) = Stream::pair().unwrap();
    drop(peer);
    assert!(wait_readable(&stream, Duration::from_secs(2)).unwrap());
    assert_eq!(peek(&stream).unwrap(), 0);
}

#[test]
fn account_profile_comes_from_the_token() {
    let profile = profile_directory().unwrap();
    assert!(profile.is_absolute(), "{}", profile.display());
    assert!(profile.is_dir(), "{}", profile.display());
}
