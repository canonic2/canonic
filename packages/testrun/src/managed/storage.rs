//! Private bounded manager records, atomically published without input payloads.
use crate::{
    artifacts,
    error::{Error, Result},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{BufReader, Read, Write},
    path::Path,
};

const MAX_RECORD_BYTES: u64 = 1024 * 1024;

#[cfg(windows)]
pub(crate) fn directory(path: &Path) -> Result<()> {
    canonic_process_kit::endpoint::prepare_directory(path)
        .map(drop)
        .map_err(|error| super::host::failure(&error))
}

#[cfg(unix)]
pub(crate) fn directory(path: &Path) -> Result<()> {
    if !path.exists()
        && let Err(error) = artifacts::directory(path)
        && !path.exists()
    {
        return Err(error);
    }
    artifacts::check_private_directory(path)
}
pub(crate) fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Error::new("STATE_IO", "Cannot encode runtime state"))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Runtime record has no directory"))?;
    artifacts::check_private_directory(parent)?;
    let temporary = parent.join(format!("{}.partial", uuid::Uuid::new_v4()));
    artifacts::write(&temporary, &bytes)?;
    publish(&temporary, path, parent)
}

/// Keep the staging path attributable to its durable owner after process loss.
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Runtime record has no directory"))?;
    artifacts::check_private_directory(parent)?;
    let temporary = path.with_added_extension("partial");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| Error::new("EVIDENCE_IO", "Could not create evidence artifact"))?;
    let written = file.write_all(bytes).and_then(|()| artifacts::sync(&file));
    drop(file);
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(Error::new(
            "EVIDENCE_IO",
            "Could not complete evidence artifact",
        ));
    }
    publish(&temporary, path, parent)
}

fn publish(temporary: &Path, path: &Path, parent: &Path) -> Result<()> {
    #[cfg(unix)]
    let moved = fs::rename(temporary, path);
    #[cfg(windows)]
    let moved = artifacts::windows::publish(temporary, path, true);
    let result = moved.and_then(|()| artifacts::sync_directory(parent));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|_| Error::new("STATE_IO", "Cannot publish runtime state"))
}
pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    read_bytes(path)?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|_| Error::new("STATE_IO", "Runtime record is damaged"))
        })
        .transpose()
}

/// Stream an explicitly bounded record through a selective deserializer.
pub(crate) fn read_streamed<T: DeserializeOwned>(path: &Path, max: u64) -> Result<Option<T>> {
    let Some(file) = open_record(path, max)? else {
        return Ok(None);
    };
    let mut input = BufReader::new(file.take(max + 1));
    let value = serde_json::from_reader(&mut input);
    if input.get_ref().limit() == 0 {
        return Err(Error::new("STATE_IO", "Runtime record exceeds limit"));
    }
    value
        .map(Some)
        .map_err(|_| Error::new("STATE_IO", "Runtime record is damaged"))
}
/// Reads a record whose numeric `version` field is checked before its other
/// fields, so another format is reported as that format rather than as damage.
pub(crate) fn read_versioned<T: DeserializeOwned>(
    path: &Path,
    record: &str,
    supported: u64,
) -> Result<Option<T>> {
    read_bytes(path)?
        .map(|bytes| versioned(&bytes, path, record, supported))
        .transpose()
}
/// Decodes `bytes` after checking their `version` against `supported`. Another
/// version is refused with `STATE_FORMAT`, naming the observed and supported
/// versions, the path and the recovery; the file itself is never changed.
pub(crate) fn versioned<T: DeserializeOwned>(
    bytes: &[u8],
    path: &Path,
    record: &str,
    supported: u64,
) -> Result<T> {
    #[derive(serde::Deserialize)]
    struct Header {
        version: u64,
    }
    let damaged = || {
        Error::new("STATE_IO", format!("The {record} is damaged"))
            .details(serde_json::json!({"path":path,"recovery":"Inspect or remove the file; Testrun leaves it unchanged"}))
    };
    let header: Header = serde_json::from_slice(bytes).map_err(|_| damaged())?;
    if header.version != supported {
        let recovery = if header.version > supported {
            "Upgrade Testrun, or use the newer Testrun that wrote it; this file is unchanged"
        } else {
            "No released Testrun wrote this version; inspect or remove the file, which is unchanged"
        };
        return Err(Error::new(
            "STATE_FORMAT",
            format!(
                "The {record} has format version {}; this Testrun reads version {supported}",
                header.version
            ),
        )
        .details(serde_json::json!({"path":path,"observedVersion":header.version,"supportedVersion":supported,"recovery":recovery})));
    }
    serde_json::from_slice(bytes).map_err(|_| damaged())
}
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let Some(file) = open_record(path, MAX_RECORD_BYTES)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new("STATE_IO", "Cannot read runtime record"))?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(Error::new("STATE_IO", "Runtime record exceeds limit"));
    }
    Ok(Some(bytes))
}

fn open_record(path: &Path, max: u64) -> Result<Option<fs::File>> {
    artifacts::check_private_directory(
        path.parent()
            .ok_or_else(|| Error::invalid("Runtime record has no directory"))?,
    )?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    artifacts::no_follow(&mut options);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::new("STATE_IO", "Cannot read runtime state")),
    };
    if !file
        .metadata()
        .is_ok_and(|m| regular_record(&m) && m.len() <= max)
    {
        return Err(Error::new("STATE_IO", "Invalid runtime record"));
    }
    Ok(Some(file))
}

fn regular_record(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    if artifacts::is_reparse_point(metadata) {
        return false;
    }
    metadata.is_file()
}
