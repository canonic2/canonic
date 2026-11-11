//! Real segmented journal ownership, crash recovery, controls, reader
//! correlation and format boundaries.
use super::*;
use crate::review::{self, observations, tests::enrolled};
use std::collections::BTreeSet;
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[cfg(unix)]
mod controls;
#[cfg(windows)]
mod windows_controls;

fn decision(context: &review::Context, name: &str) -> Value {
    observations::decision(context, json!({"operation":{"kind":"argv","executable":"acme","arguments":[name]},"host_decision":"allow","evaluation":{"decision":"ask"},"disposition":"allowed","ask_fallback":"allow"})).unwrap()
}
fn operation(name: &str) -> Value {
    json!({"kind":"argv","executable":"acme","arguments":[name]})
}
#[test]
fn history_identifiers_refuse_malformed_owners_without_panicking() {
    for owner in ["host", "", "éééééééééééééééé", &"z".repeat(64)] {
        assert!(identifier("event", owner).is_err());
    }
    assert!(
        identifier("event", &"a".repeat(64))
            .unwrap()
            .starts_with("event-aaaaaaaaaaaaaaaa-")
    );
}
fn completion(
    context: &review::Context,
    delivery: &str,
    request: Option<&Value>,
    name: &str,
    status: &str,
) -> Value {
    let input = review::EventInput {
        delivery_id: delivery.into(),
        request_id: request.and_then(Value::as_str).map(str::to_owned),
        status: status.into(),
    };
    observations::completion(context, &input, &operation(name)).unwrap()
}
/// The active segment of the current generation.
fn active(store: &Store, state: &State) -> PathBuf {
    segments(store, state).unwrap().pop().unwrap().path
}
fn event(store: &Store, receipt: &Value) -> Option<Value> {
    store
        .read_history()
        .unwrap()
        .events
        .into_iter()
        .find(|e| e["event_id"] == receipt["event_id"])
}

#[test]
fn diagnostic_history_capture_waits_for_the_history_writer() {
    use clap::Parser as _;
    let (root, context) = enrolled();
    decision(&context, "before rotation");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("command", "status", None, None).unwrap();
    let guard = lock(&store).unwrap();
    std::thread::scope(|scope| {
        let (send, receive) = std::sync::mpsc::channel();
        let trace = &trace;
        let store = &store;
        let reader = scope.spawn(move || {
            let _scope = trace.enter();
            send.send(store.read_diagnostics()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while trace.recorded_count("history.lock_attempts") == 0 {
            assert!(
                Instant::now() < deadline,
                "reader did not attempt history locking"
            );
            std::thread::yield_now();
        }
        assert_eq!(trace.recorded_count("history.captures"), 0);
        assert!(receive.try_recv().is_err());
        // A generation rewrite under its ordinary writer lock cannot be
        // observed halfway through by status or doctor.
        replace(store, &state, &[], false).unwrap();
        drop(guard);
        let (loaded, pending) = receive
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert!(!pending);
        assert!(loaded.history_available);
        assert!(loaded.events.is_empty());
        reader.join().unwrap();
    });
    trace.finish("success");
}

#[test]
fn diagnostic_history_does_not_create_a_missing_lock_file() {
    let (root, _) = enrolled();
    let store = Store::inspect(root.path()).unwrap();
    fs::remove_file(store.directory.join("history.lock")).unwrap();
    let before = store.checkpoint_bytes().unwrap();
    let (state, _) = store.read_diagnostics().unwrap();
    assert!(!state.history_available);
    assert!(!store.directory.join("history.lock").exists());
    assert_eq!(store.checkpoint_bytes().unwrap(), before);
}

#[test]
fn corrupt_optional_history_owner_does_not_revoke_policy_and_clear_repairs_it() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let store = Store::open(root.path()).unwrap();
    let checkpoint = store.directory.join("state.json");
    let mut value: Value = serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
    value["history_owner"] = json!("invalid-owner");
    fs::write(&checkpoint, value.to_string()).unwrap();
    let before = store.checkpoint_bytes().unwrap();
    store.read_checkpoint().unwrap();
    assert!(!store.read_history().unwrap().history_available);
    assert!(
        crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .is_ok()
    );
    assert_eq!(store.checkpoint_bytes().unwrap(), before);
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    let repaired = store.read_history().unwrap();
    assert!(repaired.history_available);
    assert!(repaired.history_owner.is_none());
}

#[test]
fn callback_work_counters_separate_bounded_append_from_history_inspection() {
    use clap::Parser as _;
    let (root, context) = enrolled();
    let request = decision(&context, "before");
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let callback = log.request("hook", "completion", None, None).unwrap();
    {
        let _scope = callback.enter();
        completion(
            &context,
            "acme-delivery",
            Some(&request["request_id"]),
            "before",
            "completed",
        );
    }
    assert_eq!(callback.recorded_count("history.appended_records"), 1);
    assert!(callback.recorded_count("history.appended_bytes") > 0);
    assert!(callback.recorded_count("history.tail_read_bytes") <= (LINE_LIMIT + 1) as u64);
    for name in [
        "history.captures",
        "history.read_bytes",
        "history.decoded_records",
        "history.correlations",
        "history.control_rewrites",
        "policy.preparations",
    ] {
        assert_eq!(callback.recorded_count(name), 0, "{name}");
    }
    callback.finish("success");
    let inspection = log.request("command", "history", None, None).unwrap();
    {
        let _scope = inspection.enter();
        Store::open(root.path()).unwrap().read_history().unwrap();
    }
    assert_eq!(inspection.recorded_count("history.captures"), 1);
    assert_eq!(inspection.recorded_count("history.correlations"), 1);
    assert!(inspection.recorded_count("history.read_bytes") > 0);
    assert!(inspection.recorded_count("history.decoded_records") >= 2);
    assert_eq!(inspection.recorded_count("history.appended_records"), 0);
    inspection.finish("success");
}

#[test]
fn optional_history_callbacks_do_not_wait_or_read_state_behind_a_busy_writer() {
    use clap::Parser as _;
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let _guard = lock(&store).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let request = log.request("hook", "completion", None, None).unwrap();
    {
        let _scope = request.enter();
        assert!(matches!(
            observations::completion(
                &context,
                &review::EventInput {
                    delivery_id: "busy-history".into(),
                    request_id: None,
                    status: "completed".into(),
                },
                &operation("busy")
            ),
            Err(Error::Locked)
        ));
    }
    assert_eq!(request.recorded_count("history.lock_attempts"), 1);
    assert_eq!(request.recorded_count("history.lock_waits"), 1);
    assert_eq!(request.recorded_count("state.checkpoint_reads"), 0);
    assert_eq!(request.recorded_count("history.appended_records"), 0);
    request.finish("dropped");
}

#[cfg(windows)]
#[test]
fn windows_history_refuses_an_explicit_public_acl_inside_a_private_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_permissions::{
        LocalBox, SecurityDescriptor,
        constants::{SeObjectType, SecurityInformation},
        wrappers,
    };
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    decision(&context, "Acme history");
    let path = active(&store, &store.read().unwrap());
    assert!(regular(&path).is_ok());
    let original = fs::read(&path).unwrap();
    let mut file = OpenOptions::new()
        .read(true)
        .access_mode(0x0006_0080)
        .open(&path)
        .unwrap();
    let descriptor = wrappers::GetSecurityInfo(
        &file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )
    .unwrap();
    let broad: LocalBox<SecurityDescriptor> = format!(
        "{}(A;;FR;;;WD)",
        descriptor.as_sddl().unwrap().to_str().unwrap()
    )
    .parse()
    .unwrap();
    wrappers::SetSecurityInfo(
        &mut file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        broad.dacl(),
        None,
    )
    .unwrap();
    assert!(regular(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    crate::windows_security::make_private(&path).unwrap();
    assert!(regular(&path).is_ok());
}

#[test]
fn parallel_append_preserves_all_unique_records_and_leaves_authority_unchanged() {
    let (root, context) = enrolled();
    let before = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    thread::scope(|scope| {
        for worker in 0..16 {
            let context = &context;
            scope.spawn(move || {
                for n in 0..8 {
                    let began = Instant::now();
                    loop {
                        match observations::decision(context, json!({"operation":{"kind":"argv","executable":"acme","arguments":[format!("{worker}-{n}")]},"host_decision":"allow"})) {
                            Ok(receipt) => { assert_eq!(receipt["status"], "recorded"); break; }
                            Err(Error::Locked) if began.elapsed() < Duration::from_secs(10) => thread::yield_now(),
                            other => panic!("bounded caller retry failed: {other:?}"),
                        }
                    }
                }
            });
        }
    });
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        before
    );
    let events = Store::open(root.path())
        .unwrap()
        .read_history()
        .unwrap()
        .events;
    assert_eq!(events.len(), 128);
    let ids: BTreeSet<_> = events
        .iter()
        .map(|e| e["event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), events.len());
    let requests: BTreeSet<_> = events
        .iter()
        .map(|e| e["links"]["request_id"].as_str().unwrap())
        .collect();
    assert_eq!(requests.len(), events.len());
}
#[test]
fn recent_reads_newest_first_within_retention_and_the_tail_budget() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let names = |state: &State| {
        let mut seen = Vec::new();
        recent(&store, state, |event| {
            seen.push(event["details"]["operation"]["arguments"][0].clone());
            ControlFlow::Continue(())
        })
        .unwrap();
        seen
    };
    for name in ["a", "b", "c", "d"] {
        decision(&context, name);
    }
    assert_eq!(
        names(&store.read_checkpoint().unwrap()),
        [json!("d"), json!("c"), json!("b"), json!("a")]
    );
    review::history_settings(&context, None, Some(2), false, &|| Ok(())).unwrap();
    for name in ["e", "f", "g"] {
        decision(&context, name);
    }
    // Retention keeps the newest two commands; older ones are not visited.
    let state = store.read_checkpoint().unwrap();
    assert_eq!(names(&state), [json!("g"), json!("f")]);
    // A torn final append is not a record.
    OpenOptions::new()
        .append(true)
        .open(active(&store, &state))
        .unwrap()
        .write_all(b"{\"event\":")
        .unwrap();
    assert_eq!(names(&state), [json!("g"), json!("f")]);
}
#[test]
fn raising_the_limit_does_not_restore_expired_commands() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let names = || {
        store
            .read_history()
            .unwrap()
            .events
            .iter()
            .filter(|e| e["kind"] == "decision")
            .map(|e| e["details"]["operation"]["arguments"][0].clone())
            .collect::<Vec<_>>()
    };
    review::history_settings(&context, None, Some(1), false, &|| Ok(())).unwrap();
    decision(&context, "a");
    decision(&context, "b");
    assert_eq!(names(), [json!("b")]);
    review::history_settings(&context, None, Some(2), false, &|| Ok(())).unwrap();
    assert_eq!(names(), [json!("b")]);
    decision(&context, "c");
    assert_eq!(names(), [json!("b"), json!("c")]);
}
#[test]
fn recent_scans_stop_when_visitors_break_and_ignore_records_beyond_the_tail() {
    let (root, context) = enrolled();
    let operation = operation("routine");
    let store = Store::open(root.path()).unwrap();
    // Count matching records in the recent tail, stopping at `cap`.
    let tail_matches = |cap: usize| {
        let state = store.read_checkpoint().unwrap();
        let mut count = 0;
        recent(&store, &state, |event| {
            if event["details"]["operation"] == operation {
                count += 1;
            }
            if count >= cap {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
        count
    };
    // Retain every filler command below, so only the byte budget excludes records.
    review::history_settings(&context, None, Some(10_000), false, &|| Ok(())).unwrap();
    for _ in 0..5 {
        decision(&context, "routine");
    }
    assert_eq!(tail_matches(3), 3);
    // Push those records out of the newest TAIL_BUDGET bytes with valid,
    // checksummed records; the journal still holds them.
    let mut state = store.read_checkpoint().unwrap();
    let filler = "x".repeat(LINE_LIMIT / 2);
    let mut bytes = Vec::new();
    for n in 0..(2 * TAIL_BUDGET / (LINE_LIMIT / 2)) {
        let event = state.event(
            "decision",
            &context,
            json!({"request_id":format!("filler-{n}")}),
            json!({}),
            json!({"operation":{"filler":filler}}),
        );
        bytes.extend(line(&event).unwrap());
    }
    OpenOptions::new()
        .append(true)
        .open(active(&store, &state))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    let matches = |events: &[Value]| {
        events
            .iter()
            .filter(|e| e["details"]["operation"] == operation)
            .count()
    };
    assert_eq!(matches(&store.read_history().unwrap().events), 5);
    assert_eq!(tail_matches(3), 0);
    // The next append seals that oversized segment; the tail spans both.
    decision(&context, "routine");
    assert_eq!(segments(&store, &state).unwrap().len(), 2);
    assert_eq!(tail_matches(3), 1);
    assert_eq!(matches(&store.read_history().unwrap().events), 6);
}
#[test]
fn torn_tail_is_preserved_and_only_complete_new_records_are_published() {
    let (root, context) = enrolled();
    decision(&context, "first");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    OpenOptions::new()
        .append(true)
        .open(active(&store, &state))
        .unwrap()
        .write_all(b"{\"event\":")
        .unwrap();
    assert!(!store.read_history().unwrap().history_available);
    assert_eq!(store.read_history().unwrap().events.len(), 1);
    decision(&context, "second");
    let loaded = store.read_history().unwrap();
    assert_eq!(loaded.events.len(), 2);
    assert!(!loaded.history_available);
    assert_eq!(
        fs::read(error_path(&store, &state).unwrap()).unwrap(),
        b"{\"event\":"
    );
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    let loaded = store.read_history().unwrap();
    assert!(loaded.history_available);
    assert_eq!(loaded.events, [] as [Value; 0]);
}
#[test]
fn corruption_does_not_change_authority_and_clear_can_recover() {
    let (root, context) = enrolled();
    decision(&context, "first");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let file = active(&store, &state);
    let original = fs::read(&file).unwrap();
    let mut corrupt = original.clone();
    let at = corrupt.windows(5).position(|b| b == b"first").unwrap();
    corrupt[at] = b'X';
    fs::write(&file, &corrupt).unwrap();
    assert!(!store.read_history().unwrap().history_available);
    assert!(store.read_checkpoint().is_ok());
    assert!(review::history_settings(&context, None, Some(1), false, &|| Ok(())).is_err());
    assert_eq!(fs::read(&file).unwrap(), corrupt);
    review::history_settings(&context, Some(false), None, false, &|| Ok(())).unwrap();
    assert_eq!(decision(&context, "disabled")["status"], "disabled");
    assert_eq!(fs::read(&file).unwrap(), corrupt);
    // Unavailable optional evidence cannot prevent a minimum authority update.
    fs::remove_file(&file).unwrap();
    let store_path = store.directory.join("state.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&store_path).unwrap()).unwrap();
    value["history_generation"] = json!("invalid-optional-generation");
    fs::write(&store_path, value.to_string()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.sequence += 1;
            Ok(json!({}))
        })
        .unwrap();
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    assert!(store.read_history().unwrap().history_available);
}
#[test]
fn appends_never_read_earlier_records_so_corruption_is_reader_evidence() {
    let (root, context) = enrolled();
    let first = decision(&context, "first");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let file = active(&store, &state);
    let mut corrupt = fs::read(&file).unwrap();
    let at = corrupt.windows(5).position(|b| b == b"first").unwrap();
    corrupt[at] = b'X';
    fs::write(&file, &corrupt).unwrap();
    // Neither callback scans earlier records, so both still publish.
    assert_eq!(decision(&context, "second")["status"], "recorded");
    let receipt = completion(
        &context,
        "after-corruption",
        Some(&first["request_id"]),
        "first",
        "completed",
    );
    assert_eq!(receipt["status"], "recorded");
    assert!(receipt["request_id"].is_null());
    let loaded = store.read_history().unwrap();
    assert!(!loaded.history_available);
    assert_eq!(loaded.events, [] as [Value; 0]);
}
#[test]
fn retention_controls_and_late_completion_do_not_restore_pruned_commands() {
    let (root, context) = enrolled();
    let first = decision(&context, "first");
    let latest = decision(&context, "latest");
    review::history_settings(&context, None, Some(1), false, &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_history().unwrap();
    assert_eq!(state.events.len(), 1);
    assert_eq!(state.events[0]["links"]["request_id"], latest["request_id"]);
    let late = completion(
        &context,
        "late",
        Some(&first["request_id"]),
        "first",
        "completed",
    );
    assert!(late["request_id"].is_null());
    let late = event(&store, &late).unwrap();
    assert!(late["links"]["request_id"].is_null());
    assert_eq!(late["details"]["reported_request_id"], first["request_id"]);
    review::history_settings(&context, Some(false), None, false, &|| Ok(())).unwrap();
    assert_eq!(decision(&context, "disabled")["status"], "disabled");
    // A segment temporary left by a crashed writer is removed under the lock.
    let leftover = store.directory.join(".history-file-crashed.tmp");
    fs::write(&leftover, b"partial").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&leftover, fs::Permissions::from_mode(0o600)).unwrap();
    }
    review::history_settings(&context, Some(true), None, true, &|| Ok(())).unwrap();
    assert_eq!(store.read_history().unwrap().events, [] as [Value; 0]);
    assert!(!leftover.exists());
}
#[test]
fn captured_readers_keep_their_snapshot_through_later_appends_rotation_and_pruning() {
    let (root, context) = enrolled();
    review::history_settings(&context, None, Some(3), false, &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    for name in ["a", "b", "c"] {
        decision(&context, name);
    }
    let mut state = store.read_checkpoint().unwrap();
    let view = capture(&store, &state).unwrap();
    // Writers rotate past the captured segment. Its shared kernel pin keeps
    // bytes linked and accounted until this reader finishes outside the lock.
    for n in 0..400 {
        observations::decision(&context, json!({"operation":operation(&format!("later-{n}")),"request":{"padding":"x".repeat(6500)},"evaluation":{"decision":"deny"}})).unwrap();
    }
    assert_eq!(segments(&store, &state).unwrap()[0].sequence, 0);
    finish(Ok(view), &mut state);
    assert!(state.history_available && !state.history_evicted);
    let names: Vec<_> = state
        .events
        .iter()
        .map(|e| e["details"]["operation"]["arguments"][0].clone())
        .collect();
    assert_eq!(names, [json!("a"), json!("b"), json!("c")]);
    for n in 400..600 {
        observations::decision(&context, json!({"operation":operation(&format!("later-{n}")),"request":{"padding":"x".repeat(6500)},"evaluation":{"decision":"deny"}})).unwrap();
    }
    assert!(segments(&store, &state).unwrap()[0].sequence > 0);
}

#[test]
fn holes_after_a_pinned_oldest_segment_report_byte_pressure_loss() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read_checkpoint().unwrap();
    state.history_generation = generation().unwrap();
    for sequence in [0, 2] {
        let event = state.event(
            "decision",
            &context,
            json!({"request_id":format!("gap-{sequence}")}),
            json!({}),
            json!({}),
        );
        write_segment(
            &store,
            &state,
            sequence,
            (sequence == 0).then_some(1),
            &line(&event).unwrap(),
        )
        .unwrap();
    }
    finish(capture(&store, &state), &mut state);
    assert!(state.history_available);
    assert!(
        state.history_evicted,
        "a pinned sequence zero cannot conceal later gaps"
    );
    assert_eq!(distinct_decisions(&state.events), 2);
}
#[test]
fn rotation_seals_counted_segments_and_prunes_commands_outside_retention() {
    let (root, context) = enrolled();
    review::history_settings(&context, None, Some(5), false, &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    for n in 0..400 {
        observations::decision(&context, json!({"operation":operation(&n.to_string()),"request":{"padding":"x".repeat(6500)},"evaluation":{"decision":"deny"}})).unwrap();
    }
    let state = store.read_history().unwrap();
    assert!(state.history_available);
    assert!(!state.history_evicted);
    assert_eq!(state.events.len(), 5);
    assert_eq!(state.events[4]["details"]["operation"], operation("399"));
    let list = segments(&store, &state).unwrap();
    // Older segments held no retained command and were removed at rotation.
    assert!(list[0].sequence > 0 && list.len() <= 3, "{}", list.len());
    for sealed in &list[..list.len() - 1] {
        assert!(sealed.decisions.unwrap() > 5);
        assert!(fs::metadata(&sealed.path).unwrap().len() <= (SEGMENT_LIMIT + 1024) as u64);
    }
    assert!(list.last().unwrap().decisions.is_none());
}
#[test]
fn byte_cap_evicts_oldest_segments_and_reports_fewer_commands() {
    let (root, context) = enrolled();
    // Retain more commands than fit, so only the byte cap evicts.
    review::history_settings(&context, None, Some(10_000), false, &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    decision(&context, "seed");
    let mut state = store.read_checkpoint().unwrap();
    for old in segments(&store, &state).unwrap() {
        fs::remove_file(old.path).unwrap();
    }
    // Full segments just under the byte cap, each with distinct retained requests.
    let padding = "x".repeat(9000);
    let mut request = 0;
    let mut fill = |state: &mut State| {
        let (mut bytes, mut count) = (Vec::new(), 0);
        loop {
            let event = state.event(
                "decision",
                &context,
                json!({"request_id":format!("filled-{request}")}),
                json!({}),
                json!({"operation":{"padding":padding}}),
            );
            let row = line(&event).unwrap();
            if bytes.len() + row.len() + 1024 > SEGMENT_LIMIT {
                break;
            }
            request += 1;
            count += 1;
            bytes.extend(row);
        }
        (bytes, count)
    };
    let sealed = (STORE_LIMIT / SEGMENT_LIMIT - 2) as u64;
    for sequence in 0..sealed {
        let (bytes, count) = fill(&mut state);
        write_segment(&store, &state, sequence, Some(count), &bytes).unwrap();
    }
    let (bytes, _) = fill(&mut state);
    write_segment(&store, &state, sealed, None, &bytes).unwrap();
    let at_cap = store.read_history().unwrap();
    assert!(at_cap.history_available && !at_cap.history_evicted);
    // The next append seals the full active segment and evicts the oldest.
    let mut latest = decision(&context, "latest");
    // Several ordinary callbacks cross the fixture's remaining record slack
    // without relying on an oversized segment.
    for _ in 0..16 {
        latest = decision(&context, "latest");
    }
    let list = segments(&store, &state).unwrap();
    let total: u64 = list
        .iter()
        .map(|s| fs::metadata(&s.path).unwrap().len())
        .sum();
    assert!(list[0].sequence > 0);
    assert!(total <= STORE_LIMIT as u64);
    let loaded = store.read_history().unwrap();
    assert!(loaded.history_available);
    assert!(loaded.history_evicted);
    assert!(
        loaded
            .events
            .iter()
            .all(|e| e["links"]["request_id"] != "filled-0")
    );
    assert!(event(&store, &latest).is_some());
    // A limit change rewrites at most the snapshot bound and keeps reporting
    // both that loss and the earlier byte-cap eviction; clearing ends it.
    let checkpoint = store.checkpoint_bytes().unwrap();
    assert!(
        review::history_settings(&context, None, Some(9_999), false, &|| Ok(())).is_err(),
        "a full store cannot allocate a second large control copy"
    );
    assert_eq!(store.checkpoint_bytes().unwrap(), checkpoint);
    review::history_settings(&context, None, Some(3), false, &|| Ok(())).unwrap();
    let rewritten = store.read_history().unwrap();
    assert!(rewritten.history_available && !rewritten.history_evicted);
    assert_eq!(distinct_decisions(&rewritten.events), 3);
    assert!(event(&store, &latest).is_some());
    review::history_settings(&context, None, Some(1000), false, &|| Ok(())).unwrap();
    let raised = store.read_history().unwrap();
    assert!(raised.history_available && raised.history_evicted);
    assert_eq!(
        distinct_decisions(&raised.events),
        3,
        "raising a limit cannot resurrect evicted requests"
    );
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    assert!(!store.read_history().unwrap().history_evicted);
}
#[test]
fn copied_history_is_the_copys_own_journal_and_leaves_the_original_alone() {
    let (original, mut context) = enrolled();
    context.surface = "sdk".into();
    context.cwd = Some(context.project_root.clone());
    context.tool_use_id = Some("acme-call".into());
    let decided = decision(&context, "read");
    let source = Store::open(original.path()).unwrap();
    let checkpoint = source.checkpoint_bytes().unwrap();
    let history = source.read_history().unwrap().events;
    let copied = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(original.path(), copied.path());
    let opened = review::open(copied.path()).unwrap();
    let mut child = context.clone();
    child.project_id = opened.project_id;
    child.project_root = opened.project_root;
    child.cwd = Some(child.project_root.clone());
    assert_eq!(child.project_id, context.project_id);
    let store = Store::open(copied.path()).unwrap();
    let retained = event(&store, &decided).expect("copied historical decision remains inspectable");
    assert_eq!(retained["provenance"]["project_id"], context.project_id);
    assert_eq!(retained["provenance"]["cwd"], ".");
    assert!(retained["provenance"].get("project_root").is_none());
    let fresh = decision(&child, "read");
    let completed = completion(
        &child,
        "copy-fresh-request",
        Some(&fresh["request_id"]),
        "read",
        "completed",
    );
    assert_eq!(
        event(&store, &completed).unwrap()["links"]["decision_event_id"],
        fresh["event_id"]
    );
    assert_eq!(source.checkpoint_bytes().unwrap(), checkpoint);
    assert_eq!(source.read_history().unwrap().events, history);
    let completed = completion(
        &context,
        "original-request",
        Some(&decided["request_id"]),
        "read",
        "completed",
    );
    assert_eq!(
        event(&source, &completed).unwrap()["links"]["decision_event_id"],
        decided["event_id"]
    );
}

#[test]
fn reader_links_exact_completions_and_marks_duplicate_and_conflicting_deliveries() {
    let (root, context) = enrolled();
    review::history_settings(&context, None, Some(3), false, &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    let decided = decision(&context, "read");
    let linked = completion(
        &context,
        "one",
        Some(&decided["request_id"]),
        "read",
        "completed",
    );
    assert!(linked["request_id"].is_null());
    let view = event(&store, &linked).unwrap();
    assert_eq!(view["links"]["request_id"], decided["request_id"]);
    assert_eq!(view["links"]["decision_event_id"], decided["event_id"]);
    // A repeated identical delivery is recorded raw and recognized by readers.
    let repeated = completion(
        &context,
        "one",
        Some(&decided["request_id"]),
        "read",
        "completed",
    );
    assert_ne!(repeated["event_id"], linked["event_id"]);
    assert!(event(&store, &repeated).is_none());
    // Changed content under the same delivery conflicts; it is not a later winner.
    let changed = completion(
        &context,
        "one",
        Some(&decided["request_id"]),
        "read",
        "failed",
    );
    let first = event(&store, &linked).unwrap();
    let later = event(&store, &changed).unwrap();
    assert_eq!(first["details"]["delivery_conflict"], true);
    assert_eq!(first["links"]["decision_event_id"], decided["event_id"]);
    assert_eq!(later["details"]["delivery_conflict"], true);
    assert!(later["links"]["request_id"].is_null());
    // Another operation or origin does not link, whatever request it reports.
    let other = completion(
        &context,
        "two",
        Some(&decided["request_id"]),
        "write",
        "completed",
    );
    assert!(event(&store, &other).unwrap()["links"]["request_id"].is_null());
    let mut elsewhere = context.clone();
    elsewhere.session_id = Some("acme-other-session".into());
    let foreign = completion(
        &elsewhere,
        "three",
        Some(&decided["request_id"]),
        "read",
        "completed",
    );
    assert!(event(&store, &foreign).unwrap()["links"]["request_id"].is_null());
    // After three newer commands the request left retention: a late completion
    // stays unlinked, and the original command is no longer an entry.
    for name in ["a", "b", "c"] {
        decision(&context, name);
    }
    let late = completion(
        &context,
        "late",
        Some(&decided["request_id"]),
        "read",
        "completed",
    );
    assert!(event(&store, &late).unwrap()["links"]["request_id"].is_null());
    assert!(event(&store, &decided).is_none());
    assert!(event(&store, &linked).is_none());
}
#[test]
fn retention_rewrite_keeps_raw_records_that_the_reader_links() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let decided = decision(&context, "kept");
    let linked = completion(
        &context,
        "kept",
        Some(&decided["request_id"]),
        "kept",
        "completed",
    );
    // More newer uncorrelated completions than the uncorrelated view retains.
    for n in 0..130 {
        completion(
            &context,
            &format!("unlinked-{n}"),
            None,
            "other",
            "completed",
        );
    }
    review::history_settings(&context, None, Some(2), false, &|| Ok(())).unwrap();
    let view = event(&store, &linked).unwrap();
    assert_eq!(view["links"]["decision_event_id"], decided["event_id"]);
    // The rewritten journal stores the completion raw, without links.
    let state = store.read_checkpoint().unwrap();
    let stored = fs::read_to_string(active(&store, &state)).unwrap();
    let raw: Value = stored
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["event"].clone())
        .find(|e| e["event_id"] == linked["event_id"])
        .unwrap();
    assert!(raw["links"]["request_id"].is_null());
    assert_eq!(raw["details"]["reported_request_id"], decided["request_id"]);
    let uncorrelated = store
        .read_history()
        .unwrap()
        .events
        .iter()
        .filter(|e| e["kind"] == "completion" && e["links"]["request_id"].is_null())
        .count();
    assert_eq!(uncorrelated, 128);
}
#[test]
fn unsupported_history_generation_refuses_without_changing_authority_or_files() {
    let (root, context) = enrolled();
    decision(&context, "old");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_history().unwrap();
    let bytes = serde_json::to_vec(&state.events).unwrap();
    let digest = crate::policy::fingerprint(&bytes);
    let legacy = store.directory.join(format!("history-{digest}.json"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    options.open(&legacy).unwrap().write_all(&bytes).unwrap();
    let checkpoint = store.directory.join("state.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    value["history_generation"] = json!(digest);
    fs::write(&checkpoint, value.to_string()).unwrap();
    let before = fs::read(&checkpoint).unwrap();
    let loaded = store.read_history().unwrap();
    assert!(!loaded.history_available);
    assert_eq!(loaded.events, [] as [Value; 0]);
    assert!(observations::decision(&context, json!({"host_decision":"allow"})).is_err());
    assert_eq!(fs::read(&checkpoint).unwrap(), before);
    assert_eq!(fs::read(&legacy).unwrap(), bytes);
    assert!(!legacy.with_extension("jsonl").exists());
    // Optional evidence does not prevent authority publication or explicit reset.
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.sequence += 1;
            Ok(json!({}))
        })
        .unwrap();
    review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    assert!(store.read_history().unwrap().history_available);
    assert_eq!(fs::read(legacy).unwrap(), bytes);
}
#[test]
fn compacted_guidance_keeps_exact_requests_or_omits_them() {
    let exact = json!({"source":"decisive","message":"Acme asks.",
        "request":{"executable":"acme","arguments":["x".repeat(300)]}});
    let oversized = json!({"source":"decisive","message":"Acme asks.",
        "request":{"executable":"acme","arguments":["x".repeat(5000)]}});
    let event = bound_event(json!({"kind":"decision","details":{"decision":"ask",
        "evaluation":{"explanation":{"padding":"x".repeat(9000),
            "guidance":[oversized,exact],"matches":[]}}}}));
    assert!(event.to_string().len() <= 8192);
    assert_eq!(event["details"]["evaluation"]["guidance"], json!([exact]));
    assert!(
        event["details"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("guidance"))
    );

    let event = bound_event(
        json!({"kind":"decision","provenance":{"cwd":"x".repeat(10000)},
        "details":{"evaluation":{"explanation":{"guidance":[exact],"matches":[]}}}}),
    );
    assert!(event.to_string().len() <= 8192);
    assert_eq!(event["details"]["evaluation"]["guidance"], json!([]));
    assert!(
        event["details"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("guidance"))
    );
}

#[test]
fn compacted_decision_matches_keep_override_provenance() {
    let provenance = json!({"profile":"recommended","source":"local","default_decision":"deny","before_override":"ask","final_decision":"allow","override_layer":"local"});
    let event = bound_event(
        json!({"kind":"decision","details":{"evaluation":{"explanation":{
            "padding":"x".repeat(9000),"matches":[{"source":"acme/tools","rule_id":"status","decision":"allow","applied":true,"decision_provenance":provenance}]
        }}}}),
    );
    assert!(event.to_string().len() <= 8192);
    assert_eq!(event["truncated"], true);
    assert_eq!(
        event["details"]["evaluation"]["matches"][0]["decision_provenance"],
        provenance
    );
}

#[test]
fn oversized_policy_diagnostics_keep_the_original_command_decision_and_fallback() {
    let (root, context) = enrolled();
    observations::decision(&context, json!({"command":{"kind":"shell","source":"echo hello world"},"evaluation":{"decision":"ask","explanation":{"packs":"x".repeat(100_000),"matches":[{"rule_id":"acme-rule","rule_ref":{"rule_id":"acme-rule","scope":"project"},"decision":"ask","applied":true}]}},"policy_revision":"acme-revision","disposition":"allowed","ask_fallback":"allow"})).unwrap();
    let event = Store::open(root.path())
        .unwrap()
        .read_history()
        .unwrap()
        .events
        .remove(0);
    assert!(event.to_string().len() <= 8192);
    assert_eq!(event["details"]["command"]["source"], "echo hello world");
    assert_eq!(event["details"]["evaluation"]["decision"], "ask");
    assert_eq!(event["details"]["policy_revision"], "acme-revision");
    assert_eq!(event["details"]["ask_fallback"], "allow");
    assert_eq!(
        event["details"]["evaluation"]["matches"][0]["rule_id"],
        "acme-rule"
    );
    assert_eq!(event["truncated"], true);
}

#[test]
fn semantic_record_checksums_still_accept_reformatted_json() {
    let event =
        json!({"kind":"completion","links":{"request_id":null},"details":{"delivery_id":"acme"}});
    let digest = blake3::hash(&serde_json::to_vec(&event).unwrap())
        .to_hex()
        .to_string();
    let reformatted = serde_json::to_string_pretty(&event)
        .unwrap()
        .replace('\n', "");
    let record = format!("{{\"event\":{reformatted},\"digest\":\"{digest}\"}}\n");
    assert_eq!(decode(record.as_bytes()).unwrap(), event);
}

#[test]
fn failed_optional_publication_marks_incomplete_history_without_changing_authority() {
    let (root, context) = enrolled();
    decision(&context, "first");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let before = fs::read(store.directory.join("state.json")).unwrap();
    // Force a bounded publication failure without depending on filesystem access
    // restrictions, which privileged CI users can bypass.
    assert!(
        store
            .observe(&context, |state| {
                state
                    .pending_history
                    .push(json!({"fault_injection":"x".repeat(LINE_LIMIT + 1)}));
                Ok(json!({}))
            })
            .is_err()
    );
    let loaded = store.read_history().unwrap();
    assert!(!loaded.history_available);
    assert_eq!(loaded.events.len(), 1);
    assert_eq!(
        fs::read(store.directory.join("state.json")).unwrap(),
        before
    );
    assert!(error_path(&store, &state).unwrap().exists());
}

#[test]
#[ignore = "owned subprocess invoked by crashed_writer_releases_os_lock_and_recovers_partial_append"]
fn journal_crash_process() {
    let root = PathBuf::from(std::env::var_os("SHIELD_HISTORY_CRASH_ROOT").unwrap());
    let store = Store::open(&root).unwrap();
    let _guard = lock(&store).unwrap();
    let state = store.read_checkpoint().unwrap();
    OpenOptions::new()
        .append(true)
        .open(active(&store, &state))
        .unwrap()
        .write_all(b"{\"event\":")
        .unwrap();
    fs::write(root.join("journal-writer-ready"), b"ready").unwrap();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn crashed_writer_releases_os_lock_and_recovers_partial_append() {
    let (root, context) = enrolled();
    decision(&context, "before-crash");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "review::storage::history::tests::journal_crash_process",
            "--ignored",
            "--nocapture",
        ])
        .env("SHIELD_HISTORY_CRASH_ROOT", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let began = Instant::now();
    while !root.path().join("journal-writer-ready").exists() {
        if began.elapsed() > Duration::from_secs(10) || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned journal writer failed to reach barrier");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let locked = observations::decision(&context, json!({}));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(matches!(locked, Err(Error::Locked)));
    decision(&context, "after-crash");
    let state = Store::open(root.path()).unwrap().read_history().unwrap();
    assert_eq!(state.events.len(), 2);
    assert!(!state.history_available);
}
