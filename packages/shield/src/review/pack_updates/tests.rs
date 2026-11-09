//! Cached availability contracts through real private storage and checked metadata.
use super::*;
use crate::review::{self, storage::Store};
use std::{fs, time::Instant};

fn fixture() -> (assert_fs::TempDir, Context, State, Source) {
    let (root, context) = review::tests::enrolled();
    let source = Source::local(root.path()).unwrap();
    let mut state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    state.operator["sources"]["acme"] = json!(source);
    state.operator["installed"] = json!([{
        "selector":"pack:acme-node","id":"acme/node","version":"1.0.0",
        "source":source,"source_identity":source.identity(),"source_alias":"acme",
        "pinned":false,"direct":true,"digest":"a".repeat(64)
    }]);
    (root, context, state, source)
}

fn catalog(source: &Source, sequence: u64, releases: &[(&str, &str)]) -> Catalog {
    let entries: Vec<_> = releases.iter().map(|(id, version)| json!({
        "id":id,"version":version,"path":format!("packs/{}-{}.yml", id.replace('/',"-"), version.replace('+',"-")),"sha256":"b".repeat(64)
    })).collect();
    Catalog::verify(
        source,
        json!({"schema_version":1,"sequence":sequence,"packs":entries})
            .to_string()
            .as_bytes(),
        None,
        std::time::SystemTime::now(),
    )
    .unwrap()
}

#[test]
fn pack_updates_truncation_preserves_fresh_notices_after_eight_stale_candidates() {
    let (_root, context, mut state, first) = fixture();
    let other_root = assert_fs::TempDir::new().unwrap();
    let second = Source::local(other_root.path()).unwrap();
    let (expired_source, fresh) = if first.identity() < second.identity() {
        (first, second)
    } else {
        (second, first)
    };
    let now = DAY_MS * 10;
    let ids: Vec<_> = (0..8).map(|index| format!("acme/stale-{index}")).collect();
    let releases: Vec<_> = ids.iter().map(|id| (id.as_str(), "2.0.0")).collect();
    retain_at(
        &context,
        &expired_source,
        &catalog(&expired_source, 1, &releases),
        now - DAY_MS,
    )
    .unwrap();
    retain_at(
        &context,
        &fresh,
        &catalog(&fresh, 1, &[("acme/fresh", "2.0.0")]),
        now,
    )
    .unwrap();
    state.operator["sources"] = json!({"stale":expired_source,"fresh":fresh});
    let mut installed: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({
                "selector":format!("pack:{id}"),"id":id,"version":"1.0.0",
                "source":expired_source,"source_identity":expired_source.identity(),"source_alias":"stale",
                "pinned":false,"direct":true,"digest":"a".repeat(64)
            })
        })
        .collect();
    installed.push(json!({
        "selector":"pack:acme-fresh","id":"acme/fresh","version":"1.0.0",
        "source":fresh,"source_identity":fresh.identity(),"source_alias":"fresh",
        "pinned":false,"direct":true,"digest":"a".repeat(64)
    }));
    state.operator["installed"] = json!(installed);

    let view = inspect_at(&context, &state, None, now);
    assert_eq!(view["total"], 9);
    assert_eq!(view["fresh_total"], 1);
    assert_eq!(view["availability"], "partial");
    assert_eq!(view["truncated"], true);
    let items = view["items"].as_array().unwrap();
    assert_eq!(items.len(), 8);
    assert!(items.iter().any(|row| row["id"] == "acme/fresh"));
    assert!(items.windows(2).all(|rows| {
        (rows[0]["source_identity"].as_str(), rows[0]["id"].as_str())
            <= (rows[1]["source_identity"].as_str(), rows[1]["id"].as_str())
    }));
    assert!(inspection_summary(&view).contains("fresh/acme/fresh 1.0.0 → 2.0.0"));
    let notice =
        pending(&context, "cli", &view).expect("fresh update remains eligible for a notice");
    assert_eq!(notice.value["items"][0]["id"], "acme/fresh");
    assert_eq!(notice.value["remaining"], 0);
}

#[test]
fn pack_updates_only_advertise_newer_stable_tracking_roots_and_keep_unknown_honest() {
    let (_root, context, mut state, source) = fixture();
    let now = DAY_MS * 10;
    assert_eq!(
        inspect_at(&context, &state, None, now)["availability"],
        "unknown"
    );
    let catalog = catalog(
        &source,
        1,
        &[
            ("acme/node", "1.0.0+new"),
            ("acme/node", "2.0.0-beta.1"),
            ("acme/node", "1.2.0"),
        ],
    );
    retain_at(&context, &source, &catalog, now).unwrap();
    let view = inspect_at(&context, &state, None, now);
    assert_eq!(view["availability"], "available", "{view}");
    assert_eq!(view["items"][0]["available_version"], "1.2.0");
    assert_eq!(view["items"][0]["compatibility"], "unchecked");
    assert_eq!(view["items"][0]["selector"], "pack:acme-node");
    assert_eq!(
        inspect_at(&context, &state, None, now + DAY_MS)["availability"],
        "unknown"
    );
    assert_eq!(
        inspect_at(&context, &state, None, now - 1)["availability"],
        "unknown"
    );
    state.operator["installed"][0]["pinned"] = json!(true);
    let pinned = inspect_at(&context, &state, None, now);
    assert_eq!(pinned["excluded_pinned"], 1);
    assert_eq!(pinned["items"], json!([]));
    state.operator["installed"][0]["direct"] = json!(false);
    assert_eq!(
        inspect_at(&context, &state, None, now)["excluded_dependencies"],
        1
    );
    state.operator["installed"][0]["direct"] = json!(true);
    state.operator["installed"][0]["pinned"] = json!(false);
    state.operator["installed"][0]["version"] = json!("1.2.0+other");
    assert_eq!(
        inspect_at(&context, &state, None, now)["availability"],
        "none"
    );
}

#[test]
fn pack_updates_preserve_source_binding_checkpoint_and_expiry() {
    let (_root, context, mut state, source) = fixture();
    let now = DAY_MS * 10;
    let first = catalog(&source, 2, &[("acme/node", "2.0.0")]);
    retain_at(&context, &source, &first, now).unwrap();
    let old = catalog(&source, 1, &[("acme/node", "3.0.0")]);
    retain_at(&context, &source, &old, now + 1).unwrap();
    assert_eq!(
        inspect_at(&context, &state, None, now + 1)["items"][0]["available_version"],
        "2.0.0"
    );
    let mut snapshot = cache::load(&context, &source).unwrap();
    snapshot.expires_at = Some(timestamp::format_utc(now / 1000 + 1));
    cache::retain(&context, &snapshot).unwrap();
    assert_eq!(
        inspect_at(&context, &state, None, now + 1000)["items"][0]["freshness"],
        "expired"
    );
    state.registry_checkpoints.insert(
        source.identity().into(),
        json!({"sequence":3,"digest":"f".repeat(64),"releases":{}}),
    );
    assert_eq!(
        inspect_at(&context, &state, None, now)["availability"],
        "unknown"
    );
    state.registry_checkpoints.clear();
    state.operator["sources"] = json!({});
    assert_eq!(
        inspect_at(&context, &state, None, now)["sources"][0]["reason"],
        "source_ineligible"
    );
}

#[test]
fn pack_updates_are_selected_bounded_read_only_and_independent_of_registry_liveness() {
    let (root, context, mut state, source) = fixture();
    let now = DAY_MS * 10;
    let catalog = catalog(&source, 1, &[("acme/node", "2.0.0")]);
    retain_at(&context, &source, &catalog, now).unwrap();
    let before = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    // No registry catalog or artifact exists. Cached inspection cannot fetch it.
    for id in 1..64 {
        let mut row = state.operator["installed"][0].clone();
        row["source_identity"] = json!(format!("missing-{id}"));
        row["selector"] = json!(format!("pack:missing-{id}"));
        state.operator["installed"]
            .as_array_mut()
            .unwrap()
            .push(row);
    }
    let started = Instant::now();
    let all = inspect_at(&context, &state, None, now);
    eprintln!(
        "pack_updates bounded cached inspection elapsed_us={}",
        started.elapsed().as_micros()
    );
    assert_eq!(all["availability"], "partial");
    assert!(crate::json::fits(&all, OUTPUT_BYTES));
    let selected = inspect_at(&context, &state, Some("pack:acme-node"), now);
    assert_eq!(selected["availability"], "available");
    assert_eq!(selected["sources"].as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        before
    );
    let directory = cache::directory(&context, "catalogs", false).unwrap();
    let held = crate::review::storage::objects::Directory::try_enter(&directory).unwrap();
    assert_eq!(
        inspect_at(&context, &state, Some("pack:acme-node"), now)["availability"],
        "available",
        "readers need no writer lock"
    );
    drop(held);
}

#[test]
fn pack_updates_corruption_links_and_unknown_schemas_do_not_claim_currency() {
    let (_root, context, state, source) = fixture();
    let now = DAY_MS * 10;
    retain_at(
        &context,
        &source,
        &catalog(&source, 1, &[("acme/node", "2.0.0")]),
        now,
    )
    .unwrap();
    let directory = cache::directory(&context, "catalogs", false).unwrap();
    let path = directory.join(format!(
        "{}.json",
        crate::policy::fingerprint(source.identity().as_bytes())
    ));
    let mut value = cache::read(&path, cache::FILE_BYTES).unwrap();
    value["schema_version"] = json!(99);
    cache::write(&path, &value, cache::FILE_BYTES).unwrap();
    assert_eq!(
        inspect_at(&context, &state, None, now)["availability"],
        "unknown"
    );
    fs::write(&path, b"invalid JSON").unwrap();
    assert_eq!(
        inspect_at(&context, &state, None, now)["availability"],
        "unknown"
    );
    #[cfg(unix)]
    {
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/dev/null", &path).unwrap();
        assert_eq!(
            inspect_at(&context, &state, None, now)["availability"],
            "unknown"
        );
    }
}
