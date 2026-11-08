//! Signed lazy history uses real TLS, retained checkpoints and immutable artifacts.
use canonic_registry_kit::{
    acquisition::AcquisitionError,
    catalog::{Catalog, history::Reference},
    checkpoint::{CheckpointError, Checkpoints},
    digest,
    testing::{Response, Server, SignedRegistry},
};
use serde_json::json;
use std::time::{Duration, Instant, SystemTime};

#[test]
fn hosted_history_pages_share_one_deadline_without_partial_admission() {
    let mut registry = SignedRegistry::new("acme");
    let entries = [
        registry.release("acme/box", "1.0.0", b"old"),
        registry.release("acme/box", "1.0.1", b"head"),
    ];
    // Each response fits a two-second request budget, but two cannot fit
    // the shared budget. Checking only between reads would take three seconds.
    let delay = Duration::from_millis(1500);
    let mut children = Vec::new();
    for entry in &entries {
        let bytes = json!({"schema_version":1,"id":"acme/box","entries":[entry]}).to_string();
        let sha256 = digest::sha256_hex(bytes.as_bytes());
        let path = format!("metadata/{sha256}.json");
        registry.server().set(
            &path,
            Response {
                delay,
                ..Response::ok(bytes)
            },
        );
        children.push(Reference {
            path,
            sha256,
            first: entry.version.clone(),
            last: entry.version.clone(),
        });
    }
    let bytes = json!({"schema_version":1,"id":"acme/box","children":children}).to_string();
    let sha256 = digest::sha256_hex(bytes.as_bytes());
    let path = format!("metadata/{sha256}.json");
    registry.server().set(
        &path,
        Response {
            delay,
            ..Response::ok(bytes)
        },
    );
    registry.publish_bytes(
        json!({"schema_version":3,"registry_id":"acme","sequence":1,
            "expires_at":"9999-12-31T00:00:00Z","packs":[entries[1]],
            "histories":{"acme/box":{"path":path,"sha256":sha256,
                "first":"1.0.0","last":"1.0.1"}}})
        .to_string()
        .as_bytes(),
    );
    let discovery = Server::fetcher(Duration::from_secs(5))
        .discover(registry.server().url(), SystemTime::now())
        .unwrap();
    let mut catalog = discovery.catalog;
    let before = catalog.clone();
    let start = registry.server().requests().len();
    let began = Instant::now();
    assert!(matches!(
        Server::fetcher(Duration::from_secs(2)).history(
            &discovery.source,
            &mut catalog,
            &"acme/box".parse().unwrap(),
            None,
        ),
        Err(AcquisitionError::Timeout)
    ));
    // Allow scheduling overhead, but not a full independent wait for page two.
    assert!(began.elapsed() < Duration::from_millis(2750));
    assert_eq!(registry.server().requests().len() - start, 2);
    assert_eq!(catalog, before);
}

#[test]
fn history_old_pins_and_unrelated_growth_keep_lookup_work_bounded() {
    let mut registry = SignedRegistry::new("acme");
    let mut original = None;
    for index in 0..130 {
        let entry = registry.release("acme/box", &format!("1.0.{index}"), b"original");
        if index == 3 {
            original = Some(entry);
        }
    }
    registry.release("acme/other", "1.0.0", b"other");
    registry.publish_bytes(
        serde_json::json!({"schema_version":1,"registry_id":"acme","sequence":1,
        "expires_at":"9999-12-31T00:00:00Z","packs":[original]})
        .to_string()
        .as_bytes(),
    );
    let fetcher = Server::fetcher(Duration::from_secs(2));
    let discovery = fetcher
        .discover(registry.server().url(), SystemTime::now())
        .unwrap();
    let source = discovery.source;
    let id = "acme/box".parse().unwrap();
    let version = "1.0.3".parse().unwrap();
    let mut checkpoints = Checkpoints::default();
    let protected = std::collections::BTreeMap::from([(
        "acme/box@1.0.3".into(),
        original.as_ref().unwrap().sha256.clone(),
    )]);
    checkpoints.admit(&discovery.catalog).unwrap();
    registry.publish_indexed(2, "9999-12-31T00:00:00Z");
    let mut catalog = fetcher.catalog(&source, SystemTime::now()).unwrap();
    checkpoints.admit_retaining(&catalog, &protected).unwrap();
    let start = registry.server().requests().len();
    fetcher
        .history(&source, &mut catalog, &id, Some(&version))
        .unwrap();
    assert_eq!(registry.server().requests().len() - start, 2);
    assert_eq!(catalog.release(&id, &version), original.as_ref());
    checkpoints.admit_retaining(&catalog, &protected).unwrap();
    assert_eq!(
        fetcher
            .artifact(&source, catalog.release(&id, &version).unwrap(), 1024)
            .unwrap(),
        b"original"
    );
    for index in 1..200 {
        registry.release("acme/other", &format!("1.0.{index}"), b"other");
    }
    registry.publish_indexed(3, "9999-12-31T00:00:00Z");
    let mut next = fetcher.catalog(&source, SystemTime::now()).unwrap();
    let start = registry.server().requests().len();
    fetcher
        .history(&source, &mut next, &id, Some(&version))
        .unwrap();
    assert_eq!(registry.server().requests().len() - start, 2);
    checkpoints.admit_retaining(&next, &protected).unwrap();
    registry.publish_indexed(2, "9999-12-31T00:00:00Z");
    assert_eq!(
        checkpoints.admit(&fetcher.catalog(&source, SystemTime::now()).unwrap()),
        Err(CheckpointError::Rollback)
    );
    // Valid new metadata signed by the same publisher cannot replace an observed old release.
    let mut document = serde_json::to_value(original.unwrap()).unwrap();
    document["sha256"] = serde_json::json!("b".repeat(64));
    let bytes = serde_json::json!({"schema_version":1,"id":id,"entries":[document]}).to_string();
    let hash = canonic_registry_kit::digest::sha256_hex(bytes.as_bytes());
    registry
        .server()
        .set("replacement.json", Response::ok(bytes));
    let bytes=serde_json::json!({"schema_version":3,"registry_id":"acme","sequence":4,
        "expires_at":"9999-12-31T00:00:00Z","packs":[document],
        "histories":{"acme/box":{"path":"replacement.json","sha256":hash,"first":"1.0.3","last":"1.0.3"}}}).to_string();
    let catalog = Catalog::verify(
        &source,
        bytes.as_bytes(),
        Some(registry.key().sign_hex(bytes.as_bytes()).as_bytes()),
        SystemTime::now(),
    )
    .unwrap();
    assert!(matches!(
        checkpoints.admit(&catalog),
        Err(CheckpointError::Replaced(_))
    ));
}
