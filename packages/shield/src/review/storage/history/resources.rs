//! Physical admission and kernel-owned reader pins for optional history.
use super::{Error, File, OpenOptions, Path, STORE_LIMIT, State, Store, fs, private, regular};
use crate::review::storage::{objects, private_files};

pub(super) const DIRECTORY_LIMIT: usize = 4096;
const READERS: usize = 8;

pub(in crate::review::storage) fn prepare_readers(store: &Store) -> Result<(), Error> {
    let directory = store.directory.join("history-readers");
    // Directory-copy APIs may preserve private file modes but create folders
    // with ordinary defaults. Only a writer normalizes this owned runtime pool;
    // inspection continues to refuse unsafe/unprepared storage without writes.
    if private_files::directory(&directory, false).is_err() {
        super::super::private_directory(&directory)?;
    }
    private_files::directory(&directory, true)?;
    for index in 0..READERS {
        let path = directory.join(format!("{index}.lease"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        match options.open(&path) {
            Ok(file) => {
                #[cfg(windows)]
                crate::windows_security::make_private_file(&file)?;
                drop(file);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if objects::read_file(&path)?.metadata()?.len() != 0 {
                    return Err(Error::Invalid("history reader lease must be empty"));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn reader(store: &Store) -> Result<File, Error> {
    let directory = store.directory.join("history-readers");
    // Inspection is read-only, including when an older installation has not
    // yet allocated its bounded reader pool. Its next writer prepares it.
    private_files::directory(&directory, false)?;
    for index in 0..READERS {
        let path = directory.join(format!("{index}.lease"));
        let file = objects::read_file(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::Invalid("history reader lease must be empty"));
        }
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::WouldBlock) => {}
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Err(Error::Locked)
}

/// The fixed reader namespace cannot hide retained payload bytes. Unknown or
/// unsafe entries preserve their bytes and make optional storage unavailable.
pub(super) fn reader_bytes(store: &Store) -> Result<u64, Error> {
    let directory = store.directory.join("history-readers");
    match private_files::directory(&directory, false) {
        Ok(()) => {}
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    }
    let mut bytes = 0_u64;
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        crate::diagnostics::trace::count("history.directory_entries", 1);
        if index >= READERS {
            return Err(Error::Invalid("history reader namespace exceeds limit"));
        }
        let entry = entry?;
        if !(0..READERS).any(|slot| entry.file_name() == format!("{slot}.lease").as_str()) {
            return Err(Error::Invalid("unknown history reader namespace entry"));
        }
        bytes = bytes
            .checked_add(objects::read_file(&entry.path())?.metadata()?.len())
            .ok_or(Error::Invalid("history byte limit"))?;
    }
    Ok(bytes)
}

/// The history mutex excludes new captures until deletion completes. Existing
/// readers hold shared file locks, so unlinked files cannot hide pinned storage.
pub(super) fn remove(path: &Path) -> Result<bool, Error> {
    let file = objects::read_file(path)?;
    match file.try_lock() {
        Ok(()) => {
            drop(file);
            fs::remove_file(path)?;
            Ok(true)
        }
        Err(fs::TryLockError::WouldBlock) => Ok(false),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

fn generation(name: &str) -> Option<&str> {
    let (generation, suffix) = name.strip_prefix("history-")?.split_once('.')?;
    (generation.len() == 64
        && generation.bytes().all(|byte| byte.is_ascii_hexdigit())
        && (matches!(suffix, "error" | "json")
            || suffix.strip_suffix(".jsonl").is_some_and(|segment| {
                let (sequence, count) = segment
                    .split_once('.')
                    .map_or((segment, None), |(sequence, count)| (sequence, Some(count)));
                super::number(sequence, Some(10)).is_ok()
                    && count.is_none_or(|count| super::number(count, None).is_ok())
            })))
    .then_some(generation)
}

/// Called at segment/control boundaries, reserving the whole next segment so
/// ordinary appends need no aggregate scan. Known corruption and reader pins
/// are preserved; exhaustion drops optional evidence rather than widening policy.
pub(super) fn admit(store: &Store, state: &State, additional: u64) -> Result<(), Error> {
    let current = super::digest(state)?;
    let mut protected = vec![current];
    if state.history_rewrite.is_some() {
        protected.push(super::digest(&store.read_checkpoint()?)?);
    }
    // Leave space for a bounded current-generation failure record even when
    // admission refuses the observation that would otherwise exhaust storage.
    let pool = reader_bytes(store)?;
    let mut bytes = additional
        .checked_add(super::SEGMENT_LIMIT as u64)
        .and_then(|bytes| bytes.checked_add(pool))
        .ok_or(Error::Invalid("history byte limit"))?;
    let mut reclaim = Vec::new();
    for (index, entry) in fs::read_dir(&store.directory)?.enumerate() {
        crate::diagnostics::trace::count("history.directory_entries", 1);
        if index >= DIRECTORY_LIMIT {
            return Err(Error::Invalid("history directory entry limit"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(charge) = super::inline::charge(store, &entry.path(), name)? {
            bytes = bytes
                .checked_add(charge)
                .ok_or(Error::Invalid("history byte limit"))?;
            continue;
        }
        let old = generation(name);
        if old.is_none()
            && !super::inline::receipt(name)
            && !(name.starts_with(".history-")
                && Path::new(&name).extension().is_some_and(|ext| ext == "tmp"))
        {
            continue;
        }
        regular(&entry.path())?;
        let length = fs::symlink_metadata(entry.path())?.len();
        bytes = bytes
            .checked_add(length)
            .ok_or(Error::Invalid("history byte limit"))?;
        if let Some(generation) = old
            && !protected.iter().any(|keep| keep == generation)
            && Path::new(&name)
                .extension()
                .is_some_and(|ext| ext == "jsonl")
            && !store
                .directory
                .join(format!("history-{generation}.error"))
                .exists()
        {
            reclaim.push((entry.path(), length));
        }
    }
    // Stable filename order bounds cleanup without payload inspection. This
    // evicts only retired generations; their exact command count is unknown.
    reclaim.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, length) in reclaim {
        if bytes <= STORE_LIMIT as u64 {
            break;
        }
        if remove(&path)? {
            bytes -= length;
        }
    }
    if bytes > STORE_LIMIT as u64 {
        return Err(Error::Invalid(
            "combined history byte limit; readers or recovery evidence retain capacity",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sized(path: &Path, size: u64) {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let file = options.open(path).unwrap();
        #[cfg(windows)]
        crate::windows_security::make_private_file(&file).unwrap();
        file.set_len(size).unwrap();
    }

    #[test]
    fn history_reader_leases_are_bounded_reusable_and_keep_files_accounted() {
        let (root, _) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        prepare_readers(&store).unwrap();
        let mut held = (0..READERS)
            .map(|_| reader(&store).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(reader(&store), Err(Error::Locked)));
        held.pop();
        assert!(reader(&store).is_ok());
        let path = store
            .directory
            .join(format!("history-{}.0000000000.1.jsonl", "a".repeat(64)));
        sized(&path, 1024);
        let pin = objects::read_file(&path).unwrap();
        pin.try_lock_shared().unwrap();
        assert!(!remove(&path).unwrap());
        assert!(path.exists());
        drop(pin);
        assert!(remove(&path).unwrap());
    }

    #[test]
    fn history_admission_counts_retired_generations_temporaries_and_corruption() {
        let (root, _) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        let old = store
            .directory
            .join(format!("history-{}.0000000000.1.jsonl", "a".repeat(64)));
        sized(&old, STORE_LIMIT as u64);
        admit(&store, &state, super::super::SEGMENT_LIMIT as u64).unwrap();
        assert!(!old.exists(), "unneeded retired bytes free capacity");
        sized(&old, STORE_LIMIT as u64);
        let error = store
            .directory
            .join(format!("history-{}.error", "a".repeat(64)));
        sized(&error, 1);
        assert!(admit(&store, &state, 1).is_err());
        assert!(
            old.exists() && error.exists(),
            "known corrupt evidence stays intact"
        );
        fs::remove_file(&error).unwrap();
        fs::remove_file(&old).unwrap();
        let pending = store.directory.join(".history-file-crashed.tmp");
        sized(&pending, STORE_LIMIT as u64);
        assert!(admit(&store, &state, 1).is_err());
        assert!(
            pending.exists(),
            "admission never deletes interrupted source bytes"
        );
    }

    #[test]
    fn explicit_history_clear_stays_available_at_capacity_without_precommit_deletion() {
        let (root, context) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        let path = super::super::segments(&store, &state)
            .unwrap()
            .pop()
            .unwrap()
            .path;
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len((STORE_LIMIT - 1024) as u64)
            .unwrap();
        assert!(admit(&store, &state, 1024).is_err());
        crate::review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
        assert!(!path.exists());
        let cleared = store.read_history().unwrap();
        assert!(cleared.history_available && cleared.events.is_empty());
    }
    #[test]
    fn malformed_reader_slots_cannot_hide_bytes_or_be_repaired_by_inspection() {
        let (root, _) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        prepare_readers(&store).unwrap();
        let path = store.directory.join("history-readers/0.lease");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(STORE_LIMIT as u64)
            .unwrap();
        assert_eq!(reader_bytes(&store).unwrap(), STORE_LIMIT as u64);
        assert!(prepare_readers(&store).is_err());
        assert!(reader(&store).is_err());
        assert!(admit(&store, &state, 1).is_err());
        super::super::mark_failure(&store, &state);
        let marker = super::super::error_path(&store, &state).unwrap();
        assert_eq!(fs::metadata(marker).unwrap().len(), 0);
        assert_eq!(fs::metadata(&path).unwrap().len(), STORE_LIMIT as u64);
        fs::remove_file(&path).unwrap();
        prepare_readers(&store).unwrap();
        let foreign = store.directory.join("history-readers/foreign");
        fs::write(&foreign, b"preserve").unwrap();
        assert!(reader_bytes(&store).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"preserve");
    }
    #[test]
    fn inline_recovery_sources_share_history_capacity_and_are_never_reclaimed() {
        let (root, _) = crate::review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        let path = store
            .directory
            .join(format!("upgrade-{}.json", "b".repeat(64)));
        sized(&path, STORE_LIMIT as u64);
        assert!(admit(&store, &state, 1).is_err());
        assert!(
            path.exists(),
            "history admission cannot prune recovery evidence"
        );
    }
}
