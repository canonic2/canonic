//! Diagnostics observe moves; actual runtime use restores reviewed source copies.
use crate::{cli::Cli, configuration::Bundle, review};
use clap::Parser;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn source(root: &Path) -> Vec<u8> {
    fs::create_dir_all(root).expect("source project");
    let config = "schema_version: 1\npacks: []\n";
    let bundle = Bundle::capture(config, &[], None).expect("source policy");
    review::mcp::initialize_fixture(root, &bundle);
    fs::write(root.join(".shield/config.yml"), config).expect("project configuration");
    fs::read(root.join(".shield/review/state.json")).expect("source checkpoint")
}

fn diagnostic(root: &Path, command: &str) -> Value {
    let input = Cli::try_parse_from([
        "shield",
        "--project",
        root.to_str().expect("UTF-8 fixture"),
        "--json",
        command,
    ])
    .expect("diagnostic arguments");
    let output = super::super::dispatch(&input).expect("bootstrap diagnostics");
    output.value
}

#[test]
fn relocation_cli_diagnostics_open_a_move_without_enrollment_review() {
    let parent = assert_fs::TempDir::new().unwrap();
    let base = parent.path().canonicalize().unwrap();
    let original = base.join("before");
    source(&original);
    let moved = base.join("after");
    fs::rename(&original, &moved).unwrap();
    for command in ["status", "doctor"] {
        let report = diagnostic(&moved, command);
        assert_ne!(report["bootstrap_required"], true);
        assert_eq!(report["project_root"], json!(moved));
        assert!(report["active"].is_object());
    }
    assert!(review::open(&moved).is_ok());
}

#[test]
fn relocation_cli_copy_and_config_only_child_have_independent_runtime_state() {
    let parent = assert_fs::TempDir::new().unwrap();
    let base = parent.path().canonicalize().unwrap();
    let original = base.join("original");
    let bytes = source(&original);
    let copied = base.join("copy");
    source(&copied);
    fs::write(copied.join(".shield/review/state.json"), &bytes).unwrap();
    let copy_report = diagnostic(&copied, "status");
    assert_eq!(copy_report["project_root"], json!(copied));
    assert!(review::open(&copied).is_ok());
    assert!(review::open(&original).is_ok());
    let child = original.join("child");
    fs::create_dir_all(child.join(".shield")).unwrap();
    fs::write(
        child.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .unwrap();
    let preview = diagnostic(&child, "doctor");
    assert_eq!(preview["bootstrap_required"], true);
    assert!(
        !child.join(".shield/review/state.json").exists(),
        "diagnosis cannot adopt a source-only policy"
    );
    review::open(&child).expect("actual runtime use restores privately recognized content");
    let report = diagnostic(&child, "doctor");
    assert_ne!(report["bootstrap_required"], true);
    assert_eq!(report["project_root"], json!(child));
    assert!(child.join(".shield/review/state.json").is_file());
    assert_ne!(report["project_id"], copy_report["project_id"]);
    assert_ne!(
        review::open(&original).unwrap().project_id,
        report["project_id"]
    );
    #[cfg(unix)]
    {
        fs::remove_file(child.join(".shield/config.yml")).unwrap();
        std::os::unix::fs::symlink("missing-policy", child.join(".shield/config.yml")).unwrap();
        assert_eq!(
            super::super::project(&child).unwrap().project_root,
            child.to_str().unwrap(),
            "a broken projection never selects parent authority"
        );
    }
}
