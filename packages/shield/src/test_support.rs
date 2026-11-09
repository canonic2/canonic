//! Native contexts and owned host fixtures shared by Shield tests.
use std::{borrow::Cow, path::Path};

pub(crate) fn absolute(path: &str) -> Cow<'_, str> {
    if cfg!(windows) && path.starts_with('/') && !Path::new(path).is_absolute() {
        Cow::Owned(format!("C:{path}"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Run a home-sensitive test in an owned child without changing process-global
/// environment while other test/runtime threads may be active. The caller
/// returns when false; only the exact child test runs its assertions.
pub(crate) fn isolated_home(test: &str) -> bool {
    if std::env::var("SHIELD_ISOLATED_HOME_TEST").as_deref() == Ok(test) {
        let home = std::env::var_os("SHIELD_ISOLATED_HOME").expect("owned test home");
        assert_eq!(
            std::env::home_dir().unwrap(),
            std::path::PathBuf::from(home)
        );
        return true;
    }
    let home = assert_fs::TempDir::new().unwrap();
    let name = test.split_once("::").expect("crate-qualified test name").1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SHIELD_ISOLATED_HOME_TEST", test)
        .env("SHIELD_ISOLATED_HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "isolated fixture did not execute: {name}"
    );
    false
}

/// A real Git with test-owned configuration and no detached maintenance.
pub(crate) fn git() -> std::process::Command {
    #[cfg(windows)]
    let executable = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .filter(|path| path.is_absolute())
        .filter_map(|path| path.join("git.exe").canonicalize().ok())
        .find(|path| path.is_file())
        .expect("Git for Windows is installed");
    #[cfg(not(windows))]
    let executable = Path::new("/usr/bin/git");
    let mut command = std::process::Command::new(executable);
    command
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"]);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
}

/// Create a real Windows junction without requiring symbolic-link privileges.
#[cfg(windows)]
pub(crate) fn junction(path: &Path, target: &Path) -> std::io::Result<()> {
    use base64::Engine;
    use std::{
        io,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let quote = |path: &Path| {
        path.to_str()
            .map(|text| text.replace('\'', "''"))
            .ok_or_else(|| io::Error::other("fixture path is not Unicode"))
    };
    let source = format!(
        "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        quote(path)?,
        quote(target)?
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        source
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let shell = std::path::PathBuf::from(
        winsafe::GetSystemDirectory().map_err(|error| io::Error::other(error.to_string()))?,
    )
    .join("WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(shell)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(status)) => {
                break Err(io::Error::other(format!(
                    "junction fixture failed: {status}"
                )));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "junction fixture deadline",
                ));
            }
            Err(error) => break Err(error),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
