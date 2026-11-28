//! Bounded selected-target observation readiness; never acquires control or sends input.
use super::{Android, Device};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::time::Instant;

#[cfg(test)]
mod tests;

const CHECKS: &[&str] = &[
    "adb",
    "authorization",
    "transport",
    "shell",
    "os",
    "boot",
    "screenshot",
    "accessibility",
];

impl Android {
    pub(crate) async fn doctor(&self, target: &str) -> Result<Value> {
        self.diagnose(target, Instant::now() + Duration::from_secs(25))
            .await
    }

    async fn diagnose(&self, target: &str, deadline: Instant) -> Result<Value> {
        if target.starts_with(super::emulator::AVD_PREFIX) {
            return Err(Error::invalid(
                "An AVD target has no adb connection; connect with boot to start it, then diagnose its android:emulator- target",
            ));
        }
        let serial = target
            .strip_prefix("android:")
            .ok_or_else(|| Error::invalid("Choose an explicit android: target from target_list"))?;
        super::adb::validate_serial(serial)?;
        let mut checks = json!({});
        for key in CHECKS {
            checks[*key] = json!({"state":"unknown","reason":"Prerequisites have not been verified","nextAction":"doctor"});
        }
        let inventory = match bounded(deadline, 10, self.devices()).await {
            Ok(devices) => devices,
            Err(error) => {
                checks["adb"] = failed(
                    error,
                    "Check Android SDK Platform Tools and the selected adb executable (PATH or TESTRUN_ADB), then retry discovery",
                );
                return Ok(report(target, checks));
            }
        };
        checks["adb"] = json!({"state":"ready"});
        let Some(device) = inventory.into_iter().find(|device| device.serial == serial) else {
            checks["authorization"] = json!({"state":"blocked","observedState":"absent","manualAction":"Connect the selected target and enable debugging; rediscover its explicit identity","nextAction":"target_list"});
            return Ok(report(target, checks));
        };
        checks["authorization"] = authorization(&device.state);
        checks["transport"] = transport(&device);
        if device.state != "device" {
            return Ok(report(target, checks));
        }
        let shell = bounded(
            deadline,
            2,
            self.shell(
                serial,
                &["echo", "testrun-readiness"],
                Duration::from_secs(2),
                1024,
            ),
        )
        .await
        .and_then(text)
        .and_then(|value| {
            if value == "testrun-readiness" {
                Ok(())
            } else {
                Err(Error::new(
                    "PLATFORM_PROTOCOL",
                    "Android shell probe returned an unexpected response",
                ))
            }
        });
        checks["shell"] = match shell {
            Ok(()) => json!({"state":"ready"}),
            Err(error) => {
                checks["shell"] = failed(
                    error,
                    "Check the selected target's adb shell access, unlock it if required, and retry",
                );
                return Ok(report(target, checks));
            }
        };
        self.observations(serial, deadline, &mut checks).await;
        // Do not certify a mixture of observations from different adb transports.
        checks["transport"] = match bounded(deadline, 2, self.devices()).await {
            Ok(devices)
                if devices.iter().any(|current| {
                    current.serial == serial
                        && current.state == device.state
                        && current.transport == device.transport
                }) =>
            {
                transport(&device)
            }
            Ok(_) => {
                json!({"state":"unknown","reason":"Selected target disconnected or changed transport during diagnosis","nextAction":"doctor","manualAction":"Rediscover the selected target and repeat diagnosis"})
            }
            Err(error) => failed(
                error,
                "Repeat diagnosis to verify that the selected adb transport is still current",
            ),
        };
        let mut result = report(target, checks);
        result["usbInstallVerification"] = self.install_verification(serial, deadline).await;
        Ok(result)
    }

    async fn install_verification(&self, serial: &str, deadline: Instant) -> Value {
        install_verification(
            bounded(
                deadline,
                2,
                self.shell(
                    serial,
                    &["settings", "get", "global", "verifier_verify_adb_installs"],
                    Duration::from_secs(2),
                    1024,
                ),
            )
            .await
            .and_then(text),
        )
    }

    async fn observations(&self, serial: &str, deadline: Instant, checks: &mut Value) {
        let api = self
            .property(serial, "ro.build.version.sdk", deadline)
            .await;
        let release = self
            .property(serial, "ro.build.version.release", deadline)
            .await;
        checks["os"] = os(api, release);
        checks["boot"] = match self.property(serial, "sys.boot_completed", deadline).await {
            Ok(value) => match value.as_str() {
                "1" => json!({"state":"ready","bootCompleted":true}),
                "0" => {
                    json!({"state":"blocked","bootCompleted":false,"manualAction":"Wait for the selected target to finish booting, then unlock it and retry","nextAction":"doctor"})
                }
                _ => {
                    json!({"state":"unknown","reason":"Boot completion was not reported as 0 or 1","manualAction":"Inspect boot progress on the selected target and retry","nextAction":"doctor"})
                }
            },
            Err(error) => failed(
                error,
                "Inspect boot progress on the selected target and retry",
            ),
        };
        // Successful capture is independent evidence even if a property is absent.
        checks["screenshot"] = match bounded(deadline, 5, self.screenshot(serial))
            .await
            .and_then(|bytes| crate::ui::png_dimensions(&bytes))
        {
            Ok((width, height)) => {
                json!({"state":"ready","width":width,"height":height,"format":"png"})
            }
            Err(error) => failed(
                error,
                "Unlock the selected target and check that its main display can be captured, then retry",
            ),
        };
        checks["accessibility"] = match bounded(deadline, 10, self.snapshot(serial)).await {
            Ok(tree)
                if tree["nodeCount"].as_u64().is_some_and(|count| count > 0)
                    && tree["truncated"] != true
                    && tree["quality"] == "complete" =>
            {
                json!({"state":"ready","nodeCount":tree["nodeCount"]})
            }
            Ok(tree) => {
                json!({"state":"unknown","nodeCount":tree["nodeCount"],"reason":"Hierarchy is empty or partial; UI readiness is unverified","manualAction":"Unlock the selected target and open an inspectable screen, then retry","nextAction":"doctor"})
            }
            Err(error) => failed(
                error,
                "Unlock the selected target and check Android automation availability; close competing inspection tools if needed, then retry",
            ),
        };
    }

    async fn property(&self, serial: &str, name: &str, deadline: Instant) -> Result<String> {
        bounded(
            deadline,
            2,
            self.shell(serial, &["getprop", name], Duration::from_secs(2), 1024),
        )
        .await
        .and_then(text)
    }
}

async fn bounded<T>(
    deadline: Instant,
    seconds: u64,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    if Instant::now() >= deadline {
        return Err(timeout());
    }
    tokio::time::timeout_at(
        deadline.min(Instant::now() + Duration::from_secs(seconds)),
        work,
    )
    .await
    .unwrap_or_else(|_| Err(timeout()))
}
fn timeout() -> Error {
    Error::new(
        "READINESS_PROBE_TIMEOUT",
        "Android readiness probe exhausted its budget",
    )
}
/// Whether Google Play Protect checks apps installed over USB. Unset means the
/// platform default, which is on. It informs installs; it never blocks readiness.
fn install_verification(setting: Result<String>) -> Value {
    match setting.as_deref() {
        Ok("0") => json!({"enabled":false,"setting":"0"}),
        Ok(value @ ("1" | "null" | "")) => {
            json!({"enabled":true,"setting":value,"warning":"Google Play Protect may ask whether to send an installed app for a security check, and the install waits for the answer; pass playProtect to app_install or answer it on the device"})
        }
        Ok(value) => json!({"enabled":Value::Null,"setting":value}),
        Err(error) => json!({"enabled":Value::Null,"error":error}),
    }
}
fn text(bytes: Vec<u8>) -> Result<String> {
    let value = std::str::from_utf8(&bytes)
        .map(str::trim)
        .ok()
        .filter(|value| value.len() <= 256 && !value.chars().any(char::is_control))
        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Invalid Android readiness property"))?;
    Ok(value.into())
}
fn authorization(state: &str) -> Value {
    match state {
        "device" => json!({"state":"ready","observedState":"device"}),
        "unauthorized" => {
            json!({"state":"blocked","observedState":"unauthorized","manualAction":"Unlock the selected Android device and accept its USB debugging authorization prompt, then retry","nextAction":"doctor"})
        }
        "offline" => {
            json!({"state":"blocked","observedState":"offline","manualAction":"Restore the selected target's debugging connection and wait for adb to report device, then retry","nextAction":"doctor"})
        }
        _ => {
            json!({"state":"unknown","observedState":state,"manualAction":"Check the selected target's boot mode and debugging connection","nextAction":"doctor"})
        }
    }
}
fn transport(device: &Device) -> Value {
    if device
        .transport
        .parse::<u64>()
        .ok()
        .is_none_or(|id| id == 0)
    {
        json!({"state":"unknown","kind":device.link,"reason":"adb did not report a transport identity","manualAction":"Check adb platform-tools compatibility and repeat discovery","nextAction":"doctor"})
    } else {
        json!({"state":"ready","kind":device.link,"transportId":device.transport})
    }
}
fn os(api: Result<String>, release: Result<String>) -> Value {
    let mut result = json!({"state":"unknown","manualAction":"Check Android build properties on the selected target","nextAction":"doctor"});
    match api {
        Ok(value) => match value.parse::<u32>().ok().filter(|level| *level > 0) {
            Some(level) => result["apiLevel"] = json!(level),
            None => result["apiReason"] = json!("Android API level is unavailable or invalid"),
        },
        Err(error) => result["apiError"] = json!(error),
    }
    match release {
        Ok(value) if !value.is_empty() => result["release"] = json!(value),
        Ok(_) => result["releaseReason"] = json!("Android release is unavailable"),
        Err(error) => result["releaseError"] = json!(error),
    }
    if result["apiLevel"].is_number() && result["release"].is_string() {
        return json!({"state":"ready","apiLevel":result["apiLevel"],"release":result["release"]});
    }
    result
}
fn failed(error: Error, action: &str) -> Value {
    let state = if matches!(error.code.as_str(), "TIMEOUT" | "READINESS_PROBE_TIMEOUT") {
        "unknown"
    } else {
        "blocked"
    };
    json!({"state":state,"error":error,"manualAction":action,"nextAction":"doctor"})
}
fn report(target: &str, checks: Value) -> Value {
    let ready = CHECKS.iter().all(|key| checks[*key]["state"] == "ready");
    let blocked = CHECKS.iter().any(|key| checks[*key]["state"] == "blocked");
    json!({"targetId":target,"ready":ready,"state":if ready {"ready"} else if blocked {"blocked"} else {"unknown"},"scope":"adb_observation","checks":checks,"inputReadiness":"unknown"})
}
