//! Selected-pack traversal and all-or-nothing admission at a real file boundary.
use super::*;
use crate::{catalog::history::Reference, digest};
use assert_fs::{TempDir, prelude::*};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

fn page(root: &TempDir, value: &Value, first: usize, last: usize) -> Reference {
    let bytes = value.to_string();
    let sha256 = digest::sha256_hex(bytes.as_bytes());
    let path = format!("metadata/{sha256}.json");
    root.child(&path).write_str(&bytes).unwrap();
    Reference {
        path,
        sha256,
        first: format!("1.0.{first}").parse().unwrap(),
        last: format!("1.0.{last}").parse().unwrap(),
    }
}
fn fixture() -> (TempDir, Source, Catalog, Vec<Reference>) {
    let root = TempDir::new().unwrap();
    let entries: Vec<_> = (0..130)
        .map(|index| {
            json!({"id":"acme/box","version":format!("1.0.{index}"),
        "path":format!("packs/{index}.json"),"sha256":"a".repeat(64)})
        })
        .collect();
    let leaves: Vec<_> = entries
        .chunks(64)
        .enumerate()
        .map(|(index, entries)| {
            page(
                &root,
                &json!({"schema_version":1,"id":"acme/box","entries":entries}),
                index * 64,
                index * 64 + entries.len() - 1,
            )
        })
        .collect();
    let branch = page(
        &root,
        &json!({"schema_version":1,"id":"acme/box","children":leaves}),
        0,
        129,
    );
    let document =
        json!({"schema_version":3,"packs":[entries[129]],"histories":{"acme/box":branch}});
    let source = Source::local(&root.path().canonicalize().unwrap()).unwrap();
    let catalog = Catalog::verify(
        &source,
        document.to_string().as_bytes(),
        None,
        SystemTime::now(),
    )
    .unwrap();
    (root, source, catalog, leaves)
}

#[test]
fn exact_old_pin_reads_only_its_branch_and_history_exceeds_flat_limit() {
    let (root, source, mut catalog, leaves) = fixture();
    let fetcher = Fetcher::new(Duration::from_secs(2));
    // Unrelated versions are inaccessible: exact lookup must never open them.
    std::fs::remove_file(root.path().join(&leaves[1].path)).unwrap();
    std::fs::remove_file(root.path().join(&leaves[2].path)).unwrap();
    let id = "acme/box".parse().unwrap();
    fetcher
        .history(&source, &mut catalog, &id, Some(&"1.0.3".parse().unwrap()))
        .unwrap();
    assert!(catalog.release(&id, &"1.0.3".parse().unwrap()).is_some());
    assert_eq!(catalog.entries().len(), 65);
    // Root bytes still identify the view after adding authenticated history.
    let before = catalog.clone();
    assert!(fetcher.history(&source, &mut catalog, &id, None).is_err());
    assert_eq!(catalog, before);
}

#[test]
fn full_selected_history_is_cached_and_tampering_never_partially_extends() {
    let (root, source, mut catalog, leaves) = fixture();
    let id = "acme/box".parse().unwrap();
    let fetcher = Fetcher::new(Duration::from_secs(2));
    let before = catalog.clone();
    root.child(&leaves[0].path).write_str("tampered").unwrap();
    assert!(fetcher.history(&source, &mut catalog, &id, None).is_err());
    assert_eq!(catalog, before);
    let (root, source, mut catalog, _) = fixture();
    fetcher.history(&source, &mut catalog, &id, None).unwrap();
    assert_eq!(catalog.entries().len(), 130);
    assert_eq!(catalog.digest(), before.digest());
    std::fs::remove_dir_all(root.path().join("metadata")).unwrap();
    fetcher.history(&source, &mut catalog, &id, None).unwrap();
    let other = Source::local(&TempDir::new().unwrap().path().canonicalize().unwrap()).unwrap();
    assert!(fetcher.history(&other, &mut catalog, &id, None).is_err());
}

#[test]
fn excessive_depth_refuses_without_partial_catalog_admission() {
    let (root, source, original, _) = fixture();
    let id = "acme/box".parse().unwrap();
    let mut reference = original.history(&id).unwrap().clone();
    for _ in 0..history::MAX_DEPTH {
        reference = page(
            &root,
            &json!({"schema_version":1,"id":"acme/box","children":[reference]}),
            0,
            129,
        );
    }
    let document =
        json!({"schema_version":3,"packs":original.entries(),"histories":{"acme/box":reference}});
    let mut catalog = Catalog::verify(
        &source,
        document.to_string().as_bytes(),
        None,
        SystemTime::now(),
    )
    .unwrap();
    let before = catalog.clone();
    assert!(matches!(
        Fetcher::new(Duration::from_secs(2)).history(
            &source,
            &mut catalog,
            &id,
            Some(&"1.0.3".parse().unwrap())
        ),
        Err(AcquisitionError::Catalog(CatalogError::Limit))
    ));
    assert_eq!(catalog, before);
}

#[test]
fn exhausted_history_deadline_reports_timeout_without_mutation() {
    let (_root, source, mut catalog, _) = fixture();
    let before = catalog.clone();
    assert!(matches!(
        Fetcher::new(Duration::ZERO).history(
            &source,
            &mut catalog,
            &"acme/box".parse().unwrap(),
            None
        ),
        Err(AcquisitionError::Timeout)
    ));
    assert_eq!(catalog, before);
}
