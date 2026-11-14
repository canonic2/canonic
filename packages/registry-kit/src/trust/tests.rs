//! Offline keys exercise threshold succession, strict parsing and durable denial.
use super::*;
use crate::testing::SigningKey;
use serde_json::{Value, json};
use std::time::{Duration, UNIX_EPOCH};

pub(crate) fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1000)
}

pub(crate) fn source(key: &SigningKey) -> Source {
    Source::signed(
        "https://registry.example.com/",
        &key.public_hex(),
        "acme-registry",
    )
    .unwrap()
}

pub(crate) fn payload(
    source: &Source,
    version: u64,
    keys: &[&SigningKey],
    threshold: usize,
) -> Vec<u8> {
    let keys: BTreeMap<_, _> = keys
        .iter()
        .map(|key| (key_id(&key.public_hex()).unwrap(), key.public_hex()))
        .collect();
    let ids: Vec<_> = keys.keys().cloned().collect();
    let role = json!({"key_ids":ids,"threshold":threshold});
    json!({"schema_version":1,"kind":"registry-root","source_identity":source.identity(),"location":source.location(),"registry_id":source.registry_id(),"version":version,"expires_at":"2100-01-01T00:00:00Z","keys":keys,"roles":{"root":role,"catalog":role,"revocations":role}}).to_string().into_bytes()
}

pub(crate) fn signatures(keys: &[&SigningKey], bytes: &[u8]) -> Vec<u8> {
    json!({"schema_version":1,"kind":"registry-signatures","signatures":keys.iter().map(|key|json!({"key_id":key_id(&key.public_hex()).unwrap(),"signature":key.sign_hex(bytes)})).collect::<Vec<_>>()} ).to_string().into_bytes()
}

pub(crate) fn revocations(root: &TrustedRoot, sequence: u64, releases: Value) -> Vec<u8> {
    let mut document = json!({"schema_version":1,"kind":"registry-revocations","source_identity":root.source().identity(),"location":root.source().location(),"registry_id":root.source().registry_id(),"root_version":root.version(),"sequence":sequence,"expires_at":"2100-01-01T00:00:00Z"});
    document["releases"] = releases;
    document.to_string().into_bytes()
}

fn mutate(bytes: &[u8], change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    change(&mut value);
    value.to_string().into_bytes()
}

#[test]
fn rotating_roots_require_both_authorities_and_preserve_source_identity() {
    let old = SigningKey::generate();
    let new = SigningKey::generate();
    let third = SigningKey::generate();
    let source = source(&old);
    let bytes = payload(&source, 1, &[&new], 1);
    assert_eq!(
        TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old], &bytes)).unwrap_err(),
        TrustError::Signature
    );
    assert_eq!(
        TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&new], &bytes)).unwrap_err(),
        TrustError::Signature
    );
    let first =
        TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old, &new], &bytes)).unwrap();
    let bytes = payload(&source, 2, &[&third], 1);
    assert_eq!(
        first
            .advance(&bytes, &signatures(&[&old, &third], &bytes))
            .unwrap_err(),
        TrustError::Signature
    );
    let second = first
        .advance(&bytes, &signatures(&[&new, &third], &bytes))
        .unwrap();
    assert_eq!(second.source(), &source);
    assert_eq!(second.version(), 2);
    second.check_fresh(now()).unwrap();
    let restored = TrustedRoot::restore_checkpoint(&source, &second.checkpoint().unwrap()).unwrap();
    assert_eq!(restored.revision(), second.revision());
    let catalog = b"exact catalog bytes";
    assert_eq!(
        second.verify_catalog(catalog, &signatures(&[&old], catalog), now()),
        Err(TrustError::Signature)
    );
    second
        .verify_catalog(catalog, &signatures(&[&third], catalog), now())
        .unwrap();
}

#[test]
fn root_thresholds_count_distinct_signers_and_refuse_skips() {
    let old = SigningKey::generate();
    let new = SigningKey::generate();
    let source = source(&old);
    let bytes = payload(&source, 1, &[&old, &new], 2);
    assert_eq!(
        TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old, &old], &bytes)).unwrap_err(),
        TrustError::Malformed
    );
    assert_eq!(
        TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old], &bytes)).unwrap_err(),
        TrustError::Signature
    );
    let root = TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old, &new], &bytes)).unwrap();
    let initial = root.revision();
    for version in [1, 3] {
        let bytes = payload(&source, version, &[&new], 1);
        assert_eq!(
            root.advance(&bytes, &signatures(&[&old, &new], &bytes))
                .unwrap_err(),
            TrustError::Rollback
        );
    }
    assert_eq!(root.revision(), initial);
}

#[test]
fn strict_metadata_refuses_duplicate_unknown_foreign_and_oversized_inputs() {
    let key = SigningKey::generate();
    let source = source(&key);
    let original = payload(&source, 1, &[&key], 1);
    let duplicate = String::from_utf8(original.clone())
        .unwrap()
        .replacen("\"version\":1", "\"version\":1,\"version\":1", 1)
        .into_bytes();
    let id = key_id(&key.public_hex()).unwrap();
    let duplicate_key = String::from_utf8(original.clone())
        .unwrap()
        .replace(
            &format!("\"keys\":{{\"{id}\":\"{}\"}}", key.public_hex()),
            &format!(
                "\"keys\":{{\"{id}\":\"{}\",\"{id}\":\"{}\"}}",
                key.public_hex(),
                key.public_hex()
            ),
        )
        .into_bytes();
    for bytes in [
        duplicate,
        duplicate_key,
        mutate(&original, |v| v["unexpected"] = json!(true)),
    ] {
        assert_ne!(bytes, original);
        assert_eq!(
            TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&key], &bytes)).unwrap_err(),
            TrustError::Malformed
        );
    }
    let foreign = mutate(&original, |v| {
        v["location"] = json!("https://other.example.com/");
    });
    assert_eq!(
        TrustedRoot::bootstrap(&source, &foreign, &signatures(&[&key], &foreign)).unwrap_err(),
        TrustError::Identity
    );
    assert_eq!(
        TrustedRoot::bootstrap(&source, &vec![b' '; MAX_ROOT_BYTES + 1], b"{}").unwrap_err(),
        TrustError::Limit
    );
    assert_eq!(
        TrustedRoot::bootstrap(&source, &original, &vec![b' '; MAX_SIGNATURE_BYTES + 1])
            .unwrap_err(),
        TrustError::Limit
    );
}

#[test]
fn expired_intermediate_roots_can_only_lead_to_a_fresh_final_root() {
    let key = SigningKey::generate();
    let source = source(&key);
    let bytes = payload(&source, 1, &[&key], 1);
    let bytes = mutate(&bytes, |v| v["expires_at"] = json!("1970-01-01T00:00:01Z"));
    let root = TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&key], &bytes)).unwrap();
    assert_eq!(root.check_fresh(now()), Err(TrustError::Expired));
    let next = payload(&source, 2, &[&key], 1);
    root.advance(&next, &signatures(&[&key], &next))
        .unwrap()
        .check_fresh(now())
        .unwrap();
}

#[test]
fn revoked_exact_releases_stay_denied_across_expiry_and_updates() {
    let key = SigningKey::generate();
    let source = source(&key);
    let bytes = payload(&source, 1, &[&key], 1);
    let root = TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&key], &bytes)).unwrap();
    let digest = "a".repeat(64);
    let id = "acme/tool".parse().unwrap();
    let version = "1.0.0".parse().unwrap();
    let row = json!({"id":"acme/tool","version":"1.0.0","sha256":digest,"reason":"Publisher withdrew this artifact"});
    let bytes = revocations(&root, 4, json!([row]));
    let revoked =
        Revocations::verify(&root, &bytes, &signatures(&[&key], &bytes), None, now()).unwrap();
    assert_eq!(
        revoked.check_release(source.identity(), &id, &version, &digest),
        Err(TrustError::Revoked)
    );
    revoked
        .check_release(source.identity(), &id, &version, &"b".repeat(64))
        .unwrap();
    assert_eq!(
        revoked.check_release("other", &id, &version, &digest),
        Err(TrustError::Identity)
    );
    for (sequence, rows, error) in [
        (3, json!([row]), TrustError::Rollback),
        (4, json!([]), TrustError::Rollback),
        (5, json!([]), TrustError::RevocationRemoved),
    ] {
        let bytes = revocations(&root, sequence, rows);
        assert_eq!(
            Revocations::verify(
                &root,
                &bytes,
                &signatures(&[&key], &bytes),
                Some(&revoked),
                now()
            )
            .unwrap_err(),
            error
        );
    }
    assert_eq!(revoked.sequence(), 4);
    let expired = mutate(&bytes, |v| v["expires_at"] = json!("1970-01-01T00:00:01Z"));
    assert_eq!(
        Revocations::verify(&root, &expired, &signatures(&[&key], &expired), None, now())
            .unwrap_err(),
        TrustError::Expired
    );
    // Offline denial has no clock argument and cannot disappear when refresh expires.
    let restored =
        Revocations::restore_checkpoint(&root, &expired, &signatures(&[&key], &expired)).unwrap();
    assert_eq!(
        restored.check_release(source.identity(), &id, &version, &digest),
        Err(TrustError::Revoked)
    );
    assert_eq!(
        revoked.check_release(source.identity(), &id, &version, &digest),
        Err(TrustError::Revoked)
    );
}

#[test]
fn revocations_bind_the_current_root_and_do_not_survive_as_a_fresh_old_role() {
    let old = SigningKey::generate();
    let new = SigningKey::generate();
    let source = source(&old);
    let bytes = payload(&source, 1, &[&old], 1);
    let root = TrustedRoot::bootstrap(&source, &bytes, &signatures(&[&old], &bytes)).unwrap();
    let first = revocations(&root, 1, json!([]));
    let retained =
        Revocations::verify(&root, &first, &signatures(&[&old], &first), None, now()).unwrap();
    let bytes = payload(&source, 2, &[&new], 1);
    let root = root
        .advance(&bytes, &signatures(&[&old, &new], &bytes))
        .unwrap();
    assert_eq!(
        Revocations::verify(
            &root,
            &first,
            &signatures(&[&new], &first),
            Some(&retained),
            now()
        )
        .unwrap_err(),
        TrustError::Identity
    );
    let next = revocations(&root, 2, json!([]));
    assert_eq!(
        Revocations::verify(
            &root,
            &next,
            &signatures(&[&old], &next),
            Some(&retained),
            now()
        )
        .unwrap_err(),
        TrustError::Signature
    );
    Revocations::verify(
        &root,
        &next,
        &signatures(&[&new], &next),
        Some(&retained),
        now(),
    )
    .unwrap();
}
