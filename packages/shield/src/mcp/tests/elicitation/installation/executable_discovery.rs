//! Isolated real setup captures local executable values once for approval.
use super::*;

#[test]
fn executable_discovery_install_update_and_repair_use_reviewed_local_bindings() {
    if std::env::var_os("SHIELD_EXECUTABLE_DISCOVERY_CHILD").is_some() {
        exercise();
        return;
    }
    let binary = std::env::current_exe().unwrap();
    let child = std::process::Command::new(&binary)
        .args(["--exact", "mcp::tests::elicitation::installation::executable_discovery::executable_discovery_install_update_and_repair_use_reviewed_local_bindings", "--nocapture"])
        .env("SHIELD_EXECUTABLE_DISCOVERY_CHILD", "1")
        .env("CARGO_HOME", binary.parent().unwrap())
        .output().unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}

fn pack(version: &str) -> String {
    let binary = std::env::current_exe().unwrap();
    json!({"schema_version":1,"kind":"pack","id":"acme/settings","version":version,
        "description":"Acme executable setup","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},
        "capabilities":["structured-rules-v1","argument-rules-v1","variables-v1","executable-discovery-v1"],
        "variables":{"cargo_executable":{"type":"string","description":"Acme Cargo","layer":"local",
            "discover":[{"env":"CARGO_HOME","path":binary.file_name().unwrap().to_str().unwrap()}]}},
        "rules":[{"id":"build","conditions":{"domain":"argv","executable":{"var":"cargo_executable"},
            "arguments":{"kind":"exact","values":["build"]}},"decision":"allow"}]
    }).to_string()
}

fn exercise() {
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    publish(&source, &[("pack.yml", pack("1.0.0"))]);
    let root = std::path::Path::new(&project.review.project_root);
    let local = root.join(".shield/config.local.yml");
    let shared = root.join(".shield/config.yml");
    let cargo = std::env::current_exe().unwrap();
    let expected = cargo.to_str().unwrap();
    let before = std::fs::read(&shared).unwrap();
    let added = transport.call(
        json!(2),
        "pack_add",
        &json!({"reference":"fixture/acme/settings"}),
    );
    assert!(
        data(&added)["data"]["apply_arguments"].is_object(),
        "{added}"
    );
    assert_eq!(std::fs::read(&shared).unwrap(), before);
    approve(&transport, 3, &added);
    assert!(std::fs::read_to_string(&local).unwrap().contains(expected));
    assert!(!std::fs::read_to_string(&shared).unwrap().contains(expected));
    // Simulate an existing installation with its executable still unconfigured.
    clear_bindings(&local);
    let repaired = transport.call(
        json!(4),
        "pack_configure",
        &json!({"reference":"acme/settings"}),
    );
    assert!(
        data(&repaired)["data"]["apply_arguments"].is_object(),
        "{repaired}"
    );
    assert!(!std::fs::read_to_string(&local).unwrap().contains(expected));
    approve(&transport, 5, &repaired);
    assert!(std::fs::read_to_string(&local).unwrap().contains(expected));
    clear_bindings(&local);
    publish(
        &source,
        &[("pack.yml", pack("1.0.0")), ("next.yml", pack("1.1.0"))],
    );
    let updated = transport.call(
        json!(6),
        "pack_update",
        &json!({"reference":"fixture/acme/settings@1.1.0"}),
    );
    assert!(
        data(&updated)["data"]["apply_arguments"].is_object(),
        "{updated}"
    );
    assert!(!std::fs::read_to_string(&local).unwrap().contains(expected));
    approve(&transport, 7, &updated);
    assert!(std::fs::read_to_string(&local).unwrap().contains(expected));
    assert!(!std::fs::read_to_string(&shared).unwrap().contains(expected));
    // Supplied installation values take precedence over discovered candidates.
    let (transport, project, source) = fresh(false, VERSION, &json!({"elicitation":{"form":{}}}));
    publish(&source, &[("pack.yml", pack("1.0.0"))]);
    let explicit = "/opt/acme/bin/cargo";
    let supplied = transport.call(
        json!(8),
        "pack_add",
        &json!({"reference":"fixture/acme/settings","set":{"cargo_executable":explicit}}),
    );
    approve(&transport, 9, &supplied);
    let shared = std::fs::read_to_string(
        std::path::Path::new(&project.review.project_root).join(".shield/config.yml"),
    )
    .unwrap();
    assert!(shared.contains(explicit));
    let local = std::fs::read_to_string(
        std::path::Path::new(&project.review.project_root).join(".shield/config.local.yml"),
    )
    .unwrap();
    assert!(!local.contains(expected));
}

fn clear_bindings(path: &std::path::Path) {
    let mut config: Value =
        serde_saphyr::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    config["packs"] = json!([]);
    std::fs::write(path, config.to_string()).unwrap();
}
