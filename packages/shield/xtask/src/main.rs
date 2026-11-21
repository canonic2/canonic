//! Shield's development tasks beyond single Cargo commands: installing the
//! binary, and the Node adapter, native-host and history-volume runs that
//! need a fresh build or test settings in their environment. Run them as
//! `cargo xtask <task>` from packages/shield.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  install [--prefix DIR] [-- CARGO_ARGS...]
      Install shield into DIR/bin, default ~/.local/bin, ad-hoc signed on
      macOS. CARGO_ARGS reach cargo install: --profile diagnostic installs a
      build with debug symbols and automatic diagnostic logging.
  adapter-test --file FILE --filter PATTERN
      Build target/debug/shield and run one named Node adapter or SDK test
      against it.
  native-preflight --host copilot|opencode
      Inspect one isolated installed native agent host.
  native-acceptance --host copilot|opencode [--binary PATH] [--filter PATTERN]
      Run the no-cost generated native callbacks on this Mac against a fresh
      debug build, or against the retained executable PATH without compiling.
      Needs loopback, PTY and process-group access.
  history-volume [--count N] [--report DIR] [-- NEXTEST_ARGS...]
      Run the ignored history-volume campaign: 500 calibrates, the default
      500000 measures volume. Reports go to DIR, default labs/history-volume.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("install") => install(rest),
        Some("adapter-test") => adapter_test(rest),
        Some("native-preflight") => native_preflight(rest),
        Some("native-acceptance") => native_acceptance(rest),
        Some("history-volume") => history_volume(rest),
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

/// packages/shield, whichever directory the task was started from.
fn package_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside its package")
        .to_path_buf()
}

fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(package_root());
    command
}

fn node() -> Command {
    let mut command = Command::new("node");
    command
        .current_dir(package_root())
        .args(["--test", "--test-concurrency=1"]);
    command
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

/// Options of the form `--name value`, plus everything after `--`.
struct Options {
    values: Vec<(String, String)>,
    rest: Vec<OsString>,
}

impl Options {
    fn parse(args: &[String], names: &[&str]) -> Result<Self, String> {
        let mut values = Vec::new();
        let mut rest = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg == "--" {
                rest.extend(args.by_ref().map(OsString::from));
            } else if let Some(name) = arg.strip_prefix("--").filter(|name| names.contains(name)) {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                if value.starts_with('-') {
                    return Err(format!("--{name} needs a value, not the flag {value}"));
                }
                values.push((name.to_owned(), value.clone()));
            } else {
                return Err(format!("unknown option {arg}\n\n{USAGE}"));
            }
        }
        Ok(Self { values, rest })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn require(&self, name: &str) -> Result<&str, String> {
        self.get(name)
            .ok_or_else(|| format!("--{name} is required\n\n{USAGE}"))
    }
}

fn install(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args, &["prefix"])?;
    let prefix = match options.get("prefix") {
        Some(prefix) => PathBuf::from(prefix),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(".local"),
    };
    run(cargo()
        .args(["install", "--path", ".", "--bin", "shield"])
        .args(["--target-dir", "target", "--locked", "--force", "--root"])
        .arg(&prefix)
        .args(&options.rest))?;
    if cfg!(target_os = "macos") {
        run(Command::new("/usr/bin/codesign")
            .args(["--sign", "-", "--force"])
            .arg(prefix.join("bin").join("shield")))?;
    }
    Ok(())
}

/// Builds target/debug/shield and returns its absolute path.
fn debug_binary() -> Result<PathBuf, String> {
    run(cargo().args(["build", "--locked"]))?;
    Ok(package_root().join("target/debug/shield"))
}

fn adapter_test(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args, &["file", "filter"])?;
    let file = options.require("file")?;
    let filter = options.require("filter")?;
    let binary = debug_binary()?;
    run(node()
        .env("SHIELD_TEST_BINARY", binary)
        .arg(format!("--test-name-pattern={filter}"))
        .arg(file))
}

fn native_host(options: &Options) -> Result<&str, String> {
    match options.require("host")? {
        host @ ("copilot" | "opencode") => Ok(host),
        other => Err(format!("--host must be copilot or opencode, not {other}")),
    }
}

const NATIVE_TESTS: &str = "adapters/sdk-tests/generated-native.test.mjs";

fn native_preflight(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args, &["host"])?;
    run(node()
        .env("SHIELD_NATIVE_ACCEPTANCE_HOST", native_host(&options)?)
        .arg("--test-name-pattern=native acceptance preflight")
        .arg(NATIVE_TESTS))
}

fn native_acceptance(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args, &["host", "binary", "filter"])?;
    let host = native_host(&options)?;
    let binary = match options.get("binary") {
        Some(binary) if Path::new(binary).is_absolute() => PathBuf::from(binary),
        Some(binary) => return Err(format!("--binary must be an absolute path, not {binary}")),
        None => debug_binary()?,
    };
    let filter = options
        .get("filter")
        .unwrap_or("generated native callback acceptance");
    run(node()
        .env("SHIELD_TEST_BINARY", binary)
        .env("SHIELD_NATIVE_ACCEPTANCE_HOST", host)
        .arg(format!("--test-name-pattern={filter}"))
        .arg(NATIVE_TESTS))
}

fn history_volume(args: &[String]) -> Result<(), String> {
    let options = Options::parse(args, &["count", "report"])?;
    let count = options.get("count").unwrap_or("500000");
    count
        .parse::<u64>()
        .map_err(|_| format!("--count needs a whole number, not {count}"))?;
    let report = options
        .get("report")
        .map_or_else(|| package_root().join("labs/history-volume"), PathBuf::from);
    run(cargo()
        .env("SHIELD_HISTORY_VOLUME_COUNT", count)
        .env("SHIELD_HISTORY_VOLUME_REPORT_DIR", report)
        .args([
            "nextest",
            "run",
            "--locked",
            "--lib",
            "--profile",
            "history-volume",
        ])
        .args(["--run-ignored", "only"])
        .args(&options.rest)
        .arg("history_volume_campaign"))
}
