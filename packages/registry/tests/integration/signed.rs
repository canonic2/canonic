//! Signed sources: first-use discovery, authentication, rollback and replacement.
use canonic_registry::{
    acquisition::AcquisitionError,
    catalog::CatalogError,
    checkpoint::{CheckpointError, Checkpoints},
    testing::{Response, Server, SignedRegistry, SigningKey},
};
use std::time::{Duration, SystemTime};

const FUTURE: &str = "9999-12-31T00:00:00Z";

fn now() -> SystemTime {
    SystemTime::now()
}

#[test]
fn discovery_trusts_the_published_key_only_when_it_signs_the_catalog() {
    let mut registry = SignedRegistry::new("acme");
    registry.release("acme/git", "1.0.0", b"git pack");
    registry.publish(1, FUTURE);
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let location = registry.server().url().trim_end_matches('/');
    let discovery = fetcher.discover(location, now()).unwrap();
    assert_eq!(discovery.source.location(), registry.server().url());
    assert_eq!(discovery.source.registry_id(), Some("acme"));
    assert_eq!(
        discovery.source.key(),
        Some(registry.key().public_hex().as_str())
    );
    assert_eq!(discovery.catalog.entries().len(), 1);
    // The reviewed catalog is the one whose signature was checked: one fetch each.
    assert_eq!(
        registry.server().requests(),
        [
            "/registry/catalog-key.pub",
            "/registry/catalog.json",
            "/registry/catalog.sig"
        ]
    );

    let impostor = SigningKey::generate();
    registry
        .server()
        .set("catalog-key.pub", Response::ok(impostor.public_hex()));
    assert!(matches!(
        fetcher.discover(location, now()),
        Err(AcquisitionError::Catalog(CatalogError::SignatureInvalid))
    ));
    registry
        .server()
        .set("catalog-key.pub", Response::ok("not a key"));
    assert!(matches!(
        fetcher.discover(location, now()),
        Err(AcquisitionError::Key)
    ));
}

#[test]
fn discovery_identifies_a_source_however_its_host_and_key_are_spelled() {
    let mut registry = SignedRegistry::new("acme");
    registry.release("acme/git", "1.0.0", b"git pack");
    registry.publish(1, FUTURE);
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let canonical = fetcher
        .discover(registry.server().url(), now())
        .unwrap()
        .source;
    // The operator types the host in capitals; the origin serves its key in upper case.
    registry.server().set(
        "catalog-key.pub",
        Response::ok(registry.key().public_hex().to_ascii_uppercase()),
    );
    let spelled = fetcher
        .discover(
            &registry.server().url().replace("localhost", "LocalHost"),
            now(),
        )
        .unwrap()
        .source;
    assert_eq!(spelled, canonical);
}

#[test]
fn enrolled_keys_are_never_replaced_by_the_network() {
    let mut registry = SignedRegistry::new("acme");
    registry.release("acme/git", "1.0.0", b"git pack");
    registry.publish(1, FUTURE);
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let enrolled = fetcher
        .discover(registry.server().url(), now())
        .unwrap()
        .source;
    // The origin rotates to a new key and re-signs; the enrolled key no longer verifies.
    let rotated = SigningKey::generate();
    let bytes = fetcher.read(&enrolled, "catalog.json", 4096).unwrap();
    registry
        .server()
        .set("catalog-key.pub", Response::ok(rotated.public_hex()));
    registry
        .server()
        .set("catalog.sig", Response::ok(rotated.sign_hex(&bytes)));
    assert!(matches!(
        fetcher.catalog(&enrolled, now()),
        Err(AcquisitionError::Catalog(CatalogError::SignatureInvalid))
    ));
}

#[test]
fn expiry_and_missing_registry_ids_refuse() {
    let registry = SignedRegistry::new("acme");
    registry.publish(1, "2000-01-01T00:00:00Z");
    let fetcher = Server::fetcher(Duration::from_secs(2));
    assert!(matches!(
        fetcher.discover(registry.server().url(), now()),
        Err(AcquisitionError::Catalog(CatalogError::Expired))
    ));
    registry.publish_bytes(
        br#"{"schema_version":1,"sequence":1,"expires_at":"9999-12-31T00:00:00Z","packs":[]}"#,
    );
    assert!(matches!(
        fetcher.discover(registry.server().url(), now()),
        Err(AcquisitionError::RegistryId)
    ));
}

#[test]
fn rollback_and_release_replacement_are_refused_by_checkpoints() {
    let mut registry = SignedRegistry::new("acme");
    let original = registry.release("acme/git", "1.0.0", b"git pack");
    registry.publish(2, FUTURE);
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let source = fetcher
        .discover(registry.server().url(), now())
        .unwrap()
        .source;
    let mut checkpoints = Checkpoints::default();
    checkpoints
        .admit(&fetcher.catalog(&source, now()).unwrap())
        .unwrap();

    registry.publish(1, FUTURE);
    assert_eq!(
        checkpoints.admit(&fetcher.catalog(&source, now()).unwrap()),
        Err(CheckpointError::Rollback)
    );

    let bytes = serde_json::json!({
        "schema_version":1,"registry_id":"acme","sequence":3,"expires_at":FUTURE,
        "packs":[{"id":"acme/git","version":"1.0.0","path":original.path,"sha256":canonic_registry::digest::sha256_hex(b"different bytes")}]
    })
    .to_string();
    registry.publish_bytes(bytes.as_bytes());
    assert_eq!(
        checkpoints.admit(&fetcher.catalog(&source, now()).unwrap()),
        Err(CheckpointError::Replaced("acme/git@1.0.0".into()))
    );
    assert_eq!(
        fetcher.artifact(&source, &original, 1024).unwrap(),
        b"git pack"
    );
}
