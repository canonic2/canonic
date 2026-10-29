//! Deterministic owned-process crashes at the actual store publication boundaries.
use super::*;
use crate::review::{runtime, tests::enrolled};
use std::{
    cell::Cell,
    process::{Child, Command, Stdio},
    time::Instant,
};

#[test]
fn updates_append_missing_local_state_ignores_to_an_existing_file() {
    let (root, context) = enrolled();
    let path = root.path().join(".shield/.gitignore");
    fs::write(&path, "review/").unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |_| Ok(serde_json::Value::Null))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n"
    );
}

fn names(directory: &Path) -> Vec<String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect()
}

#[test]
fn a_busy_history_lock_without_observations_is_not_a_history_failure() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let guard = history::lock(&store).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
        .unwrap();
    drop(guard);
    let review = root.path().join(".shield/review");
    assert!(!names(&review).iter().any(|name| {
        Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("error"))
    }));
    assert!(store.read().unwrap().history_available);
}

#[cfg(unix)]
#[test]
fn writes_remove_leftover_temporaries_that_no_writer_owns() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let review = root.path().join(".shield/review");
    let sources = review.join("sources");
    fs::create_dir_all(&sources).unwrap();
    let dead = super::exited_pid();
    let digest = "0".repeat(64);
    // The writer lock serializes state writers, so every state leftover goes.
    let removed = [
        review.join(format!(".state-{dead}-1.tmp")),
        review.join(".state-1-1.tmp"),
        sources.join(format!("{digest}.{dead}-1.tmp")),
    ];
    let kept = [
        review.join(".state-acme.tmp"),
        review.join("acme.tmp"),
        sources.join(format!("{digest}.1-1.tmp")),
        sources.join(format!("{digest}.{}-1.tmp", std::process::id())),
        sources.join(format!("acme.{dead}-1.tmp")),
    ];
    for path in removed.iter().chain(&kept) {
        fs::write(path, b"partial").unwrap();
    }
    store
        .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
        .unwrap();
    sources::compact(
        &store,
        &mut json!({"kind":"candidate-bundle","config":"acme"}),
    )
    .unwrap();
    for path in &removed {
        assert!(!path.exists(), "{}", path.display());
    }
    for path in &kept {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn incomplete_state_and_proposal_records_are_refused_without_mutation() {
    let (root, context) = enrolled();
    crate::review::tests::propose(&context, &crate::review::tests::input("read"), &|| Ok(()))
        .unwrap();
    let store = Store::open(root.path()).unwrap();
    let path = root.path().join(".shield/review/state.json");
    let original = fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for (proposal, fields) in [
        (
            false,
            &[
                "operator",
                "registry_checkpoints",
                "read_requests",
                "run_requests",
                "history_generation",
                "active",
            ][..],
        ),
        (
            true,
            &[
                "base_candidate",
                "base_metadata",
                "candidate_metadata",
                "receipt",
                "operator",
            ][..],
        ),
    ] {
        for &field in fields {
            let mut incomplete = value.clone();
            let record = if proposal {
                &mut incomplete["proposals"][0]
            } else {
                &mut incomplete
            };
            record.as_object_mut().unwrap().remove(field);
            let bytes = serde_json::to_vec(&incomplete).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(store.read().is_err(), "missing {field}");
            assert_eq!(fs::read(&path).unwrap(), bytes, "missing {field}");
        }
    }
    fs::write(&path, &original).unwrap();
    assert!(store.read().is_ok());
    let mut missing_generation = value.clone();
    missing_generation["history_generation"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<State>(missing_generation).is_err());
    for field in [
        "base_candidate",
        "candidate",
        "base_metadata",
        "candidate_metadata",
    ] {
        let mut incomplete = value.clone();
        incomplete["proposals"][0][field] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&incomplete).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(store.read().is_err(), "null proposal {field}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "null proposal {field}");
    }
    let mut null_operator = value;
    null_operator["operator"] = serde_json::Value::Null;
    let bytes = serde_json::to_vec(&null_operator).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(store.read().is_err(), "null operator metadata");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, &original).unwrap();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Created,
    Written,
    Synced,
    Renamed,
    Complete,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Written => "written",
            Self::Synced => "synced",
            Self::Renamed => "renamed",
            Self::Complete => "complete",
        }
    }
}
thread_local! {
    static OBSERVER: Cell<Option<fn(Stage)>> = const { Cell::new(None) };
}
pub(super) fn observe(stage: Stage) {
    if let Some(observer) = OBSERVER.with(Cell::get) {
        observer(stage);
    }
}

fn barrier(stage: Stage) {
    if std::env::var("SHIELD_TEST_PUBLICATION_STAGE").unwrap() == stage.name() {
        let root = std::env::var("SHIELD_TEST_PUBLICATION_ROOT").unwrap();
        fs::write(Path::new(&root).join("ready"), stage.name()).unwrap();
        loop {
            thread::park();
        }
    }
}

#[test]
#[ignore = "subprocess fixture invoked explicitly by publication_crashes_keep_complete_authorized_snapshots"]
fn crash_writer_process() {
    let root = std::env::var("SHIELD_TEST_PUBLICATION_ROOT").unwrap();
    let context = crate::review::open(Path::new(&root)).unwrap();
    #[cfg(target_os = "macos")]
    if std::env::var_os("SHIELD_TEST_CONSUMPTION").is_some() {
        crash_consumption(&context);
        return;
    }
    let status = runtime::status(&context).unwrap();
    OBSERVER.with(|slot| slot.set(Some(barrier)));
    runtime::publish_fixture(
        &context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        status["active"]["activation_id"].as_str(),
        &|| Ok(()),
    )
    .unwrap();
    panic!("writer did not reach the requested publication barrier");
}

#[cfg(target_os = "macos")]
fn crash_consumption(operator: &Context) {
    let row = json!(
        Store::open(Path::new(&operator.project_root))
            .unwrap()
            .read()
            .unwrap()
            .run_requests
    )[0]
    .clone();
    let mut context = operator.clone();
    context.surface = "run".into();
    let request = serde_json::from_value(row["request"].clone()).unwrap();
    let evidence = crate::runtime::Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate(request)
        .unwrap();
    OBSERVER.with(|slot| slot.set(Some(barrier)));
    crate::review::run_approval::gate(
        &context,
        &evidence,
        &row["resource"],
        row["request_id"].as_str(),
        || true,
        |decision| {
            assert_eq!(decision["allowed"], true);
            fs::write(Path::new(&context.project_root).join("launched"), b"effect").unwrap();
        },
    )
    .unwrap();
    panic!("consumption did not reach publication barrier");
}

#[cfg(target_os = "macos")]
#[test]
fn publication_crashes_never_launch_before_durable_consumption() {
    for stage in [
        Stage::Created,
        Stage::Written,
        Stage::Synced,
        Stage::Renamed,
        Stage::Complete,
    ] {
        let (root, operator) = enrolled();
        let status = runtime::status(&operator).unwrap();
        runtime::publish_fixture(
            &operator,
            status["review"]["policy_revision"].as_str().unwrap(),
            status["review"]["candidate_digest"].as_str().unwrap(),
            None,
            &|| Ok(()),
        )
        .unwrap();
        let mut context = operator.clone();
        context.surface = "run".into();
        // Match the child harness's intentionally absent origin attribution.
        context.agent = None;
        context.session_id = None;
        let request = crate::policy::Request {
            executable: "/bin/echo".into(),
            arguments: vec!["Acme".into()],
            cwd: context.project_root.clone(),
            project: context.project_root.clone(),
        };
        let resource = crate::execution::approval_fixture(&request, 5000).unwrap();
        let evidence = crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .evaluate(request)
            .unwrap();
        let decision = crate::review::run_approval::gate(
            &context,
            &evidence,
            &resource,
            None,
            || true,
            |_| (),
        )
        .unwrap()
        .0;
        let id = decision["approval"]["request_id"].as_str().unwrap();
        crate::review::run_approval::authorize(&operator, id, 300, None, false).unwrap();
        let mut child = consumption_writer_at(root.path(), stage);
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!root.path().join("launched").exists());
        let store = Store::open(root.path()).unwrap();
        let state = store.read().unwrap();
        let committed = matches!(stage, Stage::Renamed | Stage::Complete);
        assert_eq!(
            json!(state.run_requests)[0]["status"],
            if committed { "consumed" } else { "approved" }
        );
        assert_eq!(
            state
                .authorization_events
                .iter()
                .filter(|e| e["kind"] == "run_grant_consumed")
                .count(),
            usize::from(committed)
        );
        // Only recover the stale lock after this owned writer has been reaped.
        fs::remove_file(root.path().join(".shield/review/state.lock")).unwrap();
        let retry = crate::review::run_approval::gate(
            &context,
            &evidence,
            &resource,
            Some(id),
            || true,
            |_| (),
        )
        .unwrap()
        .0;
        assert_eq!(retry["allowed"], !committed);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(target_os = "macos")]
fn consumption_writer_at(root: &Path, stage: Stage) -> OwnedChild {
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "review::storage::tests::crash_writer_process",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIELD_TEST_PUBLICATION_STAGE", stage.name())
            .env("SHIELD_TEST_PUBLICATION_ROOT", root)
            .env("SHIELD_TEST_CONSUMPTION", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(root.join("ready")).ok().as_deref() != Some(stage.name()) {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before {stage:?}"
        );
        assert!(Instant::now() < deadline, "barrier deadline at {stage:?}");
        thread::sleep(Duration::from_millis(10));
    }
    child
}

#[test]
fn publication_crashes_keep_complete_authorized_snapshots() {
    for stage in [
        Stage::Created,
        Stage::Written,
        Stage::Synced,
        Stage::Renamed,
        Stage::Complete,
    ] {
        let (root, context) = enrolled();
        let status = runtime::status(&context).unwrap();
        let revision = status["review"]["policy_revision"].as_str().unwrap();
        let digest = status["review"]["candidate_digest"].as_str().unwrap();
        let initial =
            runtime::publish_fixture(&context, revision, digest, None, &|| Ok(())).unwrap();
        let previous = initial["activation_id"].as_str().unwrap();
        let state_path = root.path().join(".shield/review/state.json");
        let before = fs::read(&state_path).unwrap();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "review::storage::tests::crash_writer_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SHIELD_TEST_PUBLICATION_STAGE", stage.name())
                .env("SHIELD_TEST_PUBLICATION_ROOT", root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(root.path().join("ready"))
                .ok()
                .as_deref()
                == Some(stage.name())
            {
                break;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "writer exited before {stage:?}"
            );
            assert!(Instant::now() < deadline, "writer did not reach {stage:?}");
            thread::sleep(Duration::from_millis(10));
        }
        let visible = runtime::status(&context).unwrap();
        let committed = matches!(stage, Stage::Renamed | Stage::Complete);
        assert_eq!(
            visible["active"] == initial,
            !committed,
            "visibility at {stage:?}"
        );
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        assert_eq!(runtime::status(&context).unwrap(), visible);
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.read().unwrap().authorization_events.len(), 1);
        if !committed {
            assert_eq!(fs::read(&state_path).unwrap(), before);
        }
        assert!(matches!(
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(())),
            Err(Error::Locked)
        ));
        // The owned writer was killed and reaped, so manual lock recovery is established.
        fs::remove_file(root.path().join(".shield/review/state.lock")).unwrap();
        let recovered =
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(()))
                .unwrap();
        if committed {
            assert_eq!(recovered, visible["active"]);
        }
        assert!(
            !names(&root.path().join(".shield/review"))
                .iter()
                .any(|name| name.starts_with(".state-")),
            "killed writer's temporary remains after {stage:?}"
        );
        let state = store.read().unwrap();
        assert_eq!(state.authorization_events.len(), 1);
        assert_eq!(
            state.authorization_events[0]["links"]["activation_id"],
            recovered["activation_id"]
        );
        assert_eq!(
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(()))
                .unwrap(),
            recovered
        );
        assert_eq!(store.read().unwrap().authorization_events.len(), 1);
    }
}
