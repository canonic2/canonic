//! Native owned PTY anchoring, alternate return, resize, output and exact mode restoration.
#[cfg(unix)]
#[test]
fn inline_native_host_preserves_surrounding_output_and_terminal_modes() {
    let output = std::process::Command::new("python3")
        .args([
            "-B",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/inline_host.py"),
        ])
        .arg(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .output()
        .expect("owned inline PTY fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[cfg(unix)]
fn native_suspend_releases_modes_and_retains_draft_and_borrowed_view() {
    let output = std::process::Command::new("python3")
        .args([
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/inline_suspend.py"
            ),
        ])
        .arg(assert_cmd::cargo::cargo_bin!("tui-gallery"))
        .output()
        .expect("owned job-control PTY fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn plain_input_retries_empty_lines_and_eof_never_selects_a_default() {
    for (input, expected) in [
        ("\n2\n", "Deploy API only · fixture only"),
        ("", "cancelled · nothing executed"),
        ("3\n", "cancelled · nothing executed"),
    ] {
        let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tui-gallery"))
            .args([
                "--inline",
                "--component",
                "inline-composition",
                "--scenario",
                "plain",
            ])
            .env("TERM", "dumb")
            .write_stdin(input)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(expected), "{text}");
        assert!(!text.contains('\x1b'));
    }
}
