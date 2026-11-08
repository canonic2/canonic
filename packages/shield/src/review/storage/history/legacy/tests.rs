use super::*;
use crate::review::tests::enrolled;

fn observation() -> Value {
    json!({"schema_version":1,"event_id":"legacy-event","kind":"decision",
        "links":{"request_id":"legacy-request"},"details":{"decision":"allow"}})
}

fn write_private(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
}

#[test]
fn legacy_unreadable_implicit_journal_survives_conversion_and_cleanup() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read_checkpoint().unwrap();
    state.history_generation = format!("journal-{}", state.project_id);
    // Two active segments are inconsistent, but both remain recovery evidence.
    for sequence in 0..2 {
        write_segment(
            &store,
            &state,
            sequence,
            None,
            &line(&observation()).unwrap(),
        )
        .unwrap();
    }
    let before: Vec<_> = fs::read_dir(&store.directory)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(&format!("history-{}.", state.project_id))
                    && Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
            })
        })
        .map(|entry| (entry.path(), fs::read(entry.path()).unwrap()))
        .collect();
    let generation = state.history_generation.clone();
    let raw = json!({"history_generation":null,"events":[]});
    inspect(&store, &raw, &mut state);
    assert!(!state.history_available);
    upgrade(&store, &raw, &mut state).unwrap();
    assert_eq!(state.history_generation, generation);
    assert!(!state.history_available);
    assert!(error_path(&store, &state).unwrap().exists());
    let _guard = lock(&store).unwrap();
    super::super::cleanup(&store, &state);
    for (path, bytes) in before {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn legacy_inline_history_upgrade_is_idempotent_and_preserves_every_record() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read_checkpoint().unwrap();
    let events = json!([observation()]);
    let raw = json!({"events":events});
    upgrade(&store, &raw, &mut state).unwrap();
    assert!(state.history_available);
    let generation = state.history_generation.clone();
    let before = segments(&store, &state)
        .unwrap()
        .into_iter()
        .map(|segment| (segment.path.clone(), fs::read(segment.path).unwrap()))
        .collect::<Vec<_>>();
    let (decoded, evicted) = read(capture(&store, &state).unwrap(), &state).unwrap();
    assert_eq!(decoded, events.as_array().unwrap().clone());
    assert!(!evicted);
    upgrade(&store, &raw, &mut state).unwrap();
    assert_eq!(state.history_generation, generation);
    assert_eq!(raw["events"], events);
    for (path, bytes) in before {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn legacy_array_history_upgrade_checks_digest_and_keeps_original_bytes() {
    for corrupt in [false, true] {
        let (root, _) = enrolled();
        let store = Store::open(root.path()).unwrap();
        let mut state = store.read_checkpoint().unwrap();
        let bytes = serde_json::to_vec(&vec![observation()]).unwrap();
        let digest = crate::policy::fingerprint(&bytes);
        let path = store.directory.join(format!("history-{digest}.json"));
        let original = if corrupt { b"[]".to_vec() } else { bytes };
        write_private(&path, &original);
        let raw = json!({"history_generation":digest,"events":[]});
        upgrade(&store, &raw, &mut state).unwrap();
        assert_eq!(state.history_available, !corrupt);
        assert_eq!(fs::read(path).unwrap(), original);
        let view = capture(&store, &state).unwrap();
        assert_eq!(view.complete, !corrupt);
        if !corrupt {
            assert_eq!(read(view, &state).unwrap().0, [observation()]);
        }
    }
}

#[test]
fn legacy_missing_generation_keeps_existing_project_journal_namespace() {
    for generation in [None, Some(Value::Null)] {
        let (root, _) = enrolled();
        let store = Store::open(root.path()).unwrap();
        let mut state = store.read_checkpoint().unwrap();
        state.history_generation = format!("journal-{}", state.project_id);
        replace(&store, &state, &[observation()], false).unwrap();
        let path = segments(&store, &state).unwrap().remove(0).path;
        let bytes = fs::read(&path).unwrap();
        let mut raw = json!({"events":[]});
        if let Some(generation) = generation {
            raw["history_generation"] = generation;
        }
        upgrade(&store, &raw, &mut state).unwrap();
        assert_eq!(
            state.history_generation,
            format!("journal-{}", state.project_id)
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert!(state.history_available);
        assert_eq!(
            read(capture(&store, &state).unwrap(), &state).unwrap().0,
            [observation()]
        );
    }
}

#[test]
fn legacy_history_upgrade_retries_partial_preparation_and_reports_invalid_history() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let mut state = store.read_checkpoint().unwrap();
    let raw = json!({"history_generation":null,"events":[observation()]});
    upgrade(&store, &raw, &mut state).unwrap();
    let path = segments(&store, &state).unwrap().remove(0).path;
    fs::remove_file(path).unwrap();
    write_segment(&store, &state, 0, Some(1), &line(&observation()).unwrap()).unwrap();
    upgrade(&store, &raw, &mut state).unwrap();
    assert_eq!(
        read(capture(&store, &state).unwrap(), &state).unwrap().0,
        [observation()]
    );
    let invalid = json!({"history_generation":null,"events":[42]});
    upgrade(&store, &invalid, &mut state).unwrap();
    assert!(!state.history_available);
    assert!(!capture(&store, &state).unwrap().complete);
    assert_eq!(invalid["events"][0], 42);
}

#[test]
fn legacy_journal_with_inline_history_reports_degradation_without_rewriting_sources() {
    for explicit_generation in [false, true] {
        let (root, _) = enrolled();
        let store = Store::open(root.path()).unwrap();
        let mut state = store.read_checkpoint().unwrap();
        state.history_generation = format!("journal-{}", state.project_id);
        replace(&store, &state, &[observation()], false).unwrap();
        let path = segments(&store, &state).unwrap().remove(0).path;
        let bytes = fs::read(&path).unwrap();
        let mut inline = observation();
        inline["event_id"] = json!("inline-event");
        inline["links"]["request_id"] = json!("inline-request");
        let mut raw = json!({"events":[inline]});
        if explicit_generation {
            raw["history_generation"] = json!(state.history_generation);
        }
        let original = raw.clone();
        inspect(&store, &raw, &mut state);
        assert!(!state.history_available);
        assert!(!error_path(&store, &state).unwrap().exists());
        upgrade(&store, &raw, &mut state).unwrap();
        assert!(!state.history_available);
        assert!(!capture(&store, &state).unwrap().complete);
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(raw, original);
        assert_eq!(
            read(capture(&store, &state).unwrap(), &state).unwrap().0,
            [observation()]
        );
    }
}
