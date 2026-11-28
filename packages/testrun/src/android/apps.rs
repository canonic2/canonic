//! Android application identity validation, inventory and lifecycle operations.
use super::Android;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::time::Duration;

impl Android {
    pub async fn launch(&self, serial: &str, package: &str, timeout: Duration) -> Result<Value> {
        validate_package(package)?;
        let output = self
            .shell(
                serial,
                &["cmd", "package", "resolve-activity", "--brief", package],
                Duration::from_secs(5),
                65536,
            )
            .await?;
        let text = std::str::from_utf8(&output)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android launch component"))?;
        let component = text
            .lines()
            .rev()
            .find(|line| line.contains('/'))
            .ok_or_else(|| {
                Error::new(
                    "APP_NOT_FOUND",
                    "Android package has no launchable activity",
                )
            })?;
        if !component
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._/$".contains(&c))
        {
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Android launch component contains invalid characters",
            ));
        }
        self.shell(
            serial,
            &["am", "start", "-W", "-n", component],
            timeout,
            65536,
        )
        .await?;
        Ok(json!({"completed":true,"observed":false}))
    }
    /// Dispatches one VIEW intent; the caller validates the URL before admission.
    pub async fn open_url(&self, serial: &str, url: &str, timeout: Duration) -> Result<Value> {
        let output = self
            .shell(
                serial,
                &[
                    "am",
                    "start",
                    "-W",
                    "-a",
                    "android.intent.action.VIEW",
                    "-d",
                    url,
                ],
                timeout,
                65536,
            )
            .await?;
        let text = String::from_utf8_lossy(&output);
        // `am` reports an unresolved intent on its output and may still exit zero.
        if let Some(line) = text.lines().find(|line| line.starts_with("Error")) {
            return Err(Error::new(
                "PLATFORM_FAILED",
                format!(
                    "Android did not open the URL: {}",
                    line.chars().take(200).collect::<String>()
                ),
            ));
        }
        Ok(json!({"completed":true,"observed":false}))
    }
    pub async fn terminate(&self, serial: &str, package: &str, timeout: Duration) -> Result<Value> {
        validate_package(package)?;
        self.shell(serial, &["am", "force-stop", package], timeout, 4096)
            .await?;
        Ok(json!({"terminated":true}))
    }
    /// Reads the package whose window has input focus, if it is an app window.
    pub(crate) async fn focused_package(&self, serial: &str) -> Result<Option<String>> {
        let output = self
            .script(
                serial,
                "dumpsys window | grep -E 'mCurrentFocus=' || true",
                Duration::from_secs(5),
                65536,
            )
            .await?;
        Ok(focused_package(&String::from_utf8_lossy(&output)))
    }
    /// Lists installed packages with their home-screen names, versions and
    /// whether they can be opened, from one package-manager read on the
    /// device. When that read cannot run, the shell's package list answers
    /// with names and versions marked unavailable.
    pub async fn apps(&self, serial: &str) -> Result<Value> {
        match self.named_apps(serial).await {
            Ok(inventory) => Ok(inventory),
            Err(error) if error.code == "CLEANUP_FAILED" => Err(error),
            Err(error) => {
                let mut inventory = self.listed_apps(serial).await?;
                inventory["namesUnavailable"] = json!(error);
                Ok(inventory)
            }
        }
    }
    /// Runs the shell helper once in its package mode, then removes it.
    async fn named_apps(&self, serial: &str) -> Result<Value> {
        super::adb::validate_serial(serial)?;
        let adapter = Android {
            adb: self.adb.clone(),
        };
        let serial = serial.to_owned();
        let (reply, result) = tokio::sync::oneshot::channel();
        // Dropping the read cannot abandon owned provisioning or cleanup. The
        // worker finishes bounded I/O and removes the exact remote artifact.
        // Its own runtime also survives shutdown of the caller's read runtime;
        // spawn_blocking workers are drained rather than aborted at shutdown.
        tokio::task::spawn_blocking(move || {
            if reply.is_closed() {
                return;
            }
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| Error::new("PLATFORM_FAILED", "Cannot start Android inventory worker"))
                .and_then(|runtime| runtime.block_on(adapter.named_apps_owned(&serial, &reply)));
            if let Err(Err(error)) = reply.send(result)
                && error.code == "CLEANUP_FAILED"
            {
                eprintln!("Cancelled Android inventory failed: {}", error.code);
            }
        });
        result.await.map_err(|_| {
            Error::new(
                "CLEANUP_FAILED",
                "Android inventory owner stopped before cleanup completed",
            )
        })?
    }

    async fn named_apps_owned(
        &self,
        serial: &str,
        reply: &tokio::sync::oneshot::Sender<Result<Value>>,
    ) -> Result<Value> {
        let name = format!("testrun-apps-{}", uuid::Uuid::new_v4().simple());
        let remote = format!("/data/local/tmp/{name}.jar");
        let result = async {
            if reply.is_closed() {
                return Err(Error::new(
                    "CANCELLED",
                    "Android inventory read was cancelled",
                ));
            }
            let output = crate::process::run_input(
                &self.adb,
                &["-s", serial, "shell", "-T", &inventory_script(&remote)],
                crate::playback::ANDROID_HELPER,
                Duration::from_secs(25),
                4 * 1024 * 1024,
            )
            .await?;
            named_inventory(&output.bytes)
        }
        .await;
        // The remote trap exists before any bytes are written. This retry also
        // verifies removal after the bounded transport finishes.
        let cleanup = self
            .shell(serial, &["rm", "-f", &remote], Duration::from_secs(5), 4096)
            .await;
        if let Err(cause) = cleanup {
            return Err(
                Error::new("CLEANUP_FAILED", "Android inventory helper removal failed")
                    .details(json!({"cause":cause,"inventoryError":result.as_ref().err()})),
            );
        }
        result
    }
    /// Lists installed packages with version codes from one bounded package query.
    async fn listed_apps(&self, serial: &str) -> Result<Value> {
        let mut command = vec!["cmd", "package", "list", "packages", "--show-versioncode"];
        // Every Android system has packages, so an empty reply means the
        // option is unsupported; repeat the read-only query without it.
        for _ in 0..2 {
            let output = self
                .shell(serial, &command, Duration::from_secs(10), 1024 * 1024)
                .await?;
            let inventory = inventory(&output)?;
            if inventory["apps"].as_array().is_some_and(|a| !a.is_empty()) {
                return Ok(inventory);
            }
            command.pop();
        }
        Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Android package query returned no packages",
        ))
    }
}

fn inventory_script(remote: &str) -> String {
    format!(
        "umask 077; remote='{remote}'; trap 'rm -f \"$remote\" || exit 125' EXIT; trap 'exit 143' HUP INT TERM; cat > \"$remote\" || exit 1; [ \"$(wc -c < \"$remote\")\" -eq {} ] || exit 1; CLASSPATH=\"$remote\" app_process / io.canonic.testrun.media.Main apps",
        crate::playback::ANDROID_HELPER.len()
    )
}

/// Extracts the package of `mCurrentFocus=Window{hash u0 package/activity}`.
/// A focus without an activity component, such as a starting window, a system
/// dialog or `null`, is not an app window.
pub(super) fn focused_package(text: &str) -> Option<String> {
    let line = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("mCurrentFocus=Window{")?
            .strip_suffix('}')
    })?;
    let (package, _) = line.split_whitespace().last()?.split_once('/')?;
    validate_package(package).ok()?;
    Some(package.to_owned())
}

/// Parses `package:ID versionCode:N` lines into the shared inventory shape.
/// Display names and version names need a per-package query, so they are
/// reported as unavailable rather than looked up for every installed app.
pub(super) fn inventory(output: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(output)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android package list"))?;
    let mut apps = Vec::new();
    for line in text.lines() {
        let Some(entry) = line.trim().strip_prefix("package:") else {
            continue;
        };
        let mut fields = entry.split_whitespace();
        let Some(package) = fields.next() else {
            continue;
        };
        validate_package(package)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android package identifier"))?;
        let build = fields
            .find_map(|field| field.strip_prefix("versionCode:"))
            .filter(|code| code.bytes().all(|b| b.is_ascii_digit()))
            .unwrap_or("");
        apps.push(json!({"appId":package,"name":"","version":"","build":build}));
    }
    apps.sort_by(|a, b| a["appId"].as_str().cmp(&b["appId"].as_str()));
    Ok(json!({"apps":apps,"unavailableFields":["name","version"]}))
}

/// Reads the helper's package frame: its last line, ignoring any framework
/// chatter before it. Every identifier is validated like the shell list's.
pub(super) fn named_inventory(output: &[u8]) -> Result<Value> {
    let invalid = || Error::new("PLATFORM_PROTOCOL", "Invalid Android package read");
    let text = std::str::from_utf8(output).map_err(|_| invalid())?;
    let frame: Value = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok())
        .ok_or_else(invalid)?;
    if frame["ok"] != true {
        return Err(Error::new(
            "PLATFORM_FAILED",
            "Android package read failed on the device",
        )
        .details(json!({"cause":frame["error"]})));
    }
    let mut apps = frame["data"]["apps"]
        .as_array()
        .cloned()
        .ok_or_else(invalid)?;
    if apps.is_empty() {
        return Err(invalid());
    }
    for app in &apps {
        validate_package(app["appId"].as_str().unwrap_or_default())
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android package identifier"))?;
        if !app["name"].is_string() || !app["launchable"].is_boolean() {
            return Err(invalid());
        }
    }
    apps.sort_by(|a, b| a["appId"].as_str().cmp(&b["appId"].as_str()));
    Ok(json!({"apps":apps}))
}

fn validate_package(package: &str) -> Result<()> {
    if package.is_empty()
        || package.len() > 256
        || !package
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b))
    {
        return Err(Error::invalid("Invalid Android package identifier"));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
