//! Recognize a checkpoint written by an earlier format that bound operations
//! to the folder's native identity. Read only, for one conversion: matching
//! means this is the installation that earlier format recorded, upgraded in
//! place. Nothing here is persisted or used as authority afterwards.
use super::{Error, State, Store, private_files};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

const LIMIT: usize = 1024;
#[cfg(test)]
thread_local! { static TEST_DIRECTORY: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }

fn directory() -> Result<PathBuf, Error> {
    #[cfg(test)]
    return if let Some(path) = TEST_DIRECTORY.with(|value| value.borrow().clone()) {
        Ok(path)
    } else {
        // Unit subprocesses are the same local host, even after their parent
        // dies. Explicit thread-local fixtures still model a different host.
        let executable = std::env::current_exe()?;
        let namespace = crate::policy::fingerprint(executable.to_string_lossy().as_bytes());
        Ok(std::env::temp_dir()
            .canonicalize()?
            .join(format!("shield-runtime-incarnation-{namespace}")))
    };
    #[cfg(not(test))]
    {
        #[cfg(windows)]
        let base = std::env::var_os("LOCALAPPDATA");
        #[cfg(not(windows))]
        let base = std::env::var_os("HOME");
        let base = base
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(Error::Invalid("operation continuity storage unavailable"))?;
        Ok(base.join(".shield").join("runtime-incarnation"))
    }
}

fn identity(path: &Path) -> Result<(u64, u64), Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Invalid(
            "operation continuity entry cannot be a link",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0x0080)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        let information = winapi_util::file::information(&file)?;
        Ok((
            u64::from(information.volume_serial_number()),
            information.file_index(),
        ))
    }
}

fn nonce(directory: &Path) -> Result<(String, (u64, u64)), Error> {
    private_files::directory(directory, false)?;
    let path = directory.join("host.json");
    let value = private_files::read(&path, LIMIT)?;
    let nonce = value["nonce"]
        .as_str()
        .filter(|v| valid(v))
        .ok_or(Error::Invalid("operation continuity nonce invalid"))?;
    if value["schema_version"] != 1 || value.as_object().is_none_or(|o| o.len() != 2) {
        return Err(Error::Invalid("operation continuity nonce invalid"));
    }
    Ok((nonce.into(), identity(&path)?))
}

fn valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn observe(store: &Store) -> Result<String, Error> {
    let root = identity(Path::new(&store.root))?;
    let review = identity(&store.directory)?;
    let host = nonce(&directory()?)?;
    crate::json::fingerprint(&json!([
        "shield-operation-incarnation-v1",
        root,
        review,
        host
    ]))
    .map_err(|_| Error::Invalid("operation continuity encoding"))
}

/// Whether a checkpoint carrying the earlier format's identity stamp is the
/// installation it was stamped for. Unavailable evidence proves nothing.
pub(super) fn recorded_here(store: &Store, state: &State) -> bool {
    state.operator["runtime_incarnation"]["schema_version"] == 1
        && observe(store)
            .is_ok_and(|observed| state.operator["runtime_incarnation"]["digest"] == observed)
}
