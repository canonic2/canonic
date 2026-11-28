//! Discover Windows system-wide Stable executables without running them.

use super::{Channel, Error, Result};
use std::{
    ffi::OsString,
    fs,
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::{
    Win32::{
        Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
        },
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFilesX64, FOLDERID_ProgramFilesX86, SHGetKnownFolderPath},
    },
    core::GUID,
};

pub(super) fn inspect(channel: Channel) -> Result<(PathBuf, String)> {
    let (components, folders) = match channel {
        Channel::Chrome => (
            ["Google", "Chrome", "Application", "chrome.exe"],
            [FOLDERID_ProgramFilesX64, FOLDERID_ProgramFilesX86],
        ),
        Channel::Msedge => (
            ["Microsoft", "Edge", "Application", "msedge.exe"],
            [FOLDERID_ProgramFilesX86, FOLDERID_ProgramFilesX64],
        ),
    };
    for folder in &folders {
        let Some(root) = known_folder(folder) else {
            continue;
        };
        if let Some(executable) = executable(root, components).map_err(|()| invalid(channel))? {
            let version = file_version(&executable).ok_or_else(|| invalid(channel))?;
            return Ok((executable, version));
        }
    }
    Err(Error::new(
        "BROWSER_CHANNEL_UNAVAILABLE",
        format!("{} Stable is unavailable", channel.product()),
    ))
}

fn invalid(channel: Channel) -> Error {
    Error::new(
        "BROWSER_CHANNEL_INVALID",
        format!(
            "{} Stable executable identity is invalid",
            channel.product()
        ),
    )
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut path = ptr::null_mut();
    // SAFETY: a native folder GUID and writable pointer are live for the call.
    let result = unsafe { SHGetKnownFolderPath(id, 0, ptr::null_mut(), &raw mut path) };
    let value = if result >= 0 && !path.is_null() {
        // SAFETY: on success the Shell returns an allocated NUL-terminated path.
        let length = (0..)
            .take_while(|offset| unsafe { *path.add(*offset) } != 0)
            .count();
        // SAFETY: the scan found the terminator in the same live allocation.
        let wide = unsafe { std::slice::from_raw_parts(path, length) };
        canonic_process_kit::endpoint::local_path(PathBuf::from(OsString::from_wide(wide))).ok()
    } else {
        None
    };
    // SAFETY: Windows owns the allocation protocol, including an optional null pointer.
    unsafe { CoTaskMemFree(path.cast()) };
    value
}

fn executable(
    mut path: PathBuf,
    components: [&str; 4],
) -> std::result::Result<Option<PathBuf>, ()> {
    let root = fs::symlink_metadata(&path).map_err(|_| ())?;
    if !root.is_dir() || crate::artifacts::is_reparse_point(&root) {
        return Err(());
    }
    for (index, component) in components.into_iter().enumerate() {
        path.push(component);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        };
        let expected_type = if index + 1 == components.len() {
            metadata.is_file()
        } else {
            metadata.is_dir()
        };
        if !expected_type || crate::artifacts::is_reparse_point(&metadata) {
            return Err(());
        }
    }
    Ok(Some(path))
}

fn file_version(path: &Path) -> Option<String> {
    let filename = path
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let mut ignored = 0;
    // SAFETY: filename is a NUL-terminated Windows path and the output is writable.
    let size = unsafe { GetFileVersionInfoSizeW(filename.as_ptr(), &raw mut ignored) };
    if !(1..=1024 * 1024).contains(&size) {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: filename and the writable buffer of size bytes remain alive.
    if unsafe { GetFileVersionInfoW(filename.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let mut address = ptr::null_mut();
    let mut length = 0;
    // SAFETY: the version buffer is initialized; the root subblock and outputs are live.
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            [u16::from(b'\\'), 0].as_ptr(),
            &raw mut address,
            &raw mut length,
        )
    } == 0
        || (length as usize) < size_of::<VS_FIXEDFILEINFO>()
    {
        return None;
    }
    let offset = address.addr().checked_sub(data.as_ptr().addr())?;
    if offset.checked_add(size_of::<VS_FIXEDFILEINFO>())? > data.len() {
        return None;
    }
    // SAFETY: the range above fits inside the live allocation; every field is an integer.
    let version = unsafe {
        data.as_ptr()
            .add(offset)
            .cast::<VS_FIXEDFILEINFO>()
            .read_unaligned()
    };
    if version.dwSignature != 0xFEEF_04BD || version.dwProductVersionMS >> 16 == 0 {
        return None;
    }
    Some(format!(
        "{}.{}.{}.{}",
        version.dwProductVersionMS >> 16,
        version.dwProductVersionMS & 0xffff,
        version.dwProductVersionLS >> 16,
        version.dwProductVersionLS & 0xffff
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_stable_path_requires_a_regular_file() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().to_path_buf();
        let components = ["Google", "Chrome", "Application", "chrome.exe"];
        assert_eq!(executable(root.clone(), components), Ok(None));
        let path = root.join("Google/Chrome/Application/chrome.exe");
        fs::create_dir_all(&path).unwrap();
        assert_eq!(executable(root.clone(), components), Err(()));
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"Acme fixture without version metadata").unwrap();
        assert_eq!(executable(root, components), Ok(Some(path.clone())));
        assert_eq!(file_version(&path), None);
    }

    #[test]
    fn reads_native_pe_product_version_without_launching() {
        let dll = super::super::super::windows::system_directory()
            .unwrap()
            .join("kernel32.dll");
        let version = file_version(&dll).unwrap();
        assert!(super::super::version_major(&version).is_some());
    }
}
