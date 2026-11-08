//! Authenticated but malformed metadata never becomes release authority.
use super::*;
use serde_json::{Value, json};

fn entry(index: usize) -> Value {
    json!({"id":"acme/box","version":format!("1.0.{index}"),"path":format!("packs/{index}.json"),"sha256":"a".repeat(64)})
}
fn check(value: &Value, first: &str, last: &str) -> Result<Page, CatalogError> {
    let bytes = value.to_string();
    let reference = Reference {
        path: "metadata/page.json".into(),
        sha256: digest::sha256_hex(bytes.as_bytes()),
        first: first.parse().unwrap(),
        last: last.parse().unwrap(),
    };
    Page::verify(&"acme/box".parse().unwrap(), &reference, bytes.as_bytes())
}

#[test]
fn history_page_shape_identity_bounds_order_and_paths_are_checked() {
    let valid = json!({"schema_version":1,"id":"acme/box","entries":[entry(0),entry(1)]});
    assert!(check(&valid, "1.0.0", "1.0.1").is_ok());
    for (field, value) in [
        ("id", json!("acme/other")),
        ("schema_version", json!(2)),
        ("entries", json!([])),
        ("entries", json!([entry(1), entry(0)])),
        ("entries", json!([entry(0), entry(0)])),
        ("children", json!([{}])),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(check(&bad, "1.0.0", "1.0.1").is_err(), "{bad}");
    }
    for (field, value) in [
        ("id", json!("acme/other")),
        ("path", json!("../escape")),
        ("sha256", json!("bad")),
    ] {
        let mut bad = valid.clone();
        bad["entries"][0][field] = value;
        assert!(check(&bad, "1.0.0", "1.0.1").is_err());
    }
    assert!(check(&valid, "1.0.0", "1.0.2").is_err());
    let too_many = json!({"schema_version":1,"id":"acme/box","entries":(0..65).map(entry).collect::<Vec<_>>()});
    assert!(matches!(
        check(&too_many, "1.0.0", "1.0.64"),
        Err(CatalogError::Limit)
    ));
}

#[test]
fn release_digests_accept_either_case_and_are_normalized() {
    let mut document = json!({"schema_version":1,"id":"acme/box","entries":[entry(0),entry(1)]});
    document["entries"][0]["sha256"] = json!("ABCDEF01".repeat(8));
    document["entries"][1]["sha256"] = json!("aBcDeF01".repeat(8));
    let page = check(&document, "1.0.0", "1.0.1").unwrap();
    assert!(
        page.entries
            .iter()
            .all(|entry| entry.sha256 == "abcdef01".repeat(8))
    );
}

#[test]
fn child_ranges_are_ordered_disjoint_and_bound_to_the_parent() {
    let child = |first: &str, last: &str| json!({"path":"metadata/leaf.json","sha256":"a".repeat(64),"first":first,"last":last});
    for children in [
        json!([child("1.0.2", "1.0.3"), child("1.0.0", "1.0.1")]),
        json!([child("1.0.0", "1.0.2"), child("1.0.2", "1.0.3")]),
        json!([child("1.0.0", "1.0.1"), child("1.0.2", "1.0.4")]),
    ] {
        assert!(
            check(
                &json!({"schema_version":1,"id":"acme/box","children":children}),
                "1.0.0",
                "1.0.3"
            )
            .is_err()
        );
    }
}
