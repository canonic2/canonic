//! Native Windows fixtures for held-inode DACL controls, never user state.
use super::*;
use windows_permissions::{
    LocalBox, SecurityDescriptor,
    constants::{SeObjectType, SecurityInformation},
    wrappers,
};

fn restrict(path: &Path, denied: u32) {
    let token = winsafe::HACCESSTOKEN::GetCurrentProcessToken();
    let winsafe::TokenInfo::User(user) = token
        .GetTokenInformation(winsafe::co::TOKEN_INFORMATION_CLASS::User)
        .unwrap()
    else {
        panic!("user token");
    };
    let sid = user.User.Sid().unwrap().to_string();
    let desired: LocalBox<SecurityDescriptor> =
        format!("D:P(D;;0x{denied:x};;;{sid})(A;;FA;;;{sid})(A;;FA;;;SY)")
            .parse()
            .unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    wrappers::SetSecurityInfo(
        &file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        desired.dacl(),
        None,
    )
    .unwrap();
}

#[test]
fn history_control_windows_available_direction_repairs_after_lease_without_replacing_lock() {
    for denied in [1, 2] {
        let (root, context) = enrolled();
        let store = Store::open(root.path()).unwrap();
        drop(lock(&store).unwrap());
        let path = store.directory.join("history.lock");
        let original = crate::windows_security::inspect_regular(&path).unwrap();
        let identity = winapi_util::file::information(&original).unwrap();
        restrict(&path, denied);
        assert!(lock(&store).is_err());
        let held = lock_control(&store).unwrap();
        crate::windows_security::check_private_file(&held).unwrap();
        let after = winapi_util::file::information(&held).unwrap();
        assert_eq!(
            (identity.volume_serial_number(), identity.file_index()),
            (after.volume_serial_number(), after.file_index())
        );
        assert!(lock(&store).is_err());
        drop(held);
        assert!(lock(&store).is_ok());
        assert_eq!(
            store.read_checkpoint().unwrap().project_id,
            context.project_id
        );
    }
}

#[test]
fn history_control_windows_denied_both_directions_refuses_without_normalization() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    drop(lock(&store).unwrap());
    let path = store.directory.join("history.lock");
    restrict(&path, 3);
    assert!(lock_control(&store).is_err());
    assert!(crate::windows_security::check_private(&path).is_err());
    // Restore only this fixture so its temporary-directory cleanup can proceed.
    crate::windows_security::make_private(&path).unwrap();
}
