//! Verifies local policy files, preview statuses, and registry fixture workflows.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use serde_json::{Value, json};

pub(super) fn candidate(project: &assert_fs::TempDir) -> assert_fs::fixture::ChildPath {
    let pack = project.child("pack.json");
    pack.write_str(&json!({"schema_version":1,"id":"acme/test","version":"0.1.0","description":"Acme integration fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"mutation","reason":"Preview profile choices","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["write"]}},"decision":{"loose":"allow","recommended":"ask","strict":"deny"}}],"kind":"pack"}).to_string()).expect("write synthetic pack");
    pack
}

fn output_json(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&output.get_output().stdout).expect("CLI returns JSON")
}

#[test]
fn fixture_runner_distinguishes_pass_mismatch_and_invalid_input() {
    let project = assert_fs::TempDir::new().unwrap();
    let pack = candidate(&project);
    let cases = project.child("cases.json");
    for (expected, code) in [("deny", 0), ("allow", 1)] {
        cases.write_str(&json!({"schema_version":1,"cases":[{"name":"strict-write","profile":"strict","argv":["acme","write"],"expected":expected}]}).to_string()).unwrap();
        let output = cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["test-pack", "--pack"])
            .arg(pack.path())
            .arg("--cases")
            .arg(cases.path())
            .assert()
            .code(code);
        assert_eq!(output_json(&output)["passed"], code == 0);
        assert_eq!(output_json(&output)["cases"][0]["actual"], "deny");
    }
    cases.write_str("{\"schema_version\":1,\"cases\":[{\"name\":\"bad\",\"profile\":\"strict\",\"argv\":[],\"expected\":\"ask\"}]}").unwrap();
    cargo_bin_cmd!("shield")
        .arg("__internal")
        .args(["test-pack", "--pack"])
        .arg(pack.path())
        .arg("--cases")
        .arg(cases.path())
        .assert()
        .code(2);
}

#[test]
fn fixture_cases_bind_pack_variables_and_leave_omitted_ones_unset() {
    let project = assert_fs::TempDir::new().unwrap();
    let pack = project.child("pack.json");
    pack.write_str(&json!({"schema_version":1,"id":"acme/node","version":"0.1.0","description":"Acme variables fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","preconditions-v1","structured-rules-v1","variables-v1"],"kind":"pack",
        "variables":{"package_manager":{"type":"choice","description":"Package manager","values":["npm","pnpm"]}},
        "rules":[{"id":"install","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["npm","pnpm"]},"arguments":{"kind":"exact","values":["install"]}}}],
        "preconditions":[{"id":"manager","conditions":{"domain":"argv","executable":{"one_of":["npm","pnpm"]},"arguments":{"kind":"any"}},"requires_any":[{"executable":{"var":"package_manager"}}],"on_unmet":{"decision":"deny"}}]}).to_string()).unwrap();
    let cases = project.child("cases.json");
    cases.write_str(&json!({"schema_version":1,"cases":[
        {"name":"unset","profile":"recommended","argv":["npm","install"],"expected":"allow"},
        {"name":"pnpm-gates-npm","profile":"recommended","argv":["npm","install"],"expected":"deny","variables":{"package_manager":"pnpm"}},
        {"name":"pnpm-allows-pnpm","profile":"recommended","argv":["pnpm","install"],"expected":"allow","variables":{"package_manager":"pnpm"}}
    ]}).to_string()).unwrap();
    let output = cargo_bin_cmd!("shield")
        .arg("__internal")
        .args(["test-pack", "--pack"])
        .arg(pack.path())
        .arg("--cases")
        .arg(cases.path())
        .assert()
        .code(0);
    assert_eq!(
        output_json(&output)["cases"][1]["variables"]["package_manager"],
        "pnpm"
    );
}
