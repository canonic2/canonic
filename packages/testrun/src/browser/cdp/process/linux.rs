//! User-namespace-restricted Ubuntu hosts can use the vendor setuid sandbox.

use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub(super) fn private_chrome_sandbox() -> Option<&'static Path> {
    // Chromium loads this helper by path. Keep every component under
    // administrator control and reject symlinks and writable directories.
    let path = Path::new("/opt/google/chrome/chrome-sandbox");
    let helper = fs::symlink_metadata(path).ok()?;
    if !helper.is_file()
        || helper.uid() != 0
        || helper.gid() != 0
        || helper.mode() & 0o7777 != 0o4755
    {
        return None;
    }
    path.ancestors()
        .skip(1)
        .all(|parent| {
            fs::symlink_metadata(parent).is_ok_and(|directory| {
                directory.is_dir() && directory.uid() == 0 && directory.mode() & 0o022 == 0
            })
        })
        .then_some(path)
}
