//! Per-user Windows state protection through safe handle-based security APIs.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use windows_permissions::{
    LocalBox, SecurityDescriptor, Sid,
    constants::{AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation},
    wrappers,
};
use winsafe::{self as w, co};

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Shield state requires private Windows permissions",
    )
}

fn current_sid() -> io::Result<String> {
    let token = w::HACCESSTOKEN::GetCurrentProcessToken();
    let information = token
        .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let w::TokenInfo::User(user) = information else {
        return Err(refused());
    };
    Ok(user.User.Sid().ok_or_else(refused)?.to_string())
}

fn open(path: &Path, modify: bool) -> io::Result<File> {
    // READ_CONTROL and FILE_READ_ATTRIBUTES; mutation additionally needs WRITE_DAC.
    let access = 0x0002_0080 | if modify { 0x0004_0000 } else { 0 };
    let file = OpenOptions::new()
        .read(true)
        .access_mode(access)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    if file.metadata()?.file_attributes() & 0x0400 != 0 {
        return Err(refused());
    }
    Ok(file)
}

fn descriptor(file: &File) -> io::Result<LocalBox<SecurityDescriptor>> {
    wrappers::GetSecurityInfo(
        file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner | SecurityInformation::Dacl,
    )
}

// The Windows Modules Installer service owns protected operating-system images.
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

fn trusted_system_sid(sid: &Sid) -> bool {
    ["S-1-5-18", "S-1-5-32-544", TRUSTED_INSTALLER].contains(&sid.to_string().as_str())
}

fn check_system(sd: &SecurityDescriptor) -> io::Result<()> {
    let refuse = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows system resource is not protected from ordinary-user writes",
        )
    };
    if sd.owner().is_none_or(|sid| !trusted_system_sid(sid)) {
        return Err(refuse());
    }
    let acl = sd.dacl().ok_or_else(refuse)?;
    if acl.len() > 128 {
        return Err(refuse());
    }
    for index in 0..acl.len() {
        let ace = acl.get_ace(index).ok_or_else(refuse)?;
        if ace.flags().contains(AceFlags::InheritOnly)
            || ace.ace_type() == AceType::ACCESS_DENIED_ACE_TYPE
        {
            continue;
        }
        // Generic/all writes, file data/EA/attributes, child deletion, DELETE,
        // WRITE_DAC and WRITE_OWNER can change code or its launch namespace.
        if ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE
            || (ace.mask().bits() & 0x500d_0156 != 0
                && ace.sid().is_none_or(|sid| !trusted_system_sid(sid)))
        {
            return Err(refuse());
        }
    }
    Ok(())
}

/// Hold protected OS code without write/delete sharing, or an OS directory
/// without delete sharing. Security reads remain separate from per-user state.
pub(crate) fn open_system(path: &Path, directory: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(if directory { 3 } else { 1 })
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & 0x0400 != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsupported Windows system resource",
        ));
    }
    check_system(descriptor(&file)?.as_ref())?;
    unchanged(path, &file)?;
    Ok(file)
}

fn owner_matches(sd: &SecurityDescriptor, user: &Sid) -> bool {
    // SYSTEM and Administrators retain the OS's privileged owner authority.
    sd.owner().is_some_and(|owner| {
        owner == user || ["S-1-5-18", "S-1-5-32-544"].contains(&owner.to_string().as_str())
    })
}

fn check(sd: &SecurityDescriptor, user: &Sid) -> io::Result<()> {
    if !owner_matches(sd, user) {
        return Err(refused());
    }
    let acl = sd.dacl().ok_or_else(refused)?;
    if acl.len() > 128 {
        return Err(refused());
    }
    for index in 0..acl.len() {
        let ace = acl.get_ace(index).ok_or_else(refused)?;
        if ace.ace_type() == AceType::ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE
            || ace
                .sid()
                .is_none_or(|sid| sid != user && sid.to_string() != "S-1-5-18")
        {
            return Err(refused());
        }
    }
    let trustee = user.into();
    if !acl
        .effective_rights(&trustee)
        .map_err(|error| io::Error::other(error.to_string()))?
        .contains(AccessRights::FileAllAccess)
    {
        return Err(refused());
    }
    Ok(())
}

fn unchanged(path: &Path, file: &File) -> io::Result<()> {
    let before = winapi_util::file::information(file)?;
    let current = open(path, false)?;
    let after = winapi_util::file::information(&current)?;
    if (before.volume_serial_number(), before.file_index())
        != (after.volume_serial_number(), after.file_index())
    {
        return Err(refused());
    }
    Ok(())
}

/// Hold a regular settings file without following a swapped final reparse point.
pub(crate) fn read_regular(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x0400 != 0
        || winapi_util::file::information(&file)?.number_of_links() != 1
    {
        return Err(refused());
    }
    unchanged(path, &file)?;
    Ok(file)
}

/// Verify a private state file or directory without changing it.
pub(crate) fn check_private(path: &Path) -> io::Result<()> {
    let file = open(path, false)?;
    let user: LocalBox<Sid> = current_sid()?.parse().map_err(|_| refused())?;
    check(descriptor(&file)?.as_ref(), &user)?;
    unchanged(path, &file)
}

/// Restrict an operator-owned state directory before it receives private data.
pub(crate) fn make_private(path: &Path) -> io::Result<()> {
    let mut file = open(path, true)?;
    let sid = current_sid()?;
    let user: LocalBox<Sid> = sid.parse().map_err(|_| refused())?;
    if !owner_matches(descriptor(&file)?.as_ref(), &user) {
        return Err(refused());
    }
    let desired: LocalBox<SecurityDescriptor> = format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)")
        .parse()
        .map_err(|_| refused())?;
    wrappers::SetSecurityInfo(
        &mut file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        desired.dacl(),
        None,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    check(descriptor(&file)?.as_ref(), &user)?;
    unchanged(path, &file)
}

/// Preserve a settings file's discretionary access and inheritance on replacement.
pub(crate) fn copy_dacl(replacement: &mut File, target: &Path) -> io::Result<()> {
    let source = open(target, false)?;
    if !source.metadata()?.is_file()
        || winapi_util::file::information(&source)?.number_of_links() != 1
    {
        return Err(refused());
    }
    let original = wrappers::GetSecurityInfo(
        &source,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )?;
    let sddl = original.as_sddl()?;
    let flags = sddl
        .to_str()
        .ok_or_else(refused)?
        .split('(')
        .next()
        .ok_or_else(refused)?;
    let inheritance = if flags
        .strip_prefix("D:")
        .is_some_and(|flags| flags.contains('P'))
    {
        SecurityInformation::ProtectedDacl
    } else {
        SecurityInformation::UnprotectedDacl
    };
    wrappers::SetSecurityInfo(
        replacement,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | inheritance,
        None,
        None,
        original.dacl(),
        None,
    )?;
    let copied = wrappers::GetSecurityInfo(
        replacement,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )?;
    if copied.as_sddl()? != sddl {
        return Err(refused());
    }
    unchanged(target, &source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_system_images_allow_shared_reads_but_no_untrusted_writer_or_owner() {
        for source in [
            "O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;BU)(A;;GR;;;WD)".to_owned(),
            format!("O:{TRUSTED_INSTALLER}D:P(A;;FA;;;{TRUSTED_INSTALLER})(A;;GRGX;;;BU)"),
        ] {
            let descriptor: LocalBox<SecurityDescriptor> = source.parse().unwrap();
            check_system(&descriptor).unwrap();
        }
        for source in [
            "O:SYD:P(A;;FA;;;SY)(A;;GW;;;WD)".to_owned(),
            "O:SYD:P(A;;FA;;;SY)(A;;SD;;;BU)".to_owned(),
            "O:SYD:P(A;;FA;;;SY)(A;;WD;;;BU)".to_owned(),
            "O:BUD:P(A;;FA;;;SY)".to_owned(),
        ] {
            let descriptor: LocalBox<SecurityDescriptor> = source.parse().unwrap();
            assert!(check_system(&descriptor).is_err(), "{source}");
        }
        let image =
            std::path::PathBuf::from(winsafe::GetSystemDirectory().unwrap()).join("hostname.exe");
        let held = open_system(&image, false).unwrap();
        assert!(held.metadata().unwrap().is_file());
    }

    #[test]
    fn private_state_rejects_broad_access_and_protects_new_children() {
        let root = assert_fs::TempDir::new().unwrap();
        let state = root.path().join("state");
        std::fs::create_dir(&state).unwrap();
        make_private(&state).unwrap();
        check_private(&state).unwrap();
        let child = state.join("data.json");
        std::fs::write(&child, "Acme").unwrap();
        check_private(&child).unwrap();
        let mut file = open(&child, true).unwrap();
        let sid = current_sid().unwrap();
        let broad: LocalBox<SecurityDescriptor> =
            format!("D:P(A;;FA;;;{sid})(A;;FR;;;WD)").parse().unwrap();
        wrappers::SetSecurityInfo(
            &mut file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
            None,
            None,
            broad.dacl(),
            None,
        )
        .unwrap();
        assert!(check_private(&child).is_err());
        assert_eq!(std::fs::read_to_string(&child).unwrap(), "Acme");
        make_private(&child).unwrap();
        check_private(&child).unwrap();
    }
}
