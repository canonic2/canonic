//! Configuration selection, state location and launch arguments against real
//! folders. Test builds bind `--config` per thread, so each test's binding is
//! its own under nextest's processes and under a shared-process runner alike.
use super::*;
use assert_fs::{TempDir, prelude::*};

fn project() -> (TempDir, PathBuf) {
    let root = TempDir::new().unwrap();
    let path = root.path().canonicalize().unwrap();
    (root, path)
}

#[test]
fn the_root_default_wins_over_the_grouped_one_and_shadows_it() {
    let (dir, root) = project();
    assert_eq!(select(&root).unwrap().source, Source::Absent);
    assert_eq!(config_file(&root), root.join(ROOT_CONFIG));
    assert!(!is_project(&root));

    dir.child(GROUPED_CONFIG)
        .write_str("schema_version: 1\n")
        .unwrap();
    let grouped = select(&root).unwrap();
    assert_eq!(grouped.source, Source::Grouped);
    assert_eq!(grouped.bundle(), root.join(GROUPED_BUNDLE));
    assert_eq!(grouped.shadowed, [] as [std::path::PathBuf; 0]);
    assert!(is_project(&root));

    // A present root file is selected even when it is not a valid file, so
    // its error is not hidden by loading the grouped tree.
    dir.child(".shield/config.yml").create_dir_all().unwrap();
    let root_selection = select(&root).unwrap();
    assert_eq!(root_selection.source, Source::Root);
    assert_eq!(root_selection.file, root.join(ROOT_CONFIG));
    assert_eq!(root_selection.shadowed, vec![root.join(GROUPED_CONFIG)]);
    assert!(!configured(&root));
}

#[test]
fn an_explicit_file_overrides_both_defaults_and_never_falls_back() {
    let (dir, root) = project();
    dir.child(ROOT_CONFIG)
        .write_str("schema_version: 1\n")
        .unwrap();
    dir.child(GROUPED_CONFIG)
        .write_str("schema_version: 1\n")
        .unwrap();
    dir.child("policy/acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    bind(&root.join("policy/acme.yml"), Base::Invocation);
    let selection = select(&root).unwrap();
    assert_eq!(selection.source, Source::Explicit);
    assert_eq!(selection.file, root.join("policy/acme.yml"));
    assert_eq!(
        selection.shadowed,
        vec![root.join(ROOT_CONFIG), root.join(GROUPED_CONFIG)]
    );
    assert_eq!(
        bundle_file(&root, "config.yml"),
        root.join("policy/acme.yml")
    );
    assert_eq!(
        bundle_file(&root, "rules.yml"),
        root.join("policy/rules.yml")
    );
    assert_eq!(
        launch_args(&root).unwrap(),
        ["--config".to_owned(), "policy/acme.yml".to_owned()]
    );

    // Removing the explicit file refuses; the defaults are not used instead.
    std::fs::remove_file(root.join("policy/acme.yml")).unwrap();
    let error = select(&root).unwrap_err();
    assert!(error.to_string().contains("--config"), "{error}");
    assert_eq!(config_file(&root), root.join("policy/acme.yml"));
    assert_eq!(init_destination(&root), root.join("policy/acme.yml"));
}

#[test]
fn an_explicit_file_binds_only_to_the_first_project_selected() {
    let (dir, root) = project();
    let (other_dir, other) = project();
    dir.child("acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    other_dir
        .child(ROOT_CONFIG)
        .write_str("schema_version: 1\n")
        .unwrap();
    bind(Path::new("acme.yml"), Base::Project);
    // A relative argument from a launcher resolves against its project.
    assert_eq!(select(&root).unwrap().file, root.join("acme.yml"));
    // Another project the same process touches keeps its own default.
    let elsewhere = select(&other).unwrap();
    assert_eq!(elsewhere.source, Source::Root);
    assert_eq!(elsewhere.file, other.join(ROOT_CONFIG));
}

#[test]
fn an_explicit_file_outside_the_project_cannot_be_recorded_in_agent_settings() {
    let (_dir, root) = project();
    let outside = TempDir::new().unwrap();
    outside
        .child("acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    bind(&outside.path().join("acme.yml"), Base::Invocation);
    assert_eq!(select(&root).unwrap().source, Source::Explicit);
    assert!(launch_args(&root).is_err());
}

#[test]
fn launch_arguments_name_a_file_init_has_yet_to_create() {
    let (_dir, root) = project();
    bind(Path::new("policy/new/acme.yml"), Base::Project);
    // Agent setup during init can record the destination…
    assert_eq!(
        launch_args(&root).unwrap(),
        ["--config".to_owned(), "policy/new/acme.yml".to_owned()]
    );
    // …while reading policy from it still refuses until it exists.
    assert!(select(&root).is_err());
    assert_eq!(init_destination(&root), root.join("policy/new/acme.yml"));
}

#[test]
fn a_missing_file_named_through_parent_folders_is_refused_for_launch_arguments() {
    let (_dir, root) = project();
    bind(Path::new("policy/../escape.yml"), Base::Project);
    assert!(launch_args(&root).is_err());
}

#[test]
fn discovery_stops_at_the_nearest_folder_holding_the_explicit_file() {
    let (dir, root) = project();
    dir.child(".shield/config.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    let checkout = root.join("checkout");
    dir.child("checkout/policy/acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    dir.child("checkout/src").create_dir_all().unwrap();
    bind(&checkout.join("policy/acme.yml"), Base::Invocation);
    let found = |start: &Path| {
        start
            .ancestors()
            .find(|path| discovery_root(path))
            .map(Path::to_path_buf)
    };
    // The checkout holds the file, so it is found before the ancestor project.
    assert_eq!(found(&checkout.join("src")), Some(checkout.clone()));
    assert_eq!(found(&checkout), Some(checkout.clone()));
    // Once bound, only the owner claims the file.
    select(&checkout).unwrap();
    assert!(!discovery_root(&checkout.join("src")));
    assert!(discovery_root(&root));
}

#[test]
fn a_launcher_argument_claims_the_folder_where_it_names_a_file() {
    let (dir, root) = project();
    dir.child("policy/acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    dir.child("src").create_dir_all().unwrap();
    bind(Path::new("policy/acme.yml"), Base::Project);
    assert!(!discovery_root(&root.join("src")));
    assert!(discovery_root(&root));
}

#[test]
fn default_selections_add_no_launch_arguments() {
    let (dir, root) = project();
    assert_eq!(launch_args(&root).unwrap(), [] as [std::string::String; 0]);
    dir.child(GROUPED_CONFIG)
        .write_str("schema_version: 1\n")
        .unwrap();
    assert_eq!(launch_args(&root).unwrap(), [] as [std::string::String; 0]);
}

#[test]
fn locating_state_follows_the_earlier_name_without_renaming_it() {
    let (dir, root) = project();
    assert_eq!(state(&root), root.join(STATE));
    dir.child(".shield/review/state.json")
        .write_str("{}")
        .unwrap();
    assert!(is_project(&root));
    assert!(enrolled(&root));
    assert_eq!(state(&root), root.join(".shield/review"));
    assert!(root.join(".shield/review").is_dir());
    assert!(!root.join(STATE).exists());
    assert!(!split_state(&root));
}

#[test]
fn the_current_name_wins_and_a_recreated_earlier_one_is_reported() {
    let (dir, root) = project();
    dir.child(".shield/state/state.json")
        .write_str("current")
        .unwrap();
    dir.child(".shield/review/state.json")
        .write_str("earlier")
        .unwrap();
    assert_eq!(state(&root), root.join(STATE));
    assert!(split_state(&root));
}

#[test]
fn shield_files_cover_both_bundles_and_an_explicit_selection() {
    let (dir, root) = project();
    dir.child("policy/acme.yml")
        .write_str("schema_version: 1\n")
        .unwrap();
    bind(&root.join("policy/acme.yml"), Base::Invocation);
    select(&root).unwrap();
    for path in [
        ".shield/state/state.json",
        ".shield/rules.yml",
        ".config/shield/config.yml",
        "policy/acme.yml",
        "policy/config.local.yml",
        "policy/cache/acme.yml",
    ] {
        assert!(in_shield_files(&root, &root.join(path)), "{path}");
    }
    for path in ["src/main.rs", ".config/acme.yml", "policy/notes.txt"] {
        assert!(!in_shield_files(&root, &root.join(path)), "{path}");
    }
}
