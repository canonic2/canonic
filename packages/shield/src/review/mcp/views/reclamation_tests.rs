use super::*;

#[test]
fn recorded_source_handles_charge_logical_source_and_prepare_only_on_evaluation() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let state = Store::inspect(root.path())
        .unwrap()
        .read_diagnostic_checkpoint()
        .unwrap()
        .0;
    let reference = &state.candidates["project"];
    let mut views = Views::new("recorded-source-admission".into());
    let id = views
        .retain_prepared(
            &context,
            reference,
            &state.operator,
            None,
            Some(&state.policy_revision),
            None,
        )
        .unwrap();
    let logical = usize::try_from(reference["length"].as_u64().unwrap()).unwrap();
    assert!(
        views.bytes >= logical + crate::json::encoded_len(&[reference, &state.operator]).unwrap()
    );
    let captured = views.get(&id).unwrap();
    assert!(captured.source.is_some());
    assert!(captured.cached.is_none());
    assert!(captured.evaluator.get().is_none());
    assert_eq!(
        views
            .prepare(&context, &id)
            .unwrap()
            .evaluator()
            .unwrap()
            .revision(),
        state.policy_revision
    );
}

#[test]
fn storage_reclamation_lazy_mcp_capture_keeps_its_source_after_cache_eviction_and_activation() {
    use crate::review::{self, storage::sources};
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut context = review::mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "cli".into();
    let mut views = Views::new("acme-lazy-reader".into());
    let id = views.active(&context).unwrap();
    let captured = views.get(&id).unwrap();
    let candidate = captured.candidate.clone();
    let original = captured.source().unwrap();
    let revision = captured.revision().to_owned();
    assert!(captured.evaluator.get().is_none());
    for index in 0..4 {
        sources::activate_fixture(&context, &format!("acme-{index}"));
    }
    let descriptor = json!(captured.cached.as_ref().unwrap().artifact);
    let path = std::path::PathBuf::from(descriptor["directory"].as_str().unwrap())
        .join("compiled")
        .join(descriptor["project_key"].as_str().unwrap())
        .join(format!("{}.bin", descriptor["plan_key"].as_str().unwrap()));
    assert!(!path.exists(), "unused old plans are reclaimed");
    assert_eq!(captured.source().unwrap(), original);
    assert_eq!(
        views
            .prepare(&context, &id)
            .unwrap()
            .evaluator()
            .unwrap()
            .revision(),
        revision
    );
    drop(captured);
    drop(views);
    sources::activate_fixture(&context, "acme-after-reader-release");
    assert!(
        Store::open(root.path())
            .unwrap()
            .captured_candidate(&candidate)
            .is_err()
    );
}

#[test]
fn inspection_reclamation_finishes_large_traversals_and_replays_without_growth() {
    let mut views = Views::new("acme-long-list".into());
    let first = views
        .page(
            "acme",
            &json!({"limit":1}),
            json!({"rows":(0..1000).collect::<Vec<_>>(),"revision":"captured"}),
            "rows",
        )
        .unwrap();
    let mut cursor = first["next_cursor"].clone();
    let original = cursor.clone();
    let mut expected = 1;
    while !cursor.is_null() {
        let args = json!({"limit":1,"cursor":cursor});
        let page = views.resume("acme", &args).unwrap();
        assert_eq!(page["rows"], json!([expected]));
        assert_eq!(page["revision"], "captured");
        let count = views.cursors.len();
        let bytes = views.cursor_bytes;
        for _ in 0..3 {
            assert_eq!(views.resume("acme", &args).unwrap(), page);
        }
        assert_eq!(views.cursors.len(), count);
        assert_eq!(views.cursor_bytes, bytes);
        assert!(count <= 256);
        cursor = page["next_cursor"].clone();
        expected += 1;
    }
    assert_eq!(expected, 1000);
    assert!(
        views
            .cursors
            .values()
            .all(|cursor| matches!(cursor.content, paging::Content::Replay { .. }))
    );
    assert!(matches!(
        views.resume("acme", &json!({"limit":1,"cursor":original})),
        Err(Error::CursorUnavailable)
    ));
    let next = views
        .page(
            "acme",
            &json!({"limit":1}),
            json!({"rows":["fresh","next"]}),
            "rows",
        )
        .unwrap();
    assert_eq!(
        views
            .resume("acme", &json!({"limit":1,"cursor":next["next_cursor"]}))
            .unwrap()["rows"],
        json!(["next"])
    );
}

#[test]
fn inspection_reclamation_releases_completed_list_bytes_and_bounds_abandoned_views() {
    let mut views = Views::new("acme-bytes".into());
    let data = json!({"rows":["x".repeat(300_000),"y".repeat(300_000)]});
    let first = views
        .page("acme", &json!({"limit":1}), data.clone(), "rows")
        .unwrap();
    let before = views.cursor_bytes;
    let args = json!({"limit":1,"cursor":first["next_cursor"]});
    let retained = match &views.cursors[args["cursor"].as_str().unwrap()].content {
        paging::Content::Pending { data, .. } => Arc::downgrade(data),
        paging::Content::Replay { .. } => unreachable!(),
    };
    let last = views.resume("acme", &args).unwrap();
    assert!(
        retained.upgrade().is_none(),
        "final page must release the full retained list"
    );
    assert!(last["next_cursor"].is_null());
    assert!(views.cursor_bytes < before);
    assert_eq!(views.resume("acme", &args).unwrap(), last);
    for _ in 0..120 {
        views
            .page("acme", &json!({"limit":1}), data.clone(), "rows")
            .unwrap();
        assert!(views.cursor_bytes <= 64 * 1024 * 1024);
    }
    assert!(
        views.cursors.len() < 256,
        "byte pressure must reclaim before count pressure"
    );
    assert!(matches!(
        views.resume("acme", &args),
        Err(Error::CursorUnavailable)
    ));
}

#[test]
fn inspection_reclamation_does_not_consume_invalid_or_failed_continuations() {
    let mut views = Views::new("acme-failures".into());
    let first = views
        .page("acme", &json!({"limit":1}), json!({"rows":[0,1,2]}), "rows")
        .unwrap();
    let args = json!({"limit":1,"cursor":first["next_cursor"]});
    assert!(views.resume("foreign", &args).is_err());
    assert!(
        views
            .resume("acme", &json!({"limit":2,"cursor":first["next_cursor"]}))
            .is_err()
    );
    let next = views.resume("acme", &args).unwrap();
    assert_eq!(next["rows"], json!([1]));
    let mut foreign = Views::new("another".into());
    assert!(matches!(
        foreign.resume("acme", &args),
        Err(Error::CursorUnavailable)
    ));
    let huge = json!({"rows":[0,"x".repeat(1024*1024)]});
    let first = views
        .page("acme", &json!({"limit":1}), huge, "rows")
        .unwrap();
    let args = json!({"limit":1,"cursor":first["next_cursor"]});
    let bytes = views.cursor_bytes;
    assert!(views.resume("acme", &args).is_err());
    assert_eq!(views.cursor_bytes, bytes);
    assert!(views.resume("acme", &args).is_err());
}
