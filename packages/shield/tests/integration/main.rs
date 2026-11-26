//! Runs Shield's CLI behavior and output checks as one integration binary.

mod diagnostics;
mod execution;
#[cfg(unix)]
mod http_environment;
mod mcp;
mod operator;
#[cfg(unix)]
mod operator_terminal;
mod output;
mod policy;
#[cfg(any(unix, windows))]
mod review;
#[cfg(any(unix, windows))]
mod runtime;
#[cfg(windows)]
mod windows;
mod workflows;
#[cfg(unix)]
mod working_files;

/// Public-library fixtures need the same private home as their CLI children.
/// Re-execute only this test rather than mutating the parallel harness environment.
fn isolated_home() -> bool {
    if std::env::var_os("SHIELD_INTEGRATION_HOME").is_some() {
        return false;
    }
    let home = assert_fs::TempDir::new().expect("owned integration home");
    let name = std::thread::current()
        .name()
        .expect("named integration test")
        .to_owned();
    let output =
        std::process::Command::new(std::env::current_exe().expect("integration executable"))
            .args(["--exact", &name, "--include-ignored", "--nocapture"])
            .env("SHIELD_INTEGRATION_HOME", home.path())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("LOCALAPPDATA", home.path())
            .output()
            .expect("isolated integration subprocess");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

fn fixture_home(root: &std::path::Path) -> std::path::PathBuf {
    std::env::var_os("SHIELD_INTEGRATION_HOME").map_or_else(|| root.to_path_buf(), Into::into)
}
