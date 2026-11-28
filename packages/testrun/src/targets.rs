//! Platform target discovery and generation tracking; names never select devices.
use crate::error::{Error, Result};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};
use uuid::Uuid;

pub(crate) mod capabilities;
mod model;
pub use model::{Target, TargetRef};

#[derive(Clone, Default)]
pub struct Discovery {
    targets: HashMap<String, Target>,
}

/// A generation follows the target's observed epoch, its connection or boot
/// record, and nothing else, so every Testrun process names an unchanged
/// target the same way and any epoch change names it anew.
fn generation(target_id: &str, epoch: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{target_id}\n{epoch}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes).to_string()
}

#[derive(Deserialize)]
struct DeviceList {
    devices: BTreeMap<String, Vec<Simulator>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Simulator {
    udid: String,
    name: String,
    state: String,
    #[serde(default)]
    is_available: bool,
    last_booted_at: Option<String>,
}

impl Discovery {
    /// Publish one completed platform refresh without holding a lock over I/O.
    pub(crate) fn publish(&mut self, kind: &str, mut targets: Vec<Target>) -> Vec<Target> {
        for target in &mut targets {
            if let Some(previous) = self.targets.get(&target.identity.target_id)
                && previous.epoch == target.epoch
            {
                target
                    .identity
                    .generation
                    .clone_from(&previous.identity.generation);
            }
        }
        self.targets.retain(|_, target| target.kind != kind);
        self.targets.extend(
            targets
                .iter()
                .map(|target| (target.identity.target_id.clone(), target.clone())),
        );
        targets
    }
    pub(crate) fn reconcile(&self, target: &Target) -> Result<Target> {
        self.targets
            .get(&target.identity.target_id)
            .filter(|current| current.epoch == target.epoch)
            .cloned()
            .ok_or_else(|| Error::new("STALE_TARGET", "A newer inventory superseded this target"))
    }
    pub async fn refresh(&mut self) -> Result<Vec<Target>> {
        self.refresh_set(&crate::simulator::device_set::DeviceSet::current()?)
            .await
    }
    pub(crate) async fn refresh_set(
        &mut self,
        set: &crate::simulator::device_set::DeviceSet,
    ) -> Result<Vec<Target>> {
        if !cfg!(target_os = "macos") {
            return Err(Error::new(
                "UNSUPPORTED_HOST",
                "iOS simulator discovery requires macOS",
            ));
        }
        let output = set
            .run(
                &["list", "devices", "--json"],
                Duration::from_secs(15),
                4 * 1024 * 1024,
            )
            .await?;
        self.update_simulators_in(&output.bytes, set)
    }

    #[cfg(test)]
    fn update_simulators(&mut self, bytes: &[u8]) -> Result<Vec<Target>> {
        self.update_simulators_in(bytes, &crate::simulator::device_set::DeviceSet::current()?)
    }
    fn update_simulators_in(
        &mut self,
        bytes: &[u8],
        set: &crate::simulator::device_set::DeviceSet,
    ) -> Result<Vec<Target>> {
        let list: DeviceList = serde_json::from_slice(bytes)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid simulator discovery response"))?;
        let mut next = HashMap::new();
        for (runtime, devices) in list.devices {
            if !runtime.contains("iOS") {
                continue;
            }
            for device in devices.into_iter().filter(|d| d.is_available) {
                let target_id = set.target_id(&device.udid);
                // simctl supplies the last boot time even after shutdown. A boot epoch
                // plus state detects restart and invalidates refs across shutdown.
                let epoch = format!(
                    "{}:{}:{}",
                    runtime,
                    device.state,
                    device.last_booted_at.as_deref().unwrap_or("never")
                );
                let generation = generation(&target_id, &epoch);
                let mut capabilities = BTreeMap::new();
                for name in [
                    "lifecycle",
                    "app_management",
                    "clipboard_read",
                    "screenshots",
                ] {
                    capabilities.insert(name.into(), serde_json::json!({"state":"supported"}));
                }
                for name in ["input", "accessibility", "preview", "recording"] {
                    capabilities.insert(name.into(), serde_json::json!({"state":"unknown","reason":"Native adapter has not been probed"}));
                }
                capabilities::complete(&mut capabilities, "simulator");
                next.insert(
                    target_id.clone(),
                    Target {
                        identity: TargetRef {
                            target_id,
                            generation,
                        },
                        name: device.name,
                        kind: "simulator".into(),
                        state: device.state,
                        runtime: runtime.clone(),
                        epoch,
                        capabilities,
                        platform_id: device.udid,
                        metadata: BTreeMap::from([
                            ("deviceSetIdentity".into(), serde_json::json!(set.identity)),
                            ("deviceSetPath".into(), serde_json::json!(set.path)),
                        ]),
                        usb_id: None,
                    },
                );
            }
        }
        self.targets.retain(|_, target| target.kind != "simulator");
        self.targets.extend(next);
        let mut result: Vec<_> = self
            .targets
            .values()
            .filter(|t| t.kind == "simulator")
            .cloned()
            .collect();
        result.sort_by(|a, b| a.identity.target_id.cmp(&b.identity.target_id));
        Ok(result)
    }
    pub async fn refresh_android(&mut self) -> Result<Vec<Target>> {
        let devices = crate::android::Android::default().devices().await?;
        let mut next = HashMap::new();
        for device in devices {
            let target_id = format!("android:{}", device.serial);
            let epoch = format!("{}:{}", device.state, device.transport);
            let generation = generation(&target_id, &epoch);
            let mut capabilities = BTreeMap::new();
            capabilities::android(&mut capabilities, &device.state, &device.link);
            next.insert(
                target_id.clone(),
                Target {
                    identity: TargetRef {
                        target_id,
                        generation,
                    },
                    name: device.name,
                    kind: "android".into(),
                    state: if device.state == "device" {
                        "Booted".into()
                    } else {
                        device.state
                    },
                    runtime: "unknown".into(),
                    epoch,
                    capabilities,
                    platform_id: device.serial,
                    metadata: BTreeMap::new(),
                    usb_id: None,
                },
            );
        }
        for target in self.avd_targets() {
            next.insert(target.identity.target_id.clone(), target);
        }
        self.targets.retain(|_, t| t.kind != "android");
        self.targets.extend(next);
        let mut result: Vec<_> = self
            .targets
            .values()
            .filter(|t| t.kind == "android")
            .cloned()
            .collect();
        result.sort_by(|a, b| a.identity.target_id.cmp(&b.identity.target_id));
        Ok(result)
    }
    /// AVD definitions as startable targets, read from files only. Whether an
    /// AVD is running is not observed here; a running instance is listed as its
    /// own `android:emulator-PORT` target.
    fn avd_targets(&self) -> Vec<Target> {
        use crate::android::emulator;
        let Some(avds) = emulator::home().and_then(|home| emulator::list(&home).ok()) else {
            return Vec::new();
        };
        let launcher = match emulator::executable() {
            Ok(_) => serde_json::json!({"state":"ready"}),
            Err(error) => serde_json::json!({"state":"missing","error":error}),
        };
        avds.into_iter()
            .map(|avd| {
                let target_id = format!("{}{}", emulator::AVD_PREFIX, avd.name);
                let epoch = "avd".to_owned();
                let generation = generation(&target_id, &epoch);
                let mut capabilities = BTreeMap::new();
                capabilities::complete(&mut capabilities, "android");
                for value in capabilities.values_mut() {
                    *value = serde_json::json!({"state":"unsupported","reason":"Connect with boot to start this AVD; operations use the started emulator target"});
                }
                Target {
                    identity: TargetRef {
                        target_id,
                        generation,
                    },
                    name: avd.display_name.unwrap_or_else(|| avd.name.clone()),
                    kind: "android".into(),
                    state: "Available".into(),
                    runtime: avd.api.unwrap_or_else(|| "unknown".into()),
                    epoch,
                    capabilities,
                    // No adb serial exists; adapter calls refuse an empty identity.
                    platform_id: String::new(),
                    metadata: BTreeMap::from([(
                        "avd".into(),
                        serde_json::json!({"name":avd.name,"abi":avd.abi,"tag":avd.tag,"dataPolicy":emulator::DATA_POLICY,"emulator":launcher}),
                    )]),
                    usb_id: None,
                }
            })
            .collect()
    }
    pub async fn refresh_devices(&mut self) -> Result<Vec<Target>> {
        let devices = crate::usbmux::list(std::path::Path::new("/var/run/usbmuxd")).await?;
        let inventory = crate::device_setup::inventory().await;
        let mut metadata = inventory.as_ref().cloned().unwrap_or_default();
        let mut next = HashMap::new();
        // Loaded once per refresh, and only when a phone lacks its name.
        let mut product_names: Option<Option<BTreeMap<String, String>>> = None;
        // Each phone's own lookups take up to 3 s each; phones run them at once.
        let looked_up = futures_util::future::join_all(devices.iter().map(|device| {
            let known = metadata.remove(&device.serial);
            Self::usb_details(&device.serial, known)
        }))
        .await;
        for (device, mut details) in devices.into_iter().zip(looked_up) {
            let target_id = format!("device:{}", device.serial);
            details.insert("transport".into(), serde_json::json!("usb"));
            if let Some(product) = details
                .get("model")
                .and_then(serde_json::Value::as_str)
                .filter(|model| crate::device_setup::is_product_type(model))
                .map(str::to_owned)
            {
                if product_names.is_none() {
                    product_names = Some(crate::device_setup::product_names().await);
                }
                if let Some(name) = product_names
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|names| names.get(&product))
                {
                    details.insert("model".into(), serde_json::json!(name));
                    details.insert("productType".into(), serde_json::json!(product));
                }
            }
            let supported = details.get("platform") == Some(&serde_json::json!("iOS"))
                || details
                    .get("model")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|model| model.starts_with("iPhone") || model.starts_with("iPad"));
            details.insert("controlSupported".into(), serde_json::json!(supported));
            if let Err(error) = &inventory {
                details.insert("metadataWarning".into(), serde_json::json!(error));
            }

            let epoch = device.id.to_string();
            let generation = generation(&target_id, &epoch);
            let capabilities = Self::usb_capabilities();
            next.insert(
                target_id.clone(),
                Target {
                    identity: TargetRef {
                        target_id,
                        generation,
                    },
                    name: details
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Unidentified USB device")
                        .into(),
                    kind: "device".into(),
                    state: "Connected".into(),
                    runtime: details
                        .get("osVersion")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .into(),
                    epoch,
                    capabilities,
                    platform_id: device.serial,
                    metadata: details,
                    usb_id: Some(device.id),
                },
            );
        }
        for (serial, details) in metadata {
            if details.get("transport") != Some(&serde_json::json!("network")) {
                continue;
            }
            let target = self.network_target(serial, details);
            next.insert(target.identity.target_id.clone(), target);
        }
        self.targets.retain(|_, t| t.kind != "device");
        self.targets.extend(next);
        let mut result: Vec<_> = self
            .targets
            .values()
            .filter(|t| t.kind == "device")
            .cloned()
            .collect();
        result.sort_by(|a, b| a.identity.target_id.cmp(&b.identity.target_id));
        Ok(result)
    }

    /// A USB phone's identity, from the inventory or its own lookup, with its
    /// Developer Mode state. Each lookup takes up to 3 s.
    async fn usb_details(
        serial: &str,
        known: Option<BTreeMap<String, serde_json::Value>>,
    ) -> BTreeMap<String, serde_json::Value> {
        let mut details = match known {
            Some(details) => details,
            None => tokio::time::timeout(
                Duration::from_secs(3),
                crate::device_setup::identity(serial),
            )
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default(),
        };
        if !details
            .get("developerMode")
            .is_some_and(|mode| mode == "enabled" || mode == "disabled")
        {
            let mode = tokio::time::timeout(
                Duration::from_secs(3),
                crate::device_setup::developer_mode(serial),
            )
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or("unknown");
            details.insert("developerMode".into(), serde_json::json!(mode));
        }
        details
    }

    /// What a USB phone may support before its runner and HID services are checked.
    fn usb_capabilities() -> BTreeMap<String, serde_json::Value> {
        let mut capabilities = BTreeMap::new();
        capabilities.insert("type_text".into(),serde_json::json!({"state":"unknown","reason":"HID US ASCII input requires a selected keyboard/video session and uses current display focus; XCTest focused input requires owned runner setup"}));
        for name in ["screenshots", "accessibility"] {
            capabilities.insert(name.into(),serde_json::json!({"state":"unknown","reason":"Owned signed runner setup and trust must be verified"}));
        }
        capabilities.insert("input".into(),serde_json::json!({"state":"unknown","reason":"Requires selected-device HID services and calibrated video; use doctor"}));
        for name in ["preview", "recording"] {
            capabilities.insert(name.into(),serde_json::json!({"state":"unknown","reason":"Requires iOS 27+ and compatible CoreDevice developer support","minimumOS":"27","sourceMode":"continuous_video","formats":["hevc"]}));
        }
        capabilities::complete(&mut capabilities, "device");
        capabilities
    }

    fn network_target(
        &self,
        serial: String,
        details: BTreeMap<String, serde_json::Value>,
    ) -> Target {
        let target_id = format!("device:{serial}");
        let epoch = format!(
            "network:{}",
            details.get("osVersion").unwrap_or(&serde_json::Value::Null)
        );
        let generation = generation(&target_id, &epoch);
        let mut capabilities = BTreeMap::new();
        capabilities::complete(&mut capabilities, "device");
        // CoreDevice app management may reach network devices; USB control cannot.
        for (key, value) in &mut capabilities {
            if !matches!(key.as_str(), "app_management" | "app_list") {
                *value = serde_json::json!({"state":"unsupported","reason":"Physical control requires USB; network devices are discoverable only"});
            }
        }
        Target {
            identity: TargetRef {
                target_id,
                generation,
            },
            name: details
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Network iOS device")
                .into(),
            kind: "device".into(),
            state: "Network".into(),
            runtime: details
                .get("osVersion")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .into(),
            metadata: details,
            capabilities,
            platform_id: serial,
            usb_id: None,
            epoch,
        }
    }

    pub fn resolve(&self, reference: &TargetRef) -> Result<Target> {
        let target = self
            .targets
            .get(&reference.target_id)
            .ok_or_else(|| Error::new("NOT_FOUND", "Target is not connected or available"))?;
        if target.identity.generation != reference.generation {
            return Err(Error::new(
                "STALE_TARGET",
                "Target generation changed; discover and open a new session",
            ));
        }
        Ok(target.clone())
    }
}

#[cfg(test)]
pub(crate) fn qa_fixture_target() -> Target {
    Target {
        identity: TargetRef {
            target_id: "simulator:Acme".into(),
            generation: "fixture".into(),
        },
        name: "Acme QA".into(),
        kind: "simulator".into(),
        state: "Booted".into(),
        runtime: "fixture".into(),
        capabilities: BTreeMap::new(),
        platform_id: "Acme".into(),
        metadata: BTreeMap::new(),
        usb_id: None,
        epoch: "fixture".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list(state: &str, boot: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"devices":{"iOS-26":[{"udid":"abc","name":"Acme phone","state":state,"isAvailable":true,"lastBootedAt":boot}]}})).unwrap()
    }
    #[test]
    fn generations_reject_restarted_targets() {
        let mut discovery = Discovery::default();
        let first = discovery
            .update_simulators(&list("Booted", "one"))
            .unwrap()
            .remove(0);
        let second = discovery
            .update_simulators(&list("Booted", "one"))
            .unwrap()
            .remove(0);
        assert_eq!(first.identity, second.identity);
        // Another process's inventory names the unchanged target the same way.
        let other = Discovery::default()
            .update_simulators(&list("Booted", "one"))
            .unwrap()
            .remove(0);
        assert_eq!(first.identity, other.identity);
        discovery.update_simulators(&list("Booted", "two")).unwrap();
        assert_eq!(
            discovery.resolve(&first.identity).unwrap_err().code,
            "STALE_TARGET"
        );
    }
}
