//! Real object leases, private namespace boundaries and sparse-file admission.
use super::*;

#[test]
fn storage_reclamation_work_backlog_is_distinct_from_a_busy_owner() {
    let root = assert_fs::TempDir::new().unwrap();
    super::super::private_directory(root.path()).unwrap();
    for index in 0..FILE_LIMIT * 4 + 20 {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        options
            .open(root.path().join(format!("{index:064x}.bin")))
            .unwrap();
    }
    let mut directory = Directory::enter(root.path()).unwrap();
    assert!(matches!(
        directory.sweep(&BTreeSet::new(), |name| Path::new(name)
            .extension()
            .is_some_and(|ext| ext == "bin")),
        Err(Error::CleanupPending)
    ));
    directory
        .sweep(&BTreeSet::new(), |name| {
            Path::new(name).extension().is_some_and(|ext| ext == "bin")
        })
        .unwrap();
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn storage_reclamation_oversized_legacy_directories_make_bounded_cleanup_progress() {
    let root = assert_fs::TempDir::new().unwrap();
    super::super::private_directory(root.path()).unwrap();
    for index in 0..FILE_LIMIT + 3 {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        options
            .open(root.path().join(format!("{index:064x}.bin")))
            .unwrap();
    }
    let mut directory = Directory::enter(root.path()).unwrap();
    directory
        .sweep(&BTreeSet::new(), |name| {
            name.strip_suffix(".bin").is_some_and(|s| s.len() == 64)
        })
        .unwrap();
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert!(root.path().join(".objects.lock").exists());
    assert!(directory.admit(1, 1, false).is_ok());
}

#[test]
fn storage_reclamation_refreshes_publication_roots_under_exclusive_object_leases() {
    let root = assert_fs::TempDir::new().unwrap();
    super::super::private_directory(root.path()).unwrap();
    let path = root.path().join("published.bin");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    options.open(&path).unwrap();
    let mut directory = Directory::enter(root.path()).unwrap();
    directory
        .sweep_refresh(
            &BTreeSet::new(),
            |_| true,
            |keep| {
                // Model a checkpoint committed after the initial root capture.
                // A new preparer cannot take a reader lease during this refresh.
                let reader = read_file(&path)?;
                assert!(reader.try_lock_shared().is_err());
                keep.insert("published.bin".into());
                Ok(())
            },
        )
        .unwrap();
    assert!(path.exists());
    directory.sweep(&BTreeSet::new(), |_| true).unwrap();
    assert!(!path.exists());
}

#[test]
fn storage_reclamation_live_reader_pins_release_without_deleting_foreign_names() {
    let root = assert_fs::TempDir::new().unwrap();
    super::super::private_directory(root.path()).unwrap();
    for name in ["obsolete.bin", "reader.bin", "acme-not-owned"] {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        options.open(root.path().join(name)).unwrap();
    }
    let mut directory = Directory::enter(root.path()).unwrap();
    let pins = directory
        .pins(&BTreeSet::from(["reader.bin".into()]))
        .unwrap();
    directory
        .sweep(&BTreeSet::new(), |name| {
            Path::new(name).extension().is_some_and(|ext| ext == "bin")
        })
        .unwrap();
    assert!(!root.path().join("obsolete.bin").exists());
    assert!(root.path().join("reader.bin").exists());
    assert!(root.path().join("acme-not-owned").exists());
    drop(pins);
    directory
        .sweep(&BTreeSet::new(), |name| {
            Path::new(name).extension().is_some_and(|ext| ext == "bin")
        })
        .unwrap();
    assert!(!root.path().join("reader.bin").exists());
    assert!(root.path().join("acme-not-owned").exists());
}

#[cfg(unix)]
#[test]
fn storage_reclamation_reserves_recovery_bytes_before_any_new_write() {
    let root = assert_fs::TempDir::new().unwrap();
    super::super::private_directory(root.path()).unwrap();
    let path = root.path().join("pinned.bin");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    // POSIX ftruncate creates a sparse extent: no multi-gigabyte payload is written.
    options
        .open(&path)
        .unwrap()
        .set_len(BYTE_LIMIT - RESERVE)
        .unwrap();
    let mut directory = Directory::enter(root.path()).unwrap();
    let keep = BTreeSet::from(["pinned.bin".into()]);
    directory.sweep(&keep, |_| true).unwrap();
    assert!(directory.admit(1, 1, false).is_err());
    assert!(directory.admit(RESERVE, 1, true).is_ok());
    assert!(directory.admit(RESERVE + 1, 1, true).is_err());
    assert_eq!(fs::metadata(&path).unwrap().len(), BYTE_LIMIT - RESERVE);
}

#[cfg(unix)]
#[test]
fn storage_reclamation_never_follows_links_or_deletes_linked_targets() {
    use std::os::unix::fs::symlink;
    for hard in [false, true] {
        let root = assert_fs::TempDir::new().unwrap();
        super::super::private_directory(root.path()).unwrap();
        let foreign = assert_fs::TempDir::new().unwrap();
        let target = foreign.path().join("acme-original.bin");
        fs::write(&target, b"Acme bytes").unwrap();
        let path = root.path().join("orphan.bin");
        if hard {
            fs::hard_link(&target, &path).unwrap();
        } else {
            symlink(&target, &path).unwrap();
        }
        let mut directory = Directory::enter(root.path()).unwrap();
        directory
            .sweep(&BTreeSet::new(), |name| {
                Path::new(name).extension().is_some_and(|ext| ext == "bin")
            })
            .unwrap();
        assert!(directory.admit(1, 1, false).is_ok());
        assert_eq!(fs::read(&target).unwrap(), b"Acme bytes");
        assert!(fs::symlink_metadata(path).is_ok());
    }
}
