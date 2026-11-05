//! Native CLI enrollment consent and console cleanup in an owned `ConPTY`.
pub(super) mod prompts;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    fs,
    io::{self, Read, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const FIXTURE: &str = "windows::console_fixture";
const CONTROL_BREAK: &[u8] = b"SHIELD_FIXTURE_CTRL_BREAK";

fn modes() -> (u32, u32) {
    (
        winapi_util::console::mode(io::stdin()).expect("fixture console input mode"),
        winapi_util::console::mode(io::stdout()).expect("fixture console output mode"),
    )
}

#[test]
#[ignore = "owned child of windows_enrollment_requires_consent_and_restores_console"]
#[expect(
    clippy::too_many_lines,
    reason = "the isolated fixture keeps Job ownership, native control delivery and exact restoration checks in one process lifetime"
)]
fn console_fixture() {
    // The fixture owns every CLI descendant even if the parent test times out.
    static JOB: Mutex<Option<win32job::Job>> = Mutex::new(None);
    let mut limits = win32job::ExtendedLimitInfo::new();
    limits.limit_kill_on_job_close();
    let job = win32job::Job::create_with_limit_info(&limits).unwrap();
    job.assign_current_process().unwrap();
    *JOB.lock().unwrap() = Some(job);
    let root = std::env::var_os("SHIELD_WINDOWS_CONSOLE_ROOT").unwrap();
    let root = Path::new(&root);
    let original = modes();
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("SHIELD_WINDOWS_CONSOLE_ARGS").unwrap()).unwrap();
    let control_break = args.first().is_some_and(|argument| {
        ["__break-probe", "__run-break-probe"].contains(&argument.as_str())
    });
    let automatic_break = args
        .first()
        .is_some_and(|argument| argument == "__run-break-probe");
    let operator = args
        .first()
        .is_some_and(|argument| argument == "__operator-probe");
    let release = args
        .first()
        .is_some_and(|argument| argument == "__release-probe");
    let cli_args = if release {
        &args[2..]
    } else if control_break || operator {
        &args[1..]
    } else {
        &args[..]
    };
    let mut command = if args
        .first()
        .is_some_and(|argument| argument == "__powershell-probe")
    {
        let mut command = Command::new("node");
        command
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/windows-powershell-argv.mjs"
            ))
            .arg(root)
            .arg(
                std::path::PathBuf::from(winsafe::GetSystemDirectory().unwrap())
                    .join("WindowsPowerShell/v1.0/powershell.exe"),
            );
        command
    } else if args
        .first()
        .is_some_and(|argument| argument == "__agent-probe")
    {
        let mut command = Command::new("node");
        command
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/windows_agent.mjs"
            ))
            .arg(root)
            .arg(&args[1])
            .arg(std::env::var_os("SHIELD_WINDOWS_AGENTS_ROOT").unwrap())
            .arg(assert_cmd::cargo::cargo_bin!("shield"));
        command
    } else {
        let executable = if release {
            Path::new(&args[1])
        } else {
            Path::new(assert_cmd::cargo::cargo_bin!("shield"))
        };
        let mut command = Command::new(executable);
        let separator = cli_args
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(cli_args.len());
        command
            .args(&cli_args[..separator])
            .arg("--no-debug-log")
            .arg("--project")
            .arg(root)
            .args(&cli_args[separator..]);
        if cli_args.first().is_some_and(|argument| argument == "init") {
            command.arg("--cache-dir").arg(root.join(".shield/cache"));
        }
        command
    };
    if operator {
        command.stderr(fs::File::create(root.join("console-operator.stderr")).unwrap());
    }
    let status = if control_break {
        command.creation_flags(0x0000_0200); // New process group within this owned console.
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut sent = false;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "console-control fixture deadline"
            );
            let launched = automatic_break
                && JOB
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .query_process_id_list()
                    .unwrap()
                    .iter()
                    .any(|pid| {
                        *pid != usize::try_from(std::process::id()).unwrap()
                            && *pid != usize::try_from(child.id()).unwrap()
                    });
            if !sent && (root.join("console-break-request").exists() || launched) {
                let shell = std::path::PathBuf::from(winsafe::GetSystemDirectory().unwrap())
                    .join("WindowsPowerShell/v1.0/powershell.exe");
                let signal = Command::new(shell)
                    .args(["-NoProfile", "-NonInteractive", "-File"])
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/windows-control-break.ps1"
                    ))
                    .args(["-ProcessId", &child.id().to_string()])
                    .creation_flags(0x0800_0000) // Attach only after PowerShell startup; do not alter console modes.
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
                    .unwrap();
                assert!(
                    signal.status.success(),
                    "owned console-control helper failed ({}): {}",
                    signal.status,
                    String::from_utf8_lossy(&signal.stderr)
                );
                sent = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        command.status().unwrap()
    };
    if args
        .first()
        .is_none_or(|argument| argument != "__agent-probe")
    {
        assert_eq!(
            modes(),
            original,
            "Shield must restore its inherited native modes"
        );
    }
    let expected = std::env::var("SHIELD_WINDOWS_CONSOLE_EXIT")
        .unwrap()
        .parse::<i32>()
        .unwrap();
    if operator {
        fs::write(
            root.join("console-operator.code"),
            status.code().unwrap().to_string(),
        )
        .unwrap();
    } else {
        assert_eq!(status.code(), Some(expected));
    }
    fs::write(root.join("console-result"), "restored").unwrap();
}

struct Console {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<JoinHandle<()>>,
}
impl Console {
    fn start(root: &Path, args: &[&str], expected: i32) -> (Self, Arc<Mutex<Vec<u8>>>) {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 32,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("create owned ConPTY");
        let mut command = CommandBuilder::new(std::env::current_exe().expect("fixture executable"));
        command.args([
            "--exact",
            FIXTURE,
            "--ignored",
            "--nocapture",
            "--color",
            "never",
        ]);
        command.env("SHIELD_WINDOWS_CONSOLE_ROOT", root);
        command.env(
            "SHIELD_WINDOWS_CONSOLE_ARGS",
            serde_json::to_string(args).expect("fixture arguments"),
        );
        command.env("SHIELD_WINDOWS_CONSOLE_EXIT", expected.to_string());
        command.env("USERPROFILE", root);
        command.env("HOME", root);
        command.env("NO_COLOR", "1");
        command.env_remove("TERM");
        let mut reader = pair.master.try_clone_reader().expect("console reader");
        let writer = pair.master.take_writer().expect("console writer");
        let child = pair
            .slave
            .spawn_command(command)
            .expect("spawn console fixture");
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let captured = output.clone();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut bytes = captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                bytes.extend_from_slice(&buffer[..count]);
                assert!(bytes.len() <= 1_048_576, "bounded console capture");
            }
        });
        (
            Self {
                child,
                master: Some(pair.master),
                writer: Some(writer),
                reader: Some(reader),
            },
            output,
        )
    }

    fn send(&mut self, keys: &[u8]) {
        let writer = self.writer.as_mut().expect("retained console writer");
        writer.write_all(keys).expect("send console input");
        writer.flush().expect("flush console input");
    }

    fn finish_capture(&mut self) {
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.finish_capture();
    }
}

pub(super) fn enroll(root: &Path, cancel: bool) {
    run(
        root,
        &["init", "--no-setup", "--no-baseline"],
        &[("Review and confirm", if cancel { b"\x03" } else { b"2\r" })],
        if cancel { 130 } else { 0 },
    );
}

pub(super) fn run(root: &Path, args: &[&str], steps: &[(&str, &[u8])], expected: i32) {
    let (mut console, output) = Console::start(root, args, expected);
    let deadline = Instant::now()
        + Duration::from_secs(if args.first() == Some(&"__agent-probe") {
            60
        } else {
            20
        });
    let mut answered_queries = 0;
    let mut step = 0;
    let mut frame_start = 0;
    loop {
        if let Some(status) = console.child.try_wait().expect("poll console fixture") {
            console.finish_capture();
            let detail =
                String::from_utf8_lossy(&output.lock().expect("captured output")).into_owned();
            assert!(status.success(), "fixture exit {status:?}: {detail}");
            break;
        }
        let bytes = output.lock().expect("captured output").clone();
        assert!(
            Instant::now() < deadline,
            "owned Shield console timed out: {}",
            String::from_utf8_lossy(&bytes)
        );
        let queries = bytes
            .windows(4)
            .filter(|bytes| *bytes == b"\x1b[6n")
            .count();
        assert!(queries <= 16, "unexpected cursor query loop");
        while answered_queries < queries {
            console
                .writer
                .as_mut()
                .expect("retained console writer")
                .write_all(b"\x1b[1;1R")
                .expect("answer cursor query");
            console
                .writer
                .as_mut()
                .expect("retained console writer")
                .flush()
                .expect("flush cursor answer");
            answered_queries += 1;
        }
        if let Some((title, keys)) = steps.get(step)
            && bytes
                .get(frame_start..)
                .unwrap_or_default()
                .windows(title.len())
                .any(|bytes| bytes == title.as_bytes())
        {
            if args.first() == Some(&"init")
                || (args.first() == Some(&"__release-probe") && args.get(2) == Some(&"init"))
            {
                assert!(
                    !root.join(".shield/review/state.json").exists(),
                    "no enrollment before explicit consent"
                );
            }
            if *keys == CONTROL_BREAK {
                fs::write(root.join("console-break-request"), "break")
                    .expect("request console break");
            } else {
                console
                    .writer
                    .as_mut()
                    .expect("retained console writer")
                    .write_all(keys)
                    .expect("send prompt response");
                console
                    .writer
                    .as_mut()
                    .expect("retained console writer")
                    .flush()
                    .expect("flush prompt response");
            }
            step += 1;
            frame_start = bytes.len();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        step,
        steps.len(),
        "the actual CLI must present each reviewed step"
    );
    assert_eq!(
        fs::read_to_string(root.join("console-result")).expect("console restoration result"),
        "restored"
    );
}

#[test]
#[ignore = "explicit release acceptance requires SHIELD_WINDOWS_RELEASE_BINARY"]
fn windows_release_binary_runs_from_owned_installation() {
    let source = std::env::var_os("SHIELD_WINDOWS_RELEASE_BINARY").expect("release candidate path");
    let source = Path::new(&source);
    assert!(source.is_absolute() && source.is_file());
    let installation = assert_fs::TempDir::new().unwrap();
    let prefix = installation.path().join("Acme Programs/bin");
    fs::create_dir_all(&prefix).unwrap();
    let installed = prefix.join("shield.exe");
    fs::copy(source, &installed).unwrap();
    let version = Command::new(&installed).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("shield {}", env!("CARGO_PKG_VERSION"))
    );
    let help = Command::new(&installed).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout).unwrap().contains("Usage:"));
    let project = assert_fs::TempDir::new().unwrap();
    run(
        project.path(),
        &[
            "__release-probe",
            installed.to_str().unwrap(),
            "init",
            "--no-setup",
            "--no-baseline",
        ],
        &[("Review and confirm", b"2\r")],
        0,
    );
    let state = project.path().join(".shield/review/state.json");
    let before = fs::read(&state).unwrap();
    // Replace only after the running CLI and its owned console have exited.
    fs::copy(source, &installed).unwrap();
    let status = Command::new(&installed)
        .args(["--no-debug-log", "status", "--json", "--project"])
        .arg(project.path())
        .env("USERPROFILE", project.path())
        .env("HOME", project.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(status["active"].is_object());
    fs::remove_file(&installed).unwrap();
    assert!(!installed.exists());
    assert_eq!(fs::read(state).unwrap(), before);
}

#[test]
fn windows_rule_editor_preserves_json_altgr_and_unicode_text() {
    let root = assert_fs::TempDir::new().unwrap();
    let initialized = super::operator::interactive(&root, &["init"], "yes\n");
    assert_eq!(initialized["code"], 0, "{initialized}");
    let arguments = serde_json::json!(["[Acme]", "{scope}", "@acme", "€", "Ω"]);
    let reason = "Inspect Acme € Ω";
    let answers = format!("native-json\nargv\nacme\n{arguments}\nallow\n{reason}\n\nyes\n");
    let changed = super::operator::interactive(&root, &["rule", "add"], &answers);
    assert_eq!(changed["code"], 0, "{changed}");
    let rules = super::operator::query(&root, &["rule", "list"]);
    assert_eq!(
        rules["rules"][0]["conditions"]["arguments"]["values"],
        arguments
    );
    assert_eq!(rules["rules"][0]["reason"], reason);
}

#[test]
fn windows_powershell_native_argument_contract_matches_literal_preview() {
    let root = assert_fs::TempDir::new().unwrap();
    run(root.path(), &["__powershell-probe"], &[], 0);
    assert_eq!(
        fs::read_to_string(root.path().join("powershell-argv-result")).unwrap(),
        "verified"
    );
}

#[test]
fn windows_agent_setup_repair_and_removal_preserve_policy_and_other_settings() {
    let root = assert_fs::TempDir::new().unwrap();
    enroll(root.path(), false);
    let state_path = root.path().join(".shield/review/state.json");
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    fs::create_dir(root.path().join(".claude")).unwrap();
    fs::write(
        root.path().join(".claude/settings.local.json"),
        r#"{"env":{"ACME_SETTING":"kept"}}"#,
    )
    .unwrap();
    for agent in ["claude", "codex"] {
        for _ in 0..2 {
            run(
                root.path(),
                &["mcp", "setup", "--agent", agent],
                &[("Tool permissions", b"\r"), ("Apply MCP setup?", b"2\r")],
                0,
            );
        }
    }
    let settings_path = root.path().join(".claude/settings.local.json");
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    let hook = &settings["hooks"]["PreToolUse"][0]["hooks"][0];
    assert_eq!(hook["onFailure"], "block");
    assert!(
        hook["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument == "native-hook")
    );
    for agent in ["claude", "codex"] {
        run(
            root.path(),
            &["mcp", "setup", "--remove", "--agent", agent],
            &[("Apply MCP setup?", b"2\r")],
            0,
        );
    }
    let remaining: serde_json::Value =
        serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
    assert_eq!(
        remaining,
        serde_json::json!({"env":{"ACME_SETTING":"kept"}})
    );
    let after: serde_json::Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(after["policy_revision"], before["policy_revision"]);
    assert_eq!(after["active"], before["active"]);
}

#[test]
fn windows_console_break_cancels_enrollment_and_restores_modes() {
    let root = assert_fs::TempDir::new().unwrap();
    run(
        root.path(),
        &["__break-probe", "init", "--no-setup", "--no-baseline"],
        &[("Review and confirm", CONTROL_BREAK)],
        130,
    );
    assert!(!root.path().join(".shield/review/state.json").exists());
}

#[test]
fn windows_enrollment_requires_consent_and_restores_console() {
    let root = assert_fs::TempDir::new().unwrap();
    enroll(root.path(), true);
    assert!(!root.path().join(".shield/review/state.json").exists());
    enroll(root.path(), false);
    assert!(root.path().join(".shield/review/state.json").is_file());
    let status = Command::new(assert_cmd::cargo::cargo_bin!("shield"))
        .args(["status", "--json", "--no-debug-log", "--project"])
        .arg(root.path())
        .env("USERPROFILE", root.path())
        .env("HOME", root.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(report.is_object());
    assert!(!root.path().join(".claude").exists());
    assert!(!root.path().join(".codex").exists());
}

#[test]
#[ignore = "explicit native host acceptance requires pinned CLIs in SHIELD_WINDOWS_AGENTS_ROOT and Node 24"]
fn windows_agent_hosts_load_native_hooks_against_local_providers() {
    native_agent("claude");
}

#[test]
#[ignore = "explicit native host acceptance requires pinned CLIs in SHIELD_WINDOWS_AGENTS_ROOT and Node 24"]
fn windows_agent_hosts_codex_loads_native_hooks_against_local_provider() {
    native_agent("codex");
}

fn native_agent(agent: &str) {
    let root = assert_fs::TempDir::new().expect("owned native-agent project");
    fs::create_dir(root.path().join(".shield")).expect("agent fixture configuration directory");
    let mut rules: Vec<_> = [("allowed", "allow"), ("denied", "deny")].into_iter().map(|(name, decision)| {
            serde_json::json!({"id":name,"decision":decision,"conditions":{"domain":"file","action":"read","sources":{"all":{"exact":{"root":"absolute","path":root.path().join(format!("{name}.txt"))}}}}})
        }).collect();
    for (name, decision, value) in [
        ("allowed", "allow", "present"),
        ("denied", "deny", "forbidden"),
    ] {
        let arguments = serde_json::json!(["config", "--local", format!("acme.{name}"), value]);
        let mut rule = serde_json::json!({"id":format!("command-{name}"),"decision":decision,
                "conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":arguments}}});
        if decision == "allow" {
            rule["agents"] = serde_json::json!({"codex":{"prefixes":[["git","config","--local","acme.allowed","present"]]}});
        }
        rules.push(rule);
    }
    for (name, text) in [
        ("allowed", "ACME_ALLOWED_TEXT"),
        ("denied", "ACME_DENIED_TEXT"),
        ("asked", "ACME_ASKED_TEXT"),
    ] {
        fs::write(root.path().join(format!("{name}.txt")), text).expect("agent fixture input");
    }
    fs::write(
        root.path().join(".shield/config.yml"),
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
    )
    .expect("write agent fixture configuration");
    let rules = serde_json::json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","location-v1","argument-rules-v1","agent-outputs-v1"],"rules":rules}).to_string();
    shield::configuration::Candidate::from_yaml(
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
    )
    .expect("parse agent fixture configuration")
    .capture(&[], Some(&rules))
    .expect("native fixture rules must validate before enrollment");
    fs::write(root.path().join(".shield/rules.yml"), rules).expect("write agent fixture policy");
    enroll(root.path(), false);
    run(
        root.path(),
        &["mcp", "setup", "--agent", agent],
        &[("Tool permissions", b"\r"), ("Apply MCP setup?", b"2\r")],
        0,
    );
    run(root.path(), &["__agent-probe", agent], &[], 0);
    let result: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join("agent-probe.json")).expect("agent probe result"),
    )
    .expect("JSON agent probe result");
    assert_eq!(result["agent"], agent);
    eprintln!("native agent acceptance: {result}");
}
