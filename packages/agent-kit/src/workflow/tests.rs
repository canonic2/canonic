//! Work-count regressions for inspection, preparation and unchanged publication.
use super::*;
use crate::{Tool, document::edits};
use std::{collections::BTreeMap, fmt::Write as _, fs};

/// Large unrelated JSON, TOML and YAML user configuration, plus Claude
/// permission rules, with Acme already installed in each.
fn installed(prompt_hook: Option<Value>) -> (tempfile::TempDir, Context, Options, Vec<PathBuf>) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let context = Context {
        product: "acme".into(),
        program: base.join("acme"),
        arguments: vec!["mcp".into(), "serve".into()],
        launch_command: None,
        check_arguments: Vec::new(),
        root: base.join("project"),
        home: base.join("home"),
        state: base.join("state"),
        agent_homes: BTreeMap::new(),
        tools: vec![
            Tool::new("observe", "Observe.", true),
            Tool::new("remove", "Remove.", false),
        ],
        prompt_hook,
    };
    let history = "h".repeat(1024);
    let projects: serde_json::Map<_, _> = (0..256)
        .map(|i| (format!("/work/p{i}"), json!({"history":[&history]})))
        .collect();
    let (mut toml, mut yaml) = (String::new(), String::from("extensions:\n"));
    for i in 0..256 {
        writeln!(toml, "[mcp_servers.other{i}]\ncommand = \"other\"\n").unwrap();
        writeln!(yaml, "  other{i}:\n    cmd: other").unwrap();
    }
    let rules: Vec<_> = (0..64).map(|i| format!("Bash(tool{i}:*)")).collect();
    let files = [
        (".claude.json", json!({"projects":projects}).to_string()),
        (".codex/config.toml", toml),
        (".config/goose/config.yaml", yaml),
        (
            ".claude/settings.json",
            json!({"permissions":{"allow":rules}}).to_string(),
        ),
    ];
    let paths: Vec<_> = files
        .into_iter()
        .map(|(relative, text)| {
            let path = context.home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
    fs::create_dir_all(&context.root).unwrap();
    assert_eq!(paths[3], agents::claude_settings(&context, Scope::User));
    let options = Options {
        agents: vec!["claude".into(), "codex".into(), "goose".into()],
        scope: Scope::User,
        ..Options::default()
    };
    prepare(&context, &options).unwrap().apply().unwrap();
    (temp, context, options, paths)
}

#[test]
fn unchanged_rerun_inspects_without_edits_and_bounds_preparation_edits() {
    let hook = json!({"hooks":[{"type":"command","command":"acme hook","timeout":5}]});
    // Without a hook: three server entries and Claude's rules, one edit each.
    // A hook adds Codex's hooks.json. Claude still renders its settings once.
    for (prompt_hook, expected_changes) in [(None, 4), (Some(hook), 5)] {
        let (_temp, context, options, paths) = installed(prompt_hook);
        let ledgers: Vec<_> = paths.iter().map(|p| ledger(&context, p)).collect();
        let snapshot =
            |paths: &[PathBuf]| -> Vec<_> { paths.iter().map(|p| fs::read(p).unwrap()).collect() };
        let (files_before, ledgers_before) = (snapshot(&paths), snapshot(&ledgers));
        let inputs = Inputs::default();
        let start = edits();
        for (id, _) in agents::CATALOG {
            let status = inspect_inputs(&context, id, Scope::User, false, &inputs);
            if options.agents.iter().any(|a| a == id) {
                assert_eq!(status.detail, "configured; ownership checked on review");
            }
        }
        // Read-only inspection never constructs a discarded edited document.
        assert_eq!(edits(), start);
        let plan = prepare_inputs(&context, &options, &inputs, SetupPolicy::default()).unwrap();
        assert_eq!(edits() - start, expected_changes);
        assert_eq!(plan.changes.len(), expected_changes);
        for path in &paths {
            assert_eq!(inputs.counts(path), (1, 1), "{}", path.display());
        }
        let review = plan.apply().unwrap();
        assert!(
            review["registrations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["change"] == "unchanged")
        );
        assert_eq!(snapshot(&paths), files_before);
        assert_eq!(snapshot(&ledgers), ledgers_before);
        // The reduced reads still refuse a file changed after review.
        let plan = prepare(&context, &options).unwrap();
        fs::write(&paths[1], "[mcp_servers.external]\ncommand = \"x\"\n").unwrap();
        assert!(matches!(plan.apply(), Err(Error::Conflict(_))));
    }
}
