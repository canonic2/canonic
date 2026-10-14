//! Exact-byte signed metadata and catalog schema acceptance.
use super::*;
use crate::{
    test_support::{local, now, release, signed},
    testing::SigningKey,
};
use serde_json::{Value, json};

fn local_registry() -> Source {
    local("/srv/acme-registry")
}

/// A release with an uppercase digest, which catalogs normalize.
fn entry(id: &str, version: &str) -> Value {
    release(id, version, 'A')
}

#[test]
fn signed_metadata_binds_exact_bytes_registry_identity_and_valid_expiry() {
    let key = SigningKey::generate();
    let source = signed(&key);
    let bytes = json!({"schema_version":1,"registry_id":"acme","sequence":1,"expires_at":"9999-12-31T00:00:00Z","packs":[]}).to_string();
    let catalog = Catalog::verify(
        &source,
        bytes.as_bytes(),
        Some(key.sign_hex(bytes.as_bytes()).as_bytes()),
        now(),
    )
    .unwrap();
    assert_eq!(catalog.digest(), digest::sha256_hex(bytes.as_bytes()));
    assert_eq!(catalog.sequence(), Some(1));
    assert_eq!(
        Catalog::verify(
            &source,
            format!("{bytes} ").as_bytes(),
            Some(key.sign_hex(bytes.as_bytes()).as_bytes()),
            now()
        ),
        Err(CatalogError::SignatureInvalid)
    );
    assert_eq!(
        Catalog::verify(&source, bytes.as_bytes(), None, now()),
        Err(CatalogError::SignatureMissing)
    );
    for (field, replacement, error) in [
        (
            "expires_at",
            json!("9999-99-99T00:00:00Z"),
            CatalogError::ExpiryInvalid,
        ),
        (
            "expires_at",
            json!("2000-01-01T00:00:00Z"),
            CatalogError::Expired,
        ),
        ("expires_at", json!(null), CatalogError::ExpiryInvalid),
        ("registry_id", json!("foreign"), CatalogError::Identity),
        ("sequence", json!(null), CatalogError::SequenceMissing),
    ] {
        let mut altered: Value = serde_json::from_str(&bytes).unwrap();
        altered[field] = replacement;
        let altered = altered.to_string();
        assert_eq!(
            Catalog::verify(
                &source,
                altered.as_bytes(),
                Some(key.sign_hex(altered.as_bytes()).as_bytes()),
                now()
            ),
            Err(error),
            "{field}"
        );
    }
}

#[test]
fn a_catalog_signed_by_another_key_is_rejected() {
    let (enrolled, other) = (SigningKey::generate(), SigningKey::generate());
    let bytes = json!({"schema_version":1,"registry_id":"acme","sequence":1,"expires_at":"9999-12-31T00:00:00Z","packs":[]}).to_string();
    assert_eq!(
        Catalog::verify(
            &signed(&enrolled),
            bytes.as_bytes(),
            Some(other.sign_hex(bytes.as_bytes()).as_bytes()),
            now()
        ),
        Err(CatalogError::SignatureInvalid)
    );
}

#[test]
fn malformed_catalogs_refuse() {
    for (document, error) in [
        (
            json!({"schema_version":1,"packs":null}),
            CatalogError::Malformed,
        ),
        (
            json!({"schema_version":2,"packs":[]}),
            CatalogError::Malformed,
        ),
        (
            json!({"schema_version":1,"packs":[],"install_script":"touch marker"}),
            CatalogError::Malformed,
        ),
        (
            json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"../pack.json","sha256":"0".repeat(64)}]}),
            CatalogError::InvalidEntry,
        ),
        (
            json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":"invalid"}]}),
            CatalogError::InvalidEntry,
        ),
        (
            json!({"schema_version":1,"packs":[{"id":"test","version":"0.1.0","path":"pack.json","sha256":"0".repeat(64)}]}),
            CatalogError::Malformed,
        ),
        (
            json!({"schema_version":1,"packs":[{"id":"acme/test","version":"1","path":"pack.json","sha256":"0".repeat(64)}]}),
            CatalogError::Malformed,
        ),
        (
            json!({"schema_version":1,"packs":[entry("acme/a","1.0.0"),entry("acme/a","1.0.0")]}),
            CatalogError::DuplicateRelease,
        ),
        (
            json!({"schema_version":1,"expires_at":"soon","packs":[]}),
            CatalogError::ExpiryInvalid,
        ),
        (
            json!({"schema_version":1,"packs":(0..=MAX_ENTRIES).map(|i| entry("acme/a", &format!("1.0.{i}"))).collect::<Vec<_>>()}),
            CatalogError::Limit,
        ),
    ] {
        assert_eq!(
            Catalog::verify(
                &local_registry(),
                document.to_string().as_bytes(),
                None,
                now()
            ),
            Err(error),
            "{document}"
        );
    }
}

#[test]
fn local_catalogs_need_no_signature_and_normalize_digests() {
    let bytes = json!({"schema_version":1,"packs":[entry("acme/git","1.0.0"),entry("acme/git","1.2.0"),entry("other/git","2.0.0")]}).to_string();
    let catalog = Catalog::verify(&local_registry(), bytes.as_bytes(), None, now()).unwrap();
    assert!(catalog.entries().iter().all(|e| e.sha256 == "a".repeat(64)));
    assert_eq!(catalog.sequence(), None);
    let id: PackId = "acme/git".parse().unwrap();
    assert_eq!(
        catalog
            .releases(&id)
            .iter()
            .map(|e| e.version.to_string())
            .collect::<Vec<_>>(),
        ["1.2.0", "1.0.0"]
    );
    assert!(catalog.release(&id, &"1.0.0".parse().unwrap()).is_some());
    assert!(catalog.release(&id, &"9.0.0".parse().unwrap()).is_none());
    assert_eq!(catalog.lookup("acme/git"), Ok(id));
    assert_eq!(
        catalog.lookup("git"),
        Err(LookupError::Ambiguous(vec![
            "acme/git".parse().unwrap(),
            "other/git".parse().unwrap()
        ]))
    );
    assert_eq!(catalog.lookup("missing"), Err(LookupError::Missing));
    assert_eq!(catalog.lookup("acme/missing"), Err(LookupError::Missing));
    assert_eq!(
        catalog
            .search("git")
            .iter()
            .map(|e| e.id.to_string())
            .collect::<Vec<_>>(),
        ["acme/git", "acme/git", "other/git"]
    );
    assert_eq!(catalog.search("tokens"), Vec::<&Entry>::new());
}

#[test]
fn selection_prefers_the_highest_stable_release_unless_exact() {
    let bytes = json!({"schema_version":1,"packs":[
        entry("acme/git","1.0.0"),entry("acme/git","1.1.0"),entry("acme/git","2.0.0-beta.1")
    ]})
    .to_string();
    let catalog = Catalog::verify(&local_registry(), bytes.as_bytes(), None, now()).unwrap();
    let id: PackId = "acme/git".parse().unwrap();
    assert_eq!(
        catalog.select(&id, None).unwrap().version.to_string(),
        "1.1.0"
    );
    let beta = "2.0.0-beta.1".parse().unwrap();
    assert_eq!(catalog.select(&id, Some(&beta)).unwrap().version, beta);
    assert!(
        catalog
            .select(&id, Some(&"3.0.0".parse().unwrap()))
            .is_none()
    );
    let prerelease_only =
        json!({"schema_version":1,"packs":[entry("acme/next","1.0.0-rc.1")]}).to_string();
    let catalog =
        Catalog::verify(&local_registry(), prerelease_only.as_bytes(), None, now()).unwrap();
    assert!(
        catalog
            .select(&"acme/next".parse().unwrap(), None)
            .is_none()
    );
}

#[test]
fn digests_with_whitespace_and_non_portable_paths_refuse() {
    for entry in [
        json!({"id":"acme/a","version":"1.0.0","path":"pack.yml","sha256":format!(" {}", "a".repeat(63))}),
        json!({"id":"acme/a","version":"1.0.0","path":"pack.yml","sha256":format!("{} ", "a".repeat(64))}),
        json!({"id":"acme/a","version":"1.0.0","path":"packs/CON.yml","sha256":"a".repeat(64)}),
        json!({"id":"acme/a","version":"1.0.0","path":"pack.","sha256":"a".repeat(64)}),
    ] {
        let bytes = json!({"schema_version":1,"packs":[entry]}).to_string();
        assert_eq!(
            Catalog::verify(&local_registry(), bytes.as_bytes(), None, now()),
            Err(CatalogError::InvalidEntry),
            "{entry}"
        );
    }
}

#[test]
fn direct_verification_is_bounded_before_any_signature_or_parse_work() {
    let limit = usize::try_from(MAX_CATALOG_BYTES).unwrap();
    // A valid empty catalog padded with whitespace, and a catalog of many releases.
    let padded = format!(
        "{}{}",
        json!({"schema_version":1,"packs":[]}),
        " ".repeat(limit)
    );
    let many = json!({"schema_version":1,"packs":(0..20_000).map(|i| entry("acme/a", &format!("1.0.{i}"))).collect::<Vec<_>>()}).to_string();
    assert!(many.len() > limit);
    for bytes in [&padded, &many] {
        assert_eq!(
            Catalog::verify(&local_registry(), bytes.as_bytes(), None, now()),
            Err(CatalogError::Limit)
        );
    }
    // A signed source refuses the size before verifying a (valid) signature.
    let key = SigningKey::generate();
    assert_eq!(
        Catalog::verify(
            &signed(&key),
            padded.as_bytes(),
            Some(key.sign_hex(padded.as_bytes()).as_bytes()),
            now()
        ),
        Err(CatalogError::Limit)
    );
    // Exactly at the bound is still parsed.
    let exact = format!(
        "{}{}",
        json!({"schema_version":1,"packs":[]}),
        " ".repeat(limit - json!({"schema_version":1,"packs":[]}).to_string().len())
    );
    assert_eq!(exact.len(), limit);
    assert!(Catalog::verify(&local_registry(), exact.as_bytes(), None, now()).is_ok());
}
