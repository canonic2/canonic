//! Records earlier releases kept that this one never reads: per-user reviewed
//! policies and authority records other than the consumed-grant ledger, and
//! per-folder records of the deleted policy copy. They are removed in bounded
//! batches, never following links, so each run's cost stays small; once they
//! are gone a run costs a few metadata checks and writes nothing.
use std::{fs, io, path::Path};

/// Most entries one run removes.
pub(in crate::review) const BATCH: usize = 256;

/// Remove this user's retired records, a batch per run, from the user's
/// Shield folder. Failures leave records in place for a later run.
pub(crate) fn retire_user_records() {
    #[cfg(not(test))]
    if let Some(base) = user_folder() {
        let _ = retire(&base, BATCH);
    }
}

#[cfg(not(test))]
fn user_folder() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("LOCALAPPDATA");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.map(|home| std::path::PathBuf::from(home).join(".shield"))
        .filter(|path| path.is_absolute())
}

/// Remove up to `budget` retired entries under the user folder `base`, which
/// must be a directory this user owns and not a link. Returns whether all are
/// gone.
///
/// # Errors
/// Reports I/O failures other than missing entries.
pub(in crate::review) fn retire(base: &Path, budget: usize) -> io::Result<bool> {
    let mut left = budget;
    for target in [
        &["reviewed-policies"][..],
        &["authority", "administration"],
        &["authority", "registration"],
        &["authority", "policy"],
    ] {
        if !remove_within(base, target, &mut left)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Remove the entry `parts` names under `base`, as [`remove_tree`] does, once
/// `base` and every folder between it and the entry are directories this user
/// owns and not links. A link anywhere on the way would point the removal at
/// another folder, so then nothing is removed.
///
/// # Errors
/// Reports I/O failures reading the tree.
pub(in crate::review) fn remove_within(
    base: &Path,
    parts: &[&str],
    left: &mut usize,
) -> io::Result<bool> {
    let Some((last, folders)) = parts.split_last() else {
        return Ok(true);
    };
    let mut path = base.to_path_buf();
    for folder in std::iter::once(None).chain(folders.iter().map(Some)) {
        if let Some(folder) = folder {
            path.push(folder);
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && owned(&metadata) => {}
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        }
    }
    remove_tree(&path.join(last), left)
}

/// Remove `path` depth first without following links, one budget unit per
/// entry. Returns false when the budget ran out before it was gone; entries
/// another user owns, or that cannot be removed, are left in place. The caller
/// has checked that no folder above `path` is a link ([`remove_within`]).
///
/// # Errors
/// Reports I/O failures reading the tree.
fn remove_tree(path: &Path, left: &mut usize) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error),
        Ok(metadata) => metadata,
    };
    if !owned(&metadata) {
        return Ok(true);
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            if !remove_tree(&entry?.path(), left)? {
                return Ok(false);
            }
        }
    }
    if *left == 0 {
        return Ok(false);
    }
    *left -= 1;
    // A directory still holding a kept entry stays, as does anything the
    // system refuses to remove; a link is removed itself, never followed.
    let _ = if metadata.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    Ok(true)
}

fn owned(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.uid() == rustix::process::geteuid().as_raw()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_records_go_in_batches_and_the_ledger_and_neighbors_stay() {
        let base = assert_fs::TempDir::new().unwrap();
        let reviewed = base.path().join("reviewed-policies");
        fs::create_dir_all(&reviewed).unwrap();
        for index in 0..5 {
            fs::write(reviewed.join(format!("{index}.json")), b"{}").unwrap();
        }
        let consent = base.path().join("authority/policy");
        fs::create_dir_all(&consent).unwrap();
        fs::write(consent.join("acme.json"), b"{}").unwrap();
        let ledger = base.path().join("authority/consumed");
        fs::create_dir_all(&ledger).unwrap();
        fs::write(ledger.join("grant.json"), b"{}").unwrap();
        let neighbor = base.path().join("registries.json");
        fs::write(&neighbor, b"{}").unwrap();
        #[cfg(unix)]
        {
            let outside = assert_fs::TempDir::new().unwrap();
            fs::write(outside.path().join("kept.json"), b"{}").unwrap();
            std::os::unix::fs::symlink(outside.path(), reviewed.join("link")).unwrap();
            // The first batch runs out before everything is gone.
            assert!(!retire(base.path(), 3).unwrap());
            assert!(retire(base.path(), BATCH).unwrap());
            assert!(outside.path().join("kept.json").exists());
        }
        #[cfg(not(unix))]
        assert!(retire(base.path(), BATCH).unwrap());
        assert!(!reviewed.exists());
        assert!(!consent.exists());
        assert!(ledger.join("grant.json").exists());
        assert!(neighbor.exists());
        // Once done, a run removes and writes nothing.
        assert!(retire(base.path(), 0).unwrap());
        #[cfg(unix)]
        {
            // A linked user folder is never entered.
            let target = assert_fs::TempDir::new().unwrap();
            fs::create_dir_all(target.path().join("reviewed-policies")).unwrap();
            let home = assert_fs::TempDir::new().unwrap();
            let linked = home.path().join(".shield");
            std::os::unix::fs::symlink(target.path(), &linked).unwrap();
            assert!(retire(&linked, BATCH).unwrap());
            assert!(target.path().join("reviewed-policies").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_on_the_way_is_never_removed_through() {
        // `authority` links to another folder holding same-named records.
        let base = assert_fs::TempDir::new().unwrap();
        let elsewhere = assert_fs::TempDir::new().unwrap();
        for name in ["administration", "registration", "policy"] {
            fs::create_dir_all(elsewhere.path().join(name)).unwrap();
            fs::write(elsewhere.path().join(name).join("kept.json"), b"{}").unwrap();
        }
        std::os::unix::fs::symlink(elsewhere.path(), base.path().join("authority")).unwrap();
        fs::create_dir_all(base.path().join("reviewed-policies")).unwrap();
        assert!(retire(base.path(), BATCH).unwrap());
        for name in ["administration", "registration", "policy"] {
            assert!(
                elsewhere.path().join(name).join("kept.json").exists(),
                "{name}"
            );
        }
        // The records outside the link still go.
        assert!(!base.path().join("reviewed-policies").exists());

        // A project whose `.shield/review` links elsewhere keeps that folder.
        let project = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::TempDir::new().unwrap();
        fs::create_dir_all(outside.path().join("portability")).unwrap();
        fs::write(outside.path().join("portability/kept.json"), b"{}").unwrap();
        fs::create_dir(project.path().join(".shield")).unwrap();
        std::os::unix::fs::symlink(outside.path(), project.path().join(".shield/review")).unwrap();
        let mut left = BATCH;
        assert!(
            remove_within(
                project.path(),
                &[".shield", "review", "portability"],
                &mut left
            )
            .unwrap()
        );
        assert!(outside.path().join("portability/kept.json").exists());
        assert_eq!(left, BATCH);
    }
}
