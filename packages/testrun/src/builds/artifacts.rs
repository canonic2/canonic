//! Bounded artifact selection and private immutable package handoff.
use super::*;
use crate::configuration::Artifact;
use crate::process::Cancellation;
use std::fs;

pub(super) fn stamp_checked(path: &Path, cancel: &Cancellation) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut paths = vec![path.to_owned()];
    let mut count = 0;
    while let Some(path) = paths.pop() {
        cancel.check()?;
        count += 1;
        if count > 20_000 {
            return Err(Error::invalid("Artifact stamp exceeds entry limit"));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| Error::new("BUILD_IO", "Cannot inspect prior artifact"))?;
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update(metadata.len().to_be_bytes());
        hash.update(format!("{:?}", metadata.modified().ok()));
        if metadata.is_dir() {
            let mut entries = Vec::new();
            for entry in fs::read_dir(&path)
                .map_err(|_| Error::new("BUILD_IO", "Cannot inspect prior bundle"))?
                .take(20_001)
            {
                cancel.check()?;
                entries.push(
                    entry.map_err(|_| Error::new("BUILD_IO", "Cannot inspect prior bundle"))?,
                );
            }
            if entries.len() > 20_000 {
                return Err(Error::invalid("Artifact stamp exceeds entry limit"));
            }
            entries.sort_by_key(|entry| entry.file_name());
            paths.extend(entries.into_iter().map(|e| e.path()));
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
pub(super) fn select(
    cwd: &Path,
    selector: &Artifact,
    root: &Path,
    output: &Path,
) -> Result<PathBuf> {
    select_checked(cwd, selector, root, output, &Cancellation::default())
}
pub(super) fn select_checked(
    cwd: &Path,
    selector: &Artifact,
    root: &Path,
    output: &Path,
    cancel: &Cancellation,
) -> Result<PathBuf> {
    cancel.check()?;
    let root = root
        .canonicalize()
        .map_err(|_| Error::new("BUILD_ARTIFACT_PATH", "Project directory is unavailable"))?;
    let output = output.canonicalize().unwrap_or_else(|_| output.to_owned());
    if let Some(path) = &selector.path {
        return contained(&cwd.join(path), &root, &output);
    }
    let pattern = cwd.join(
        selector
            .glob
            .as_deref()
            .ok_or_else(|| Error::invalid("Missing artifact selector"))?,
    );
    let pattern = pattern.to_string_lossy();
    let expression = glob_regex(&pattern)?;
    let first = pattern.find(['*', '?']).unwrap_or(pattern.len());
    let prefix = &pattern[..first];
    let directory = if prefix.ends_with('/') {
        Path::new(prefix.trim_end_matches('/'))
    } else {
        Path::new(prefix).parent().unwrap_or(cwd)
    };
    let mut queue = vec![directory.to_path_buf()];
    let mut matches = Vec::new();
    let mut count = 0;
    while let Some(directory) = queue.pop() {
        cancel.check()?;
        if directory.components().count() > 128 {
            return Err(Error::invalid("Artifact path nesting exceeds limit"));
        }
        let scan = contained(&directory, &root, &output)?;
        for entry in fs::read_dir(scan).map_err(|_| {
            Error::new(
                "BUILD_ARTIFACT_MISSING",
                "Cannot inspect artifact output directory",
            )
        })? {
            cancel.check()?;
            count += 1;
            if count > 50_000 {
                return Err(Error::invalid(
                    "Artifact search exceeds 50000 entries; use a narrower selector",
                ));
            }
            let entry = entry.map_err(|_| Error::new("BUILD_IO", "Cannot inspect build output"))?;
            let path = directory.join(entry.file_name());
            if expression.is_match(&path.to_string_lossy()) {
                matches.push(contained(&entry.path(), &root, &output)?);
            } else if entry.file_type().is_ok_and(|t| t.is_dir())
                && path.extension().and_then(|v| v.to_str()) != Some("app")
            {
                queue.push(path);
            }
        }
    }
    if matches.len() != 1 {
        return Err(Error::new(
            "BUILD_ARTIFACT_SELECTION",
            "Artifact selector must match exactly one package",
        )
        .details(json!({"matches":matches.len()})));
    }
    Ok(matches.remove(0))
}
fn contained(path: &Path, root: &Path, output: &Path) -> Result<PathBuf> {
    let path = path.canonicalize().map_err(|_| {
        Error::new(
            "BUILD_ARTIFACT_MISSING",
            "Build produced no selected artifact",
        )
    })?;
    if !path.starts_with(root) && !path.starts_with(output) {
        return Err(Error::new(
            "BUILD_ARTIFACT_PATH",
            "Artifact escapes the project and owned output directory",
        ));
    }
    Ok(path)
}
fn glob_regex(pattern: &str) -> Result<regex::Regex> {
    let mut output = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    output.push_str("(?:.*/)?");
                } else {
                    output.push_str(".*");
                }
            }
            '*' => output.push_str("[^/]*"),
            '?' => output.push_str("[^/]"),
            '[' | ']' | '{' | '}' => {
                return Err(Error::invalid("Artifact globs support *, ** and ? only"));
            }
            _ => output.push_str(&regex::escape(&c.to_string())),
        }
    }
    output.push('$');
    regex::Regex::new(&output).map_err(|_| Error::invalid("Invalid artifact glob"))
}

#[cfg(test)]
pub(super) fn unpack(path: &Path, output: &Path) -> Result<PathBuf> {
    unpack_checked(path, output, &Cancellation::default())
}
pub(super) fn unpack_checked(path: &Path, output: &Path, cancel: &Cancellation) -> Result<PathBuf> {
    cancel.check()?;
    let directory = output.join("unpacked");
    crate::managed::storage::directory(&directory)?;
    if path.extension().and_then(|v| v.to_str()) == Some("ipa") {
        unpack_ipa(path, &directory, cancel)?;
        return select_checked(
            &directory,
            &Artifact {
                path: None,
                glob: Some("Payload/*.app".into()),
            },
            &directory,
            output,
            cancel,
        );
    }
    let file =
        fs::File::open(path).map_err(|_| Error::new("BUILD_IO", "Cannot open Expo archive"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut size = 0u64;
    for (index, entry) in archive
        .entries()
        .map_err(|_| Error::invalid("Invalid Expo archive"))?
        .enumerate()
    {
        cancel.check()?;
        if index >= 20_000 {
            return Err(Error::invalid("Expo archive exceeds entry limit"));
        }
        let mut entry = entry.map_err(|_| Error::invalid("Invalid Expo archive entry"))?;
        size = size
            .checked_add(entry.size())
            .ok_or_else(|| Error::invalid("Expo archive too large"))?;
        if size > 2 * 1024 * 1024 * 1024
            || !matches!(
                entry.header().entry_type(),
                tar::EntryType::Regular | tar::EntryType::Directory
            )
        {
            return Err(Error::invalid(
                "Expo archive contains unsupported entries or exceeds 2 GiB",
            ));
        }
        let relative = entry
            .path()
            .map_err(|_| Error::invalid("Invalid Expo archive path"))?
            .into_owned();
        if relative.is_absolute()
            || relative.components().any(|component| {
                !matches!(
                    component,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
            || relative.components().count() > 128
        {
            return Err(Error::invalid(
                "Expo archive path escapes output or exceeds nesting limit",
            ));
        }
        let destination = directory.join(relative);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|_| Error::new("BUILD_IO", "Cannot create extracted directory"))?;
        } else {
            fs::create_dir_all(destination.parent().unwrap())
                .map_err(|_| Error::new("BUILD_IO", "Cannot create extracted parent"))?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|_| Error::invalid("Duplicate or invalid Expo archive entry"))?;
            copy_checked(&mut entry, &mut file, cancel)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    destination,
                    fs::Permissions::from_mode(
                        entry
                            .header()
                            .mode()
                            .map_err(|_| Error::invalid("Invalid archive mode"))?
                            & 0o777,
                    ),
                )
                .map_err(|_| Error::new("BUILD_IO", "Cannot preserve executable mode"))?;
            }
        }
    }
    select_checked(
        &directory,
        &Artifact {
            path: None,
            glob: Some("**/*.app".into()),
        },
        &directory,
        output,
        cancel,
    )
}

fn unpack_ipa(path: &Path, root: &Path, cancel: &Cancellation) -> Result<()> {
    let file = fs::File::open(path).map_err(|_| Error::new("BUILD_IO", "Cannot open EAS IPA"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| Error::invalid("Invalid EAS IPA"))?;
    if archive.len() > 20_000 {
        return Err(Error::invalid("IPA exceeds entry limit"));
    }
    let mut size = 0u64;
    for index in 0..archive.len() {
        cancel.check()?;
        let mut entry = archive
            .by_index(index)
            .map_err(|_| Error::invalid("Invalid IPA entry"))?;
        size = size
            .checked_add(entry.size())
            .ok_or_else(|| Error::invalid("IPA too large"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| Error::invalid("IPA entry escapes archive"))?;
        if size > 2 * 1024 * 1024 * 1024
            || relative.components().count() > 128
            || entry.is_symlink()
        {
            return Err(Error::invalid(
                "IPA contains unsupported entries or exceeds limits",
            ));
        }
        let destination = root.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|_| Error::new("BUILD_IO", "Cannot unpack IPA directory"))?;
        } else {
            fs::create_dir_all(destination.parent().unwrap())
                .map_err(|_| Error::new("BUILD_IO", "Cannot unpack IPA parent"))?;
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|_| Error::invalid("Duplicate or invalid IPA file"))?;
            copy_checked(&mut entry, &mut out, cancel)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    destination,
                    fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o600) & 0o777),
                )
                .map_err(|_| {
                    Error::new("BUILD_IO", "Cannot preserve IPA executable permissions")
                })?;
            }
        }
    }
    Ok(())
}
pub(super) fn copy_checked(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    cancel: &Cancellation,
) -> Result<()> {
    let mut buffer = [0; 65536];
    loop {
        cancel.check()?;
        let size = input
            .read(&mut buffer)
            .map_err(|_| Error::new("BUILD_IO", "Cannot read archive entry"))?;
        if size == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..size])
            .map_err(|_| Error::new("BUILD_IO", "Cannot write extracted entry"))?;
    }
}
