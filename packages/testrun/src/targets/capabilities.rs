//! Common capability vocabulary and backend restrictions; support requires adapter evidence.
use super::Target;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Capability keys use the operation's tool name, or the family name for a
/// family of tools (`app_management`, `preview`, `recording`, `volume`, `network`).
pub(crate) fn complete(values: &mut BTreeMap<String, Value>, kind: &str) {
    for key in [
        "gesture",
        "pinch",
        "rotate_gesture",
        "scroll",
        "orientation_set",
    ] {
        values.insert(key.into(), json!({"state":"unsupported","reason":"No multi-contact, semantic-scroll or orientation mutation adapter is registered"}));
    }
    for key in [
        "input",
        "tap",
        "swipe",
        "long_press",
        "key",
        "type_text",
        "app_management",
        "app_list",
        "app_launch",
        "app_terminate",
        "app_restart",
        "open_url",
        "clipboard_read",
        "accessibility",
        "screenshots",
        "preview",
        "recording",
        "media_status",
        "media_send",
        "volume",
        "audio_probe",
        "network",
    ] {
        values.entry(key.into()).or_insert_with(|| json!({"state":"unknown","reason":"Selected adapter has not verified this capability"}));
    }
    if kind != "android" {
        values.insert(
            "network".into(),
            json!({"state":"unsupported","reason":"Network control is registered for Android targets only"}),
        );
    }
    if kind == "simulator" {
        for key in ["media_status", "media_send", "volume", "audio_probe"] {
            values.insert(key.into(),json!({"state":"unsupported","reason":"No simulator system-media adapter is registered"}));
        }
        for key in ["app_list", "app_terminate", "open_url"] {
            values.insert(key.into(), json!({"state":"supported"}));
        }
        for key in ["app_launch", "app_restart"] {
            values.insert(
                key.into(),
                json!({"state":"supported","launchArguments":true,"launchEnvironment":true,"readiness":["foreground","foreground_element"]}),
            );
        }
    } else if kind == "device" {
        for key in ["media_status", "volume"] {
            values.insert(key.into(),json!({"state":"unsupported","reason":"No native playback-state or volume reader is registered; audio probes measure samples only"}));
        }
        values.insert("audio_probe".into(),json!({"state":"unknown","backend":"coredevice_system_audio","reason":"Requires explicit bounded capture and native acceptance","integratedPhysicalAcceptance":false}));
        values.insert("media_send".into(),json!({"state":"unknown","backend":"coredevice_indigo","destinations":["system_player"],"commands":["play","pause","play_pause","next","previous","volume_up","volume_down"],"reason":"Requires selected-controller Indigo readiness"}));
        for key in ["clipboard_read", "open_url"] {
            values.insert(
                key.into(),
                json!({"state":"unsupported","reason":"No physical adapter is registered"}),
            );
        }
        // devicectl passes a `--` separator ahead of any launch arguments.
        for key in ["app_launch", "app_restart"] {
            values.insert(
                key.into(),
                json!({"state":"supported","backend":"coredevice","readiness":[],"launchArguments":"after_separator","launchEnvironment":true}),
            );
        }
        values.insert(
            "app_terminate".into(),
            json!({"state":"supported","backend":"coredevice","identity":"installed_executable_path"}),
        );
    }
}

/// Android adb adapter operations and limits. An adb state other than `device`
/// (offline, unauthorized) leaves every operation unknown until it is usable.
/// `link` is the adb connection kind (`usb`, `emulator` or `other`).
pub(crate) fn android(values: &mut BTreeMap<String, Value>, adb_state: &str, link: &str) {
    if adb_state != "device" {
        complete(values, "android");
        for value in values.values_mut() {
            value["state"] = json!("unknown");
            value["reason"] = json!(format!("adb reports the target as {adb_state}"));
        }
        return;
    }
    let display =
        json!({"backend":"adb_input","scope":"display","frameUnit":"pixels","maxContacts":1});
    let unsupported = |reason: &str| json!({"state":"unsupported","reason":reason});
    for (key, details) in [
        ("input", display.clone()),
        ("tap", display.clone()),
        ("swipe", display.clone()),
        ("long_press", display.clone()),
        (
            "key",
            json!({"backend":"adb_input","scope":"display","catalog":"android","modifiers":false,"modifiersReason":"Key chords are probed when a control session opens"}),
        ),
        (
            "type_text",
            json!({"backend":"adb_input","scope":"display","encoding":"printable_ascii","replace":true,"elementTarget":true,"replacement":{"backend":"accessibility_set_text","encoding":"utf8","requiresTarget":true,"targetKinds":["query"],"requiresFocusedField":true,"imeComposition":false},"requiresVerifiedFocus":true,"focusInspection":"ui_automation_direct","requiresAndroidApi":24,"unsupportedSequences":["%s"]}),
        ),
        ("app_management", json!({"package":"apk"})),
        (
            "app_list",
            json!({"metadata":["home_screen_name","version","launchable"],"fallback":"package_id"}),
        ),
        (
            "app_launch",
            json!({"launchArguments":false,"launchEnvironment":false,"readiness":["foreground","foreground_element"]}),
        ),
        ("app_terminate", json!({})),
        (
            "app_restart",
            json!({"launchArguments":false,"launchEnvironment":false,"readiness":["foreground","foreground_element"]}),
        ),
        ("open_url", json!({"intent":"android.intent.action.VIEW"})),
        (
            "accessibility",
            json!({"backend":"ui_automation_direct","scope":"display","coverage":"active_window_exposed_tree","requiresAndroidApi":24,"frameUnit":"pixels"}),
        ),
        (
            "screenshots",
            json!({"backend":"adb_screencap","profiles":["fast","evidence"]}),
        ),
    ] {
        let mut value = details;
        value["state"] = json!("supported");
        values.insert(key.into(), value);
    }
    for (key, details) in [
        (
            "media_status",
            json!({"backend":"shell_media_helper","requiresAndroidApi":31}),
        ),
        (
            "media_send",
            json!({"backend":"shell_media_helper","destinations":["media_session"],"commands":["play","pause","play_pause","next","previous","stop"]}),
        ),
        (
            "volume",
            json!({"backend":"shell_media_helper","destinations":["music_stream","media_session"]}),
        ),
    ] {
        let mut value = details;
        value["state"] = json!("unknown");
        value["reason"] = json!("Helper readiness is verified in the selected controller");
        values.insert(key.into(), value);
    }
    values.insert(
        "audio_probe".into(),
        json!({"state":"unknown","backend":"android_remote_submix","experimental":true,"changesOutputRouting":true,"requiresAcceptRouteChange":true,"reason":"Recorder support is independent of media helper readiness"}),
    );
    values.insert(
        "clipboard_read".into(),
        unsupported("No Android clipboard adapter is registered"),
    );
    values.insert(
        "preview".into(),
        unsupported("Android preview is not registered"),
    );
    values.insert(
        "recording".into(),
        json!({"state":"unknown","backend":"adb_screenrecord","format":"h264","reason":"screenrecord availability and its time limit are read when a recording starts"}),
    );
    values.insert("network".into(), android_network_discovery(link));
    complete(values, "android");
}

/// Discovery can rule out wireless debugging, which the offline switch would
/// sever; the API level that selects the methods is read when a control
/// session opens.
fn android_network_discovery(link: &str) -> Value {
    let mut value = json!({"backend":"adb_connectivity","link":link});
    if link == "other" {
        value["state"] = json!("unsupported");
        value["reason"] = json!(
            "Network control requires a USB or emulator adb link; taking a wireless debugging target offline would sever its own control"
        );
    } else {
        value["state"] = json!("unknown");
        value["reason"] = json!("The Android API level is read when a control session opens");
    }
    value
}

/// Records the session's API-level probe: airplane mode needs API 30, the
/// radio switches API 21. A missing level keeps the capability unknown.
pub(crate) fn android_network(target: &mut Target, api_level: Option<u32>) {
    let Some(value) = target.capabilities.get_mut("network") else {
        return;
    };
    if value["state"] == "unsupported" {
        return;
    }
    let Some(api) = api_level else {
        value["state"] = json!("unknown");
        value["reason"] = json!("The target did not report its API level");
        return;
    };
    value["apiLevel"] = json!(api);
    let methods: Vec<&str> = [
        crate::network::Method::Airplane,
        crate::network::Method::Radios,
    ]
    .into_iter()
    .filter(|method| api >= method.minimum_api())
    .map(crate::network::Method::name)
    .collect();
    if methods.is_empty() {
        value["state"] = json!("unsupported");
        value["reason"] =
            json!("Android API 21 or later is required for adb connectivity switches");
        return;
    }
    value["state"] = json!("supported");
    value["methods"] = json!(methods);
    value["sideEffects"] = json!({"airplane":["bluetooth_off"]});
    value.as_object_mut().unwrap().remove("reason");
}

/// The methods a session verified for this target, if any.
pub(crate) fn android_network_methods(capabilities: &Value) -> Option<(Vec<String>, String)> {
    let network = &capabilities["network"];
    if network["state"] != "supported" {
        return None;
    }
    Some((
        network["methods"]
            .as_array()?
            .iter()
            .filter_map(|m| m.as_str().map(str::to_owned))
            .collect(),
        network["link"].as_str().unwrap_or("other").to_owned(),
    ))
}

/// Records the session's probe of `input keycombination` support.
pub(crate) fn android_key_chords(target: &mut Target, supported: bool) {
    if let Some(key) = target.capabilities.get_mut("key")
        && key["state"] == "supported"
    {
        key["modifiers"] = json!(supported);
        if supported {
            key.as_object_mut().unwrap().remove("modifiersReason");
        } else {
            key["modifiersReason"] =
                json!("The selected target's input utility does not list keycombination");
        }
    }
}

/// Whether a session verified Android key chords for this target.
pub(crate) fn android_chords(capabilities: &Value) -> bool {
    capabilities["key"]["modifiers"] == true
}

pub(crate) fn support(target: &mut Target, key: &str, details: Value) {
    let mut value = details;
    value["state"] = json!("supported");
    target.capabilities.insert(key.into(), value);
}

pub(crate) fn input(target: &mut Target, backend: &str) {
    for key in ["input", "tap", "swipe", "long_press", "key", "type_text"] {
        let mut details = if key == "type_text" {
            json!({"backend":backend,"scope":"display","encoding":"us_ascii","replace":target.kind=="simulator","elementTarget":false,"imeComposition":false,"requiresVerifiedFocus":target.kind=="simulator"})
        } else {
            json!({"backend":backend,"scope":"display"})
        };
        if matches!(key, "input" | "tap" | "swipe" | "long_press") {
            details["maxContacts"] = json!(1);
        }
        if target.kind == "device" && matches!(key, "key" | "type_text") {
            details["state"] = json!("unknown");
            details["reason"] = json!("Controller keyboard registration has not been verified");
            target.capabilities.insert(key.into(), details);
            continue;
        }
        support(target, key, details);
    }
}

/// Records that the session's virtual keyboard registered and accepted
/// reports. iOS acknowledges no key, so each effect still needs an observation.
#[cfg(target_os = "macos")]
pub(crate) fn keyboard(target: &mut Target) {
    for key in ["key", "type_text"] {
        let mut details = target.capabilities.get(key).cloned().unwrap_or(json!({}));
        details["registered"] = json!(true);
        details["deliveryAcknowledged"] = json!(false);
        details["reason"] = json!(
            "The keyboard is registered and keys are sent, but iOS does not acknowledge them; confirm each effect with observe"
        );
        support(target, key, details);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_probe_reports_methods_without_overriding_the_request_default() {
        for api in [21, 29, 30, 36] {
            let mut target = crate::targets::qa_fixture_target();
            target.kind = "android".into();
            target.capabilities.clear();
            android(&mut target.capabilities, "device", "usb");
            android_network(&mut target, Some(api));
            let capability = &target.capabilities["network"];
            assert_eq!(capability["state"], "supported");
            assert_eq!(capability["apiLevel"], api);
            assert_eq!(
                capability["methods"],
                if api < 30 {
                    json!(["radios"])
                } else {
                    json!(["airplane", "radios"])
                }
            );
            assert!(capability.get("defaultMethod").is_none());
            let capabilities = json!(target.capabilities);
            let (methods, link) = android_network_methods(&capabilities).unwrap();
            assert_eq!(link, "usb");
            let request = crate::network::Request::parse(&json!({"mode":"offline"})).unwrap();
            assert_eq!(request.method, crate::network::Method::Airplane);
            assert_eq!(
                methods.iter().any(|method| method == request.method.name()),
                api >= 30
            );
            assert!(methods.iter().any(|method| method == "radios"));
        }
    }

    #[test]
    fn android_discovery_publishes_common_operations_and_explicit_limits() {
        let mut values = BTreeMap::new();
        android(&mut values, "device", "usb");
        for key in ["tap", "swipe", "long_press", "key", "type_text", "open_url"] {
            assert_eq!(values[key]["state"], "supported", "{key}");
        }
        assert_eq!(values["network"]["state"], "unknown");
        assert_eq!(values["network"]["link"], "usb");
        let mut wireless = BTreeMap::new();
        android(&mut wireless, "device", "other");
        assert_eq!(wireless["network"]["state"], "unsupported");
        assert_eq!(wireless["tap"]["state"], "supported");
        assert!(values.keys().all(|key| !key.contains(char::is_uppercase)));
        assert_eq!(values["type_text"]["replace"], true);
        assert_eq!(values["key"]["modifiers"], false);
        for key in ["clipboard_read", "preview"] {
            assert_eq!(values[key]["state"], "unsupported", "{key}");
        }
        assert_eq!(values["recording"]["state"], "unknown");
        let mut offline = BTreeMap::new();
        android(&mut offline, "unauthorized", "usb");
        assert!(offline.values().all(|value| value["state"] == "unknown"));
        assert!(
            offline["tap"]["reason"]
                .as_str()
                .unwrap()
                .contains("unauthorized")
        );
    }
    #[test]
    fn selected_input_support_retains_backend_limits_and_shared_names() {
        for kind in ["device", "simulator"] {
            let mut target = crate::targets::qa_fixture_target();
            target.kind = kind.into();
            complete(&mut target.capabilities, kind);
            assert_eq!(target.capabilities["key"]["state"], "unknown");
            input(&mut target, "fixture");
            assert_eq!(
                target.capabilities["key"]["state"],
                if kind == "simulator" {
                    "supported"
                } else {
                    "unknown"
                }
            );
            assert_eq!(
                target.capabilities["type_text"]["requiresVerifiedFocus"],
                kind == "simulator"
            );
            assert!(!target.capabilities.contains_key("text"));
            assert_eq!(
                target.capabilities["type_text"]["replace"],
                kind == "simulator"
            );
        }
    }
}
