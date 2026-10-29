//! Owned native PTYs prove inline consent, paste/resize safety and terminal restoration.
use assert_fs::TempDir;
use serde_json::Value;
use std::process::Command;

fn probe(mode: &str) -> Value {
    let root = TempDir::new().expect("owned project fixture");
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/operator_terminal.py"
    );
    let output = Command::new("python3")
        .arg(script)
        .arg(assert_cmd::cargo::cargo_bin!("shield"))
        .arg(root.path())
        .arg(mode)
        .output()
        .expect("owned PTY harness runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("terminal acceptance receipt")
}
#[test]
fn unsupported_tiny_and_unresponsive_terminals_refuse_without_plain_prompts() {
    for mode in ["unsupported", "initial-tiny", "missing-report"] {
        let result = probe(mode);
        assert_ne!(result["code"], 0, "{result}");
        assert_eq!(result["activated"], false, "{result}");
        assert_eq!(result["restored"], true, "{result}");
    }
}
#[test]
fn inline_picker_digits_and_paste_never_activate_without_enter() {
    let result = probe("apply");
    assert_eq!(result["code"], 0);
    assert_eq!(result["activated"], true);
    assert_eq!(result["restored"], true, "{result}");
    assert_eq!(result["paste_did_not_confirm"], true);
    assert_eq!(result["digit_did_not_confirm"], true);
}
#[test]
fn inline_escape_and_external_term_cancel_and_restore_terminal_modes() {
    for mode in ["escape", "term"] {
        let result = probe(mode);
        assert_eq!(result["code"], 130, "{result}");
        assert_eq!(result["activated"], false);
        assert_eq!(result["restored"], true, "{result}");
    }
}
#[test]
fn inline_resize_cannot_confirm_in_an_insufficient_region() {
    let result = probe("resize");
    assert_eq!(result["tiny_did_not_confirm"], true);
    assert_eq!(result["activated"], false);
    assert_eq!(result["restored"], true);
}
#[test]
fn inline_editor_multiline_paste_does_not_submit_or_change_policy_on_cancel() {
    let result = probe("editor");
    assert_eq!(result["code"], 130, "{result}");
    assert_eq!(result["paste_did_not_submit"], true, "{result}");
    assert_eq!(result["policy_unchanged"], true, "{result}");
    assert_eq!(result["restored"], true, "{result}");
}
#[test]
fn inline_suspension_releases_modes_and_retains_the_selected_choice_on_continue() {
    let result = probe("suspend");
    assert_eq!(result["suspended_modes_restored"], true, "{result}");
    assert_eq!(result["code"], 0, "{result}");
    assert_eq!(result["activated"], true, "{result}");
    assert_eq!(result["restored"], true, "{result}");
}
#[test]
fn inline_onboarding_marks_both_agents_and_keeps_removal_out_of_the_flow() {
    let result = probe("agents");
    assert_eq!(result["code"], 0, "{result}");
    assert_eq!(
        result["agents"],
        serde_json::json!(["claude", "codex"]),
        "{result}"
    );
    assert_eq!(result["restored"], true, "{result}");
}
#[test]
fn settled_error_after_inline_setup_is_safe_and_keeps_terminal_modes_restored() {
    let result = probe("error");
    assert_eq!(result["code"], 1, "{result}");
    assert_eq!(result["actionable_error"], true, "{result}");
    assert_eq!(result["safe_error"], true, "{result}");
    assert_eq!(result["restored"], true, "{result}");
}
