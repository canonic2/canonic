//! A Simulator's screen size and input, delegated to the project's Sandbox
//! command line (`.canonic/sandbox/bin/sandbox`). Input runs one action at a
//! time, in the order it arrived.

use crate::refused::refused;
use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
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

/// The project's Sandbox, and the queue its input waits in.
pub(crate) struct Sandbox {
    root: PathBuf,
    executable: PathBuf,
    queue: tokio::sync::Mutex<()>,
}

impl Sandbox {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            executable: root.join(".canonic/sandbox/bin/sandbox"),
            queue: tokio::sync::Mutex::new(()),
        }
    }

    /// The display size of the Simulator `udid`.
    ///
    /// # Errors
    /// When Sandbox fails or reports no valid size.
    pub(crate) async fn screen(&self, udid: &str) -> Result<Screen> {
        let shot = self.invoke(udid, &["screenshot".to_owned()]).await?;
        let size = |key| {
            shot.get(key)
                .and_then(Value::as_f64)
                .filter(|size| *size > 0.0)
        };
        match (size("sourceWidth"), size("sourceHeight")) {
            (Some(width), Some(height)) => Ok(Screen { width, height }),
            _ => Err(anyhow!(
                "Sandbox screenshot omitted valid display dimensions"
            )),
        }
    }

    /// Taps or swipes the Simulator `udid` at normalized coordinates, after
    /// any input before it. Answers with the action.
    ///
    /// # Errors
    /// 400 without a Simulator; invalid actions or coordinates and Sandbox
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
        if !self.executable.exists() {
            return Err(anyhow!(
                "Sandbox is not installed at .canonic/sandbox/bin/sandbox"
            ));
        }
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .args(["ios", "--id", &format!("simulator:{udid}")])
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(TIMEOUT, command.output())
            .await
            .map_err(|_| anyhow!("Sandbox didn’t answer within {}s", TIMEOUT.as_secs()))??;
        if output.stdout.len() > MAX_OUTPUT {
            return Err(anyhow!("stdout maxBuffer length exceeded"));
        }
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(anyhow!(if stderr.is_empty() {
                format!(
                    "Command failed: {} ios --id simulator:{udid} {}",
                    self.executable.display(),
                    args.join(" ")
                )
            } else {
                stderr
            }));
        }
        match serde_json::from_slice::<Value>(&output.stdout) {
            Ok(value @ Value::Object(_)) => Ok(value),
            _ => Err(anyhow!("Sandbox returned invalid JSON")),
        }
    }
}

/// Sandbox's arguments for a tap or swipe at coordinates between 0 and 1.
fn input_args(request: &Value) -> Result<Vec<String>> {
    let ratio = |key| {
        request
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| (0.0..=1.0).contains(value))
    };
    match request.get("action").and_then(Value::as_str) {
        Some("tap") => {
            if let (Some(x), Some(y)) = (ratio("x"), ratio("y")) {
                return Ok(vec!["tap".into(), number(x), number(y)]);
            }
        }
        Some("swipe") => {
            if let (Some(from_x), Some(from_y), Some(to_x), Some(to_y)) =
                (ratio("fromX"), ratio("fromY"), ratio("toX"), ratio("toY"))
            {
                let seconds = loose_number(request.get("duration"))
                    .filter(|value| *value != 0.0)
                    .unwrap_or(0.2);
                let milliseconds = (seconds.clamp(0.05, 2.0) * 1000.0).round();
                return Ok(vec![
                    "swipe".into(),
                    number(from_x),
                    number(from_y),
                    number(to_x),
                    number(to_y),
                    "--duration-ms".into(),
                    number(milliseconds),
                ]);
            }
        }
        _ => {}
    }
    Err(anyhow!(
        "Invalid simulator action or normalized coordinates"
    ))
}

/// A number as JavaScript's `String()` writes it for these ranges.
fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
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

    /// A project whose Sandbox is a Node script that records its arguments.
    fn project() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("sandbox-bridge-acme's-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&root);
        let binary = root.join(".canonic/sandbox/bin/sandbox");
        std::fs::create_dir_all(binary.parent().expect("folder")).expect("folders");
        let node = std::process::Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("node");
        let node = String::from_utf8_lossy(&node.stdout).trim().to_owned();
        std::fs::write(
            &binary,
            format!(
                "#!{node}\nconst fs = require('node:fs');\nfs.writeFileSync('invocation.json', JSON.stringify(process.argv.slice(2)));\nif (process.argv.includes('swipe')) {{ console.error('SESSION_UNCERTAIN'); process.exit(1); }}\nconsole.log(JSON.stringify({{sourceWidth:1206,sourceHeight:2622}}));\n"
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
        let root = project();
        let sandbox = Sandbox::new(&root);
        let identity = "Acme $(touch injected)";
        assert_eq!(
            sandbox.screen(identity).await.expect("screen"),
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
        assert_eq!(
            read(),
            [
                "ios",
                "--id",
                &format!("simulator:{identity}"),
                "screenshot"
            ]
        );
        assert!(!root.join("injected").exists());

        let tapped = sandbox
            .input(identity, &json!({ "action": "tap", "x": 0.5, "y": 0.2 }))
            .await
            .expect("tap");
        assert_eq!(tapped, json!({ "action": "tap" }));
        let tap = read();
        assert_eq!(&tap[3..], ["tap", "0.5", "0.2"]);
        let invalid = sandbox
            .input(identity, &json!({ "action": "tap", "x": 2, "y": 0.2 }))
            .await
            .expect_err("out of range");
        assert!(invalid.to_string().contains("Invalid simulator"));
        assert_eq!(read(), tap, "invalid input runs no process");

        let swipe = json!({ "action": "swipe", "fromX": 0.1, "fromY": 0.1, "toX": 0.5, "toY": 0.5, "duration": 0.3 });
        let failed = sandbox
            .input(identity, &swipe)
            .await
            .expect_err("Sandbox failed");
        assert!(failed.to_string().contains("SESSION_UNCERTAIN"));
        assert_eq!(read()[read().len() - 2..], ["--duration-ms", "300"]);

        let nobody = sandbox
            .input("", &json!({ "action": "tap", "x": 0.5, "y": 0.5 }))
            .await
            .expect_err("no udid");
        assert_eq!(status_of(&nobody, 500), 400);
        let _gone = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn says_when_sandbox_is_not_installed() {
        let sandbox = Sandbox::new(Path::new("/acme/nowhere"));
        let error = sandbox.screen("SIM-1").await.expect_err("missing");
        assert_eq!(
            error.to_string(),
            "Sandbox is not installed at .canonic/sandbox/bin/sandbox"
        );
    }

    #[test]
    fn swipe_durations_are_clamped_and_default_like_javascript() {
        let args = |duration: Value| {
            input_args(&json!({ "action": "swipe", "fromX": 0, "fromY": 0, "toX": 1, "toY": 1, "duration": duration })).expect("swipe")
        };
        assert_eq!(args(Value::Null).last().map(String::as_str), Some("200"));
        assert_eq!(args(json!(9)).last().map(String::as_str), Some("2000"));
        assert_eq!(args(json!(0.01)).last().map(String::as_str), Some("50"));
        assert_eq!(args(json!("0.3")).last().map(String::as_str), Some("300"));
        assert_eq!(&args(Value::Null)[1..5], ["0", "0", "1", "1"]);
    }
}
