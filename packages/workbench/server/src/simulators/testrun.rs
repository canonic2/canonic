//! A Simulator's screen size and input, delegated to the installed Testrun
//! command line (`testrun` on PATH, or in `~/.local/bin`). Input runs one
//! action at a time, in the order it arrived.

use crate::refused::refused;
use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// A Simulator's display, in its own pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Screen {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Where to look for the `testrun` command: each PATH folder, then
/// `~/.local/bin`, where editors opened from the Dock often lack PATH entries.
struct Search {
    path: Option<OsString>,
    home: Option<PathBuf>,
}

impl Search {
    fn locate(&self) -> Option<PathBuf> {
        let folders = self.path.iter().flat_map(std::env::split_paths);
        let local = self.home.iter().map(|home| home.join(".local/bin"));
        folders
            .chain(local)
            .map(|folder| folder.join("testrun"))
            .find(|candidate| executable(candidate))
    }
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// The project's Simulator input through Testrun, and the queue it waits in.
pub(crate) struct Testrun {
    root: PathBuf,
    search: Search,
    queue: tokio::sync::Mutex<()>,
}

impl Testrun {
    pub(crate) fn new(root: &Path) -> Self {
        Self::searching(
            root,
            std::env::var_os("PATH"),
            std::env::var_os("HOME").map(PathBuf::from),
        )
    }

    fn searching(root: &Path, path: Option<OsString>, home: Option<PathBuf>) -> Self {
        Self {
            root: root.to_owned(),
            search: Search { path, home },
            queue: tokio::sync::Mutex::new(()),
        }
    }

    /// The display size of the Simulator `udid`.
    ///
    /// # Errors
    /// When Testrun fails or reports no valid size.
    pub(crate) async fn screen(&self, udid: &str) -> Result<Screen> {
        let shot = self.invoke(udid, &["observe".to_owned()]).await?;
        let size = |key| {
            shot.get(key)
                .and_then(Value::as_f64)
                .filter(|size| *size > 0.0)
        };
        match (size("sourceWidth"), size("sourceHeight")) {
            (Some(width), Some(height)) => Ok(Screen { width, height }),
            _ => Err(anyhow!(
                "Testrun screenshot omitted valid display dimensions"
            )),
        }
    }

    /// Taps or swipes the Simulator `udid` at normalized coordinates, after
    /// any input before it. Answers with the action.
    ///
    /// # Errors
    /// 400 without a Simulator; invalid actions or coordinates and Testrun
    /// failures say why.
    pub(crate) async fn input(&self, udid: &str, request: &Value) -> Result<Value> {
        if udid.is_empty() {
            return Err(refused(400, "which Simulator?"));
        }
        let _turn = self.queue.lock().await;
        let args = input_args(request)?;
        self.invoke(udid, &args).await?;
        Ok(json!({ "action": request.get("action") }))
    }

    async fn invoke(&self, udid: &str, args: &[String]) -> Result<Value> {
        // Looked up on each call, so installing Testrun needs no Workbench restart.
        let Some(executable) = self.search.locate() else {
            return Err(anyhow!(
                "Testrun is not installed: no testrun command on PATH or in ~/.local/bin"
            ));
        };
        let target = format!("simulator:{udid}");
        let mut command = tokio::process::Command::new(&executable);
        command
            .args(args)
            .args(["--target", &target])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(TIMEOUT, command.output())
            .await
            .map_err(|_| anyhow!("Testrun didn’t answer within {}s", TIMEOUT.as_secs()))??;
        if output.stdout.len() > MAX_OUTPUT {
            return Err(anyhow!("stdout maxBuffer length exceeded"));
        }
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(anyhow!(if stderr.is_empty() {
                format!(
                    "Command failed: {} {} --target {target}",
                    executable.display(),
                    args.join(" ")
                )
            } else {
                stderr
            }));
        }
        match serde_json::from_slice::<Value>(&output.stdout) {
            Ok(value @ Value::Object(_)) => Ok(value),
            _ => Err(anyhow!("Testrun returned invalid JSON")),
        }
    }
}

/// Testrun's `act` arguments for a tap or swipe at coordinates between 0 and 1.
fn input_args(request: &Value) -> Result<Vec<String>> {
    let ratio = |key| {
        request
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| (0.0..=1.0).contains(value))
    };
    let action = match request.get("action").and_then(Value::as_str) {
        Some("tap") => match (ratio("x"), ratio("y")) {
            (Some(x), Some(y)) => Some(json!({"action":"tap","point":{"x":x,"y":y}})),
            _ => None,
        },
        Some("swipe") => match (ratio("fromX"), ratio("fromY"), ratio("toX"), ratio("toY")) {
            (Some(from_x), Some(from_y), Some(to_x), Some(to_y)) => {
                let seconds = loose_number(request.get("duration"))
                    .filter(|value| *value != 0.0)
                    .unwrap_or(0.2);
                let milliseconds = (seconds.clamp(0.05, 2.0) * 1000.0).round();
                let duration = Duration::try_from_secs_f64(milliseconds / 1000.0)?;
                let milliseconds = u64::try_from(duration.as_millis())?;
                Some(
                    json!({"action":"swipe","from":{"x":from_x,"y":from_y},"to":{"x":to_x,"y":to_y},"durationMs":milliseconds}),
                )
            }
            _ => None,
        },
        _ => None,
    };
    if let Some(action) = action {
        return Ok(vec![
            "act".into(),
            action.to_string(),
            "--final-only".into(),
        ]);
    }
    Err(anyhow!(
        "Invalid simulator action or normalized coordinates"
    ))
}

/// A duration as JavaScript's `Number()` reads it: numbers, numeric text and
/// booleans; anything else is no duration.
fn loose_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) if text.trim().is_empty() => Some(0.0),
        Value::String(text) => text.trim().parse().ok(),
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        _ => None,
    }
    .filter(|number: &f64| number.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refused::status_of;

    /// A project folder that is also a home whose `~/.local/bin/testrun` is a
    /// Node script recording its arguments.
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "testrun-bridge-acme's-{name}-{}",
            std::process::id()
        ));
        let _gone = std::fs::remove_dir_all(&root);
        let binary = root.join(".local/bin/testrun");
        std::fs::create_dir_all(binary.parent().expect("folder")).expect("folders");
        let node = std::process::Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("node");
        let node = String::from_utf8_lossy(&node.stdout).trim().to_owned();
        std::fs::write(
            &binary,
            format!(
                "#!{node}\nconst fs = require('node:fs');\nfs.writeFileSync('invocation.json', JSON.stringify(process.argv.slice(2)));\nif (process.argv.some((arg) => arg.includes('\"swipe\"'))) {{ console.error('SESSION_UNCERTAIN'); process.exit(1); }}\nconsole.log(JSON.stringify({{sourceWidth:1206,sourceHeight:2622}}));\n"
            ),
        )
        .expect("script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        root
    }

    #[tokio::test]
    async fn delegates_literal_target_arguments_and_reports_invalid_input_and_failures() {
        let root = project("delegates");
        let testrun = Testrun::searching(&root, None, Some(root.clone()));
        let identity = "Acme $(touch injected)";
        assert_eq!(
            testrun.screen(identity).await.expect("screen"),
            Screen {
                width: 1206.0,
                height: 2622.0
            }
        );
        let read = || {
            serde_json::from_str::<Vec<String>>(
                &std::fs::read_to_string(root.join("invocation.json")).expect("invocation"),
            )
            .expect("args")
        };
        let target = format!("simulator:{identity}");
        assert_eq!(read(), ["observe", "--target", &target]);
        assert!(!root.join("injected").exists());

        let tapped = testrun
            .input(identity, &json!({ "action": "tap", "x": 0.5, "y": 0.2 }))
            .await
            .expect("tap");
        assert_eq!(tapped, json!({ "action": "tap" }));
        let tap = read();
        assert_eq!(
            tap,
            [
                "act",
                r#"{"action":"tap","point":{"x":0.5,"y":0.2}}"#,
                "--final-only",
                "--target",
                &target
            ]
        );
        let invalid = testrun
            .input(identity, &json!({ "action": "tap", "x": 2, "y": 0.2 }))
            .await
            .expect_err("out of range");
        assert!(invalid.to_string().contains("Invalid simulator"));
        assert_eq!(read(), tap, "invalid input runs no process");

        let swipe = json!({ "action": "swipe", "fromX": 0.1, "fromY": 0.1, "toX": 0.5, "toY": 0.5, "duration": 0.3 });
        let failed = testrun
            .input(identity, &swipe)
            .await
            .expect_err("Testrun failed");
        assert!(failed.to_string().contains("SESSION_UNCERTAIN"));
        let swiped: Value = serde_json::from_str(&read()[1]).expect("action");
        assert_eq!(swiped["durationMs"], 300);

        let nobody = testrun
            .input("", &json!({ "action": "tap", "x": 0.5, "y": 0.5 }))
            .await
            .expect_err("no udid");
        assert_eq!(status_of(&nobody, 500), 400);
        let _gone = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn says_when_testrun_is_not_installed() {
        let nowhere = Path::new("/acme/nowhere");
        let testrun = Testrun::searching(
            nowhere,
            Some(OsString::from("/acme/nowhere/bin")),
            Some(nowhere.to_owned()),
        );
        let error = testrun.screen("SIM-1").await.expect_err("missing");
        assert_eq!(
            error.to_string(),
            "Testrun is not installed: no testrun command on PATH or in ~/.local/bin"
        );
    }

    #[test]
    fn finds_testrun_on_path_before_the_local_bin_folder() {
        let home = project("search");
        let local = home.join(".local/bin/testrun");
        let on_path = home.join("path bin");
        std::fs::create_dir_all(&on_path).expect("folder");
        std::fs::copy(&local, on_path.join("testrun")).expect("copy");
        let search = |path: &Path| Search {
            path: Some(std::env::join_paths([path]).expect("PATH")),
            home: Some(home.clone()),
        };
        assert_eq!(search(&on_path).locate(), Some(on_path.join("testrun")));
        assert_eq!(search(&home.join("empty")).locate(), Some(local.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o644))
                .expect("not executable");
            assert_eq!(search(&home.join("empty")).locate(), None);
        }
        let _gone = std::fs::remove_dir_all(home);
    }

    #[test]
    fn swipe_durations_are_clamped_and_default_like_javascript() {
        let action = |duration: Value| -> Value {
            let args = input_args(&json!({ "action": "swipe", "fromX": 0, "fromY": 0, "toX": 1, "toY": 1, "duration": duration })).expect("swipe");
            serde_json::from_str(&args[1]).expect("action")
        };
        assert_eq!(action(Value::Null)["durationMs"], 200);
        assert_eq!(action(json!(9))["durationMs"], 2000);
        assert_eq!(action(json!(0.01))["durationMs"], 50);
        assert_eq!(action(json!("0.3"))["durationMs"], 300);
        assert_eq!(action(Value::Null)["from"], json!({ "x": 0.0, "y": 0.0 }));
        assert_eq!(action(Value::Null)["to"], json!({ "x": 1.0, "y": 1.0 }));
    }
}
