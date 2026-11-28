//! Behavioral fixtures for shared signing selection without local credentials.
use super::*;
use crate::artifacts;
use std::fs;

fn teams(ids: &[&str]) -> Teams {
    ids.iter()
        .map(|id| ((*id).into(), format!("Acme {id}")))
        .collect()
}

/// Replace certificate discovery with counted synthetic teams while using the
/// product's real inspection, selection and private settings transactions.
pub(crate) struct Fixture {
    store: storage::Store,
    teams: Teams,
    pub discoveries: std::cell::Cell<usize>,
}

impl Fixture {
    pub fn new(root: &std::path::Path, ids: &[&str]) -> Self {
        Self {
            store: storage::Store::at(root.join("Testrun/settings.json")),
            teams: teams(ids),
            discoveries: std::cell::Cell::new(0),
        }
    }
    pub fn inspect(&self) -> Result<Value> {
        self.discoveries.set(self.discoveries.get() + 1);
        inspect_store(&self.teams, &self.store)
    }
    pub fn setup(&self, requested: Option<&str>) -> Result<Selection> {
        self.discoveries.set(self.discoveries.get() + 1);
        self.store.resolve(&self.teams, requested)
    }
    pub fn connect(&self) -> Result<Value> {
        self.discoveries.set(self.discoveries.get() + 1);
        Ok(json!({"runnerSetup":{"signing":self.store.connection(&self.teams, None)?}}))
    }
    pub fn path(&self) -> &std::path::Path {
        &self.store.path
    }
}

#[test]
fn inspection_reports_current_inventory_and_saved_choice_without_selecting_or_rewriting() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), &["ACME000001"]);
    let initial = fixture.inspect().unwrap();
    assert!(initial["savedTeamId"].is_null());
    assert_eq!(initial["availableTeamIds"], json!(["ACME000001"]));
    assert_eq!(initial["availableTeams"]["ACME000001"], "Acme ACME000001");
    assert_eq!(initial["settingsPath"], json!(fixture.path()));
    assert!(!fixture.path().exists());
    assert_eq!(fixture.discoveries.get(), 1);
    fixture.setup(None).unwrap();
    let before = fs::read(fixture.path()).unwrap();
    // Inspection preserves even an unavailable saved team so setup can offer
    // a replacement rather than silently choosing another account.
    let changed_inventory = Fixture::new(root.path(), &["ACME000002"]);
    let report = changed_inventory.inspect().unwrap();
    assert_eq!(report["savedTeamId"], "ACME000001");
    assert_eq!(report["availableTeamIds"], json!(["ACME000002"]));
    assert_eq!(fs::read(fixture.path()).unwrap(), before);
    fs::write(fixture.path(), b"broken native settings").unwrap();
    assert_eq!(
        changed_inventory.inspect().unwrap_err().code,
        "SIGNING_PREFERENCE_INVALID"
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), b"broken native settings");
}

#[test]
fn native_configuration_path_uses_the_platform_directory() {
    let store = storage::Store::host().unwrap();
    #[cfg(target_os = "macos")]
    assert_eq!(
        store.path,
        dirs::home_dir()
            .unwrap()
            .join("Library/Application Support/Testrun/settings.json")
    );
    #[cfg(target_os = "windows")]
    assert_eq!(
        store.path,
        dirs::config_local_dir()
            .unwrap()
            .join("Testrun/settings.json")
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    assert_eq!(
        store.path,
        dirs::config_local_dir()
            .unwrap()
            .join("testrun/settings.json")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn configuration_path_fixture() {
    if let Some(expected) = std::env::var_os("TESTRUN_EXPECT_SETTINGS") {
        assert_eq!(
            storage::Store::host().unwrap().path,
            std::path::PathBuf::from(expected)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn xdg_paths_honor_absolute_overrides_and_ignore_empty_or_relative_values() {
    let root = tempfile::tempdir().unwrap();
    for (override_path, expected) in [
        (
            Some(root.path().join("config")),
            root.path().join("config/testrun/settings.json"),
        ),
        (
            Some(std::path::PathBuf::from("relative")),
            root.path().join(".config/testrun/settings.json"),
        ),
        (
            Some(std::path::PathBuf::new()),
            root.path().join(".config/testrun/settings.json"),
        ),
        (None, root.path().join(".config/testrun/settings.json")),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "host_setup::tests::configuration_path_fixture"])
            .env("HOME", root.path())
            .env("TESTRUN_EXPECT_SETTINGS", expected)
            .env_remove("XDG_CONFIG_HOME");
        if let Some(value) = override_path {
            command.env("XDG_CONFIG_HOME", value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn first_use_saves_a_unique_team_and_reuses_it_when_other_teams_appear() {
    let root = tempfile::tempdir().unwrap();
    let store = storage::Store::at(root.path().join("Testrun/settings.json"));
    let selection = store.resolve(&teams(&["ACME000001"]), None).unwrap();
    assert_eq!(selection.source, "automatic");
    assert_eq!(selection.runner_team_id, "ACME000001");
    // A different client instance observes the same default, without a cache.
    let other = storage::Store::at(store.path.clone());
    let selection = other
        .resolve(&teams(&["ACME000001", "ACME000002"]), None)
        .unwrap();
    assert_eq!(selection.runner_team_id, "ACME000001");
    assert_eq!(selection.source, "saved");
    let before = fs::read(&store.path).unwrap();
    assert_eq!(
        other
            .resolve(&teams(&["ACME000002"]), None)
            .unwrap_err()
            .code,
        "SIGNING_IDENTITY_REQUIRED"
    );
    assert_eq!(fs::read(&store.path).unwrap(), before);
    other
        .resolve(&teams(&["ACME000002"]), Some("ACME000002"))
        .unwrap();
    assert_eq!(store.saved().unwrap().as_deref(), Some("ACME000002"));
}

#[test]
fn keeping_the_saved_team_reports_saved_without_rewriting_settings() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Testrun");
    artifacts::directory(&directory).unwrap();
    let path = directory.join("settings.json");
    let original = b"{\n  \"ios\": {\"runnerTeamId\": \"ACME000001\"},\n  \"other\": true\n}\n";
    artifacts::write(&path, original).unwrap();
    #[cfg(unix)]
    let before = fs::metadata(&path).unwrap();
    let store = storage::Store::at(path.clone());
    let candidates = teams(&["ACME000001", "ACME000002"]);
    // Enter, the saved team's number and its ID all resolve to the same team.
    for requested in [None, Some("ACME000001")] {
        let selected = store.resolve(&candidates, requested).unwrap();
        assert_eq!(selected.runner_team_id, "ACME000001");
        assert_eq!(selected.source, "saved");
        assert_eq!(fs::read(&path).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&path).unwrap().ino(), before.ino());
        }
    }
    assert_eq!(
        store
            .resolve(&teams(&["ACME000002"]), Some("ACME000001"))
            .unwrap_err()
            .code,
        "SIGNING_IDENTITY_REQUIRED"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let changed = store.resolve(&candidates, Some("ACME000002")).unwrap();
    assert_eq!(changed.source, "setup");
    assert_eq!(store.saved().unwrap().as_deref(), Some("ACME000002"));
    let settings: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(settings["other"], true);
}

#[test]
fn ambiguous_or_unavailable_signing_never_saves_a_guess() {
    let root = tempfile::tempdir().unwrap();
    let store = storage::Store::at(root.path().join("Testrun/settings.json"));
    let candidates = teams(&["ACME000001", "ACME000002"]);
    let error = store.resolve(&candidates, None).unwrap_err();
    assert_eq!(error.code, "SIGNING_TEAM_REQUIRED");
    assert_eq!(
        error.details.unwrap()["availableTeamIds"],
        json!(["ACME000001", "ACME000002"])
    );
    assert!(!store.path.exists());
    assert_eq!(
        store.resolve(&Teams::new(), None).unwrap_err().code,
        "SIGNING_TEAM_REQUIRED"
    );
    assert_eq!(
        store
            .resolve(&candidates, Some("ACME000003"))
            .unwrap_err()
            .code,
        "SIGNING_IDENTITY_REQUIRED"
    );
    assert!(!store.path.exists());
    assert_eq!(
        choose(Some("ACME000002"), &candidates).unwrap(),
        "ACME000002"
    );
    assert_eq!(
        choose(Some("invalid"), &candidates).unwrap_err().code,
        "INVALID_ARGUMENT"
    );
}

#[test]
fn connection_overrides_never_read_or_change_the_default() {
    let root = tempfile::tempdir().unwrap();
    let store = storage::Store::at(root.path().join("Testrun/settings.json"));
    let candidates = teams(&["ACME000001", "ACME000002"]);
    let selected = store.connection(&candidates, Some("ACME000002")).unwrap();
    assert_eq!(selected.source, "override");
    assert_eq!(selected.runner_team_id, "ACME000002");
    assert!(!store.path.parent().unwrap().exists());
    store.resolve(&candidates, Some("ACME000001")).unwrap();
    let before = fs::read(&store.path).unwrap();
    store.connection(&candidates, Some("ACME000002")).unwrap();
    assert_eq!(fs::read(&store.path).unwrap(), before);
    assert_eq!(
        store
            .connection(&candidates, Some("ACME000003"))
            .unwrap_err()
            .code,
        "SIGNING_IDENTITY_REQUIRED"
    );
    fs::write(&store.path, b"invalid native settings").unwrap();
    store.connection(&candidates, Some("ACME000002")).unwrap();
    assert_eq!(fs::read(&store.path).unwrap(), b"invalid native settings");
}

#[test]
fn old_signing_files_do_not_affect_native_reads_or_default_selection() {
    for bytes in [b"broken".as_slice(), br#"{"runnerTeamId":"ACME000003"}"#] {
        let root = tempfile::tempdir().unwrap();
        let old_directory = root.path().join(".local/share/testrun/signing");
        fs::create_dir_all(old_directory.parent().unwrap()).unwrap();
        artifacts::directory(&old_directory).unwrap();
        let old_file = old_directory.join("preferred-team.json");
        artifacts::write(&old_file, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&old_file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let store = storage::Store::at(
            root.path()
                .join("Library/Application Support/Testrun/settings.json"),
        );
        assert!(store.saved().unwrap().is_none());
        // Both explicit setup and automatic first-use selection ignore the old
        // file. Clearing only the native file exercises each fresh default.
        store
            .resolve(&teams(&["ACME000001"]), Some("ACME000001"))
            .unwrap();
        assert_eq!(store.saved().unwrap().as_deref(), Some("ACME000001"));
        fs::remove_file(&store.path).unwrap();
        let selected = store.connection(&teams(&["ACME000002"]), None).unwrap();
        assert_eq!(selected.runner_team_id, "ACME000002");
        assert_eq!(selected.source, "automatic");
        assert_eq!(fs::read(&old_file).unwrap(), bytes);
    }
}

#[test]
fn invalid_native_settings_do_not_get_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Testrun");
    artifacts::directory(&directory).unwrap();
    let path = directory.join("settings.json");
    let store = storage::Store::at(path.clone());
    for bytes in [
        b"broken".as_slice(),
        br#"{"ios":null}"#,
        br#"{"ios":{"runnerTeamId":42}}"#,
        br#"{"ios":{"runnerTeamId":"invalid"}}"#,
        b"[]",
    ] {
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        artifacts::write(&path, bytes).unwrap();
        assert_eq!(
            store
                .resolve(&teams(&["ACME000001"]), Some("ACME000001"))
                .unwrap_err()
                .code,
            "SIGNING_PREFERENCE_INVALID"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn an_update_that_exceeds_the_read_limit_preserves_existing_settings() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Testrun");
    artifacts::directory(&directory).unwrap();
    let path = directory.join("settings.json");
    let original = json!({"other":"x".repeat(64 * 1024 - 20)}).to_string();
    assert!(original.len() < 64 * 1024);
    artifacts::write(&path, original.as_bytes()).unwrap();
    let store = storage::Store::at(path.clone());
    assert_eq!(
        store
            .resolve(&teams(&["ACME000001"]), None)
            .unwrap_err()
            .code,
        "SIGNING_PREFERENCE_INVALID"
    );
    assert_eq!(fs::read(&path).unwrap(), original.as_bytes());
    assert!(store.saved().unwrap().is_none());
}

#[test]
fn concurrent_first_use_and_explicit_changes_preserve_unrelated_settings() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Testrun");
    artifacts::directory(&directory).unwrap();
    let path = directory.join("settings.json");
    artifacts::write(
        &path,
        br#"{"other":{"enabled":true},"ios":{"other":"retained"}}"#,
    )
    .unwrap();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let barrier = &barrier;
            let path = &path;
            scope.spawn(move || {
                let store = storage::Store::at(path.clone());
                barrier.wait();
                store.resolve(&teams(&["ACME000001"]), None).unwrap();
                barrier.wait();
                store
                    .resolve(&teams(&["ACME000001", "ACME000002"]), Some("ACME000002"))
                    .unwrap();
            });
        }
    });
    let settings: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(settings["other"]["enabled"], true);
    assert_eq!(settings["ios"]["other"], "retained");
    assert_eq!(settings["ios"]["runnerTeamId"], "ACME000002");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn public_settings_directory_returns_actionable_signing_error_without_changing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Acme Settings");
    artifacts::directory(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    let store = storage::Store::at(directory.join("settings.json"));
    for error in [
        store.saved().unwrap_err(),
        store
            .resolve(&teams(&["ACME000001"]), Some("ACME000001"))
            .unwrap_err(),
    ] {
        assert_eq!(error.code, "SIGNING_PREFERENCE_INVALID");
        assert!(error.message.contains(directory.to_str().unwrap()));
        let details = error.details.unwrap();
        assert_eq!(details["settingsDirectory"], json!(directory));
        assert_eq!(details["reason"], "permissions");
        assert_eq!(details["requiredMode"], "0700");
        assert_eq!(
            details["recoveryCommand"],
            json!({"program":"chmod","args":["700",directory]})
        );
        assert_eq!(details["nextAction"], "testrun signing setup");
    }
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn linked_settings_directory_does_not_suggest_changing_the_link_targets_permissions() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("Acme");
    artifacts::directory(&target).unwrap();
    let directory = root.path().join("Testrun");
    symlink(&target, &directory).unwrap();
    let store = storage::Store::at(directory.join("settings.json"));
    let error = store.saved().unwrap_err();
    assert_eq!(error.code, "SIGNING_PREFERENCE_INVALID");
    let details = error.details.unwrap();
    assert_eq!(details["settingsDirectory"], json!(directory));
    assert_eq!(details["reason"], "not_directory");
    assert!(details.get("recoveryCommand").is_none());
    assert!(
        details["manualAction"]
            .as_str()
            .unwrap()
            .contains("symlink")
    );
}

#[cfg(unix)]
#[test]
fn linked_or_public_settings_and_locks_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Testrun");
    artifacts::directory(&directory).unwrap();
    let path = directory.join("settings.json");
    let foreign = root.path().join("foreign.json");
    artifacts::write(&foreign, br#"{"ios":{"runnerTeamId":"ACME000001"}}"#).unwrap();
    let store = storage::Store::at(path.clone());
    symlink(&foreign, &path).unwrap();
    assert!(store.saved().is_err());
    fs::remove_file(&path).unwrap();
    artifacts::write(&path, br#"{"ios":{"runnerTeamId":"ACME000001"}}"#).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.saved().is_err());
    fs::remove_file(directory.join("settings.lock")).unwrap();
    symlink(&foreign, directory.join("settings.lock")).unwrap();
    assert!(store.saved().is_err());
}
