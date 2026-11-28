//! Startup checks on the bound media listener and private grant broker.
use crate::error::{Error, Result};
use std::net::SocketAddr;
#[cfg(unix)]
use std::path::Path;

pub(super) fn check_address(address: SocketAddr) -> Result<()> {
    if !address.ip().is_loopback() {
        return Err(Error::new(
            "MEDIA_UNAVAILABLE",
            "Media listener must be bound to loopback",
        ));
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn check_directory(path: &Path) -> Result<()> {
    // The system temporary root is canonicalized before exclusive creation.
    // Reject a linked leaf as well as linked ancestors of the private directory.
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| Error::new("MEDIA_UNAVAILABLE", "Cannot resolve media broker directory"))?;
    if canonical != path {
        return Err(Error::new(
            "MEDIA_UNAVAILABLE",
            "Media broker directory must not contain symlinks",
        ));
    }
    crate::artifacts::check_private_directory(path).map_err(|_| {
        Error::new(
            "MEDIA_UNAVAILABLE",
            "Media broker directory must be private and owned by this user",
        )
    })
}

#[cfg(unix)]
pub(super) fn check_socket(path: &Path) -> Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    check_directory(
        path.parent()
            .ok_or_else(|| Error::invalid("Media socket has no directory"))?,
    )?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| Error::new("MEDIA_UNAVAILABLE", "Cannot inspect media grant socket"))?;
    // SAFETY: read-only identity query.
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(Error::new(
            "MEDIA_UNAVAILABLE",
            "Media grant socket must be private and owned by this user",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_loopback_addresses() {
        for address in ["127.0.0.1:1234", "[::1]:1234"] {
            check_address(address.parse().unwrap()).unwrap();
        }
        for address in [
            "0.0.0.0:1234",
            "[::]:1234",
            "192.0.2.1:1234",
            "[::ffff:192.0.2.1]:1234",
        ] {
            assert_eq!(
                check_address(address.parse().unwrap()).unwrap_err().code,
                "MEDIA_UNAVAILABLE"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_public_and_linked_broker_directories() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let private = root.join("private");
        crate::artifacts::directory(&private).unwrap();
        check_directory(&private).unwrap();
        symlink(&private, root.join("linked")).unwrap();
        assert!(check_directory(&root.join("linked")).is_err());
        let nested = private.join("nested");
        crate::artifacts::directory(&nested).unwrap();
        assert!(check_directory(&root.join("linked/nested")).is_err());
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_directory(&private).is_err());
        assert!(check_directory(&root.join("absent")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_files_and_symlinks_instead_of_a_socket() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let private = root.join("private");
        crate::artifacts::directory(&private).unwrap();
        let socket = private.join("grants.sock");
        std::fs::write(&socket, []).unwrap();
        assert!(check_socket(&socket).is_err());
        symlink(&socket, private.join("linked.sock")).unwrap();
        assert!(check_socket(&private.join("linked.sock")).is_err());
    }
}
