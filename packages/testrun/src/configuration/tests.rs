//! Configuration contracts through strict YAML sources and effective profiles.
use super::*;
fn fixture() -> (tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let yaml="version: 1\nbuilds:\n  acme:\n    builder: custom\n    platform: android\n    target_kind: emulator\n    custom:\n      command: [make, apk]\n    artifact:\n      path: dist/acme.apk\n    env:\n      API_URL: https://staging.example.com\n      REMOVE: value\n".to_owned();
    std::fs::write(directory.path().join("testrun.yml"), &yaml).unwrap();
    (directory, yaml)
}
#[test]
fn local_maps_merge_lists_replace_and_env_null_removes() {
    let (directory, _) = fixture();
    std::fs::write(directory.path().join("testrun.local.yml"),"version: 1\nbuilds:\n  acme:\n    env:\n      REMOVE: null\n      LOCAL: personal\n    custom:\n      command: [make, local-apk]\n").unwrap();
    let loaded = load(directory.path()).unwrap();
    let p = &loaded.document.builds["acme"];
    assert!(!p.env.contains_key("REMOVE"));
    assert_eq!(p.env["LOCAL"], "personal");
    assert!(p.env.contains_key("API_URL"));
    assert_eq!(p.custom.as_ref().unwrap().command, ["make", "local-apk"]);
    assert_eq!(
        loaded.provenance["/builds/acme/env/LOCAL"],
        "testrun.local.yml"
    );
    let shown = inspect(directory.path(), None).unwrap().to_string();
    assert!(!shown.contains("personal"));
    assert!(!shown.contains("https://staging.example.com"));
}
#[test]
fn strict_yaml_refuses_duplicates_aliases_tags_and_documents() {
    let (directory, yaml) = fixture();
    for bad in [
        yaml.replace("version: 1", "version: 1\nversion: 1"),
        yaml.replace("version: 1", "version: 2"),
        format!("{yaml}\n---\nversion: 1\n"),
        yaml.replace("[make, apk]", "&args [make, apk]"),
        yaml.replace("[make, apk]", "!execute [make, apk]"),
        yaml.replace("builder: custom", "builder: custom\n    typo: true"),
    ] {
        std::fs::write(directory.path().join("testrun.yml"), bad).unwrap();
        assert!(load(directory.path()).is_err());
    }
}

#[test]
fn git_conflicts_name_the_selected_file_and_opening_line() {
    let (directory, yaml) = fixture();
    let base = directory.path().join("testrun.yml");
    let local = directory.path().join("testrun.local.yml");
    let conflicted = "version: 1\n<<<<<<< HEAD\nbuilds: {}\n=======\nbuilds: {}\n>>>>>>> topic\n";
    std::fs::write(&base, conflicted).unwrap();
    let error = inspect(directory.path(), None).unwrap_err();
    assert_eq!(error.code, "CONFIG_INVALID");
    assert_eq!(
        error.message,
        "Unresolved Git conflict markers. Resolve the conflict and retry."
    );
    let details = error.details.unwrap();
    assert_eq!(details["location"], "2:1");
    assert!(details["file"].as_str().unwrap().ends_with("testrun.yml"));

    std::fs::write(&base, yaml).unwrap();
    std::fs::write(&local, conflicted).unwrap();
    let overlay_error = inspect(directory.path(), None).unwrap_err();
    assert_eq!(overlay_error.message, error.message);
    let details = overlay_error.details.unwrap();
    assert_eq!(details["location"], "2:1");
    assert!(
        details["file"]
            .as_str()
            .unwrap()
            .ends_with("testrun.local.yml")
    );
    assert_eq!(std::fs::read_to_string(&local).unwrap(), conflicted);
    std::fs::write(local, "version: 1\nbuilds: {}\n").unwrap();
    assert!(inspect(directory.path(), None).is_ok());
}
#[test]
fn overlay_cannot_change_builder_and_invalid_base_is_not_repaired() {
    let (directory, yaml) = fixture();
    std::fs::write(
        directory.path().join("testrun.local.yml"),
        "version: 1\nbuilds:\n  acme:\n    target_kind: device\n",
    )
    .unwrap();
    assert!(load(directory.path()).is_err());
    std::fs::write(
        directory.path().join("testrun.local.yml"),
        "version: 1\nbuilds: {}\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("testrun.yml"),
        yaml.replace("command: [make, apk]", "command: []"),
    )
    .unwrap();
    assert!(load(directory.path()).is_err());
}
#[test]
fn initialize_preserves_existing_files_and_ignore_bytes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join(".gitignore"), "dist/").unwrap();
    init(directory.path(), None).unwrap();
    assert_eq!(
        std::fs::read_to_string(directory.path().join(".gitignore")).unwrap(),
        "dist/\n/testrun.local.yml\n"
    );
    assert!(init(directory.path(), None).is_err());
    assert!(load(directory.path()).unwrap().document.builds.is_empty());
}

#[test]
fn diagnostics_keep_file_and_pointer_without_exposing_value() {
    let (directory, yaml) = fixture();
    let bad = yaml.replace("command: [make, apk]", "command: false");
    std::fs::write(directory.path().join("testrun.yml"), bad).unwrap();
    let error = load(directory.path()).err().unwrap();
    let details = error.details.unwrap();
    assert_eq!(details["pointer"], "/builds/acme/custom/command");
    assert!(details["file"].as_str().unwrap().ends_with("testrun.yml"));
    std::fs::write(directory.path().join("testrun.yml"), yaml).unwrap();
    std::fs::write(
        directory.path().join("testrun.local.yml"),
        "version: 1\nbuilds:\n  acme:\n    inherit_env: [ACME_TOKEN, ACME_TOKEN]\n",
    )
    .unwrap();
    let details = load(directory.path()).err().unwrap().details.unwrap();
    assert_eq!(details["pointer"], "/builds/acme");
    assert!(
        details["file"]
            .as_str()
            .unwrap()
            .ends_with("testrun.local.yml")
    );
}

#[test]
fn diagnostic_build_declarations_are_strict_and_do_not_rewrite_builder_arguments() {
    let (directory, yaml) = fixture();
    let declared = "    diagnostics:\n      symbols: external\n      profileable: true\n      sanitizers: [address, undefined]\n      coverage: true\n      probes: [Acme-marker]\n      timing: instrumented\n";
    std::fs::write(
        directory.path().join("testrun.yml"),
        format!("{yaml}{declared}"),
    )
    .unwrap();
    let loaded = load(directory.path()).unwrap();
    let profile = &loaded.document.builds["acme"];
    assert_eq!(profile.custom.as_ref().unwrap().command, ["make", "apk"]);
    assert_eq!(
        profile.diagnostics.as_ref().unwrap().sanitizers,
        ["address", "undefined"]
    );
    std::fs::write(
        directory.path().join("testrun.local.yml"),
        "version: 1\nbuilds:\n  acme:\n    diagnostics:\n      timing: unknown\n",
    )
    .unwrap();
    let loaded = load(directory.path()).unwrap();
    let profile = &loaded.document.builds["acme"];
    assert_eq!(profile.diagnostics.as_ref().unwrap().timing, "unknown");
    assert_eq!(profile.diagnostics.as_ref().unwrap().symbols, "external");
    for bad in [
        declared.replace("[address, undefined]", "[address, address]"),
        declared.replace("symbols: external", "symbols: external\n      inject: true"),
    ] {
        std::fs::write(directory.path().join("testrun.yml"), format!("{yaml}{bad}")).unwrap();
        assert!(load(directory.path()).is_err());
    }
}

const V1: &str = "version: 1\nbuilds:\n  acme:\n    builder: custom\n    platform: android\n    target_kind: emulator\n    cwd: android\n    custom:\n      command: [make, apk]\n    artifact:\n      path: dist/acme.apk\n";
const V2: &str = "version: 2\nprojects:\n  mobile:\n    root: apps/mobile\n    kind: custom\n    profiles:\n      dev:\n        platform: android\n        target_kind: emulator\n";
fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
#[test]
fn grouped_location_keeps_root_paths_and_sibling_overlay_for_both_versions() {
    for text in [V1, V2] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        write(&root, ".config/testrun.yml", text);
        std::fs::create_dir_all(root.join("android")).unwrap();
        std::fs::create_dir_all(root.join("apps/mobile")).unwrap();
        // The root overlay belongs to the unselected location and is ignored.
        write(&root, "testrun.local.yml", "not: [valid");
        let local = if text == V1 {
            "version: 1\nbuilds:\n  acme:\n    env:\n      LOCAL: personal\n"
        } else {
            "version: 2\nprojects:\n  mobile:\n    profiles:\n      dev:\n        services: []\n"
        };
        write(&root, ".config/testrun.local.yml", local);
        let loaded = load(&root).unwrap();
        assert_eq!(loaded.root, root);
        assert_eq!(loaded.location.base, root.join(".config/testrun.yml"));
        assert_eq!(loaded.describe()["shadowed"], json!([]));
        assert_eq!(loaded.sources.as_array().unwrap().len(), 2);
        assert!(
            loaded
                .provenance
                .values()
                .any(|v| v == ".config/testrun.local.yml")
        );
        if text == V1 {
            let profile = &loaded.document.builds["acme"];
            assert_eq!(
                profile.directory(&loaded.root).unwrap(),
                root.join("android")
            );
        } else {
            let selected = projects::select_at(&root, None, "mobile").unwrap();
            assert_eq!(selected.root, root.join("apps/mobile"));
            assert_eq!(
                selected.show()["location"]["file"],
                json!(root.join(".config/testrun.yml"))
            );
        }
    }
}
#[test]
fn present_root_file_wins_even_when_invalid_and_never_falls_back() {
    for text in [V1, V2] {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), ".config/testrun.yml", text);
        write(root.path(), "testrun.yml", text);
        let loaded = load(root.path()).unwrap();
        assert!(loaded.location.base.ends_with("testrun.yml"));
        assert!(!loaded.location.base.ends_with(".config/testrun.yml"));
        assert_eq!(loaded.location.shadowed.len(), 1);
        assert!(
            inspect(root.path(), None).unwrap()["location"]["shadowed"][0]
                .as_str()
                .unwrap()
                .ends_with(".config/testrun.yml")
        );
        write(root.path(), "testrun.yml", "version: [");
        let error = load(root.path()).err().unwrap();
        assert_eq!(error.code, "CONFIG_INVALID");
        let file = error.details.unwrap()["file"].as_str().unwrap().to_owned();
        assert!(file.ends_with("testrun.yml") && !file.contains(".config"));
    }
}
#[test]
fn explicit_file_overrides_both_defaults_and_refuses_without_fallback() {
    for text in [V1, V2] {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        write(root.path(), "testrun.yml", text);
        write(root.path(), ".config/testrun.yml", text);
        write(root.path(), "testrun.local.yml", "not: [valid");
        let version = if text == V1 { 1 } else { 2 };
        write(
            other.path(),
            "acme-qa.yml",
            &format!("version: {version}\n"),
        );
        let explicit = other.path().join("acme-qa.yml");
        let loaded = load_at(root.path(), Some(&explicit)).unwrap();
        assert_eq!(loaded.location.base, explicit.canonicalize().unwrap());
        assert_eq!(loaded.root, root.path().canonicalize().unwrap());
        assert_eq!(loaded.location.shadowed.len(), 2);
        assert_eq!(loaded.sources.as_array().unwrap().len(), 1);
        // The overlay beside an explicit file is used even with another basename.
        write(other.path(), "testrun.local.yml", "version: 99\n");
        assert_eq!(
            load_at(root.path(), Some(&explicit)).err().unwrap().code,
            "CONFIG_INVALID"
        );
        std::fs::remove_file(other.path().join("testrun.local.yml")).unwrap();
        let missing = other.path().join("missing.yml");
        let error = load_at(root.path(), Some(&missing)).err().unwrap();
        assert_eq!(error.code, "CONFIG_IO");
        assert_eq!(error.details.unwrap()["file"], json!(missing));
        write(other.path(), "acme-qa.yml", "version: [");
        assert_eq!(
            load_at(root.path(), Some(&explicit)).err().unwrap().code,
            "CONFIG_INVALID"
        );
        assert!(load_at(root.path(), Some(Path::new("relative.yml"))).is_err());
        assert!(load_at(root.path(), Some(&root.path().join("testrun.local.yml"))).is_err());
    }
}
#[test]
fn selection_stays_in_the_exact_root_without_parent_discovery() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".config/testrun.yml", V1);
    std::fs::create_dir_all(root.path().join("child")).unwrap();
    let error = load(&root.path().join("child")).err().unwrap();
    assert_eq!(error.code, "CONFIG_IO");
    assert_eq!(
        error.details.unwrap()["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn initializer_writes_the_selected_location_and_ignores_its_actual_overlay() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".gitignore", "dist/\n");
    let grouped = root.path().join(".config/testrun.yml");
    let result = init(root.path(), Some(&grouped)).unwrap();
    assert_eq!(result["ignore"]["entry"], "/.config/testrun.local.yml");
    assert_eq!(
        std::fs::read_to_string(root.path().join(".gitignore")).unwrap(),
        "dist/\n/.config/testrun.local.yml\n"
    );
    assert!(!root.path().join("testrun.yml").exists());
    assert_eq!(
        load(root.path()).unwrap().location.base,
        grouped.canonicalize().unwrap()
    );
    // Either default location preserves its configuration.
    assert_eq!(init(root.path(), None).err().unwrap().code, "CONFIG_EXISTS");
    assert_eq!(
        init(root.path(), Some(&grouped)).err().unwrap().code,
        "CONFIG_EXISTS"
    );
    // An overlay outside the root is ignored beside itself.
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("qa/acme.yml");
    let result = init(root.path(), Some(&outside)).unwrap();
    assert!(
        result["ignore"]["file"]
            .as_str()
            .unwrap()
            .ends_with("qa/.gitignore")
    );
    assert_eq!(
        std::fs::read_to_string(other.path().join("qa/.gitignore")).unwrap(),
        "/testrun.local.yml\n"
    );
}
