//! Diagnostics remain reachable without admitting copied enrollment authority.
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
    super::super::dispatch(&input).expect("bootstrap diagnostics")
}

#[test]
fn relocation_cli_diagnostics_explain_a_move_without_changing_state() {
    let parent = assert_fs::TempDir::new().unwrap();
    let base = parent.path().canonicalize().unwrap();
    let original = base.join("before");
    let bytes = source(&original);
    let moved = base.join("after");
    fs::rename(&original, &moved).unwrap();
    for command in ["status", "doctor"] {
        let report = diagnostic(&moved, command);
        assert_eq!(report["bootstrap_required"], true);
        assert_eq!(report["classification"], "relocation");
        assert!(
            report["next_action"]
                .as_str()
                .unwrap()
                .contains("shield init")
        );
        assert!(
            !report["next_action"]
                .as_str()
                .unwrap()
                .contains("project_enroll")
        );
        assert_eq!(report["previous_project_root"], json!(original));
        assert_eq!(
            fs::read(moved.join(".shield/review/state.json")).unwrap(),
            bytes
        );
    }
    assert!(review::open(&moved).is_err());
}

#[test]
fn relocation_cli_copy_and_config_only_child_do_not_borrow_parent_trust() {
    let parent = assert_fs::TempDir::new().unwrap();
    let base = parent.path().canonicalize().unwrap();
    let original = base.join("original");
    let bytes = source(&original);
    let copied = base.join("copy");
    fs::create_dir_all(copied.join(".shield/review")).unwrap();
    fs::copy(
        original.join(".shield/config.yml"),
        copied.join(".shield/config.yml"),
    )
    .unwrap();
    fs::write(copied.join(".shield/review/state.json"), &bytes).unwrap();
    assert_eq!(diagnostic(&copied, "status")["classification"], "copy");
    assert!(review::open(&copied).is_err());
    assert!(review::open(&original).is_ok());
    let child = original.join("child");
    fs::create_dir_all(child.join(".shield")).unwrap();
    fs::write(
        child.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .unwrap();
    let report = diagnostic(&child, "doctor");
    assert_eq!(report["bootstrap_required"], true);
    assert_eq!(report["project_root"], json!(child));
    assert_eq!(report["classification"], "unenrolled");
    assert!(
        report["next_action"]
            .as_str()
            .unwrap()
            .contains("shield init")
    );
    assert!(!child.join(".shield/review/state.json").exists());
    #[cfg(unix)]
    {
        fs::remove_file(child.join(".shield/config.yml")).unwrap();
        std::os::unix::fs::symlink("missing-policy", child.join(".shield/config.yml")).unwrap();
        assert!(
            super::super::project(&child).is_err(),
            "a broken child marker cannot select parent authority"
        );
    }
}
