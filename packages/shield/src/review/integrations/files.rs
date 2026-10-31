//! Contained native registration files and resumable publication.
use super::{Error, MAX_CONFIG};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Change {
    pub(in crate::review) path: String,
    pub(in crate::review) before: Option<String>,
    pub(in crate::review) after: Option<String>,
}

fn checked_path(root: &Path, name: &str) -> Result<std::path::PathBuf, Error> {
    if !crate::agents::ADAPTERS.iter().any(|a| {
        a.spec().hooks_path == name
            || a.spec().mcp_path == Some(name)
            || a.spec().prefix_rules_path == Some(name)
    }) {
        return Err(Error::Invalid("invalid owned integration path"));
    }
    let mut path = root.to_path_buf();
    for part in Path::new(name).components() {
        if !matches!(part, Component::Normal(_)) {
            return Err(Error::Invalid("invalid integration path"));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if linked(&metadata) => {
                return Err(Error::Invalid("linked integration configuration path"));
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}
pub(in crate::review) fn read(root: &Path, name: &str) -> Result<Option<String>, Error> {
    let path = checked_path(root, name)?;
    match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && m.len() <= MAX_CONFIG => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if m.nlink() != 1 {
                    return Err(Error::Invalid("linked integration configuration"));
                }
            }
            let mut text = String::new();
            #[cfg(windows)]
            let file = crate::windows_security::read_regular(&path)?;
            #[cfg(not(windows))]
            let file = fs::File::open(path)?;
            file.take(MAX_CONFIG + 1).read_to_string(&mut text)?;
            if text.len() as u64 > MAX_CONFIG {
                return Err(Error::Invalid("integration configuration exceeds limit"));
            }
            Ok(Some(text))
        }
        Ok(_) => Err(Error::Invalid(
            "integration configuration must be a bounded regular file",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
/// Compose edits against pending contents, retaining one original snapshot and
/// publication per path, including paths shared by adapter configuration.
pub(in crate::review) fn edit(
    root: &Path,
    changes: &mut Vec<Change>,
    name: &str,
    transform: impl FnOnce(Option<&str>) -> Result<String, Error>,
) -> Result<(), Error> {
    let index = changes.iter().position(|change| change.path == name);
    let before = match index {
        Some(index) => changes[index].after.clone(),
        None => read(root, name)?,
    };
    let after = transform(before.as_deref())?;
    if after.len() as u64 > MAX_CONFIG {
        return Err(Error::Invalid("native configuration exceeds limit"));
    }
    if let Some(index) = index {
        changes[index].after = Some(after);
    } else {
        changes.push(Change {
            path: name.into(),
            before,
            after: Some(after),
        });
    }
    Ok(())
}
pub(in crate::review) fn validate_before(root: &Path, files: &[Change]) -> Result<(), Error> {
    for f in files {
        if read(root, &f.path)? != f.before {
            return Err(Error::Invalid(
                "integration files changed during confirmation",
            ));
        }
    }
    Ok(())
}
pub(in crate::review) fn validate_recovery(root: &Path, files: &[Change]) -> Result<(), Error> {
    for f in files {
        let value = read(root, &f.path)?;
        if value != f.before && value != f.after {
            return Err(Error::Invalid(
                "integration files changed after confirmed setup",
            ));
        }
    }
    Ok(())
}
/// Creates the replacement for `target` with the target's current permissions,
/// or the user's default file permissions when it does not exist yet, so
/// Shield's edits never change who can read a settings file.
pub(in crate::review) fn create_like(temporary: &Path, target: &Path) -> Result<fs::File, Error> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0x4006_0080);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o666);
    }
    let file = options.open(temporary)?;
    #[cfg(windows)]
    let file = {
        let mut file = file;
        match crate::windows_security::copy_dacl(&mut file, target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(temporary);
                return Err(error.into());
            }
        }
        file
    };
    match fs::metadata(target) {
        Ok(existing) => file.set_permissions(existing.permissions())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(file)
}

pub(in crate::review) fn publish(root: &Path, files: &[Change]) -> Result<(), Error> {
    validate_recovery(root, files)?;
    for f in files {
        if read(root, &f.path)? == f.after {
            continue;
        }
        let path = checked_path(root, &f.path)?;
        let parent = path
            .parent()
            .ok_or(Error::Invalid("integration file parent"))?;
        fs::create_dir_all(parent)?;
        checked_path(root, &f.path)?;
        if let Some(after) = &f.after {
            let tmp = parent.join(format!(".shield-{}.tmp", std::process::id()));
            let mut file = create_like(&tmp, &path)?;
            let result = (|| {
                file.write_all(after.as_bytes())?;
                crate::review::storage::sync(&file)?;
                if read(root, &f.path)? != f.before {
                    return Err(Error::Invalid(
                        "integration file changed during publication",
                    ));
                }
                crate::review::storage::replace(&tmp, &path)?;
                Ok::<(), Error>(())
            })();
            drop(file);
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
        } else {
            if read(root, &f.path)? != f.before {
                return Err(Error::Invalid("integration file changed during removal"));
            }
            fs::remove_file(&path)?;
        }
        #[cfg(unix)]
        crate::review::storage::sync(&fs::File::open(parent)?)?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::{
        constants::{SeObjectType, SecurityInformation},
        wrappers,
    };

    fn dacl(path: &Path) -> std::ffi::OsString {
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0x0002_0080)
            .custom_flags(0x0020_0000)
            .open(path)
            .expect("open settings security descriptor");
        wrappers::GetSecurityInfo(
            &file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl,
        )
        .expect("read settings DACL")
        .as_sddl()
        .expect("encode settings DACL")
    }

    #[test]
    fn windows_settings_replacements_preserve_private_and_inherited_permissions() {
        let root = assert_fs::TempDir::new().unwrap();
        let target = root.path().join("settings.json");
        for private in [false, true] {
            fs::write(&target, "Acme").unwrap();
            if private {
                crate::windows_security::make_private(&target).unwrap();
            }
            let temporary = root.path().join("replacement");
            drop(create_like(&temporary, &target).unwrap());
            assert_eq!(dacl(&target), dacl(&temporary));
            fs::remove_file(temporary).unwrap();
        }
    }

    #[test]
    fn windows_settings_reads_refuse_hard_link_aliases() {
        let root = assert_fs::TempDir::new().unwrap();
        fs::create_dir(root.path().join(".claude")).unwrap();
        let target = root.path().join(".claude/settings.local.json");
        fs::write(&target, "{}").unwrap();
        assert_eq!(
            read(root.path(), ".claude/settings.local.json")
                .unwrap()
                .as_deref(),
            Some("{}")
        );
        let alias = root.path().join("alias");
        fs::hard_link(&target, &alias).unwrap();
        assert!(read(root.path(), ".claude/settings.local.json").is_err());
        fs::remove_file(alias).unwrap();
        assert!(read(root.path(), ".claude/settings.local.json").is_ok());
    }
}
