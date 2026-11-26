//! Exercises Shield's setup handoff through the real Hub publisher.
use super::*;
use crate::operator::{interactive, query};
use assert_fs::TempDir;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
#[ignore = "requires a companion Hub build and a PTY"]
#[expect(
    clippy::too_many_lines,
    reason = "one temporary checkout exercises both publishers, confirmation and retained ownership"
)]
fn combined_setup_uses_scoped_reviews_and_preserves_each_owner() {
    if crate::isolated_home() {
        return;
    }
    let hub = std::env::var_os("SHIELD_TEST_HUB_BINARY").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../hub/target/debug/hub"),
        PathBuf::from,
    );
    let hub = hub
        .canonicalize()
        .expect("build the companion Hub or set SHIELD_TEST_HUB_BINARY");
    // Hub's per-user socket address must fit the Unix path limit.
    let temp = TempDir::new_in(if cfg!(target_os = "macos") {
        "/private/tmp"
    } else {
        "/tmp"
    })
    .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let definitions = root.join(".hub/mcp");
    fs::create_dir_all(&definitions).unwrap();
    fs::write(root.join(".hub/config.yml"), "schema_version: 1\n").unwrap();
    let other = "schema_version: 1\nconnection: {transport: stdio, command: acme-tool}\n";
    let policy = root.join(".shield/config.yml");
    let policy_text = "schema_version: 1\npacks: []\n";
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    fs::write(&policy, policy_text).unwrap();
    let definition = definitions.join("shield.yml");
    let settings = root.join(".codex/config.toml");
    let hooks = root.join(".codex/hooks.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let padding = "Acme native setting ".repeat(4_000);
    let native = format!("model = \"acme\"\nacme_padding = \"{padding}\"\n");
    fs::write(&settings, &native).unwrap();
    let initial = hub_cli(
        &hub,
        &root,
        &["setup", "--agent", "codex", "--yes", "--json"],
    );
    assert_eq!(initial["ok"], true, "{initial}");
    let before_publication = fs::read_to_string(&settings).unwrap();
    assert!(before_publication.contains("model = \"acme\""));
    assert!(before_publication.contains(&padding));
    for index in 0..100 {
        fs::write(definitions.join(format!("acme-{index:03}.yml")), other).unwrap();
    }

    let declined = interactive(&temp, &["setup", "--agent", "hub"], "no\n");
    assert_eq!(declined["code"], 130, "{declined}");
    assert!(!definition.exists());
    assert_eq!(fs::read_to_string(&settings).unwrap(), before_publication);
    let approved = interactive(&temp, &["setup", "--agent", "hub"], "yes\n");
    assert_eq!(approved["code"], 0, "{approved}");
    let authored = fs::read(&definition).unwrap();
    assert!(!hooks.exists());

    let pending = query(&temp, &["status"]);
    assert_eq!(pending["next"]["server"], "hub", "{pending}");
    assert_eq!(pending["next"]["tool"], "setup");
    let arguments = &pending["next"]["arguments"];
    assert_eq!(arguments, &json!({"project":root}));
    let mut no_forms = hub_session(&hub, &root, &json!({}));
    let blocked = call(&mut no_forms, 2, "setup", arguments);
    assert_eq!(
        blocked["error"]["code"], "confirmation_unavailable",
        "{blocked}"
    );
    let recovery = blocked["error"]["context"]["message"].as_str().unwrap();
    assert!(recovery.contains(root.to_str().unwrap()), "{recovery}");
    assert!(!hooks.exists());
    assert_eq!(fs::read_to_string(&settings).unwrap(), before_publication);
    drop(no_forms);

    let mut server = hub_session(&hub, &root, &json!({"elicitation":{"form":{}}}));
    let preview = call(&mut server, 3, "setup_preview", arguments);
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["data"]["applied"], false, "{preview}");
    assert_eq!(fs::read_to_string(&settings).unwrap(), before_publication);
    let changed_source = definitions.join("acme-000.yml");
    for (id, answer) in [(4, "decline"), (5, "stale"), (6, "accept")] {
        send(&mut server, id, "setup", arguments);
        let review = server.receive();
        assert_eq!(review["method"], "elicitation/create", "{review}");
        let message = review["params"]["message"].as_str().unwrap();
        assert!(message.len() < 2_000, "{} bytes", message.len());
        assert!(message.contains("101 integrations"), "{message}");
        assert!(message.contains(root.to_str().unwrap()), "{message}");
        assert!(!message.contains("Acme native setting"));
        assert!(!hooks.exists());
        assert_eq!(fs::read_to_string(&settings).unwrap(), before_publication);
        if answer == "stale" {
            fs::write(&changed_source, other.replace("acme-tool", "acme-other")).unwrap();
        }
        let action = if answer == "stale" { "accept" } else { answer };
        server.send(&json!({"jsonrpc":"2.0","id":review["id"],"result":{"action":action}}));
        let receipt = envelope(&server.receive());
        if answer == "accept" {
            assert_eq!(receipt["ok"], true, "{receipt}");
            assert!(hooks.exists());
        } else {
            if answer == "stale" {
                assert_eq!(receipt["ok"], false, "{receipt}");
                fs::write(&changed_source, other).unwrap();
            } else {
                assert_eq!(receipt["data"]["cancelled_by_user"], true, "{receipt}");
            }
            assert!(!hooks.exists());
            assert_eq!(fs::read_to_string(&settings).unwrap(), before_publication);
        }
    }
    let configured = fs::read_to_string(&settings).unwrap();
    assert!(configured.contains("model = \"acme\""));
    assert!(configured.contains(&padding));
    let published = query(&temp, &["status"]);
    assert_eq!(
        published["enforcement_evidence"]["published_hooks"], "configured",
        "{published}"
    );
    assert_eq!(
        published["enforcement_evidence"]["interception"],
        "unchecked"
    );

    let mut shield = Server::start_with(&temp, &["mcp", "serve", "--agent", "codex"]);
    shield.initialize_with(&json!({"elicitation":{"form":{}}}));
    let repeat = call(&mut shield, 2, "setup", &json!({}));
    assert_eq!(repeat["data"]["setup_status"], "unchanged", "{repeat}");
    assert!(repeat["data"]["next"].is_null(), "{repeat}");
    drop(shield);
    let repeat = hub_cli(&hub, &root, &["setup", "--yes", "--json"]);
    assert_eq!(repeat["data"]["changed"], false, "{repeat}");

    let before_hooks = fs::read(&hooks).unwrap();
    fs::remove_file(&hooks).unwrap();
    let recovery = query(&temp, &["doctor"]);
    assert_eq!(recovery["next"]["server"], "hub", "{recovery}");
    let blocked = call(&mut server, 7, "setup", &recovery["next"]["arguments"]);
    assert_eq!(blocked["error"]["code"], "ownership_conflict", "{blocked}");
    let context = &blocked["error"]["context"];
    let explanation = context["message"].as_str().unwrap();
    assert!(explanation.len() < 220, "{explanation}");
    assert!(explanation.contains(".codex/hooks.json"), "{explanation}");
    assert_eq!(context["next"]["tool"], "setup", "{context}");
    assert_eq!(
        context["next"]["arguments"],
        json!({"project":root,"adopt":true})
    );
    assert!(!hooks.exists());
    send(&mut server, 8, "setup", &context["next"]["arguments"]);
    let repair = server.receive();
    assert_eq!(repair["method"], "elicitation/create", "{repair}");
    assert!(!hooks.exists());
    server.send(&json!({"jsonrpc":"2.0","id":repair["id"],"result":{"action":"accept"}}));
    let repaired = envelope(&server.receive());
    assert_eq!(repaired["ok"], true, "{repaired}");
    assert_eq!(fs::read(&hooks).unwrap(), before_hooks);
    assert_eq!(fs::read(&definition).unwrap(), authored);
    assert_eq!(fs::read_to_string(policy).unwrap(), policy_text);
}

fn hub_command(binary: &Path, root: &Path) -> Command {
    let home = crate::fixture_home(root).canonicalize().unwrap();
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("CODEX_HOME", home.join("codex"))
        .env("CLAUDE_CONFIG_DIR", home.join("claude"))
        .env("HUB_TEST_USER_DIR", root.join("u"));
    command
}

fn hub_cli(binary: &Path, root: &Path, args: &[&str]) -> Value {
    successful_json(hub_command(binary, root).args(args))
}

fn successful_json(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn hub_session(binary: &Path, root: &Path, capabilities: &Value) -> Server {
    let mut server = Server::spawn(hub_command(binary, root).args(["mcp", "serve"]));
    server.send(
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":capabilities,
        "clientInfo":{"name":"Acme","version":"1"}}}),
    );
    let initialized = server.receive();
    assert!(
        initialized["result"]["capabilities"]["tools"].is_object(),
        "{initialized}"
    );
    server.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    server
}

fn send(server: &mut Server, id: u64, name: &str, args: &Value) {
    server.send(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":name,"arguments":args}}));
}

fn call(server: &mut Server, id: u64, name: &str, args: &Value) -> Value {
    send(server, id, name, args);
    envelope(&server.receive())
}

fn envelope(reply: &Value) -> Value {
    let value = &reply["result"]["structuredContent"];
    assert!(value.is_object(), "{reply}");
    value.clone()
}
