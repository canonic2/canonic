//! Private operation continuity, independent of portable policy consent.
use super::{Error, State, Store, history, objects, private_directory, private_files};
use serde_json::{Value, json};
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

fn nonce(directory: &Path, create: bool) -> Result<(String, (u64, u64)), Error> {
    if create {
        if let Some(parent) = directory.parent()
            && !parent.exists()
        {
            private_directory(parent)?;
        }
        private_files::directory(directory, true)?;
    } else {
        private_files::directory(directory, false)?;
    }
    let path = directory.join("host.json");
    if create && !path.try_exists()? {
        let _guard = objects::Directory::enter(directory)?;
        if !path.try_exists()? {
            let value = crate::policy::fingerprint(
                history::identifier("runtime", &crate::policy::fingerprint(b"host"))?.as_bytes(),
            );
            private_files::write(&path, &json!({"schema_version":1,"nonce":value}), LIMIT)?;
        }
    }
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

pub(super) fn observe(store: &Store, create: bool) -> Result<String, Error> {
    observe_at(store, &directory()?, create)
}

fn observe_at(store: &Store, directory: &Path, create: bool) -> Result<String, Error> {
    let root = identity(Path::new(&store.root))?;
    let review = identity(&store.directory)?;
    let host = nonce(directory, create)?;
    crate::json::fingerprint(&json!([
        "shield-operation-incarnation-v1",
        root,
        review,
        host
    ]))
    .map_err(|_| Error::Invalid("operation continuity encoding"))
}

pub(super) fn matches(state: &State, observed: &str) -> bool {
    state.operator["runtime_incarnation"]["schema_version"] == 1
        && state.operator["runtime_incarnation"]["digest"] == observed
}

pub(super) fn stamp(state: &mut State, observed: &str) {
    state.operator["runtime_incarnation"] = json!({"schema_version":1,"digest":observed});
}

pub(in crate::review) fn check(state: &State) -> Result<(), Error> {
    let store = Store::inspect(Path::new(&state.project_root))?;
    let observed = observe(&store, false).map_err(|_| {
        Error::Invalid("operation continuity unavailable; policy remains available")
    })?;
    if matches(state, &observed) {
        Ok(())
    } else {
        Err(Error::StaleSnapshot)
    }
}

pub(in crate::review) fn readiness(state: &State, root: &Path) -> Value {
    let result = Store::inspect(root).and_then(|store| observe(&store, false));
    match result {
        Ok(observed) if matches(state, &observed) => json!({"available":true}),
        Ok(_) => {
            json!({"available":false,"reason":"runtime incarnation changed","next_action":"The next runtime operation establishes a fresh local operation epoch without another policy review"})
        }
        Err(_) => json!({"available":false,"reason":"private operation continuity unavailable"}),
    }
}

#[cfg(test)]
mod tests;
