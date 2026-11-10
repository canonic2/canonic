//! Serialize derived publication independently of request authority/handoff.
use crate::review::{
    Context, Error,
    storage::{objects::Directory, private_files},
};
use std::path::{Path, PathBuf};

/// Runtime writers may restore permissions lost while copying an owned runtime
/// folder. Inspectors never call this, and links/foreign ownership stay errors.
fn normalize_owned_runtime(path: &Path) -> Result<(), Error> {
    let parent = path.parent().ok_or(Error::Invalid("private parent"))?;
    let parent_metadata = std::fs::symlink_metadata(parent)?;
    if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
        return Err(Error::Invalid("private parent cannot be a link or file"));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::Invalid("private directory cannot be a link or file"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if metadata.uid() != parent_metadata.uid() {
                    return Err(Error::Invalid("private directory owner differs"));
                }
            }
            crate::review::storage::private_directory(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            private_files::directory(path, true)?;
        }
        Err(error) => return Err(error.into()),
    }
    private_files::directory(path, false)
}

pub(in crate::review) struct ProjectionLease {
    root: PathBuf,
    lease: Directory,
}
impl ProjectionLease {
    /// Acquire before any authority writer; pass this guard through nested
    /// setup/activation calls rather than reacquiring an ambient lock.
    pub(in crate::review) fn acquire(root: &Path) -> Result<Self, Error> {
        let root = root.canonicalize()?;
        // Validate the containing source directory without changing its mode.
        let shield = root.join(".shield");
        let metadata = std::fs::symlink_metadata(&shield)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid(
                "review directories cannot be links or files",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if metadata.uid() != std::fs::metadata(&root)?.uid() {
                return Err(Error::Invalid("private directory owner differs"));
            }
        }
        normalize_owned_runtime(&root.join(".shield/review"))?;
        let directory = root.join(".shield/review/publication");
        normalize_owned_runtime(&directory)?;
        let lease = Directory::enter(&directory)?;
        Ok(Self { root, lease })
    }
    pub(in crate::review) fn check(&self, context: &Context) -> Result<(), Error> {
        self.lease.check_owner()?;
        if self.root == Path::new(&context.project_root) {
            Ok(())
        } else {
            Err(Error::StaleSnapshot)
        }
    }
}
