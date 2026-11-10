//! Optional byte attribution for observations retained inside immutable backups.
use super::{Error, Store};
use crate::review::storage::{objects, private_files};
use serde::{Deserialize, Serialize};
use std::path::Path;

const RECEIPT_BYTES: usize = 512;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u8,
    source_digest: String,
    source_bytes: u64,
    inline_bytes: u64,
    file_version: String,
}

fn version(file: &std::fs::File) -> Result<Option<String>, Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = file.metadata()?;
        Ok(Some(
            crate::json::fingerprint(&(
                metadata.dev(),
                metadata.ino(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ))
            .map_err(|_| Error::Invalid("history attribution version"))?,
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        // A modified timestamp alone cannot detect preserved-mtime writes.
        Ok(None)
    }
}

fn digest(name: &str) -> Option<&str> {
    let value = name
        .strip_prefix("upgrade-")
        .or_else(|| name.strip_prefix("bootstrap-"))?
        .strip_suffix(".json")?;
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(value)
}
fn receipt_name(name: &str) -> String {
    format!("history-inline-{name}")
}
pub(super) fn receipt(name: &str) -> bool {
    name.strip_prefix("history-inline-").is_some_and(|value| {
        digest(value).is_some()
            || value
                .strip_suffix(".pending")
                .is_some_and(|stem| digest(&format!("{stem}.json")).is_some())
    })
}

/// Conversion already owns these exact bytes. Borrow the original JSON span,
/// including its whitespace, rather than measuring a smaller re-encoding.
pub(in crate::review::storage) fn retain(store: &Store, bytes: &[u8], kind: &str) {
    #[derive(Deserialize)]
    struct Fields<'a> {
        #[serde(borrow)]
        events: Option<&'a serde_json::value::RawValue>,
    }
    if !matches!(kind, "upgrade" | "bootstrap") {
        return;
    }
    let inline_bytes = serde_json::from_slice::<Fields<'_>>(bytes)
        .map_or(bytes.len() as u64, |fields| {
            fields.events.map_or(0, |events| events.get().len()) as u64
        });
    let source_digest = crate::policy::fingerprint(bytes);
    let name = format!("{kind}-{source_digest}.json");
    let Some(file_version) = objects::read_file(&store.directory.join(&name))
        .ok()
        .and_then(|file| version(&file).ok().flatten())
    else {
        return;
    };
    let value = Receipt {
        schema_version: 1,
        source_digest: source_digest.clone(),
        source_bytes: bytes.len() as u64,
        inline_bytes,
        file_version,
    };
    // Missing attribution only increases the later charge. It cannot prevent
    // valid policy conversion or alter the original recovery evidence.
    let _ = private_files::write(
        &store.directory.join(receipt_name(&name)),
        &serde_json::to_value(value).expect("bounded history attribution"),
        RECEIPT_BYTES,
    );
}

pub(in crate::review::storage) fn remove(store: &Store, source_digest: &str, kind: &str) {
    if !matches!(kind, "upgrade" | "bootstrap") {
        return;
    }
    let path = store
        .directory
        .join(receipt_name(&format!("{kind}-{source_digest}.json")));
    if private_files::read(&path, RECEIPT_BYTES).is_ok() {
        let _ = std::fs::remove_file(path);
    }
}

/// Only metadata and a bounded receipt are read at segment boundaries. Unknown
/// attribution charges the whole backup; no recovery source is ever reclaimed.
pub(super) fn charge(store: &Store, path: &Path, name: &str) -> Result<Option<u64>, Error> {
    if name
        .strip_prefix(".upgrade-")
        .or_else(|| name.strip_prefix(".bootstrap-"))
        .and_then(|stamp| stamp.strip_suffix(".tmp"))
        .is_some_and(|stamp| !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()))
    {
        return Ok(Some(objects::read_file(path)?.metadata()?.len()));
    }
    let Some(digest) = digest(name) else {
        return Ok(None);
    };
    let file = objects::read_file(path)?;
    let length = file.metadata()?.len();
    let file_version = version(&file)?;
    let value = private_files::read(&store.directory.join(receipt_name(name)), RECEIPT_BYTES)
        .ok()
        .and_then(|value| serde_json::from_value::<Receipt>(value).ok())
        .filter(|value| {
            file_version.as_deref() == Some(value.file_version.as_str())
                && value.schema_version == 1
                && value.source_digest == digest
                && value.source_bytes == length
                && value.inline_bytes <= length
        });
    Ok(Some(value.map_or(length, |value| value.inline_bytes)))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn inline_attribution_preserves_original_byte_spans_and_refuses_bad_receipts() {
        let (root, _) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let bytes = br#"{"events":[  { "kind" : "decision" }  ],"other":"policy"}"#;
        let digest = crate::policy::fingerprint(bytes);
        let name = format!("upgrade-{digest}.json");
        let path = store.directory.join(&name);
        private_files::write(&path, &serde_json::from_slice(bytes).unwrap(), 1024).unwrap();
        // Keep the exact source spelling, including whitespace.
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            charge(&store, &path, &name).unwrap(),
            Some(bytes.len() as u64)
        );
        retain(&store, bytes, "upgrade");
        assert_eq!(
            charge(&store, &path, &name).unwrap(),
            Some(br#"[  { "kind" : "decision" }  ]"#.len() as u64)
        );
        let receipt = store.directory.join(receipt_name(&name));
        std::fs::write(&receipt, b"broken metadata").unwrap();
        assert_eq!(
            charge(&store, &path, &name).unwrap(),
            Some(bytes.len() as u64)
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        retain(&store, bytes, "upgrade");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut edited = bytes.to_vec();
        let index = edited.iter().position(|byte| *byte == b'd').unwrap();
        edited[index] = b'x';
        std::fs::write(&path, &edited).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            charge(&store, &path, &name).unwrap(),
            Some(bytes.len() as u64),
            "preserved mtime cannot preserve an old attribution hint"
        );
        std::fs::write(&path, b"changed length").unwrap();
        assert_eq!(charge(&store, &path, &name).unwrap(), Some(14));
    }
}
