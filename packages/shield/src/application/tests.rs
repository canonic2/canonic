//! Behavioral fixtures for batch composition, normalized input and refusal.
use super::*;
use assert_fs::prelude::*;

fn pack(id: &str, executable: &str) -> String {
    json!({"schema_version":1,"kind":"pack","id":id,"version":"1.0.0",
        "description":"Acme batch fixture","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"inspect","conditions":{"domain":"argv","executable":executable,
            "arguments":{"kind":"any"}},"decision":{"loose":"allow","recommended":"ask","strict":"deny"}}]}).to_string()
}

fn fixture(root: &assert_fs::TempDir, cases: &Value) -> crate::cli::PackTest {
    root.child("primary.json")
        .write_str(&pack("acme/primary", "acme"))
        .unwrap();
    root.child("companion.json")
        .write_str(&pack("acme/companion", "other"))
        .unwrap();
    root.child("cases.json")
        .write_str(&json!({"schema_version":1,"cases":cases}).to_string())
        .unwrap();
    crate::cli::PackTest {
        pack: root.path().join("primary.json"),
        with_pack: vec![root.path().join("companion.json")],
        cases: root.path().join("cases.json"),
        details: true,
    }
}

#[test]
fn batch_fixtures_compose_shell_and_argv_without_activation_or_execution() {
    let root = assert_fs::TempDir::new().unwrap();
    let mut cases = vec![];
    for (profile, expected) in [
        ("loose", "allow"),
        ("recommended", "ask"),
        ("strict", "deny"),
    ] {
        cases.push(
            json!({"name":format!("shell-{profile}"),"profile":profile,"expected":expected,
            "input":{"kind":"shell","source":"acme read | other inspect","dialect":"posix",
                "dialect_version":null,"original_input":{}}}),
        );
        cases.push(
            json!({"name":format!("argv-{profile}"),"profile":profile,"expected":expected,
            "argv":["other","inspect"]}),
        );
    }
    let marker = root.path().join("must-not-exist");
    cases.push(json!({"name":"never-execute","profile":"loose","expected":"ask",
        "input":{"kind":"shell","source":format!("python3 -c 'open(\"{}\", \"w\").close()'",marker.display()),
            "dialect":"posix","dialect_version":null,"original_input":{}}}));
    let input = fixture(&root, &json!(cases));
    let (report, code) = test_pack(&input).unwrap();
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["passed"], true);
    assert_eq!(report["cases"].as_array().unwrap().len(), 7);
    assert_eq!(report["cases"][0]["evaluation"]["disposition"], "preview");
    assert!(
        report["cases"][1]["evaluation"]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["source"] == "acme/companion")
    );
    root.child("must-not-exist")
        .assert(predicates::path::missing());
    root.child(".shield").assert(predicates::path::missing());
    let without_companion = crate::cli::PackTest {
        with_pack: vec![],
        ..input
    };
    let (report, code) = test_pack(&without_companion).unwrap();
    assert_eq!(code, 1);
    assert_eq!(report["passed"], false);
}

#[test]
fn batch_fixtures_refuse_ambiguous_inputs_and_duplicate_pack_identities() {
    let root = assert_fs::TempDir::new().unwrap();
    for fields in [
        json!({}),
        json!({"argv":[]}),
        json!({"argv":["acme"],"input":{"kind":"unknown","host_tool":"acme","original_input":{}}}),
        json!({"input":{"kind":"shell","source":"acme","dialect":"posix","unexpected":true}}),
    ] {
        let mut case = json!({"name":"invalid","profile":"loose","expected":"allow"});
        case.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let input = fixture(&root, &json!([case]));
        assert!(test_pack(&input).is_err());
    }
    let mut input = fixture(
        &root,
        &json!([{"name":"valid","profile":"loose","expected":"allow","argv":["acme"]}]),
    );
    input.with_pack = vec![input.pack.clone()];
    assert!(
        test_pack(&input)
            .unwrap_err()
            .to_string()
            .contains("duplicate pack identity")
    );
    input.with_pack = vec![input.pack.clone(); 64];
    assert!(matches!(
        test_pack(&input),
        Err(Error::Policy(policy::Error::Limit))
    ));
}
