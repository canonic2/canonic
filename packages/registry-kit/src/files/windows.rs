//! Native private ACLs and reparse-point refusal for Windows filesystem adapters.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use windows_permissions::{
    LocalBox, SecurityDescriptor,
    constants::{SeObjectType, SecurityInformation},
    wrappers,
};
use winsafe::{HACCESSTOKEN, TokenInfo, co};

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "registry state requires operator-owned Windows entries",
    )
}

pub(super) fn regular(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    Ok(metadata.is_file() && metadata.file_attributes() & 0x0400 == 0)
}

pub(super) fn private(file: &mut File) -> io::Result<()> {
    if file.metadata()?.file_attributes() & 0x0400 != 0 {
        return Err(refused());
    }
    let token = HACCESSTOKEN::GetCurrentProcessToken();
    let TokenInfo::User(user) = token
        .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
        .map_err(|error| io::Error::other(error.to_string()))?
    else {
        return Err(refused());
    };
    let sid = user.User.Sid().ok_or_else(refused)?.to_string();
    let descriptor = wrappers::GetSecurityInfo(
        file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner,
    )?;
    let owner = descriptor.owner().ok_or_else(refused)?.to_string();
    if owner != sid && !["S-1-5-18", "S-1-5-32-544"].contains(&owner.as_str()) {
        return Err(refused());
    }
    let desired: LocalBox<SecurityDescriptor> = format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)")
        .parse()
        .map_err(|_| refused())?;
    wrappers::SetSecurityInfo(
        file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        desired.dacl(),
        None,
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

fn private_directory(path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .access_mode(0x0006_0080)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(refused());
    }
    private(&mut file)
}

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut ancestor = path;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_attributes() & 0x0400 != 0 {
                    return Err(refused());
                }
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(ancestor.to_path_buf());
                ancestor = ancestor
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
            }
            Err(error) => return Err(error),
        }
    }
    for directory in missing.into_iter().rev() {
        // Seal only entries this call created. Never rewrite an existing user
        // configuration directory's ACL or adopt a concurrently created entry.
        std::fs::create_dir(&directory)?;
        private_directory(&directory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_permissions::constants::{AccessRights, AceType};

    #[test]
    fn windows_private_files_and_directories_refuse_inherited_public_access() {
        let root = assert_fs::TempDir::new().unwrap();
        let directory = root.path().join("state/cache");
        super::super::create_private_dir(&directory).unwrap();
        let target = directory.join("artifact");
        super::super::replace_private(&directory.join("temporary"), &target, b"Acme").unwrap();
        for path in [&root.path().join("state"), &directory, &target] {
            let file = OpenOptions::new()
                .read(true)
                .access_mode(0x0002_0080)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(path)
                .unwrap();
            let descriptor = wrappers::GetSecurityInfo(
                &file,
                SeObjectType::SE_FILE_OBJECT,
                SecurityInformation::Dacl,
            )
            .unwrap();
            let acl = descriptor.dacl().unwrap();
            assert_eq!(acl.len(), 2);
            for index in 0..acl.len() {
                let ace = acl.get_ace(index).unwrap();
                assert_eq!(ace.ace_type(), AceType::ACCESS_ALLOWED_ACE_TYPE);
                assert_ne!(ace.sid().unwrap().to_string(), "S-1-1-0");
                assert!(
                    acl.effective_rights(&ace.sid().unwrap().into())
                        .unwrap()
                        .contains(AccessRights::FileAllAccess)
                );
            }
        }
    }

    #[test]
    fn windows_existing_parent_permissions_are_preserved() {
        let root = assert_fs::TempDir::new().unwrap();
        let file = OpenOptions::new()
            .read(true)
            .access_mode(0x0002_0080)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(root.path())
            .unwrap();
        let read = || {
            wrappers::GetSecurityInfo(
                &file,
                SeObjectType::SE_FILE_OBJECT,
                SecurityInformation::Dacl,
            )
            .unwrap()
            .as_sddl()
            .unwrap()
        };
        let before = read();
        create_private_dir(root.path()).unwrap();
        create_private_dir(&root.path().join("state/cache")).unwrap();
        assert_eq!(read(), before);
    }
}
