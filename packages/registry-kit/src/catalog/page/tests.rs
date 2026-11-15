//! Page boundaries, mutation-safe continuation and bounded traversal.

use super::*;
use crate::test_support::{local, now, release};
use serde_json::{Value, json};
use std::cell::Cell;

thread_local! {
    pub(super) static ENTRY_READS: Cell<usize> = const { Cell::new(0) };
}

fn catalog(packs: &[Value]) -> Catalog {
    Catalog::verify(
        &local("/srv/acme-registry"),
        json!({"schema_version":1,"packs":packs})
            .to_string()
            .as_bytes(),
        None,
        now(),
    )
    .unwrap()
}

fn entry(id: &str, version: &str) -> Value {
    let mut entry = release(id, version, 'a');
    // Build metadata is valid in versions; '+' is not an artifact-path byte.
    entry["path"] = json!("pack.yml");
    entry
}

fn keys(page: &Page<'_, EntryPosition>) -> Vec<String> {
    page.items
        .iter()
        .map(|e| format!("{}@{}", e.id, e.version))
        .collect()
}

#[test]
fn limits_are_validated_without_clamping() {
    assert_eq!(PageLimit::default().get(), 50);
    for value in [1, 50, 100] {
        assert_eq!(PageLimit::try_from(value).unwrap().get(), value);
    }
    for value in [0, 101, usize::MAX] {
        assert_eq!(PageLimit::new(value), Err(InvalidPageLimit));
    }
}

#[test]
fn discovery_pages_span_more_than_maximum_page_size() {
    let packs: Vec<_> = (0..151)
        .rev()
        .map(|i| entry(&format!("acme/pack{i:03}"), "1.0.0"))
        .collect();
    let histories: serde_json::Map<_, _> = packs
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap().to_owned(),
                json!({
                    "path":"history.json", "sha256":"a".repeat(64),
                    "first":"1.0.0", "last":"1.0.0"
                }),
            )
        })
        .collect();
    let catalog = Catalog::verify(
        &local("/srv/acme-registry"),
        json!({"schema_version":3,"packs":packs,"histories":histories})
            .to_string()
            .as_bytes(),
        None,
        now(),
    )
    .unwrap();
    let first = catalog.heads_page(PageLimit::new(100).unwrap(), None);
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items[0].id.to_string(), "acme/pack000");
    let second = catalog.heads_page(PageLimit::new(100).unwrap(), first.next_position.as_ref());
    assert_eq!(second.items.len(), 51);
    assert_eq!(second.items[0].id.to_string(), "acme/pack100");
    assert_eq!(second.next_position, None);
    let empty = Catalog::verify(
        &local("/srv/acme-registry"),
        b"{\"schema_version\":1,\"packs\":[]}",
        None,
        now(),
    )
    .unwrap();
    assert!(
        empty
            .heads_page(PageLimit::default(), None)
            .items
            .is_empty()
    );
    assert_eq!(
        empty.entries_page(PageLimit::default(), None).next_position,
        None
    );
}

#[test]
fn identity_pages_ignore_input_order_and_keep_every_release_tie() {
    let catalog = catalog(&[
        entry("other/git", "1.0.0"),
        entry("acme/git", "1.0.0+z"),
        entry("acme/git", "1.0.0"),
        entry("acme/git", "1.0.0+a"),
        entry("acme/git", "1.0.0-rc.1"),
        entry("acme/abc", "2.0.0"),
    ]);
    let limit = PageLimit::new(2).unwrap();
    let first = catalog.entries_page(limit, None);
    assert_eq!(keys(&first), ["acme/abc@2.0.0", "acme/git@1.0.0-rc.1"]);
    assert_eq!(catalog.heads_page(limit, None), first);
    let position = first.next_position.unwrap();
    assert_eq!(position.version.to_string(), "1.0.0-rc.1");
    assert_eq!(
        serde_json::from_str::<EntryPosition>(&serde_json::to_string(&position).unwrap()).unwrap(),
        position
    );
    let second = catalog.entries_page(limit, Some(&position));
    assert_eq!(keys(&second), ["acme/git@1.0.0", "acme/git@1.0.0+a"]);
    let last = catalog.entries_page(limit, second.next_position.as_ref());
    assert_eq!(keys(&last), ["acme/git@1.0.0+z", "other/git@1.0.0"]);
    assert_eq!(last.next_position, None); // Exactly a full final page.
    let beyond = EntryPosition {
        id: "z/end".parse().unwrap(),
        version: Version::new(0, 0, 0),
    };
    assert!(catalog.entries_page(limit, Some(&beyond)).items.is_empty());
    assert_eq!(
        catalog.search_page("missing", limit, None).next_position,
        None
    );
    assert!(catalog.search_page("Git", limit, None).items.is_empty());
    assert_eq!(
        catalog.search_page("", limit, None),
        catalog.entries_page(limit, None)
    );
    let search = catalog.search_page("git", limit, None);
    assert_eq!(keys(&search), ["acme/git@1.0.0-rc.1", "acme/git@1.0.0"]);
    let search = catalog.search_page("git", limit, search.next_position.as_ref());
    assert_eq!(keys(&search), ["acme/git@1.0.0+a", "acme/git@1.0.0+z"]);
    let search = catalog.search_page("git", limit, search.next_position.as_ref());
    assert_eq!(keys(&search), ["other/git@1.0.0"]);
    assert_eq!(search.next_position, None);
}

#[test]
fn continuation_survives_deleted_boundary_and_insertions() {
    let before = catalog(&[
        entry("acme/a", "1.0.0"),
        entry("acme/c", "1.0.0"),
        entry("acme/e", "1.0.0"),
    ]);
    let position = before
        .entries_page(PageLimit::new(2).unwrap(), None)
        .next_position
        .unwrap();
    let after = catalog(&[
        entry("acme/b", "1.0.0"),
        entry("acme/a", "1.0.0"),
        entry("acme/d", "1.0.0"),
        entry("acme/e", "1.0.0"),
    ]);
    for page in [
        after.entries_page(PageLimit::default(), Some(&position)),
        after.heads_page(PageLimit::default(), Some(&position)),
        after.search_page("acme", PageLimit::default(), Some(&position)),
    ] {
        assert_eq!(keys(&page), ["acme/d@1.0.0", "acme/e@1.0.0"]);
        assert_eq!(page.next_position, None);
    }
}

#[test]
fn substring_search_matches_segments_and_crossing_separator() {
    let catalog = catalog(&[
        entry("acme/git", "1.0.0"),
        entry("acme/git-tools", "1.0.0"),
        entry("other/commit", "1.0.0"),
    ]);
    for (query, expected) in [
        ("", vec!["acme/git", "acme/git-tools", "other/commit"]),
        ("/", vec!["acme/git", "acme/git-tools", "other/commit"]),
        ("me/gi", vec!["acme/git", "acme/git-tools"]),
        ("acme/", vec!["acme/git", "acme/git-tools"]),
        ("/git", vec!["acme/git", "acme/git-tools"]),
        ("acme/git", vec!["acme/git", "acme/git-tools"]),
        ("me", vec!["acme/git", "acme/git-tools"]),
        ("-", vec!["acme/git-tools"]),
        ("mit", vec!["other/commit"]),
        ("git/", vec![]),
        ("acme//", vec![]),
        ("Git", vec![]),
        ("é", vec![]),
    ] {
        let page = catalog.search_page(query, PageLimit::default(), None);
        assert_eq!(
            page.items
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            expected,
            "{query}"
        );
        assert_eq!(page.next_position, None);
    }
}

#[test]
fn release_pages_use_exclusive_descending_versions_including_build_metadata() {
    let before = catalog(&[
        entry("acme/a", "0.0.0-alpha"),
        entry("acme/a", "1.0.0"),
        entry("acme/a", "1.0.0+z"),
        entry("acme/a", "2.0.0"),
        entry("other/a", "9.0.0"),
    ]);
    let id = "acme/a".parse().unwrap();
    let limit = PageLimit::new(2).unwrap();
    let first = before.releases_page(&id, limit, None);
    assert_eq!(
        first
            .items
            .iter()
            .map(|e| e.version.to_string())
            .collect::<Vec<_>>(),
        ["2.0.0", "1.0.0+z"]
    );
    let after = catalog(&[
        entry("acme/a", "0.0.0-alpha"),
        entry("acme/a", "1.0.0"),
        entry("acme/a", "3.0.0"),
    ]);
    let last = after.releases_page(&id, limit, first.next_position.as_ref());
    assert_eq!(
        last.items
            .iter()
            .map(|e| e.version.to_string())
            .collect::<Vec<_>>(),
        ["1.0.0", "0.0.0-alpha"]
    );
    assert_eq!(last.next_position, None);
    assert!(
        after
            .releases_page(&"acme/missing".parse().unwrap(), limit, None)
            .items
            .is_empty()
    );
}

#[test]
fn loaded_history_updates_index_without_entering_discovery_pages() {
    let mut catalog = catalog(&[entry("acme/a", "2.0.0")]);
    let id = "acme/a".parse().unwrap();
    let history = (0..150)
        .map(|i| serde_json::from_value(entry("acme/a", &format!("1.0.{i}"))).unwrap())
        .collect();
    catalog.extend_history(&id, history, false).unwrap();
    assert_eq!(
        catalog.heads_page(PageLimit::default(), None).items.len(),
        1
    );
    assert_eq!(
        catalog.entries_page(PageLimit::default(), None).items.len(),
        50
    );
    assert_eq!(
        catalog
            .search_page("a", PageLimit::new(100).unwrap(), None)
            .items
            .len(),
        100
    );
    let mut position = None;
    let mut versions = Vec::new();
    loop {
        let page = catalog.releases_page(&id, PageLimit::default(), position.as_ref());
        versions.extend(page.items.iter().map(|entry| entry.version.clone()));
        position = page.next_position;
        if position.is_none() {
            break;
        }
    }
    assert_eq!(versions.len(), 151);
    assert!(versions.windows(2).all(|pair| pair[0] > pair[1]));
}

#[test]
fn version_seek_reads_only_page_and_lookahead_through_public_methods() {
    let mut catalog = catalog(&[entry("acme/a", "2.0.0")]);
    let id = "acme/a".parse().unwrap();
    let history = (0..4000)
        .map(|i| serde_json::from_value(entry("acme/a", &format!("1.0.{i}"))).unwrap())
        .collect();
    catalog.extend_history(&id, history, false).unwrap();
    let position = EntryPosition {
        id,
        version: Version::new(1, 0, 3900),
    };
    ENTRY_READS.set(0);
    let page = catalog.entries_page(PageLimit::default(), Some(&position));
    assert_eq!(ENTRY_READS.get(), 51);
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.items[0].version, Version::new(1, 0, 3901));
    ENTRY_READS.set(0);
    let page = catalog.releases_page(&position.id, PageLimit::default(), Some(&position.version));
    assert_eq!(ENTRY_READS.get(), 51);
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.items[0].version, Version::new(1, 0, 3899));
}

#[test]
fn identity_seek_predicates_and_reads_only_page_and_lookahead() {
    let packs: Vec<_> = (0..2500)
        .map(|i| entry(&format!("acme/pack{i:04}"), "1.0.0"))
        .collect();
    let histories: serde_json::Map<_, _> = packs
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap().to_owned(),
                json!({
                    "path":"history.json", "sha256":"a".repeat(64),
                    "first":"1.0.0", "last":"1.0.0"
                }),
            )
        })
        .collect();
    let catalog = Catalog::verify(
        &local("/srv/acme-registry"),
        json!({"schema_version":3,"packs":packs,"histories":histories})
            .to_string()
            .as_bytes(),
        None,
        now(),
    )
    .unwrap();
    let position = EntryPosition {
        id: "acme/pack2400".parse().unwrap(),
        version: Version::new(1, 0, 0),
    };
    let predicates = Cell::new(0);
    ENTRY_READS.set(0);
    let page = identity_page(
        &catalog.entries,
        &catalog.entries_by_id,
        PageLimit::default(),
        Some(&position),
        |id| {
            predicates.set(predicates.get() + 1);
            assert!(id >= &position.id);
            true
        },
    );
    // The boundary identity is considered, but its already-returned version is not read.
    assert_eq!(predicates.get(), 52);
    assert_eq!(ENTRY_READS.get(), 51);
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.items[0].id.to_string(), "acme/pack2401");
    ENTRY_READS.set(0);
    assert_eq!(
        catalog.entries_page(PageLimit::default(), Some(&position)),
        page
    );
    assert_eq!(ENTRY_READS.get(), 51);
}

fn large_entries() -> Vec<Value> {
    (0..64)
        .map(|i| {
            let mut entry = entry("acme/a", &format!("1.0.{i}"));
            entry["path"] = json!("a".repeat(4096));
            entry
        })
        .collect()
}

#[test]
fn byte_budget_ends_every_listing_early_without_losing_releases() {
    let catalog = catalog(&large_entries());
    assert!(serde_json::to_vec(catalog.entries()).unwrap().len() > MAX_PAGE_BYTES);
    let id: PackId = "acme/a".parse().unwrap();
    let limit = PageLimit::new(100).unwrap();
    for operation in 0..4 {
        let mut position: Option<EntryPosition> = None;
        let mut versions = Vec::new();
        let mut pages = 0;
        loop {
            let page = match operation {
                0 => catalog.entries_page(limit, position.as_ref()),
                1 => catalog.heads_page(limit, position.as_ref()),
                2 => catalog.search_page("acme", limit, position.as_ref()),
                _ => {
                    let page = catalog.releases_page(
                        &id,
                        limit,
                        position.as_ref().map(|position| &position.version),
                    );
                    Page {
                        items: page.items,
                        next_position: page.next_position.map(|version| EntryPosition {
                            id: id.clone(),
                            version,
                        }),
                    }
                }
            };
            assert!(serde_json::to_vec(&page.items).unwrap().len() <= MAX_PAGE_BYTES);
            assert!(!page.items.is_empty());
            if pages == 0 {
                assert!(page.items.len() < 64);
                let last = page.items.last().unwrap();
                assert_eq!(
                    page.next_position,
                    Some(EntryPosition {
                        id: last.id.clone(),
                        version: last.version.clone(),
                    })
                );
            }
            versions.extend(page.items.iter().map(|entry| entry.version.clone()));
            pages += 1;
            position = page.next_position;
            if position.is_none() {
                break;
            }
            assert!(pages < 64);
        }
        let mut expected: Vec<_> = (0..64).map(|i| Version::new(1, 0, i)).collect();
        if operation == 3 {
            expected.reverse();
        }
        assert_eq!(versions, expected);
        assert_eq!(pages, 2);
    }
}

#[test]
fn byte_budget_includes_array_delimiters_and_separators_at_exact_boundary() {
    let mut entries = large_entries();
    let count = (1..=entries.len())
        .find(|count| serde_json::to_vec(&entries[..*count]).unwrap().len() > MAX_PAGE_BYTES)
        .unwrap();
    let excess = serde_json::to_vec(&entries[..count]).unwrap().len() - MAX_PAGE_BYTES;
    entries[count - 1]["path"] = json!("a".repeat(4096 - excess));
    assert_eq!(
        serde_json::to_vec(&entries[..count]).unwrap().len(),
        MAX_PAGE_BYTES
    );
    let catalog = catalog(&entries);
    let page = catalog.entries_page(PageLimit::new(100).unwrap(), None);
    assert_eq!(page.items.len(), count);
    assert_eq!(
        serde_json::to_vec(&page.items).unwrap().len(),
        MAX_PAGE_BYTES
    );
    assert!(page.next_position.is_some());
}

#[test]
fn oversized_single_entry_is_returned_alone_and_still_advances() {
    let version = format!("1.0.0+{}", "a".repeat(MAX_PAGE_BYTES));
    let oversized = entry("acme/a", &version);
    let loaded = catalog(&[oversized.clone(), entry("acme/b", "1.0.0")]);
    let limit = PageLimit::new(100).unwrap();
    let first = loaded.entries_page(limit, None);
    assert_eq!(first.items.len(), 1);
    assert!(serde_json::to_vec(&first.items).unwrap().len() > MAX_PAGE_BYTES);
    let position = first.next_position.unwrap();
    assert_eq!(position.version.to_string(), version);
    let last = loaded.entries_page(limit, Some(&position));
    assert_eq!(keys(&last), ["acme/b@1.0.0"]);
    assert_eq!(last.next_position, None);
    let alone = catalog(&[oversized]);
    let page = alone.entries_page(limit, None);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next_position, None);
}
