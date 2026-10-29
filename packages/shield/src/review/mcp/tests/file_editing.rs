//! Agent changes edit hand-written project files in place through activation,
//! and reviews name the files that lose formatting.
use super::application::{apply, prepare, readable};
use super::*;

const CONFIG: &str = "\
# Acme project policy.
schema_version: 1
# Packs chosen by the platform team.
packs: []
rules: rules.yml
";

const RULES: &str = "\
# Acme team rules.
schema_version: 1
kind: custom
capabilities:
- argument-rules-v1
- structured-rules-v1
rules:
# Reads run during local development.
- id: read
  decision: allow
  reason: Acme review
  conditions:
    domain: argv
    executable: acme
    arguments:
      kind: exact
      values:
      - read # the only form agents use
";

/// A project whose active files are the hand-written `CONFIG` and `RULES`,
/// as after adopting manual edits, with a Codex connection.
fn commented() -> (assert_fs::TempDir, Context, Connection) {
    let (root, mut context, connection) =
        fixture_with(&Bundle::capture(CONFIG, &[], Some(RULES)).unwrap());
    context.agent = Some("codex".into());
    (root, context, connection)
}

fn file(root: &assert_fs::TempDir, name: &str) -> String {
    std::fs::read_to_string(root.path().join(".shield").join(name)).unwrap()
}

fn comments(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.find('#').map(|at| &line[at..]))
        .collect()
}

fn propose(context: &Context, connection: &Connection, change: &Value) -> Value {
    let result = invoke(
        context,
        connection,
        "policy_propose",
        json!({"change":change}),
    );
    assert!(result["error"].is_null(), "{change}: {result}");
    result["data"].clone()
}

#[test]
fn applied_rule_changes_edit_a_commented_rules_file_in_place() {
    let (root, context, connection) = commented();
    let mut deploy = rule("deploy", "ask");
    deploy["conditions"]["executable"] = json!("deploy");
    let added = propose(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":deploy}),
    );
    assert!(added["comparison"]["formatting"].is_null(), "{added}");
    assert!(!readable(&added["apply_arguments"]).contains("will not be kept"));
    apply(&context, &connection, &added["apply_arguments"]);
    let rules = file(&root, "rules.yml");
    assert!(rules.starts_with(RULES), "{rules}");
    assert!(rules.contains("\n- id: deploy\n"), "{rules}");
    assert_eq!(file(&root, "config.yml"), CONFIG);

    let edit = json!({"kind":"rule_edit","rule_id":"read","rule":rule("read", "ask")});
    apply(
        &context,
        &connection,
        &prepare(&context, &connection, &edit),
    );
    let rules = file(&root, "rules.yml");
    assert_eq!(comments(&rules), comments(RULES), "{rules}");
    assert!(rules.contains("- id: read\n  decision: ask\n"), "{rules}");

    let remove = json!({"kind":"rule_remove","rule_id":"read"});
    apply(
        &context,
        &connection,
        &prepare(&context, &connection, &remove),
    );
    let rules = file(&root, "rules.yml");
    assert!(rules.starts_with("# Acme team rules.\n"), "{rules}");
    assert!(!rules.contains("Reads run during") && !rules.contains("id: read"));
    assert!(rules.contains("- id: deploy\n"), "{rules}");

    // The active policy retains exactly the bytes on disk.
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(state.candidates["project"]["rules"], rules);
    assert_eq!(state.candidates["project"]["config"], CONFIG);
}

#[test]
fn reviews_name_rules_files_that_lose_comments_before_confirmation() {
    let (root, context, connection) = commented();
    let mut wider = rule("read", "allow");
    wider["conditions"]["arguments"]["values"] = json!(["read", "list"]);
    let proposed = propose(
        &context,
        &connection,
        &json!({"kind":"rule_edit","rule_id":"read","rule":wider}),
    );
    let line = "rules.yml: 1 comment in the changed entries will not be kept.";
    assert_eq!(
        proposed["comparison"]["formatting"],
        json!([line]),
        "{proposed}"
    );
    assert!(
        readable(&proposed["apply_arguments"]).contains(line),
        "{}",
        readable(&proposed["apply_arguments"])
    );
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let proposal = state
        .proposals
        .iter()
        .find(|p| p.id == proposed["proposal_id"])
        .unwrap();
    assert_eq!(
        proposal.apply_arguments()["review"]["comparison"]["formatting"],
        json!([line])
    );
    assert!(
        super::super::confirmation::summary(proposal, &proposal.apply_arguments()).contains(line)
    );
    apply(&context, &connection, &proposed["apply_arguments"]);
    let rules = file(&root, "rules.yml");
    assert!(!rules.contains("the only form agents use"), "{rules}");
    assert!(rules.starts_with("# Acme team rules.\n"), "{rules}");
    assert!(
        rules.contains("# Reads run during local development.\n"),
        "{rules}"
    );

    // Files Shield last wrote from JSON were already in its layout.
    let (_root, context, connection) = fixture();
    let plain = propose(
        &context,
        &connection,
        &json!({"kind":"rule_add","rule":rule("read", "allow")}),
    );
    assert!(plain["comparison"]["formatting"].is_null(), "{plain}");
}

#[test]
fn pack_and_folder_changes_keep_config_comments_and_local_changes_stay_local() {
    let (root, context, connection) = commented();
    let build = json!({"id":"build","decision":"allow","conditions":{"domain":"argv","executable":{"var":"tool"},"arguments":{"kind":"any"}}});
    let _source = install_tools(&root, &context, &connection, &json!([build]));
    let installed = file(&root, "config.yml");
    assert!(
        installed.starts_with(
            "# Acme project policy.\nschema_version: 1\n# Packs chosen by the platform team.\npacks:\n- id: acme/tools\n"
        ),
        "{installed}"
    );
    assert!(installed.ends_with("rules: rules.yml\n"), "{installed}");
    assert_eq!(file(&root, "rules.yml"), RULES);

    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/tools","set":{"tool":"make"}}),
        "shared",
    );
    let configured = file(&root, "config.yml");
    assert_eq!(comments(&configured), comments(CONFIG), "{configured}");
    assert!(
        configured.contains("  variables:\n    tool: make\n"),
        "{configured}"
    );
    assert_eq!(decide(root.path(), "make"), crate::policy::Decision::Allow);

    let outside = assert_fs::TempDir::new().unwrap();
    let path = outside.path().canonicalize().unwrap();
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"access_path_add","access":"read","path":path,"layer":"local"}),
        "local-root",
    );
    assert_eq!(file(&root, "config.yml"), configured);
    let local = file(&root, "config.local.yml");
    assert!(local.starts_with("schema_version: 1\n"), "{local}");
    assert!(local.contains(path.to_str().unwrap()), "{local}");
}
