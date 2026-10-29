//! Exact-byte signed metadata, retained checkpoints and the pinned main registry.
mod dependencies;
mod hosted;
use super::*;
use canonic_registry::testing::SigningKey;

#[test]
fn global_registrations_need_an_absolute_home_directory() {
    let absolute = std::env::temp_dir().join("acme-home");
    assert_eq!(
        global_path(Some(absolute.clone().into_os_string())).unwrap(),
        absolute.join(".shield/registries.json")
    );
    for home in [None, Some(""), Some("acme-home"), Some("./acme")] {
        assert!(matches!(
            global_path(home.map(Into::into)),
            Err(Error::Invalid("home directory unavailable"))
        ));
    }
}

#[test]
fn catalog_timestamps_retain_exact_second_precision_and_calendar_validation() {
    use canonic_registry::timestamp::is_utc_seconds as utc;
    assert!(utc("2000-02-29T12:00:00Z"));
    assert!(utc("0000-02-29T12:00:00Z"));
    assert!(!utc("1900-02-29T12:00:00Z"));
    assert!(!utc("2024-01-01T24:00:00Z"));
    assert!(!utc("2024-01-01T12:00:00.1Z"));
}

#[test]
fn malformed_catalogs_do_not_enroll_local_sources() {
    use assert_fs::prelude::*;
    for document in [
        json!({"schema_version":1,"packs":null}),
        json!({"schema_version":1,"packs":[],"install_script":"touch marker"}),
        json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"../pack.json","sha256":"0".repeat(64)}]}),
        json!({"schema_version":1,"packs":[{"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":"invalid"}]}),
    ] {
        let root = assert_fs::TempDir::new().expect("source");
        root.child("catalog.json")
            .write_str(&document.to_string())
            .expect("catalog");
        assert!(enrollment(&fetcher(), root.path().to_str().expect("path")).is_err());
        assert!(!root.path().join("marker").exists());
    }
}

#[test]
fn release_replacement_and_catalog_rollback_preserve_retained_checkpoints() {
    use assert_fs::prelude::*;
    let (project, context) = crate::review::tests::enrolled();
    let root = assert_fs::TempDir::new().expect("source");
    let document = |sequence, digest| {
        json!({"schema_version":1,"sequence":sequence,"packs":[
        {"id":"acme/test","version":"0.1.0","path":"pack.json","sha256":digest}]})
        .to_string()
    };
    root.child("catalog.json")
        .write_str(&document(2, "0".repeat(64)))
        .expect("catalog");
    let source = enrollment(&fetcher(), root.path().to_str().expect("path")).expect("enrollment");
    checked_catalog(&context, &source).expect("retained catalog");
    let before = Store::open(project.path())
        .expect("store")
        .read()
        .expect("state")
        .registry_checkpoints;
    for (sequence, digest) in [
        (1, "0".repeat(64)),
        (2, "1".repeat(64)),
        (3, "1".repeat(64)),
    ] {
        root.child("catalog.json")
            .write_str(&document(sequence, digest))
            .expect("catalog edit");
        assert!(checked_catalog(&context, &source).is_err());
        assert_eq!(
            Store::open(project.path())
                .expect("store")
                .read()
                .expect("state")
                .registry_checkpoints,
            before
        );
    }
}

#[test]
fn search_matches_raw_substrings_while_info_requires_a_valid_reference() {
    use assert_fs::prelude::*;
    let (project, context) = crate::review::tests::enrolled();
    let root = assert_fs::TempDir::new().expect("source");
    let release =
        |id: &str| json!({"id":id,"version":"1.0.0","path":"pack.yml","sha256":"0".repeat(64)});
    root.child("catalog.json")
        .write_str(
            &json!({"schema_version":1,"packs":[release("acme/git"),release("acme/grep"),release("other/git")]})
                .to_string(),
        )
        .expect("catalog");
    let source = enrollment(&fetcher(), root.path().to_str().expect("path")).expect("enrollment");
    Store::open(project.path())
        .expect("store")
        .update(&context, &|| Ok(()), |state| {
            state.operator["sources"]["local"] = json!(source);
            state.operator["default"] = json!("local");
            Ok(json!({}))
        })
        .expect("registration");
    let found = |query: &str| {
        super::super::packs::search(&context, query).expect(query)["packs"]
            .as_array()
            .expect("packs")
            .len()
    };
    for (query, count) in [
        ("", 3),
        ("git", 2),
        ("acme/", 2),
        ("local/", 3),
        ("local/acme/g", 2),
        ("Acme", 0),
    ] {
        assert_eq!(found(query), count, "{query}");
    }
    for query in ["acme/", "Acme/git"] {
        assert!(matches!(
            super::super::packs::info(&context, query),
            Err(Error::Invalid("invalid pack reference"))
        ));
    }
}

#[test]
fn signed_metadata_binds_exact_bytes_registry_identity_and_valid_expiry() {
    let key = SigningKey::generate();
    let source =
        Source::signed("https://registry.example.com/", &key.public_hex(), "acme").expect("source");
    let verify = |bytes: &str, signature: &str| {
        Catalog::verify(
            &source,
            bytes.as_bytes(),
            Some(signature.as_bytes()),
            SystemTime::now(),
        )
    };
    let bytes=json!({"schema_version":1,"registry_id":"acme","sequence":1,"expires_at":"9999-12-31T00:00:00Z","packs":[]}).to_string();
    let signature = key.sign_hex(bytes.as_bytes());
    assert!(verify(&bytes, &signature).is_ok());
    assert!(verify(&format!("{bytes} "), &signature).is_err());
    for (field, replacement) in [
        ("expires_at", "9999-99-99T00:00:00Z"),
        ("expires_at", "2000-01-01T00:00:00Z"),
        ("registry_id", "foreign"),
    ] {
        let mut altered: Value = serde_json::from_str(&bytes).expect("fixture");
        altered[field] = json!(replacement);
        let altered = altered.to_string();
        assert!(verify(&altered, &key.sign_hex(altered.as_bytes())).is_err());
    }
}

#[test]
fn main_source_is_binary_pinned_without_enrollment_or_network_key_discovery() {
    let source = main_source().expect("main source");
    assert!(canonic_registry::source::is_https_location(
        source.location()
    ));
    assert_eq!(source.identity(), "main");
    assert_eq!(source.key(), Some(MAIN_KEY));
    let (_project, context) = crate::review::tests::enrolled();
    let registered = effective(Some(&context)).expect("registrations");
    assert_eq!(registered.select(Some("main")).expect("main"), &source);
    let listed = list(Some(&context)).expect("listing");
    let mut expected = json!(source);
    expected["scope"] = json!("builtin");
    assert_eq!(listed["sources"]["main"], expected);
}

#[test]
fn a_catalog_signed_by_a_network_supplied_replacement_key_is_rejected() {
    let key = SigningKey::generate();
    let bytes = json!({"schema_version":1,"registry_id":"canonic-shield-main","sequence":1,"expires_at":"2099-01-01T00:00:00Z","packs":[]}).to_string();
    let signature = key.sign_hex(bytes.as_bytes());
    assert!(
        Catalog::verify(
            &main_source().expect("main source"),
            bytes.as_bytes(),
            Some(signature.as_bytes()),
            SystemTime::now()
        )
        .is_err()
    );
    assert_ne!(key.public_hex(), MAIN_KEY);
}
