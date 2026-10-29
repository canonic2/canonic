//! Native client configuration must be observed even when tools omit environment metadata.
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use std::fs;

const VARIABLES: [&str; 13] = [
    "CURL_HOME",
    "XDG_CONFIG_HOME",
    "WGETRC",
    "SYSTEM_WGETRC",
    "NETRC",
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "no_proxy",
    "NO_PROXY",
];

#[test]
fn native_http_configuration_environment_requires_command_review_for_both_agents() {
    let project = assert_fs::TempDir::new().unwrap();
    let home = project.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::create_dir(project.path().join(".shield")).unwrap();
    fs::write(
        project.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    fs::write(
        project.path().join(".shield/rules.yml"),
        json!({
            "schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],
            "rules":[{"id":"fetch","decision":"allow","conditions":{"domain":"web","action":"fetch",
                "url":{"schemes":["https"],"hosts":["registry.example.com"]}}}]
        })
        .to_string(),
    )
    .unwrap();
    super::working_files::initialize(&project);
    for agent in ["claude", "codex"] {
        for source in [
            "curl -fsS https://registry.example.com/",
            "wget --no-hsts --no-netrc -qO- https://registry.example.com/",
        ] {
            for variable in [
                None,
                Some("CURL_HOME"),
                Some("XDG_CONFIG_HOME"),
                Some("WGETRC"),
                Some("SYSTEM_WGETRC"),
                Some("NETRC"),
                Some("HTTPS_PROXY"),
            ] {
                let mut command = cargo_bin_cmd!("shield");
                command
                    .args([
                        "--no-debug-log",
                        "__internal",
                        "native-hook",
                        "--agent",
                        agent,
                        "--event",
                        "PreToolUse",
                        "--project",
                    ])
                    .arg(project.path())
                    .env("HOME", &home);
                for name in VARIABLES {
                    command.env_remove(name);
                }
                if let Some(name) = variable {
                    command.env(name, "http://proxy.example.com/config");
                }
                let output = command.write_stdin(json!({
                    "hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-http-env",
                    "session_id":"acme-http-env","cwd":project.path(),"tool_input":{"command":source}
                }).to_string()).assert().success().get_output().stdout.clone();
                let response: Value = serde_json::from_slice(&output).unwrap();
                let expected = match (agent, variable.is_some()) {
                    ("claude", false) => "allow",
                    ("claude", true) => "ask",
                    (_, false) => "",
                    (_, true) => "deny",
                };
                assert_eq!(
                    response["hookSpecificOutput"]["permissionDecision"]
                        .as_str()
                        .unwrap_or(""),
                    expected,
                    "{agent}, {variable:?}, {source}: {response}"
                );
            }
        }
    }
}
