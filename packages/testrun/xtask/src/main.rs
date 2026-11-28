//! Testrun's development tasks for local installation and installed acceptance.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  install [--prefix DIR] [-- CARGO_ARGS...]
      Install testrun into DIR/bin, default ~/.local/bin, ad-hoc signed on
      macOS, and the iOS runner sources, license and notices into
      DIR/share/testrun. CARGO_ARGS reach cargo install, such as --offline.
  stage-release BINARY PREFIX
      Copy a built executable and its resources into a new package directory.
  browser-session-checkpoint --installed PATH --cache DIR
      Run the installed 5.5 session checkpoint with synthetic local fixtures.
  browser-feature-checkpoint --installed PATH --cache DIR
      Run the installed 5.6 feature checkpoint with synthetic local fixtures.";

/// Notices installed under share/testrun/browser, by source in the package.
const NOTICES: [(&str, &str); 7] = [
    (
        "third-party/browser/upstream-notices/LICENSE",
        "PLAYWRIGHT-LICENSE",
    ),
    (
        "third-party/browser/upstream-notices/NOTICE",
        "PLAYWRIGHT-NOTICE",
    ),
    ("third-party/browser/cdp-notices/LICENSE", "CDP-LICENSE"),
    (
        "third-party/browser/developer-tool-notices/LICENSE",
        "DEVELOPER-TOOL-LICENSE",
    ),
    ("third-party/browser/react-notices/LICENSE", "REACT-LICENSE"),
    ("third-party/browser/mime-notices/LICENSE", "MIME-LICENSE"),
    (
        "third-party/browser/ecmascript-notices/LICENSE",
        "ECMASCRIPT-LICENSE",
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("install") => install(&args[1..]),
        Some("stage-release") => stage_release(&args[1..]),
        Some("browser-session-checkpoint") => {
            browser_checkpoint(&args[1..], "installed_browser_session_checkpoint")
        }
        Some("browser-feature-checkpoint") => {
            browser_checkpoint(&args[1..], "installed_browser_feature_checkpoint")
        }
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(format!("unknown or missing task\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// packages/testrun, whichever directory the task was started from.
fn package_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside its package")
        .to_path_buf()
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("cannot start {command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}

fn browser_checkpoint(args: &[String], filter: &str) -> Result<(), String> {
    if !cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        return Err("Installed browser checkpoints require an Intel macOS host".into());
    }
    let [installed_flag, installed, cache_flag, cache] = args else {
        return Err(USAGE.into());
    };
    if installed_flag != "--installed" || cache_flag != "--cache" {
        return Err(USAGE.into());
    }
    let installed = Path::new(installed);
    let cache = Path::new(cache);
    if !installed.is_absolute() || !installed.is_file() {
        return Err("Select an absolute installed Testrun binary".into());
    }
    if !cache.is_absolute() || !cache.is_dir() {
        return Err("Select an absolute private managed browser cache".into());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    run(Command::new(cargo)
        .current_dir(package_root())
        .env("TESTRUN_TEST_INSTALLED_BINARY", installed)
        .env("TESTRUN_BROWSER_ROOT", cache)
        .args([
            "test",
            "--locked",
            "--offline",
            "--features",
            "acceptance-browser-public",
            "--test",
            "integration",
            filter,
            "--",
            "--test-threads=1",
            "--nocapture",
        ]))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(drop).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            from.display(),
            to.display()
        )
    })
}

/// Copies a directory tree, preserving file permissions.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to)
        .map_err(|error| format!("cannot create {}: {error}", to.display()))?;
    let entries = std::fs::read_dir(from)
        .map_err(|error| format!("cannot read {}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {}: {error}", from.display()))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), &target)?;
        } else {
            return Err(format!(
                "{} is neither a file nor a folder",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn install(args: &[String]) -> Result<(), String> {
    let mut prefix = None;
    let mut extra: Vec<OsString> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--prefix" => {
                prefix = Some(PathBuf::from(
                    args.next().ok_or("--prefix needs a directory")?,
                ));
            }
            "--" => extra.extend(args.by_ref().map(OsString::from)),
            other => return Err(format!("unknown install option {other}\n\n{USAGE}")),
        }
    }
    let prefix = match prefix {
        Some(prefix) => prefix,
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(".local"),
    };
    let root = package_root();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let target = std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into());
    run(Command::new(cargo)
        .current_dir(&root)
        .args(["install", "--path", ".", "--bin", "testrun"])
        .arg("--target-dir")
        .arg(target)
        .args(["--locked", "--force", "--root"])
        .arg(&prefix)
        .args(&extra))?;
    if cfg!(target_os = "macos") {
        run(Command::new("/usr/bin/codesign")
            .args(["--sign", "-", "--force"])
            .arg(prefix.join("bin/testrun")))?;
    }
    let runner = prefix.join("share/testrun/runner/ios");
    if runner.exists() {
        std::fs::remove_dir_all(&runner)
            .map_err(|error| format!("cannot replace {}: {error}", runner.display()))?;
    }
    install_resources(&root, &prefix)
}

fn stage_release(args: &[String]) -> Result<(), String> {
    let [binary, prefix] = args else {
        return Err(USAGE.into());
    };
    let prefix = Path::new(prefix);
    let bin = prefix.join("bin");
    std::fs::create_dir(prefix)
        .and_then(|()| std::fs::create_dir(&bin))
        .map_err(|error| {
            format!(
                "cannot create release directory {}: {error}",
                prefix.display()
            )
        })?;
    copy_file(
        Path::new(binary),
        &bin.join(format!("testrun{}", std::env::consts::EXE_SUFFIX)),
    )?;
    install_resources(&package_root(), prefix)
}

fn install_resources(root: &Path, prefix: &Path) -> Result<(), String> {
    let share = prefix.join("share/testrun");
    copy_tree(&root.join("runner/ios"), &share.join("runner/ios"))?;
    copy_file(&root.join("LICENSE"), &share.join("LICENSE"))?;
    let notices = share.join("browser");
    std::fs::create_dir_all(&notices)
        .map_err(|error| format!("cannot create {}: {error}", notices.display()))?;
    for (source, name) in NOTICES {
        copy_file(&root.join(source), &notices.join(name))?;
    }
    Ok(())
}
