//! The per-user single-use ledger for one-time grants. A grant is spent once
//! for this user, whichever folder holds it; nothing is keyed by folder.
use super::{Error, private_files};
use serde_json::json;
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

/// Most expired ledger entries one claim removes, so a claim's cost stays
/// bounded however many grants were consumed before.
const PRUNE: usize = 64;

/// The ledger folder: `authority/consumed` in the user's Shield folder.
fn ledger() -> Result<PathBuf, Error> {
    // Tests share one ledger per test binary, as one user's processes share
    // their home.
    #[cfg(test)]
    let base = std::env::temp_dir().canonicalize()?.join(format!(
        "shield-authority-{}",
        crate::policy::fingerprint(std::env::current_exe()?.to_string_lossy().as_bytes())
    ));
    #[cfg(not(test))]
    let base = {
        #[cfg(windows)]
        let home = std::env::var_os("LOCALAPPDATA");
        #[cfg(not(windows))]
        let home = std::env::var_os("HOME");
        home.map(|home| PathBuf::from(home).join(".shield"))
            .filter(|path| path.is_absolute())
            .ok_or(Error::Invalid("user Shield storage unavailable"))?
            .join("authority")
    };
    if let Some(parent) = base.parent()
        && !parent.exists()
    {
        super::private_directory(parent)?;
    }
    private_files::directory(&base, true)?;
    let path = base.join("consumed");
    private_files::directory(&path, true)?;
    Ok(path)
}

/// Spend a one-time grant once for this user, whichever folder holds it.
/// Returns false when it was already spent. A crash after creating the entry
/// leaves it spent, never reusable.
pub(in crate::review) fn claim(grant: &str, expires_at_ms: u64) -> Result<bool, Error> {
    let digest = crate::json::fingerprint(&json!(["shield-grant-v1", grant]))
        .map_err(|_| Error::Invalid("authority content encoding"))?;
    let directory = ledger()?;
    prune(&directory);
    let path = directory.join(format!("{digest}.json"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    super::private(&mut options);
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(
                &serde_json::to_vec(&json!({"expires_at_ms":expires_at_ms}))
                    .map_err(|_| Error::Invalid("ledger encoding"))?,
            )?;
            super::sync(&file)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Remove a bounded number of entries whose grants have expired. An entry is
/// needed only while its grant could still be presented.
fn prune(directory: &Path) {
    let Ok(now) = crate::review::clock::now_ms() else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if removed == PRUNE {
            break;
        }
        let path = entry.path();
        let expired = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value["expires_at_ms"].as_u64())
            .is_some_and(|expires| expires < now);
        if expired && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
}
