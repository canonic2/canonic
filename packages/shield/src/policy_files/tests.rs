use super::*;
use serde_json::json;

const RULES: &str = "\
# Acme team rules.
# Reviewed by the platform team.
schema_version: 1
kind: custom
capabilities:
- structured-rules-v1
- argument-rules-v1
rules:
# Health checks run during local development.
- id: acme-health
  decision: allow
  reason: Read the local health endpoint.
  conditions:
    domain: argv
    executable: curl
    arguments:
      kind: exact
      values: [--silent, http://localhost:3000/health]
- id: acme-start # used by the dev loop
  decision: allow
  reason: Start the Acme server.
  conditions:
    domain: argv
    executable: npm
    arguments:
      kind: exact
      values: [start]
";

fn decoded(text: &str) -> Value {
    policy::parse(text).expect("strict YAML")
}

fn rule(id: &str, executable: &str) -> Value {
    json!({"id":id,"decision":"ask","reason":"Acme review.","conditions":{"domain":"argv","executable":executable,"arguments":{"kind":"any"}}})
}

fn edited(previous: &str, change: impl FnOnce(&mut Value)) -> (Value, Written) {
    let mut target = decoded(previous);
    change(&mut target);
    let written = write(Some(previous), &target).expect("written");
    assert_eq!(decoded(&written.text), target, "{}", written.text);
    (target, written)
}

fn comments(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.find('#').map(|at| &line[at..]))
        .collect()
}

#[test]
fn rendering_leads_with_identity_fields_and_reads_back_exactly() {
    let document = json!({"rules":[{"reason":"Acme review.","id":"acme-1","decision":"allow","conditions":{"domain":"argv","executable":"curl","arguments":{"kind":"exact","values":["--max-time","2","on","null","yes","2026-10-24","- dash","# hash"]}}}],"capabilities":["structured-rules-v1"],"kind":"custom","schema_version":1});
    let text = render(&document).expect("rendered");
    assert!(
        text.starts_with("schema_version: 1\nkind: custom\n"),
        "{text}"
    );
    assert!(
        text.contains("rules:\n- id: acme-1\n  decision: allow\n  reason: Acme review.\n"),
        "{text}"
    );
    assert!(
        text.contains("capabilities:\n- structured-rules-v1\n"),
        "{text}"
    );
    assert!(
        text.contains("      values:\n      - --max-time\n"),
        "{text}"
    );
    assert!(!text.contains('{') && !text.starts_with("---"), "{text}");
    assert_eq!(decoded(&text), document);
}

#[test]
fn projected_renders_json_and_keeps_yaml_bytes() {
    let json = r#"{"schema_version":1,"packs":[]}"#;
    assert_eq!(
        projected(json).expect("projected"),
        "schema_version: 1\npacks: []\n"
    );
    assert_eq!(projected(RULES).expect("projected"), RULES);
}

#[test]
fn unchanged_documents_keep_their_bytes() {
    let written = write(Some(RULES), &decoded(RULES)).expect("written");
    assert_eq!(written.text, RULES);
    assert!(!written.rendered);
}

#[test]
fn adding_a_rule_keeps_every_comment_and_the_text_before_it() {
    let (_, written) = edited(RULES, |target| {
        target["rules"]
            .as_array_mut()
            .expect("rules")
            .push(rule("acme-deploy", "deploy"));
    });
    assert!(!written.rendered);
    assert_eq!(written.dropped_comments, 0);
    assert!(written.text.starts_with(RULES), "{}", written.text);
    assert!(written.text.ends_with(
        "- id: acme-deploy\n  decision: ask\n  reason: Acme review.\n  conditions:\n    arguments:\n      kind: any\n    domain: argv\n    executable: deploy\n"
    ));
}

#[test]
fn adding_a_rule_between_existing_rules_keeps_their_order() {
    let (_, written) = edited(RULES, |target| {
        target["rules"]
            .as_array_mut()
            .expect("rules")
            .insert(1, rule("acme-lint", "lint"));
    });
    assert!(!written.rendered);
    let lint = written.text.find("- id: acme-lint").expect("inserted");
    assert!(written.text[..lint].contains("acme-health"));
    assert!(written.text[lint..].contains("- id: acme-start # used by the dev loop"));
    assert_eq!(comments(&written.text), comments(RULES));
}

#[test]
fn editing_one_field_keeps_the_rest_of_the_rule() {
    let (_, written) = edited(RULES, |target| {
        target["rules"][1]["decision"] = json!("ask");
        target["rules"][0]["conditions"]["arguments"]["values"] = json!(["fail", "health"]);
    });
    assert!(!written.rendered);
    assert_eq!(written.dropped_comments, 0);
    assert_eq!(comments(&written.text), comments(RULES));
    let expected = RULES
        .replacen(
            "  decision: allow\n  reason: Start",
            "  decision: ask\n  reason: Start",
            1,
        )
        .replace(
            "values: [--silent, http://localhost:3000/health]",
            "values:\n      - fail\n      - health",
        );
    assert_eq!(written.text, expected);
}

#[test]
fn removing_a_rule_takes_only_its_own_comments() {
    let (_, written) = edited(RULES, |target| {
        target["rules"].as_array_mut().expect("rules").remove(0);
    });
    assert!(!written.rendered);
    assert_eq!(written.dropped_comments, 0);
    assert!(!written.text.contains("Health checks"));
    assert!(!written.text.contains("acme-health"));
    assert!(
        written
            .text
            .starts_with("# Acme team rules.\n# Reviewed by the platform team.\n")
    );
    assert!(
        written
            .text
            .contains("- id: acme-start # used by the dev loop\n")
    );
}

#[test]
fn configuration_edits_keep_comments_beside_packs_and_paths() {
    let config = "\
# Acme project policy.
schema_version: 1
packs:
- id: acme/node # runtime
  version: 1.0.0
  profile: recommended
  path: cache/node.yml
rules: rules.yml
access_paths:
  read:
  - /srv/acme # shared fixtures
";
    let (_, written) = edited(config, |target| {
        target["packs"][0]["profile"] = json!("strict");
        target["packs"]
            .as_array_mut()
            .expect("packs")
            .push(json!({"id":"acme/git","version":"2.0.0","profile":"recommended","path":"cache/git.yml"}));
        target["access_paths"]["write"] = json!(["/srv/acme-out"]);
    });
    assert!(!written.rendered);
    assert_eq!(comments(&written.text), comments(config));
    assert!(written.text.contains("  profile: strict\n"));
    assert!(written.text.contains("- id: acme/git\n"));
    assert!(
        written.text.contains("  write:\n  - /srv/acme-out\n"),
        "{}",
        written.text
    );
}

#[test]
fn a_flow_collection_is_replaced_as_one_node() {
    let previous = "# Acme rules.\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
    let (_, written) = edited(previous, |target| {
        target["rules"] = json!([rule("acme-deploy", "deploy")]);
    });
    assert!(!written.rendered);
    assert!(written.text.starts_with(
        "# Acme rules.\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules:\n- id: acme-deploy\n"
    ));
}

#[test]
fn json_sources_and_reordered_entries_render_below_the_leading_comments() {
    let json =
        r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[]}"#;
    let (target, written) = edited(json, |target| {
        target["rules"] = json!([rule("acme-deploy", "deploy")]);
    });
    assert!(written.rendered);
    assert_eq!(written.text, render(&target).expect("rendered"));

    let (target, written) = edited(RULES, |target| {
        target["rules"].as_array_mut().expect("rules").reverse();
    });
    assert!(written.rendered);
    assert_eq!(written.dropped_comments, 2);
    assert_eq!(
        written.text,
        format!(
            "# Acme team rules.\n# Reviewed by the platform team.\n{}",
            render(&target).expect("rendered")
        )
    );
}

#[test]
fn inserted_entries_follow_the_file_layout() {
    let previous = "schema_version: 1\r\nkind: custom\r\ncapabilities:\r\n    - structured-rules-v1\r\nrules:\r\n    - id: acme-health\r\n      decision: allow\r\n      conditions:\r\n          domain: argv\r\n          executable: curl\r\n          arguments:\r\n              kind: any\r\n";
    let (_, written) = edited(previous, |target| {
        target["rules"]
            .as_array_mut()
            .expect("rules")
            .push(rule("acme-deploy", "deploy"));
    });
    assert!(!written.rendered);
    assert!(written.text.starts_with(previous));
    let added = &written.text[previous.len()..];
    assert!(added.starts_with("    -"), "{added:?}");
    assert!(added.ends_with("\r\n"), "{added:?}");
    assert!(!added.replace("\r\n", "").contains('\n'), "{added:?}");
}

#[test]
fn removing_every_entry_writes_an_empty_list_in_place() {
    let config = "# Acme paths.\nschema_version: 1\npacks: []\naccess_paths:\n  read:\n  - /srv/acme\n  write:\n  - /srv/acme-out # scratch\n";
    let (_, written) = edited(config, |target| {
        target["access_paths"]["write"] = json!([]);
    });
    assert!(!written.rendered);
    // The comment went with its entry.
    assert_eq!(written.dropped_comments, 0);
    assert_eq!(
        written.text,
        "# Acme paths.\nschema_version: 1\npacks: []\naccess_paths:\n  read:\n  - /srv/acme\n  write: []\n"
    );
    // A comment beside the key is the list's own, and is lost.
    let keyed = config.replace("  write:\n", "  write: # outputs\n");
    let (_, written) = edited(&keyed, |target| {
        target["access_paths"]["write"] = json!([]);
    });
    assert_eq!(written.dropped_comments, 1);
}

#[test]
fn a_reason_with_a_line_break_is_written_on_one_line() {
    let (_, written) = edited(RULES, |target| {
        target["rules"][1]["reason"] = json!("Start the Acme server.\nUsed by the dev loop.");
    });
    assert!(!written.rendered, "{}", written.text);
    assert!(
        written
            .text
            .contains("  reason: \"Start the Acme server.\\nUsed by the dev loop.\"\n"),
        "{}",
        written.text
    );
    let mut document = decoded(RULES);
    document["rules"][0]["reason"] = json!("Line one.\nLine two.\ttabbed");
    assert_eq!(decoded(&render(&document).expect("rendered")), document);
}

#[test]
fn edits_beside_non_ascii_text_keep_it_intact() {
    let previous = "# Équipe Acme — règles.\nschema_version: 1\nkind: custom\nrules:\n- id: café\n  decision: allow\n  reason: Démarrer le serveur. # noté ✓\n  conditions:\n    domain: argv\n    executable: npm\n    arguments: {kind: exact, values: [start]}\n";
    let (_, written) = edited(previous, |target| {
        target["rules"][0]["reason"] = json!("Lancer — ü");
        target["rules"]
            .as_array_mut()
            .unwrap()
            .push(rule("thé", "acme"));
    });
    assert!(!written.rendered, "{}", written.text);
    assert_eq!(written.dropped_comments, 0);
    // Only the value's token changed; the comment to its right stays.
    assert!(
        written
            .text
            .lines()
            .any(|line| line.contains("Lancer — ü") && line.ends_with(" # noté ✓")),
        "{}",
        written.text
    );
    assert_eq!(comments(&written.text), comments(previous));
}

#[test]
fn adding_a_key_to_a_file_without_a_final_line_break() {
    let previous = "# Acme rules.\nschema_version: 1\nkind: custom\nrules: []";
    let (_, written) = edited(previous, |target| {
        target["capabilities"] = json!(["structured-rules-v1"]);
    });
    assert!(!written.rendered, "{}", written.text);
    assert!(
        written.text.starts_with(&format!("{previous}\n")),
        "{:?}",
        written.text
    );
}

#[test]
fn removing_the_first_key_keeps_the_file_header() {
    let (_, written) = edited(RULES, |target| {
        target.as_object_mut().unwrap().remove("schema_version");
    });
    assert!(
        written
            .text
            .starts_with("# Acme team rules.\n# Reviewed by the platform team.\nkind: custom\n"),
        "{}",
        written.text
    );
}

#[test]
fn replacing_a_commented_node_counts_its_comments() {
    let previous = "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules:\n- id: acme-health\n  decision: allow\n  conditions:\n    domain: argv\n    executable: curl\n    arguments:\n      kind: exact\n      values:\n      - --silent # quiet\n      - http://localhost:3000/health\n";
    let (_, written) = edited(previous, |target| {
        target["rules"][0]["conditions"]["arguments"]["values"] = json!(["--fail"]);
    });
    assert!(!written.rendered);
    assert_eq!(written.dropped_comments, 1);
}
