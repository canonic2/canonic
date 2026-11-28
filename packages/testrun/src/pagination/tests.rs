use super::*;

fn rows(ids: &[u64], bytes: usize) -> Vec<(Vec<u8>, Value)> {
    ids.iter()
        .map(|id| {
            (
                id.to_be_bytes().to_vec(),
                json!({"id":id,"text":"x".repeat(bytes)}),
            )
        })
        .collect()
}
#[test]
fn long_identities_keep_distinct_bounded_sort_keys() {
    let ids = [
        format!("{}a", "x".repeat(256)),
        format!("{}b", "x".repeat(256)),
    ];
    let mut rows = ids
        .iter()
        .map(|id| (identity_key(id), json!({"id":id})))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(rows[0].0.len(), 96);
    assert_ne!(rows[0].0, rows[1].0);
    let first = collect(
        "identities",
        &(),
        &json!({"limit":1}),
        rows.clone(),
        Ok,
        "identity read",
    )
    .unwrap();
    let last = collect(
        "identities",
        &(),
        &json!({"cursor":first.next_cursor}),
        rows,
        Ok,
        "identity read",
    )
    .unwrap();
    assert_eq!(last.items.len(), 1);
    assert!(last.next_cursor.is_none());
    assert!(identity_key("a") < identity_key("aa"));
}
#[test]
fn keysets_traverse_to_null_survive_insertion_and_bind_filters() {
    let first = collect(
        "records",
        &"active",
        &json!({"limit":1}),
        rows(&[1, 3, 5], 0),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(first.items[0]["id"], 1);
    let next = collect(
        "records",
        &"active",
        &json!({"limit":2,"cursor":first.next_cursor}),
        rows(&[0, 1, 2, 3, 5], 0),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(
        next.items
            .iter()
            .map(|row| row["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    let last = collect(
        "records",
        &"active",
        &json!({"cursor":next.next_cursor}),
        rows(&[0, 1, 2, 3, 5], 0),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(last.items[0]["id"], 5);
    assert!(last.next_cursor.is_none());
    assert_eq!(
        collect(
            "records",
            &"other",
            &json!({"cursor":first.next_cursor}),
            rows(&[1], 0),
            Ok,
            "record read"
        )
        .unwrap_err()
        .code,
        "invalid_cursor"
    );
    assert_eq!(
        collect(
            "records",
            &(),
            &json!({"cursor":"malformed"}),
            rows(&[1], 0),
            Ok,
            "record read"
        )
        .unwrap_err()
        .code,
        "invalid_cursor"
    );
    for limit in [0, 101] {
        assert!(
            collect(
                "records",
                &(),
                &json!({"limit":limit}),
                rows(&[1], 0),
                Ok,
                "record read"
            )
            .is_err()
        );
    }
}
#[test]
fn budget_stop_keeps_the_final_rejected_row_and_camelcase_shape() {
    let first = collect(
        "records",
        &(),
        &json!({}),
        rows(&[1, 2], 150_000),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(first.items.len(), 1);
    let rendered = first.render(canonic_mcp_kit::page::Casing::Camel);
    assert!(rendered["nextCursor"].is_string());
    let next = collect(
        "records",
        &(),
        &json!({"cursor":rendered["nextCursor"]}),
        rows(&[1, 2], 150_000),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(next.items[0]["id"], 2);
    assert!(next.next_cursor.is_none());
}

#[test]
fn a_large_single_row_uses_the_real_transport_limit_and_is_returned_alone() {
    let first = collect(
        "records",
        &(),
        &json!({}),
        rows(&[1, 2], 2_000_000),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(first.items.len(), 1);
    assert!(first.next_cursor.is_some());
    let last = collect(
        "records",
        &(),
        &json!({"cursor":first.next_cursor}),
        rows(&[1, 2], 2_000_000),
        Ok,
        "record read",
    )
    .unwrap();
    assert_eq!(last.items[0]["id"], 2);
    assert!(last.next_cursor.is_none());
}
#[test]
fn stream_seeks_and_reports_live_and_finished_ends() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("output");
    let absent = stream("logs", &"producer", &json!({}), &path, None, true, false).unwrap();
    std::fs::write(&path, b"abcdef").unwrap();
    assert_eq!(
        stream(
            "logs",
            &"producer",
            &json!({"cursor":absent["nextCursor"],"limit":3}),
            &path,
            None,
            true,
            false
        )
        .unwrap()["text"],
        "abc"
    );
    let first = stream(
        "logs",
        &"producer",
        &json!({"limit":3}),
        &path,
        None,
        true,
        false,
    )
    .unwrap();
    assert_eq!(first["text"], "abc");
    let args = json!({"cursor":first["nextCursor"],"limit":3});
    let last = stream("logs", &"producer", &args, &path, None, false, false).unwrap();
    assert_eq!(last["text"], "def");
    assert!(last["nextCursor"].is_null());
    let live = stream("logs", &"producer", &args, &path, None, true, false).unwrap();
    let end = stream(
        "logs",
        &"producer",
        &json!({"cursor":live["nextCursor"]}),
        &path,
        None,
        true,
        false,
    )
    .unwrap();
    assert_eq!(end["text"], "");
    assert_eq!(end["running"], true);
    assert!(end["nextCursor"].is_string());
    std::fs::write(&path, b"a").unwrap();
    assert_eq!(
        stream("logs", &"producer", &args, &path, None, false, false)
            .unwrap_err()
            .code,
        "invalid_cursor"
    );
}

#[test]
fn stream_reads_beyond_eight_mebibytes_without_an_unreachable_tail() {
    use std::io::{Seek, Write};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("output");
    let mut file = std::fs::File::create(&path).unwrap();
    file.seek(std::io::SeekFrom::Start(8 * 1024 * 1024))
        .unwrap();
    file.write_all(b"tail").unwrap();
    drop(file);
    let mut cursor = None;
    let mut read = 0;
    let mut tail = String::new();
    loop {
        let page = stream(
            "logs",
            &"producer",
            &canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(65536)),
            &path,
            None,
            false,
            false,
        )
        .unwrap();
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(page["dataBase64"].as_str().unwrap())
            .unwrap();
        read += bytes.len();
        if bytes.ends_with(b"tail") {
            tail = "tail".into();
        }
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(read, 8 * 1024 * 1024 + 4);
    assert_eq!(tail, "tail");
}
#[test]
fn ui_pages_retain_document_paths_and_refuse_changed_observations() {
    let root = json!({"role":"application","elementId":"root","children":[{"elementId":"a","role":"button","label":"a"},{"elementId":"b","role":"button","label":"b"}]});
    let query: crate::ui::Query = serde_json::from_value(json!({"role":"button"})).unwrap();
    let first = ui::find(
        &root,
        &query,
        &json!({"query":{"role":"button"},"limit":1}),
        &json!("target"),
    )
    .unwrap();
    assert_eq!(first["matches"][0]["elementId"], "a");
    let args = json!({"query":{"role":"button"},"limit":1,"cursor":first["nextCursor"]});
    let next = ui::find(&root, &query, &args, &json!("target")).unwrap();
    assert_eq!(next["matches"][0]["elementId"], "b");
    let mut changed = root.clone();
    changed["children"][0]["label"] = json!("changed");
    assert_eq!(
        ui::find(&changed, &query, &args, &json!("target"))
            .unwrap_err()
            .code,
        "invalid_cursor"
    );
    let page = ui::snapshot(&root, &json!({"limit":100}), &json!("target")).unwrap();
    assert_eq!(page["nodes"][0]["path"], "");
    assert!(page["nextCursor"].is_string());
}
