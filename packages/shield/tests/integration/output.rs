//! Snapshots help and human output for the supported operator commands and
//! internal transports.

#[cfg(unix)]
mod human;

use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn help_text_for_every_command() {
    for (name, args) in [
        ("help", vec!["--help"]),
        ("help-init", vec!["init", "--help"]),
        ("help-status", vec!["status", "--help"]),
        ("help-doctor", vec!["doctor", "--help"]),
        ("help-mcp", vec!["mcp", "--help"]),
        ("help-mcp-setup", vec!["mcp", "setup", "--help"]),
        ("help-mcp-serve", vec!["mcp", "serve", "--help"]),
        ("help-pack", vec!["pack", "--help"]),
        ("help-pack-list", vec!["pack", "list", "--help"]),
        ("help-pack-show", vec!["pack", "show", "--help"]),
        ("help-pack-add", vec!["pack", "add", "--help"]),
        ("help-pack-update", vec!["pack", "update", "--help"]),
        ("help-pack-remove", vec!["pack", "remove", "--help"]),
        ("help-pack-configure", vec!["pack", "configure", "--help"]),
        ("help-pack-search", vec!["pack", "search", "--help"]),
        ("help-pack-info", vec!["pack", "info", "--help"]),
        ("help-registry", vec!["registry", "--help"]),
        ("help-registry-list", vec!["registry", "list", "--help"]),
        ("help-policy", vec!["policy", "--help"]),
        ("help-policy-show", vec!["policy", "show", "--help"]),
        ("help-policy-review", vec!["policy", "review", "--help"]),
        ("help-rule", vec!["rule", "--help"]),
        ("help-rule-list", vec!["rule", "list", "--help"]),
        ("help-request", vec!["request", "--help"]),
        ("help-request-list", vec!["request", "list", "--help"]),
        ("help-request-show", vec!["request", "show", "--help"]),
        ("help-request-approve", vec!["request", "approve", "--help"]),
        ("help-request-deny", vec!["request", "deny", "--help"]),
        ("help-history", vec!["history", "--help"]),
        ("help-history-list", vec!["history", "list", "--help"]),
        ("help-history-show", vec!["history", "show", "--help"]),
        ("help-protection", vec!["protection", "--help"]),
        ("help-protection-set", vec!["protection", "set", "--help"]),
        ("help-rule-add", vec!["rule", "add", "--help"]),
        ("help-rule-edit", vec!["rule", "edit", "--help"]),
        ("help-rule-remove", vec!["rule", "remove", "--help"]),
        ("help-registry-add", vec!["registry", "add", "--help"]),
        ("help-registry-remove", vec!["registry", "remove", "--help"]),
        (
            "help-registry-default",
            vec!["registry", "default", "--help"],
        ),
        ("help-cache", vec!["cache", "--help"]),
        ("help-cache-prepare", vec!["cache", "prepare", "--help"]),
        ("help-validate", vec!["__internal", "validate", "--help"]),
        ("help-test-pack", vec!["__internal", "test-pack", "--help"]),
        (
            "help-native-hook",
            vec!["__internal", "native-hook", "--help"],
        ),
        ("help-run", vec!["__internal", "run", "--help"]),
        (
            "help-enforce-request",
            vec!["__internal", "enforce-request", "--help"],
        ),
    ] {
        let mut command = cargo_bin_cmd!("shield");
        let output = command.args(args).assert().success();
        insta::with_settings!({filters => vec![(r"(?m)[ \t]+$", "")]}, {
            insta::assert_snapshot!(name, String::from_utf8_lossy(&output.get_output().stdout));
        });
    }
}
