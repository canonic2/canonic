//! Caller path binding and the pinned final-extension rule; no directory writes.
use crate::browser::session::Error;
use std::path::{Component, Path, PathBuf};

pub(crate) fn validate(path: &str) -> Result<(), &'static str> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || url::Url::parse(path)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "ws" | "wss" | "file"))
    {
        return Err("TRACE_EXPORT_PATH_INVALID");
    }
    Ok(())
}
pub(crate) fn absolute(path: &str, base: &Path) -> Result<String, &'static str> {
    validate(path)?;
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };
    if !joined.is_absolute() {
        return Err("TRACE_EXPORT_PATH_INVALID");
    }
    let mut resolved = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            part => resolved.push(part.as_os_str()),
        }
    }
    let resolved = resolved.to_str().ok_or("TRACE_EXPORT_PATH_INVALID")?;
    validate(resolved)?;
    Ok(resolved.to_owned())
}
pub(super) fn destination(path: &str) -> Result<PathBuf, Error> {
    validate(path)?;
    let base = if Path::new(path).is_absolute() {
        PathBuf::from("/")
    } else {
        std::env::current_dir().map_err(|_| Error::from("TRACE_EXPORT_IO"))?
    };
    let absolute = PathBuf::from(absolute(path, &base)?);
    let mut current = absolute.as_path();
    let mut missing = Vec::new();
    let canonical = loop {
        match std::fs::canonicalize(current) {
            Ok(mut resolved) => {
                for name in missing.into_iter().rev() {
                    resolved.push(name);
                }
                break resolved;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    current
                        .file_name()
                        .ok_or("TRACE_EXPORT_PATH_INVALID")?
                        .to_os_string(),
                );
                current = current.parent().ok_or("TRACE_EXPORT_PATH_INVALID")?;
            }
            Err(_) => return Err("TRACE_EXPORT_IO".into()),
        }
    };
    let value = canonical.to_str().ok_or("TRACE_EXPORT_PATH_INVALID")?;
    let extension = if value.ends_with(".gz") { "gz" } else { "json" };
    Ok(if canonical.file_name().is_some() {
        canonical.with_extension(extension)
    } else {
        canonical.join(format!(".{extension}"))
    })
}
