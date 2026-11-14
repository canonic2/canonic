//! Owned TLS fixtures exercise explicit adoption, continuity and fail-closed reads.
use super::*;
use crate::{
    testing::{Response, Server, SigningKey},
    trust::tests::{now, payload, revocations, signatures},
};
use serde_json::json;
use std::{cell::Cell, time::Duration};

struct Fixture {
    server: Server,
    anchor: SigningKey,
    key: SigningKey,
    source: Source,
    fetcher: Fetcher,
    checkpoints: Checkpoints,
    protected: BTreeMap<String, String>,
}
impl Fixture {
    fn new() -> Self {
        let server = Server::new();
        let anchor = SigningKey::generate();
        let key = SigningKey::generate();
        let source = Source::signed(server.url(), &anchor.public_hex(), "acme-registry")
            .expect("valid owned registry source");
        let fixture = Self {
            server,
            anchor,
            key,
            source,
            fetcher: Server::fetcher(Duration::from_secs(10)),
            checkpoints: Checkpoints::default(),
            protected: BTreeMap::new(),
        };
        let bytes = payload(&fixture.source, 1, &[&fixture.key], 1);
        fixture
            .server
            .set("trust/roots/1.json", Response::ok(bytes.clone()));
        fixture.server.set(
            "trust/roots/1.sig",
            Response::ok(signatures(&[&fixture.anchor, &fixture.key], &bytes)),
        );
        let root = TrustedRoot::bootstrap(
            &fixture.source,
            &bytes,
            &signatures(&[&fixture.anchor, &fixture.key], &bytes),
        )
        .expect("bootstrap owned fixture root");
        fixture.publish(&root, 1, &fixture.key, json!([]), "a");
        fixture
    }
    fn publish(
        &self,
        root: &TrustedRoot,
        sequence: u64,
        key: &SigningKey,
        revoked: serde_json::Value,
        digest: &str,
    ) {
        let bytes = revocations(root, sequence, revoked);
        self.server
            .set("trust/revocations.json", Response::ok(bytes.clone()));
        self.server.set(
            "trust/revocations.sig",
            Response::ok(signatures(&[key], &bytes)),
        );
        self.catalog(sequence, key, digest, "2100-01-01T00:00:00Z");
    }
    fn catalog(&self, sequence: u64, key: &SigningKey, digest: &str, expires_at: &str) {
        let catalog=json!({"schema_version":1,"registry_id":"acme-registry","sequence":sequence,"expires_at":expires_at,"packs":[{"id":"acme/tool","version":"1.0.0","path":"packs/tool.yml","sha256":digest.repeat(64)}]}).to_string().into_bytes();
        self.server
            .set("catalog.json", Response::ok(catalog.clone()));
        self.server
            .set("catalog.sig", Response::ok(signatures(&[key], &catalog)));
    }
    fn update(
        &self,
        previous: Option<&VerifiedUpdate>,
    ) -> Result<VerifiedUpdate, AcquisitionError> {
        self.fetcher.versioned_catalog(
            VersionedRequest {
                source: &self.source,
                previous: previous.map(|update| (update.root(), update.revocations())),
                checkpoints: previous.map_or(&self.checkpoints, VerifiedUpdate::checkpoints),
                protected: &self.protected,
                now: now(),
            },
            &|| Ok(()),
        )
    }
}

#[test]
fn versioned_rotation_keeps_anchor_identity_and_catalog_rollback_protection() {
    let fixture = Fixture::new();
    let first = fixture.update(None).unwrap();
    assert_eq!(first.catalog().source_identity(), fixture.source.identity());
    assert!(first.expected_head_revision().is_none());
    let new = SigningKey::generate();
    let bytes = payload(&fixture.source, 2, &[&new], 1);
    fixture
        .server
        .set("trust/roots/2.json", Response::ok(bytes.clone()));
    fixture.server.set(
        "trust/roots/2.sig",
        Response::ok(signatures(&[&fixture.key, &new], &bytes)),
    );
    let root = first
        .root
        .advance(&bytes, &signatures(&[&fixture.key, &new], &bytes))
        .unwrap();
    fixture.publish(&root, 2, &new, json!([]), "a");
    let next = fixture.update(Some(&first)).unwrap();
    assert_eq!(next.root().source(), &fixture.source);
    assert_eq!(
        next.expected_head_revision(),
        Some(first.head_revision().as_str())
    );
    assert_eq!(next.checkpoints().len(), 1);
    fixture.publish(&root, 3, &new, json!([]), "b");
    assert!(matches!(
        fixture.update(Some(&next)),
        Err(AcquisitionError::Checkpoint(
            crate::checkpoint::CheckpointError::Replaced(_)
        ))
    ));
    assert_eq!(
        next.checkpoints()
            .release_digest(fixture.source.identity(), "acme/tool@1.0.0"),
        Some("a".repeat(64).as_str())
    );
    assert!(
        fixture
            .server
            .requests()
            .iter()
            .all(|path| !path.ends_with("catalog-key.pub"))
    );
}

#[test]
fn missing_or_bad_required_sidecars_never_fall_back_to_legacy() {
    let fixture = Fixture::new();
    let first = fixture.update(None).unwrap();
    let before = fixture.server.requests().len();
    fixture.server.set(
        "trust/revocations.json",
        Response {
            status: 404,
            ..Response::ok(Vec::new())
        },
    );
    assert!(matches!(
        fixture.update(Some(&first)),
        Err(AcquisitionError::Response)
    ));
    let requests = fixture.server.requests();
    assert!(
        !requests[before..]
            .iter()
            .any(|path| path.ends_with("catalog.json"))
    );
    fixture.server.set(
        "trust/roots/2.json",
        Response {
            status: 503,
            ..Response::ok(Vec::new())
        },
    );
    assert!(matches!(
        fixture.update(Some(&first)),
        Err(AcquisitionError::Response)
    ));
    fixture.server.set(
        "trust/roots/2.json",
        Response::ok(payload(&fixture.source, 2, &[&fixture.key], 1)),
    );
    // A document with a missing signature is not an end-of-chain response.
    assert!(matches!(
        fixture.update(Some(&first)),
        Err(AcquisitionError::Response)
    ));
    assert_eq!(first.root().version(), 1);
}

#[test]
fn revoked_cached_or_pinned_entry_is_denied_before_artifact_transport() {
    let fixture = Fixture::new();
    let first = fixture.update(None).unwrap();
    fixture.publish(
        first.root(),
        2,
        &fixture.key,
        json!([{"id":"acme/tool","version":"1.0.0","sha256":"a".repeat(64)}]),
        "a",
    );
    let next = fixture.update(Some(&first)).unwrap();
    let entry = &next.catalog().entries()[0];
    let before = fixture.server.requests().len();
    assert!(matches!(
        next.check_release(entry),
        Err(AcquisitionError::Trust(TrustError::Revoked))
    ));
    assert!(matches!(
        fixture
            .fetcher
            .versioned_artifact(&next, entry, 1024, now()),
        Err(AcquisitionError::Trust(TrustError::Revoked))
    ));
    assert_eq!(fixture.server.requests().len(), before);
    let mut forged = entry.clone();
    forged.sha256 = "b".repeat(64);
    assert!(matches!(
        next.check_release(&forged),
        Err(AcquisitionError::Catalog(
            crate::catalog::CatalogError::InvalidEntry
        ))
    ));
}

#[test]
fn a_retained_update_whose_catalog_alone_expired_refuses_guarded_reads_without_requests() {
    let fixture = Fixture::new();
    // Root and revocations expire in 2100; only the catalog expires at 00:30.
    fixture.catalog(1, &fixture.key, "a", "1970-01-01T00:30:00Z");
    let mut update = fixture.update(None).unwrap();
    let entry = update.catalog().entries()[0].clone();
    let later = now() + Duration::from_hours(1);
    assert!(update.root().check_fresh(later).is_ok());
    assert!(update.revocations().check_fresh(later).is_ok());
    let before = fixture.server.requests().len();
    assert!(matches!(
        fixture
            .fetcher
            .versioned_artifact(&update, &entry, 1024, later),
        Err(AcquisitionError::Catalog(
            crate::catalog::CatalogError::Expired
        ))
    ));
    let id = entry.id.clone();
    assert!(matches!(
        fixture
            .fetcher
            .versioned_history(&mut update, &id, None, later, &|| Ok(())),
        Err(AcquisitionError::Catalog(
            crate::catalog::CatalogError::Expired
        ))
    ));
    assert_eq!(fixture.server.requests().len(), before);
    assert_eq!(update.catalog().entries(), [entry]);
}

#[test]
fn metadata_update_shares_cancellation_deadline_and_byte_budget() {
    let fixture = Fixture::new();
    let first = fixture.update(None).unwrap();
    let revision = first.head_revision();
    let checks = Cell::new(0);
    let result = fixture.fetcher.versioned_catalog(
        VersionedRequest {
            source: &fixture.source,
            previous: Some((first.root(), first.revocations())),
            checkpoints: first.checkpoints(),
            protected: &fixture.protected,
            now: now(),
        },
        &|| {
            checks.set(checks.get() + 1);
            if checks.get() >= 3 {
                Err(AcquisitionError::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(AcquisitionError::Cancelled)));
    assert_eq!(first.head_revision(), revision);
    let mut budget = Budget {
        fetcher: &fixture.fetcher,
        source: &fixture.source,
        deadline: fixture.fetcher.client.deadline(),
        bytes: MAX_UPDATE_BYTES,
        checkpoint: &|| Ok(()),
    };
    assert!(matches!(
        budget.required("catalog.json", 1),
        Err(AcquisitionError::Trust(TrustError::Limit))
    ));
    let timeout = Server::fetcher(Duration::ZERO);
    assert!(matches!(
        timeout.versioned_catalog(
            VersionedRequest {
                source: &fixture.source,
                previous: Some((first.root(), first.revocations())),
                checkpoints: first.checkpoints(),
                protected: &fixture.protected,
                now: now()
            },
            &|| Ok(())
        ),
        Err(AcquisitionError::Timeout)
    ));
}

#[test]
fn too_many_authenticated_root_successors_refuse_before_any_admission() {
    let fixture = Fixture::new();
    // Every document is individually valid; only the aggregate step cap refuses.
    for version in 1..=(MAX_ROOT_STEPS as u64 + 1) {
        let bytes = payload(&fixture.source, version, &[&fixture.key], 1);
        let keys = if version == 1 {
            vec![&fixture.anchor, &fixture.key]
        } else {
            vec![&fixture.key]
        };
        fixture.server.set(
            &format!("trust/roots/{version}.json"),
            Response::ok(bytes.clone()),
        );
        fixture.server.set(
            &format!("trust/roots/{version}.sig"),
            Response::ok(signatures(&keys, &bytes)),
        );
    }
    assert!(matches!(
        fixture.update(None),
        Err(AcquisitionError::Trust(TrustError::Limit))
    ));
    assert!(fixture.checkpoints.is_empty());
    assert!(
        fixture
            .server
            .requests()
            .iter()
            .all(|path| !path.ends_with("catalog.json"))
    );
}
