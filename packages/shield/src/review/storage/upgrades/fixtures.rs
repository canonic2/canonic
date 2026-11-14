//! The one conversion over real checkpoints shipped releases wrote, through
//! the normal entry points. `tests/fixtures/checkpoints/<version>/shield` is a
//! project's `.shield` folder as that release left it.
use super::*;
use crate::review::{self, storage::Store};
use std::path::Path;

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A project holding `version`'s fixture, with private review state as the
/// release kept it.
fn install(version: &str) -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/checkpoints")
        .join(version)
        .join("shield");
    let shield = root.path().join(".shield");
    copy(&fixture, &shield);
    fs::write(
        shield.join(".gitignore"),
        "config.local.yml\nrules.local.yml\nshield.local.lock\nreview/\ncache/\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let review = shield.join("review");
        fs::set_permissions(&review, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(review.join("state.json"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    root
}

/// The converted checkpoint, which Shield keeps in `.shield/state` once it
/// has opened the folder.
fn state_json(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join(".shield/state/state.json")).unwrap()).unwrap()
}

/// How many conversion events the folder's history journal holds, after
/// Shield has opened it.
pub(super) fn conversions(root: &Path) -> usize {
    fs::read_dir(root.join(".shield/state"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("history-") && name.ends_with(".jsonl")
                })
        })
        .map(|path| {
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .filter(|line| line.contains("\"checkpoint_converted\""))
                .count()
        })
        .sum()
}

fn local(root: &Path) -> Value {
    crate::policy::parse(&fs::read_to_string(root.join(".shield/config.local.yml")).unwrap())
        .unwrap()
}

#[test]
fn a_0_29_0_folder_converts_on_its_first_hook_and_keeps_its_records() {
    let root = install("0.29.0");
    let original = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let history: Vec<_> = fs::read_dir(root.path().join(".shield/review"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("history-"))
        .collect();
    // Status inspects without writing.
    let context = review::open_diagnostics(root.path()).unwrap();
    let status = review::project::status(&context).unwrap();
    assert_eq!(status["checkpoint_upgrade_pending"], true, "{status}");
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        original
    );
    // The first hook converts, and its own decision then follows the files.
    let (_, paused) = review::open_native(root.path()).unwrap();
    assert!(
        paused.is_some(),
        "the release's paused mode stays in effect"
    );
    // The earlier `.shield/review` folder is now `.shield/state`.
    assert!(!root.path().join(".shield/review").exists());
    let saved = state_json(root.path());
    for key in [
        "active",
        "candidates",
        "policy_revision",
        "followed_files",
        "configuration_changes",
        "enforcement",
        "history_enabled",
        "history_limit",
    ] {
        assert!(saved.get(key).is_none(), "{key}: {saved}");
    }
    for key in [
        "path_access",
        "sources",
        "default",
        "policy_consent_key",
        "policy_lineage",
        "portable_policy",
    ] {
        assert!(
            saved["operator"].get(key).is_none_or(Value::is_null)
                || saved["operator"][key] == json!({}),
            "{key}: {saved}"
        );
    }
    // Settings moved into the local layer.
    let local = local(root.path());
    assert_eq!(local["enforcement"], "paused");
    assert_eq!(local["protection"]["agent_settings_read"], "allow");
    assert_eq!(local["history"]["limit"], 300);
    assert!(
        local["registries"]["sources"]["fixture"].is_object(),
        "{local}"
    );
    // History, the applied proposal and the recovery copy remain.
    for name in history {
        assert!(root.path().join(".shield/state").join(name).exists());
    }
    let digest = crate::policy::fingerprint(&original);
    assert_eq!(
        fs::read(
            root.path()
                .join(format!(".shield/state/upgrade-{digest}.json"))
        )
        .unwrap(),
        original
    );
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let statuses: Vec<_> = state.proposals.iter().map(|p| p.status.as_str()).collect();
    assert!(statuses.contains(&"applied"), "{statuses:?}");
    assert!(!statuses.contains(&"pending"), "{statuses:?}");
    // The files are the approved copy, and the change log is gone.
    assert_eq!(
        saved["operator"]["upgrade_notice"]["policy"]["compared"],
        true
    );
    assert_eq!(saved["operator"]["upgrade_notice"]["policy"]["same"], true);
    assert_eq!(conversions(root.path()), 1);
    assert!(
        !root
            .path()
            .join(".shield/state/configuration-changes")
            .exists()
    );
    let context = review::open_diagnostics(root.path()).unwrap();
    let status = review::project::status(&context).unwrap();
    assert_eq!(status["checkpoint_upgrade_pending"], false, "{status}");
    assert_eq!(status["upgrade"]["policy"]["same"], true, "{status}");
}

#[test]
fn a_0_29_0_folder_names_the_rules_and_packs_its_files_changed() {
    let root = install("0.29.0");
    let rules = root.path().join(".shield/rules.yml");
    let edited = fs::read_to_string(&rules)
        .unwrap()
        .replace("id: acme-applied", "id: acme-edited");
    fs::write(&rules, edited).unwrap();
    Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let notice = &state_json(root.path())["operator"]["upgrade_notice"]["policy"];
    assert_eq!(notice["same"], false, "{notice}");
    assert_eq!(
        notice["rules"],
        json!([{"name":"acme-applied (project)","change":"removed"},
            {"name":"acme-edited (project)","change":"added"}])
    );
    assert_eq!(notice["packs"], json!([]));
}

/// The history segments a release's unopened folder holds, with their bytes.
fn history(root: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
    let mut segments: Vec<_> = fs::read_dir(root.join(".shield/review"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("history-"))
        .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
        .collect();
    segments.sort();
    segments
}

/// Releases before 0.29.0 kept settings and a policy copy in review state;
/// each converts on first access, skipping the releases between.
#[test]
fn folders_from_earlier_releases_convert_and_enforce_their_files() {
    // 0.10.0, the first published release, could not pause enforcement.
    for (version, paused_by_release) in [("0.10.0", false), ("0.27.1", true), ("0.28.0", true)] {
        let root = install(version);
        let original = fs::read(root.path().join(".shield/review/state.json")).unwrap();
        let earlier = history(root.path());
        assert!(
            !earlier.is_empty(),
            "{version}: the release recorded history"
        );
        let context = review::open_diagnostics(root.path()).unwrap();
        let status = review::project::status(&context).unwrap();
        assert_eq!(
            status["checkpoint_upgrade_pending"], true,
            "{version}: {status}"
        );
        let (_, paused) = review::open_native(root.path()).unwrap();
        assert_eq!(
            paused.is_some(),
            paused_by_release,
            "{version}: enforcement mode kept"
        );
        let saved = state_json(root.path());
        for key in [
            "active",
            "candidates",
            "policy_revision",
            "enforcement",
            "history_limit",
        ] {
            assert!(saved.get(key).is_none(), "{version} {key}: {saved}");
        }
        for key in ["path_access", "policy_consent_key", "portable_policy"] {
            assert!(
                saved["operator"].get(key).is_none_or(Value::is_null),
                "{version} {key}: {saved}"
            );
        }
        let local = local(root.path());
        if paused_by_release {
            assert_eq!(local["enforcement"], "paused", "{version}");
        } else {
            assert!(local.get("enforcement").is_none(), "{version}: {local}");
        }
        assert_eq!(
            local["protection"]["agent_settings_read"], "allow",
            "{version}"
        );
        assert_eq!(local["history"]["limit"], 300, "{version}");
        // Earlier records stay byte for byte; the conversion adds one event.
        for (name, bytes) in &earlier {
            let now = fs::read(root.path().join(".shield/state").join(name)).unwrap();
            assert!(now.starts_with(bytes), "{version}: {name:?} kept");
        }
        assert_eq!(conversions(root.path()), 1, "{version}");
        let digest = crate::policy::fingerprint(&original);
        assert_eq!(
            fs::read(
                root.path()
                    .join(format!(".shield/state/upgrade-{digest}.json"))
            )
            .unwrap(),
            original,
            "{version}"
        );
        assert_eq!(
            saved["operator"]["upgrade_notice"]["policy"]["same"], true,
            "{version}"
        );
        // Repeated access neither converts again nor rewrites the state.
        let converted = fs::read(root.path().join(".shield/state/state.json")).unwrap();
        Store::open(root.path()).unwrap().read_checkpoint().unwrap();
        assert_eq!(conversions(root.path()), 1, "{version}");
        assert_eq!(
            fs::read(root.path().join(".shield/state/state.json")).unwrap(),
            converted
        );
        // The files decide requests, and an edit to them decides the next one.
        let decide = |argument: &str| {
            let snapshot = crate::runtime::Project::open(root.path())
                .unwrap()
                .snapshot()
                .unwrap();
            let folder = snapshot
                .identity()
                .project_root()
                .to_string_lossy()
                .into_owned();
            let request = crate::policy::Request {
                executable: "acme".into(),
                arguments: vec![argument.into()],
                cwd: folder.clone(),
                project: folder,
            };
            snapshot.evaluate(request).unwrap().evaluation().decision
        };
        assert_eq!(decide("read"), crate::policy::Decision::Allow, "{version}");
        assert_eq!(decide("remove"), crate::policy::Decision::Deny, "{version}");
        let rules = root.path().join(".shield/rules.yml");
        let edited = fs::read_to_string(&rules)
            .unwrap()
            .replace("decision: allow", "decision: deny");
        fs::write(&rules, edited).unwrap();
        assert_eq!(decide("read"), crate::policy::Decision::Deny, "{version}");
    }
}

#[test]
fn concurrent_first_accesses_convert_once() {
    let root = install("0.29.0");
    let path = root.path().to_path_buf();
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || Store::open(&path).unwrap().read_checkpoint().unwrap())
        })
        .collect();
    for reader in readers {
        reader.join().unwrap();
    }
    assert_eq!(conversions(root.path()), 1);
    assert_eq!(
        state_json(root.path())["operator"]["upgrade_recovery"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[cfg(unix)]
#[test]
fn settings_the_files_cannot_take_yet_keep_applying_and_convert_later() {
    use std::os::unix::fs::PermissionsExt;
    let root = install("0.29.0");
    let shield = root.path().join(".shield");
    let original = fs::read(shield.join("review/state.json")).unwrap();
    // The local layer cannot be created: the conversion writes nothing.
    fs::set_permissions(&shield, fs::Permissions::from_mode(0o500)).unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint();
    fs::set_permissions(&shield, fs::Permissions::from_mode(0o700)).unwrap();
    let state = state.unwrap();
    assert!(!state.settings.enabled(), "earlier settings keep applying");
    assert_eq!(state.settings.history_limit, 300);
    // Nor can the earlier `review` folder be renamed, so it keeps serving.
    assert_eq!(
        fs::read(shield.join("review/state.json")).unwrap(),
        original
    );
    // Once the folder is writable, the next access converts.
    Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(local(root.path())["enforcement"], "paused");
    assert!(state_json(root.path()).get("enforcement").is_none());
}

#[test]
fn a_future_checkpoint_keeps_its_bytes_and_names_the_formats() {
    let root = install("0.29.0");
    let path = root.path().join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["schema_version"] = json!(2);
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(&path, &bytes).unwrap();
    let error = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .err()
        .expect("future format refused");
    assert!(matches!(error, Error::StateFormat { .. }), "{error}");
    // Opening moved the folder to its current name; the bytes are unchanged.
    let path = root.path().join(".shield/state/state.json");
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
