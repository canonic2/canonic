//! Immutable durable source objects shared by review, active and proposal references.
use super::{Error, Store, private, sync};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
};

const KIND: &str = "retained-source-v1";

pub(in crate::review) fn map_digest(
    candidates: &std::collections::BTreeMap<String, Value>,
) -> String {
    if candidates.len() == 1
        && let Some(value) = candidates.get("project")
        && value["kind"] == KIND
        && check(value).is_ok()
        && let Some(digest) = value["candidate_digest"].as_str()
    {
        return digest.to_owned();
    }
    crate::policy::fingerprint(json!(candidates).to_string().as_bytes())
}

pub(super) fn checkpoint(
    store: &Store,
    value: &mut Value,
    hydrate_source: bool,
) -> Result<(), Error> {
    let apply = |value: &mut Value| {
        if hydrate_source {
            hydrate(store, value)
        } else {
            compact(store, value)
        }
    };
    for name in ["candidates", "active"] {
        if !value[name].is_null() {
            if name == "active" {
                apply(&mut value[name]["candidates"])?;
            } else {
                apply(&mut value[name])?;
            }
        }
    }
    if let Some(proposals) = value["proposals"].as_array_mut() {
        for row in proposals {
            for name in ["candidate", "base_candidate"] {
                apply(&mut row[name])?;
            }
        }
    }
    Ok(())
}
pub(super) fn compact(store: &Store, value: &mut Value) -> Result<(), Error> {
    if value["kind"] == "candidate-bundle" {
        let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid("source encoding"))?;
        if bytes.len() > crate::configuration::MAX_BUNDLE_BYTES {
            return Err(crate::policy::Error::Limit.into());
        }
        let digest = crate::policy::fingerprint(&bytes);
        let candidate_digest =
            crate::policy::fingerprint(json!({"project":value}).to_string().as_bytes());
        let directory = store.directory.join("sources");
        super::private_directory(&directory)?;
        let path = directory.join(format!("{digest}.json"));
        if path.exists() {
            if fs::symlink_metadata(&path)?.file_type().is_symlink() || fs::read(&path)? != bytes {
                return Err(Error::Invalid("retained source was replaced"));
            }
        } else {
            // Sources are also retained outside the writer lock, so only a
            // temporary whose writer has exited is removed.
            super::remove_dead_temporaries(&directory, temporary_writer);
            let temporary = path.with_extension(format!(
                "{}-{}.tmp",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            let result = (|| -> Result<(), Error> {
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                private(&mut options);
                let mut file = options.open(&temporary)?;
                file.write_all(&bytes)?;
                sync(&file)?;
                drop(file);
                fs::rename(&temporary, &path)?;
                #[cfg(unix)]
                sync(&fs::File::open(&directory)?)?;
                Ok(())
            })();
            let _ = fs::remove_file(temporary);
            result?;
        }
        *value = json!({"kind":KIND,"digest":digest,"candidate_digest":candidate_digest,"length":bytes.len()});
    } else if value["kind"] == KIND {
        check(value)?;
    } else {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    compact(store, child)?;
                }
            }
            Value::Array(rows) => {
                for child in rows {
                    compact(store, child)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
/// The writer PID of a `<digest>.<pid>-<nanoseconds>.tmp` source temporary.
fn temporary_writer(name: &str) -> Option<u32> {
    let (digest, stamp) = name.strip_suffix(".tmp")?.split_once('.')?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    super::stamp_pid(stamp, 2)
}
pub(in crate::review) fn candidate_identity(value: &Value) -> String {
    if value["kind"] == KIND && check(value).is_ok() {
        return value["digest"].as_str().unwrap_or_default().to_owned();
    }
    crate::policy::fingerprint(value.to_string().as_bytes())
}

fn check(value: &Value) -> Result<(), Error> {
    if value.as_object().is_none_or(|fields| fields.len() != 4)
        || ["digest", "candidate_digest"].iter().any(|key| {
            value[key]
                .as_str()
                .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        || value["length"]
            .as_u64()
            .is_none_or(|n| n > crate::configuration::MAX_BUNDLE_BYTES as u64)
    {
        return Err(Error::Invalid("invalid retained source reference"));
    }
    Ok(())
}

pub(super) fn hydrate(store: &Store, value: &mut Value) -> Result<(), Error> {
    if value["kind"] == KIND {
        check(value)?;
        let digest = value["digest"]
            .as_str()
            .ok_or(Error::Invalid("source digest missing"))?;
        let path = store
            .directory
            .join("sources")
            .join(format!("{digest}.json"));
        let directory = fs::symlink_metadata(store.directory.join("sources"))?;
        let metadata = fs::symlink_metadata(&path)?;
        if directory.file_type().is_symlink()
            || !directory.is_dir()
            || metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() != value["length"].as_u64().unwrap_or_default()
        {
            return Err(Error::Invalid(
                "retained source must be a bounded regular file",
            ));
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take((crate::configuration::MAX_BUNDLE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if crate::policy::fingerprint(&bytes) != digest {
            return Err(Error::Invalid("retained source digest differs"));
        }
        let candidate: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("corrupt retained source"))?;
        if candidate["kind"] != "candidate-bundle"
            || crate::policy::fingerprint(json!({"project":candidate}).to_string().as_bytes())
                != value["candidate_digest"]
        {
            return Err(Error::Invalid("retained candidate binding differs"));
        }
        *value = candidate;
    } else {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    hydrate(store, child)?;
                }
            }
            Value::Array(rows) => {
                for child in rows {
                    hydrate(store, child)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
