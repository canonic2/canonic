//! iOS Simulators: the booted ones `simctl` lists, the implementation-only
//! pages a catalogued Simulator implementation adds, and their screens and
//! input through the project's Sandbox.

mod sandbox;

pub(crate) use sandbox::Sandbox;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::time::Duration;

const SIMCTL_TIMEOUT: Duration = Duration::from_secs(10);

/// A booted Simulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Device {
    pub(crate) name: String,
    pub(crate) udid: String,
    /// The runtime as people read it, such as `iOS 26.2`.
    pub(crate) runtime: String,
}

/// The booted, available Simulators.
///
/// # Errors
/// When `simctl` can't be run or answers with something else.
pub(crate) async fn booted() -> Result<Vec<Device>> {
    let mut command = tokio::process::Command::new("xcrun");
    command
        .args(["simctl", "list", "devices", "--json"])
        .kill_on_drop(true);
    let output = tokio::time::timeout(SIMCTL_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            anyhow!(
                "couldn’t list iOS Simulators: timed out after {}s",
                SIMCTL_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| anyhow!("couldn’t list iOS Simulators: {error}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "couldn’t list iOS Simulators: Command failed: xcrun simctl list devices --json\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    devices(&String::from_utf8_lossy(&output.stdout))
}

/// The booted, available devices in `simctl list devices --json` output.
///
/// # Errors
/// When the output isn't `simctl`'s JSON.
pub(crate) fn devices(stdout: &str) -> Result<Vec<Device>> {
    let body: Value = serde_json::from_str(stdout)
        .map_err(|error| anyhow!("simctl returned invalid device data: {error}"))?;
    let mut out = Vec::new();
    let Some(runtimes) = body.get("devices").and_then(Value::as_object) else {
        return Ok(out);
    };
    for (runtime, listed) in runtimes {
        for device in listed.as_array().into_iter().flatten() {
            if device.get("state").and_then(Value::as_str) != Some("Booted")
                || device.get("isAvailable") == Some(&Value::Bool(false))
            {
                continue;
            }
            let field = |key| {
                device
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            out.push(Device {
                name: field("name"),
                udid: field("udid"),
                runtime: runtime_label(runtime),
            });
        }
    }
    Ok(out)
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-26-2` as `iOS 26.2`.
fn runtime_label(runtime: &str) -> String {
    let last = runtime.rsplit('.').next().unwrap_or(runtime);
    let named = last
        .strip_prefix("iOS-")
        .map_or_else(|| last.to_owned(), |rest| format!("iOS {rest}"));
    named.replace('-', ".")
}

/// The collection and pages a catalogued Simulator implementation adds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Catalog {
    pub(crate) collections: Vec<Value>,
    pub(crate) pages: Map<String, Value>,
}

/// One implementation-only page per device the implementation `key` wants:
/// its `device` (a UDID or name), or every booted one.
pub(crate) fn catalog(devices: &[Device], key: &str, implementation: &Value) -> Catalog {
    let wanted = implementation
        .get("device")
        .and_then(Value::as_str)
        .filter(|device| !device.is_empty())
        .unwrap_or("booted");
    let icon = implementation
        .get("catalogIcon")
        .and_then(Value::as_str)
        .filter(|icon| !icon.is_empty())
        .unwrap_or("smartphone");
    let selected: Vec<_> = devices
        .iter()
        .filter(|device| wanted == "booted" || device.udid == wanted || device.name == wanted)
        .collect();
    let mut items = Vec::new();
    let mut pages = Map::new();
    for device in &selected {
        let src = format!("__ios-simulator/{key}/{}.html", device.udid);
        items.push(json!({
            "label": device.name,
            "src": src,
            "icon": icon,
            "implementations": { key: { "device": device.udid } },
            "implementationOnly": key,
        }));
        pages.insert(
            src,
            json!({ "label": device.name, "design": null, "code": [], "simulator": device }),
        );
    }
    let mut collection = Map::new();
    if let Some(label) = implementation.get("label") {
        collection.insert("name".into(), label.clone());
    }
    collection.insert("icon".into(), json!(icon));
    collection.insert("items".into(), Value::Array(items));
    Catalog {
        collections: if selected.is_empty() {
            Vec::new()
        } else {
            vec![Value::Object(collection)]
        },
        pages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, udid: &str) -> Device {
        Device {
            name: name.into(),
            udid: udid.into(),
            runtime: "iOS 26.2".into(),
        }
    }

    #[test]
    fn turns_matching_booted_simulators_into_implementation_only_pages() {
        let imported = catalog(
            &[device("iPhone 17", "SIM-1"), device("iPad Pro", "SIM-2")],
            "ios",
            &json!({ "label": "Simulator", "device": "iPhone 17", "catalogIcon": "smartphone" }),
        );
        assert_eq!(
            imported.collections,
            [
                json!({ "name": "Simulator", "icon": "smartphone", "items": [{
                "label": "iPhone 17", "src": "__ios-simulator/ios/SIM-1.html", "icon": "smartphone",
                "implementations": { "ios": { "device": "SIM-1" } }, "implementationOnly": "ios",
            }] })
            ]
        );
        assert_eq!(
            imported.pages["__ios-simulator/ios/SIM-1.html"]["simulator"]["udid"],
            "SIM-1"
        );
        assert_eq!(
            imported.pages["__ios-simulator/ios/SIM-1.html"]["design"],
            Value::Null
        );
    }

    #[test]
    fn booted_takes_every_device_and_no_match_adds_no_collection() {
        let devices = [device("iPhone 17", "SIM-1"), device("iPad Pro", "SIM-2")];
        let every = catalog(&devices, "ios", &json!({ "label": "Simulator" }));
        assert_eq!(every.pages.len(), 2);
        assert_eq!(
            every.collections[0]["items"][1]["src"],
            "__ios-simulator/ios/SIM-2.html"
        );
        let none = catalog(
            &devices,
            "ios",
            &json!({ "label": "Simulator", "device": "SIM-9" }),
        );
        assert!(none.collections.is_empty() && none.pages.is_empty());
    }

    #[test]
    fn reads_booted_available_devices_from_simctl() {
        let listed = devices(
            r#"{"devices":{
                "com.apple.CoreSimulator.SimRuntime.iOS-26-2":[
                    {"name":"iPhone 17","udid":"SIM-1","state":"Booted","isAvailable":true},
                    {"name":"iPhone Air","udid":"SIM-2","state":"Shutdown","isAvailable":true},
                    {"name":"iPad Pro","udid":"SIM-3","state":"Booted","isAvailable":false}],
                "com.apple.CoreSimulator.SimRuntime.watchOS-12-0":[
                    {"name":"Watch","udid":"SIM-4","state":"Booted"}]}}"#,
        )
        .expect("devices");
        assert_eq!(
            listed,
            [
                Device {
                    name: "iPhone 17".into(),
                    udid: "SIM-1".into(),
                    runtime: "iOS 26.2".into()
                },
                Device {
                    name: "Watch".into(),
                    udid: "SIM-4".into(),
                    runtime: "watchOS.12.0".into()
                },
            ]
        );
        let error = devices("not json").expect_err("invalid");
        assert!(
            error
                .to_string()
                .starts_with("simctl returned invalid device data: ")
        );
    }
}
