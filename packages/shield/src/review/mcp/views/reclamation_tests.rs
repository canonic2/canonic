use super::*;

#[test]
fn handles_charge_their_capture_and_prepare_only_on_evaluation() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let state = Store::open(root.path())
        .unwrap()
        .read_following_files()
        .unwrap();
    let candidate = &state.candidates["project"];
    let mut views = Views::new("capture-admission".into());
    let id = views
        .retain_prepared(
            &context,
            candidate,
            &state.operator,
            None,
            Some(&state.policy_revision),
            None,
        )
        .unwrap();
    assert!(views.bytes >= crate::json::encoded_len(&[candidate, &state.operator]).unwrap());
    let captured = views.get(&id).unwrap();
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
fn a_handle_keeps_its_capture_after_the_files_change() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "cli".into();
    let mut views = Views::new("acme-lazy-reader".into());
    let id = views.active_readonly(&context).unwrap();
    let captured = views.get(&id).unwrap();
    let original = captured.candidate.clone();
    let revision = captured.revision().to_owned();
    std::fs::write(
        root.path().join(".shield/config.yml"),
        format!(
            "schema_version: 1\npacks: []\naccess_paths:\n  read:\n  - {}\n",
            crate::test_support::absolute("/srv/acme")
        ),
    )
    .unwrap();
    assert_eq!(captured.candidate, original);
    assert_eq!(
        views
            .prepare(&context, &id)
            .unwrap()
            .evaluator()
            .unwrap()
            .revision(),
        revision
    );
    let next = views.active_readonly(&context).unwrap();
    assert_ne!(next, id);
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
