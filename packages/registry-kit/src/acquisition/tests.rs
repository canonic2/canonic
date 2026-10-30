//! Local source confinement, bounds, digests and discovery.
use super::*;
use assert_fs::{TempDir, prelude::*};
use serde_json::json;

fn now() -> SystemTime {
    SystemTime::now()
}

fn registry(packs: &serde_json::Value) -> TempDir {
    let root = TempDir::new().unwrap();
    root.child("catalog.json")
        .write_str(&json!({"schema_version":1,"packs":packs}).to_string())
        .unwrap();
    root
}

fn fetcher() -> Fetcher {
    Fetcher::new(Duration::from_secs(1))
}

#[test]
fn local_discovery_canonicalizes_and_reads_the_catalog() {
    let artifact = b"schema_version: 1\n";
    let root = registry(
        &json!([{"id":"acme/git","version":"1.0.0","path":"packs/git.yml","sha256":digest::sha256_hex(artifact)}]),
    );
    root.child("packs/git.yml").write_binary(artifact).unwrap();
    let discovery = fetcher()
        .discover(root.path().join("packs/..").to_str().unwrap(), now())
        .unwrap();
    let canonical = root.path().canonicalize().unwrap();
    assert_eq!(discovery.source.location(), canonical.to_str().unwrap());
    assert!(!discovery.source.is_signed());
    let entry = &discovery.catalog.entries()[0];
    assert_eq!(
        fetcher().artifact(&discovery.source, entry, 1024).unwrap(),
        artifact
    );
    assert!(matches!(
        fetcher().artifact(&discovery.source, entry, 4),
        Err(AcquisitionError::TooLarge)
    ));
    root.child("packs/git.yml").write_str("replaced").unwrap();
    assert!(matches!(
        fetcher().artifact(&discovery.source, entry, 1024),
        Err(AcquisitionError::Digest)
    ));
}

#[test]
fn malformed_catalogs_and_non_directories_do_not_discover() {
    let root = registry(&json!(null));
    root.child("marker-source").touch().unwrap();
    assert!(matches!(
        fetcher().discover(root.path().to_str().unwrap(), now()),
        Err(AcquisitionError::Catalog(CatalogError::Malformed))
    ));
    assert!(matches!(
        fetcher().discover(root.path().join("marker-source").to_str().unwrap(), now()),
        Err(AcquisitionError::Location)
    ));
    assert!(matches!(
        fetcher().discover(root.path().join("missing").to_str().unwrap(), now()),
        Err(AcquisitionError::Location)
    ));
    assert!(matches!(
        fetcher().discover("https://example.com/a/../", now()),
        Err(AcquisitionError::Location)
    ));
}

#[test]
fn local_reads_stay_inside_the_canonical_source() {
    let outer = TempDir::new().unwrap();
    outer.child("secret.txt").write_str("secret").unwrap();
    let root = registry(&json!([]));
    let source = Source::local(&root.path().canonicalize().unwrap()).unwrap();
    for path in ["../secret.txt", "/etc/passwd", "a//b"] {
        assert!(matches!(
            fetcher().read(&source, path, 1024),
            Err(AcquisitionError::Path)
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outer.path().join("secret.txt"), root.path().join("link"))
            .unwrap();
        assert!(matches!(
            fetcher().read(&source, "link", 1024),
            Err(AcquisitionError::Escapes)
        ));
        root.child("dir").create_dir_all().unwrap();
        assert!(matches!(
            fetcher().read(&source, "dir", 1024),
            Err(AcquisitionError::Path)
        ));
    }
}

#[cfg(unix)]
#[test]
fn a_local_source_whose_path_no_longer_is_canonical_refuses() {
    let home = TempDir::new().unwrap();
    let root = registry(&json!([]));
    let alias = home.path().join("alias");
    std::os::unix::fs::symlink(root.path(), &alias).unwrap();
    // Enrolled through a path that now resolves elsewhere.
    let source = Source::local(&alias).unwrap();
    assert!(matches!(
        fetcher().catalog(&source, now()),
        Err(AcquisitionError::LocalIdentityChanged)
    ));
}

#[test]
fn a_local_source_gone_from_its_location_refuses_as_moved() {
    let root = registry(&json!([]));
    let source = Source::local(&root.path().canonicalize().unwrap()).unwrap();
    let location = root.path().to_path_buf();
    root.close().unwrap();
    assert!(matches!(
        fetcher().catalog(&source, now()),
        Err(AcquisitionError::LocalIdentityChanged)
    ));
    // A file where the directory was is no source either.
    std::fs::write(&location, "not a directory").unwrap();
    let result = fetcher().catalog(&source, now());
    std::fs::remove_file(&location).unwrap();
    assert!(
        matches!(result, Err(AcquisitionError::LocalIdentityChanged)),
        "{result:?}"
    );
}

#[test]
fn local_catalogs_are_bounded() {
    let root = TempDir::new().unwrap();
    let padding = " ".repeat(usize::try_from(MAX_CATALOG_BYTES).unwrap());
    root.child("catalog.json")
        .write_str(&format!("{{\"schema_version\":1,\"packs\":[]}}{padding}"))
        .unwrap();
    let source = Source::local(&root.path().canonicalize().unwrap()).unwrap();
    assert!(matches!(
        fetcher().catalog(&source, now()),
        Err(AcquisitionError::TooLarge)
    ));
}
