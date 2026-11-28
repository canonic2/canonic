//! Driver-graph identity, lifetime and bounded mutation fixtures.

use super::*;

fn root(table: &ObjectTable) -> ObjectRef {
    table.lookup("", "Root").unwrap()
}

#[test]
fn argument_channel_ownership_follows_adoption_and_rejects_disposed_guid_reuse() {
    let mut table = ObjectTable::default();
    let root = root(&table);
    let first = table.create(&root, "first", "BrowserContext").unwrap();
    let second = table.create(&root, "second", "BrowserContext").unwrap();
    let page = table.create(&first, "page", "Page").unwrap();
    let handle = table.create(&page, "argument", "JSHandle").unwrap();
    assert!(table.owned_by(&handle, &first).unwrap());
    assert!(!table.owned_by(&handle, &second).unwrap());
    table.adopt(&handle, &second).unwrap();
    assert!(!table.owned_by(&handle, &first).unwrap());
    assert!(table.owned_by(&handle, &second).unwrap());
    table.dispose(&second).unwrap();
    let second = table.create(&root, "second", "BrowserContext").unwrap();
    table.create(&second, "argument", "JSHandle").unwrap();
    assert!(table.owned_by(&handle, &second).is_err());
}

#[test]
fn creation_before_a_result_is_indexed_and_type_checked() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    let page = table.create(&parent, "acme-page", "Page").unwrap();
    assert_eq!(table.lookup("acme-page", "Page").unwrap(), page);
    assert_eq!(table.guid(&page, "Page").unwrap(), "acme-page");
    assert_eq!(table.lookup("acme-page", "Frame"), Err(Error::WrongType));
    assert_eq!(table.guid(&page, "Frame"), Err(Error::WrongType));
    assert_eq!(table.lookup("unknown", "Page"), Err(Error::Unknown));
    assert_eq!(
        table.create(&parent, "acme-page", "Page"),
        Err(Error::Duplicate)
    );
    assert_eq!(table.lookup("acme-page", "Page").unwrap(), page);
}

#[test]
fn recursive_disposal_preserves_siblings_and_rejects_reused_guid_references() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    let context = table.create(&parent, "context", "BrowserContext").unwrap();
    let page = table.create(&context, "page", "Page").unwrap();
    let frame = table.create(&page, "frame", "Frame").unwrap();
    let handle = table.create(&frame, "handle", "JSHandle").unwrap();
    let sibling = table.create(&parent, "other-page", "Page").unwrap();
    assert_eq!(table.dispose(&context).unwrap(), 4);
    for reference in [&context, &page, &frame, &handle] {
        assert_eq!(table.guid(reference, "Page"), Err(Error::Stale));
    }
    assert_eq!(table.guid(&sibling, "Page").unwrap(), "other-page");
    let replacement = table.create(&parent, "page", "Page").unwrap();
    assert_ne!(page, replacement);
    assert_eq!(table.guid(&page, "Page"), Err(Error::Stale));
    assert_eq!(table.dispose(&page), Err(Error::Stale));
    assert_eq!(table.guid(&replacement, "Page").unwrap(), "page");
}

#[test]
fn references_cannot_cross_driver_epochs_even_when_wire_guids_match() {
    let mut first = ObjectTable::default();
    let mut second = ObjectTable::default();
    let first_root = root(&first);
    let second_root = root(&second);
    let a = first.create(&first_root, "page", "Page").unwrap();
    let b = second.create(&second_root, "page", "Page").unwrap();
    assert_ne!(a, b);
    assert_eq!(second.guid(&a, "Page"), Err(Error::WrongEpoch));
    assert_eq!(
        second.create(&first_root, "new", "Page"),
        Err(Error::WrongEpoch)
    );
    assert_eq!(second.dispose(&a), Err(Error::WrongEpoch));
    assert_eq!(second.adopt(&a, &second_root), Err(Error::WrongEpoch));
    assert_eq!(second.guid(&b, "Page").unwrap(), "page");
}

#[test]
fn adoption_preserves_identity_and_moves_subtree_disposal_ownership() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    let a = table.create(&parent, "a", "BrowserContext").unwrap();
    let b = table.create(&parent, "b", "BrowserContext").unwrap();
    let page = table.create(&a, "page", "Page").unwrap();
    let frame = table.create(&page, "frame", "Frame").unwrap();
    assert_eq!(table.children(&a, "Page").unwrap(), vec![page.clone()]);
    assert!(table.children(&a, "Worker").unwrap().is_empty());
    table.adopt(&page, &b).unwrap();
    assert!(table.children(&a, "Page").unwrap().is_empty());
    assert_eq!(table.children(&b, "Page").unwrap(), vec![page.clone()]);
    table.adopt(&page, &b).unwrap();
    assert_eq!(table.dispose(&a).unwrap(), 1);
    assert_eq!(table.lookup("page", "Page").unwrap(), page);
    assert_eq!(table.guid(&frame, "Frame").unwrap(), "frame");
    assert_eq!(table.dispose(&b).unwrap(), 3);
    assert_eq!(table.guid(&page, "Page"), Err(Error::Stale));
    assert_eq!(table.children(&page, "Frame"), Err(Error::Stale));
}

#[test]
fn cycles_and_root_mutations_are_refused_without_changing_the_graph() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    let page = table.create(&parent, "page", "Page").unwrap();
    let frame = table.create(&page, "frame", "Frame").unwrap();
    assert_eq!(table.adopt(&page, &frame), Err(Error::Cycle));
    assert_eq!(table.adopt(&page, &page), Err(Error::Cycle));
    assert_eq!(table.adopt(&parent, &page), Err(Error::Root));
    assert_eq!(table.dispose(&parent), Err(Error::Root));
    assert_eq!(table.dispose(&page).unwrap(), 2);
    assert_eq!(table.lookup("", "Root").unwrap(), parent);
}

#[test]
fn capacities_and_identity_bounds_apply_before_mutation() {
    for capacity in [0, MAX_OBJECTS + 1] {
        assert!(matches!(
            ObjectTable::with_capacity(capacity),
            Err(Error::InvalidCapacity)
        ));
    }
    let mut table = ObjectTable::with_capacity(2).unwrap();
    let parent = root(&table);
    for (guid, kind) in [
        ("", "Page"),
        ("page", ""),
        ("page", "Root"),
        (&"x".repeat(MAX_GUID_BYTES + 1), "Page"),
        ("page", &"x".repeat(MAX_TYPE_BYTES + 1)),
    ] {
        assert_eq!(
            table.create(&parent, guid, kind),
            Err(Error::InvalidIdentity)
        );
    }
    let page = table
        .create(
            &parent,
            &"x".repeat(MAX_GUID_BYTES),
            &"t".repeat(MAX_TYPE_BYTES),
        )
        .unwrap();
    assert_eq!(
        table.create(&parent, "second", "Page"),
        Err(Error::Capacity)
    );
    table.dispose(&page).unwrap();
    table.create(&parent, "second", "Page").unwrap();
}

#[test]
fn driver_loss_invalidates_all_objects_and_never_reopens_an_epoch() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    let page = table.create(&parent, "page", "Page").unwrap();
    table.mark_lost();
    table.mark_lost();
    assert!(table.objects.is_empty());
    assert_eq!(table.lookup("", "Root"), Err(Error::Lost));
    assert_eq!(table.guid(&page, "Page"), Err(Error::Lost));
    assert_eq!(table.create(&parent, "new", "Page"), Err(Error::Lost));
    assert_eq!(table.adopt(&page, &parent), Err(Error::Lost));
    assert_eq!(table.dispose(&page), Err(Error::Lost));
}

#[test]
fn serial_exhaustion_does_not_publish_an_object() {
    let mut table = ObjectTable::default();
    let parent = root(&table);
    table.next_serial = u64::MAX;
    assert_eq!(table.create(&parent, "page", "Page"), Err(Error::Exhausted));
    assert_eq!(table.objects.len(), 1);
    assert!(table.objects[""].children.is_empty());
}

#[test]
fn deep_subtrees_dispose_without_recursive_stack_growth() {
    let mut table = ObjectTable::with_capacity(4096).unwrap();
    let parent = root(&table);
    let first = table.create(&parent, "first", "Frame").unwrap();
    let mut last = first.clone();
    for index in 2..4096 {
        last = table
            .create(&last, &format!("frame-{index}"), "Frame")
            .unwrap();
    }
    assert_eq!(table.dispose(&first).unwrap(), 4095);
    assert_eq!(table.guid(&last, "Frame"), Err(Error::Stale));
    assert_eq!(table.objects.len(), 1);
}
