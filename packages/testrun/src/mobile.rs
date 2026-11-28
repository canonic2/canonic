//! Pure mobile mutation validation and outcome rules; adapters own device I/O.
use crate::{
    error::{Error, Result},
    targets::Target,
    ui::Point,
};
use serde_json::{Value, json};
use std::time::Duration;
mod adapters;
pub(crate) use adapters::perform;
pub(crate) use adapters::preflight;

pub struct Mutation {
    pub operation: String,
    pub arguments: Value,
    pub target: Target,
    pub command_id: String,
    pub timeout: Duration,
}
fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::invalid(format!("Required {key} is missing")))
}
fn point(args: &Value, key: &str) -> Result<[f64; 2]> {
    let point: Point = serde_json::from_value(
        args.get(key)
            .cloned()
            .ok_or_else(|| Error::invalid("A typed point is required"))?,
    )
    .map_err(|_| Error::invalid("Invalid normalized point"))?;
    point.validate()
}
pub fn validate(mutation: &Mutation) -> Result<()> {
    let name = mutation.operation.as_str();
    let args = &mutation.arguments;
    if mutation.target.kind == "android" {
        validate_android(
            name,
            args,
            crate::targets::capabilities::android_chords(&json!(mutation.target.capabilities)),
        )?;
    } else {
        validate_backend_options(&mutation.target.kind, None, name, args)?;
    }
    if matches!(name, "tap" | "long_press") {
        crate::ui::semantic::validate_pointer(args)?;
        if args.get("point").is_some() {
            point(args, "point")?;
        }
        if mutation.target.kind == "device" && args.get("element").is_some() {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Physical semantic pointer targets are unavailable",
            ));
        }
    }
    if name == "swipe" {
        point(args, "from")?;
        point(args, "to")?;
    }
    if mutation.target.kind == "device"
        && matches!(name, "tap" | "long_press" | "swipe" | "key" | "type_text")
        && !matches!(args["scope"]["kind"].as_str(), Some("app" | "system"))
    {
        return Err(Error::invalid(
            "Physical input requires an explicit app or system UI scope",
        ));
    }
    if mutation.target.kind == "device" && args["scope"]["kind"] == "app" {
        string(&args["scope"], "appId")?;
    }
    if matches!(name, "long_press" | "swipe") {
        let duration = args["durationMs"].as_u64().ok_or_else(|| {
            Error::invalid("durationMs must be an integer between 0 and 10000 ms")
        })?;
        crate::ui::validate_gesture_duration(duration)?;
    }
    Ok(())
}

/// A UI scope as the owned XCTest runner reads it, with the app as `bundleId`.
///
/// The runner reads one app's tree or the Home Screen, so a physical iPhone
/// takes `app` with an `appId` or `system`; whole-display scope is refused here
/// rather than reaching the runner as an app scope without a bundle.
pub(crate) fn runner_scope(scope: &Value) -> Result<Value> {
    match scope["kind"].as_str() {
        Some("system") => {}
        Some("app") => {
            string(scope, "appId")
                .map_err(|_| Error::invalid("App scope on a physical iPhone requires appId"))?;
        }
        _ => {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "A physical iPhone reads one app or the Home Screen: use scope app with appId, or system; display scope is unavailable",
            ));
        }
    }
    let mut scope = scope.clone();
    if let Some(fields) = scope.as_object_mut()
        && let Some(app) = fields.remove("appId")
    {
        fields.insert("bundleId".into(), app);
    }
    Ok(scope)
}

/// Static mobile constraints shared by managed admission, batches and direct adapters.
pub(crate) fn validate_backend_options(
    kind: &str,
    control_path: Option<&str>,
    name: &str,
    args: &Value,
) -> Result<()> {
    if kind == "device" && matches!(name, "tap" | "long_press") && args.get("element").is_some() {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Physical semantic pointer targets are unavailable",
        ));
    }
    if kind == "android" {
        return validate_android(name, args, false);
    }
    if kind == "simulator"
        && args
            .get("scope")
            .is_some_and(|scope| scope["kind"] != "display")
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Simulator input supports full-display scope only; app/system confinement is unavailable",
        ));
    }
    if kind == "simulator" && name == "key" && args.get("key").is_some() {
        let modifiers: Vec<String> =
            serde_json::from_value(args.get("modifiers").cloned().unwrap_or(json!([])))
                .map_err(|_| Error::invalid("Invalid key modifiers"))?;
        crate::keyboard::key(string(args, "key")?, &modifiers)?;
    }
    if kind == "device"
        && matches!(name, "app_launch" | "app_restart")
        && args.get("readiness").is_some()
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Physical iOS launch readiness is unavailable; launch without readiness and inspect the returned image",
        ));
    }
    if matches!(kind, "simulator" | "device") && matches!(name, "app_launch" | "app_restart") {
        crate::app_lifecycle::validate(args)?;
    }
    if kind == "device" && (name == "open_url" || (name == "key" && control_path != Some("hid"))) {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Physical iOS does not support key chords or URL opening through the owned runner",
        ));
    }
    if kind == "device" && control_path == Some("hid") {
        if args.get("element").is_some() || args["replace"] == true {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "HID keyboard input targets current display focus; element targets and replacement are unavailable",
            ));
        }
        if name == "type_text" {
            crate::keyboard::text(string(args, "text")?)?;
        } else if name == "key" {
            let modifiers: Vec<String> =
                serde_json::from_value(args.get("modifiers").cloned().unwrap_or(json!([])))
                    .map_err(|_| Error::invalid("Invalid key modifiers"))?;
            crate::keyboard::key(string(args, "key")?, &modifiers)?;
        }
        return Ok(());
    }
    if name != "type_text" {
        return Ok(());
    }
    if kind == "simulator" && args.get("text").is_some() {
        crate::keyboard::text(string(args, "text")?)?;
    }
    if kind == "device" && args["text"] == "" {
        return Err(Error::invalid(
            "Empty text is only available for Android targeted replacement",
        ));
    }
    if kind == "simulator" && args.get("element").is_some() {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Simulator typing cannot resolve an element; focus the intended field and omit element",
        ));
    }
    if kind == "device" && args["replace"] == true {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Device replacement typing requires a verified semantic editing adapter",
        ));
    }
    if kind == "device" && args["element"].get("query").is_some() {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Physical iOS typing requires snapshotId and elementId",
        ));
    }
    Ok(())
}
/// Android options the adb adapter cannot enforce are refused before any receipt
/// or input. Focus and geometry stay execution checks: earlier actions change them.
/// `chords` is the selected session's keycombination probe result.
pub(crate) fn validate_android(name: &str, args: &Value, chords: bool) -> Result<()> {
    if args
        .get("scope")
        .is_some_and(|scope| scope["kind"] != "display")
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Android input supports full-display scope only; app/system confinement is unavailable",
        ));
    }
    match name {
        "key" => {
            let modifiers: Vec<String> =
                serde_json::from_value(args.get("modifiers").cloned().unwrap_or(json!([])))
                    .map_err(|_| Error::invalid("Invalid key modifiers"))?;
            if !modifiers.is_empty() && !chords {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Android key chords are unavailable on this target; its input utility does not provide keycombination",
                ));
            }
            crate::android::key_codes(string(args, "key")?, &modifiers)?;
        }
        "type_text" => {
            if args["replace"] == true {
                let target =
                    crate::ui::semantic::parse_target(args.get("element").ok_or_else(|| {
                        Error::new(
                            "CAPABILITY_UNAVAILABLE",
                            "Android replacement requires an explicit element",
                        )
                    })?)?;
                if !matches!(target, crate::ui::semantic::Target::Query { .. }) {
                    return Err(Error::new(
                        "CAPABILITY_UNAVAILABLE",
                        "Android replacement requires a fresh query; cross-connection reference identity is unavailable",
                    ));
                }
                let text = args["text"]
                    .as_str()
                    .ok_or_else(|| Error::invalid("text must be a string"))?;
                if text.len() > 65536 || text.contains('\0') {
                    return Err(Error::invalid(
                        "Replacement text must be at most 64 KiB and contain no NUL",
                    ));
                }
            } else {
                if args.get("element").is_some() {
                    return Err(Error::new(
                        "CAPABILITY_UNAVAILABLE",
                        "Android element-targeted text requires replace: true",
                    ));
                }
                crate::android::text_argument(string(args, "text")?)?;
            }
        }
        "app_launch" | "app_restart" => {
            if !crate::app_lifecycle::launch_environment(args)?.is_empty() {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Android process launch environment requires an application adapter",
                ));
            }
            if args
                .get("args")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
            {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Android launch arguments require a declared application adapter",
                ));
            }
            crate::app_lifecycle::Readiness::parse(args)?;
        }
        "open_url" => {
            url(args)?;
        }
        _ => {}
    }
    Ok(())
}

/// A bounded URL with a scheme and no whitespace or control characters.
pub(crate) fn url(args: &Value) -> Result<&str> {
    let url = string(args, "url")?;
    let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
    if url.len() > 4096
        || scheme.is_empty()
        || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(Error::invalid(
            "Invalid URL; supply a scheme and no whitespace, within 4096 bytes",
        ));
    }
    Ok(url)
}

pub fn status(command_id: &str, result: Result<Value>) -> Value {
    match result {
        Ok(data) if data["commandId"] == command_id && data["state"].is_string() => data,
        Ok(data) => {
            json!({"commandId":command_id,"state":"completed","observed":false,"result":data})
        }
        Err(error) => {
            json!({"commandId":command_id,"state":if matches!(error.code.as_str(),"TIMEOUT"|"OUTCOME_UNKNOWN"|"DEVICE_DISCONNECTED"){"outcome_unknown"}else{"failed"},"error":error})
        }
    }
}

/// Preserve the journal while exposing failed dispatches as failed tool calls.
pub fn completed(status: Value) -> Result<Value> {
    if matches!(
        status["state"].as_str(),
        Some("failed" | "outcome_unknown" | "dispatched")
    ) {
        let error = &status["error"];
        return Err(Error::new(
            error["code"].as_str().unwrap_or("OUTCOME_UNKNOWN"),
            error["message"]
                .as_str()
                .unwrap_or("Command outcome is unknown; inspect its journal before retrying"),
        )
        .details(status));
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runner_scope_names_supported_scopes_instead_of_a_missing_bundle() {
        assert_eq!(
            runner_scope(&json!({"kind":"app","appId":"com.example.acme"})).unwrap(),
            json!({"kind":"app","bundleId":"com.example.acme"})
        );
        assert_eq!(
            runner_scope(&json!({"kind":"system"})).unwrap(),
            json!({"kind":"system"})
        );
        let display = runner_scope(&json!({"kind":"display"})).unwrap_err();
        assert_eq!(display.code, "CAPABILITY_UNAVAILABLE");
        assert!(display.message.contains("display scope is unavailable"));
        let app = runner_scope(&json!({"kind":"app"})).unwrap_err();
        assert!(app.message.contains("requires appId"));
    }

    #[test]
    fn simulator_text_is_admitted_before_any_batch_input() {
        validate_backend_options(
            "simulator",
            None,
            "type_text",
            &json!({"text":"Acme","replace":true}),
        )
        .unwrap();
        assert_eq!(
            validate_backend_options("simulator", None, "type_text", &json!({"text":"日本語"}))
                .unwrap_err()
                .code,
            "CAPABILITY_UNAVAILABLE"
        );
        assert_eq!(
            validate_backend_options("simulator", None, "type_text", &json!({"text":""}))
                .unwrap_err()
                .code,
            "INVALID_ARGUMENT"
        );
    }
    #[test]
    fn hid_keyboard_admission_accepts_ascii_and_refuses_unsupported_options() {
        for (operation, args) in [
            ("type_text", json!({"text":"Acme.qa@example.com"})),
            ("key", json!({"key":"backspace"})),
            ("key", json!({"key":"a","modifiers":["shift","control"]})),
        ] {
            validate_backend_options("device", Some("hid"), operation, &args).unwrap();
        }
        for args in [
            json!({"text":"Acmeé"}),
            json!({"text":"Acme","replace":true}),
            json!({"text":"Acme","element":{"snapshotId":"s","elementId":"e"}}),
        ] {
            assert_eq!(
                validate_backend_options("device", Some("hid"), "type_text", &args)
                    .unwrap_err()
                    .code,
                "CAPABILITY_UNAVAILABLE"
            );
        }
    }
    #[test]
    fn android_static_options_accept_what_adb_supports() {
        for (name, args, chords) in [
            ("tap", json!({}), false),
            ("tap", json!({"scope":{"kind":"display"}}), false),
            ("key", json!({"key":"page_down"}), false),
            ("key", json!({"key":"z","modifiers":["meta"]}), true),
            ("type_text", json!({"text":"Acme 100%"}), false),
            (
                "type_text",
                json!({"text":"café 日本語 😀 %s","replace":true,"element":{"query":{"identifier":"acme.field"}}}),
                false,
            ),
            (
                "type_text",
                json!({"text":"","replace":true,"element":{"query":{"identifier":"acme.field"}}}),
                false,
            ),
            ("open_url", json!({"url":"acme+qa://example/path"}), false),
        ] {
            validate_android(name, &args, chords).unwrap();
        }
    }
    #[test]
    fn android_static_options_refuse_what_adb_cannot_enforce() {
        for (name, args, chords, code) in [
            (
                "tap",
                json!({"scope":{"kind":"app","appId":"com.example.other"}}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "swipe",
                json!({"scope":{"kind":"system"}}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "key",
                json!({"key":"a","modifiers":["shift"]}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "key",
                json!({"key":"a","modifiers":["shift","shift"]}),
                true,
                "INVALID_ARGUMENT",
            ),
            ("key", json!({"key":"f13"}), true, "CAPABILITY_UNAVAILABLE"),
            (
                "type_text",
                json!({"text":"Acme","element":{"snapshotId":"s","elementId":"e"}}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "type_text",
                json!({"text":"Acme","replace":true}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "type_text",
                json!({"text":"line\nbreak"}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "app_launch",
                json!({"appId":"com.example.acme","args":["--qa"]}),
                false,
                "CAPABILITY_UNAVAILABLE",
            ),
            (
                "open_url",
                json!({"url":"no-scheme"}),
                false,
                "INVALID_ARGUMENT",
            ),
            (
                "open_url",
                json!({"url":"https://example.com/a b"}),
                false,
                "INVALID_ARGUMENT",
            ),
        ] {
            assert_eq!(
                validate_android(name, &args, chords).unwrap_err().code,
                code,
                "{name} {args}"
            );
        }
    }
    #[test]
    fn android_chords_press_modifiers_before_the_key() {
        assert_eq!(
            crate::android::key_codes("a", &["control".into(), "shift".into()]).unwrap(),
            ["KEYCODE_CTRL_LEFT", "KEYCODE_SHIFT_LEFT", "KEYCODE_A"]
        );
        // Home and End are keyboard keys on every platform; Android's
        // navigation Home has its own name.
        assert_eq!(
            crate::android::key_codes("home", &[]).unwrap(),
            ["KEYCODE_MOVE_HOME"]
        );
        assert_eq!(
            crate::android::key_codes("home_screen", &[]).unwrap(),
            ["KEYCODE_HOME"]
        );
        assert_eq!(
            crate::android::key_codes("line_start", &[])
                .unwrap_err()
                .code,
            "CAPABILITY_UNAVAILABLE"
        );
    }
    #[test]
    fn simulator_scope_is_never_silently_downgraded() {
        for name in ["tap", "swipe", "key", "type_text"] {
            for scope in [
                json!({"kind":"app","appId":"com.example.acme"}),
                json!({"kind":"system"}),
            ] {
                assert_eq!(
                    validate_backend_options("simulator", None, name, &json!({"scope":scope}))
                        .unwrap_err()
                        .code,
                    "CAPABILITY_UNAVAILABLE"
                );
            }
            validate_backend_options("simulator", None, name, &json!({})).unwrap();
            validate_backend_options(
                "simulator",
                None,
                name,
                &json!({"scope":{"kind":"display"}}),
            )
            .unwrap();
        }
    }
    #[test]
    fn ios_typing_options_refuse_unsupported_targeting_before_dispatch() {
        for (kind, arguments) in [
            (
                "simulator",
                json!({"text":"Acme","element":{"snapshotId":"snapshot","elementId":"field"}}),
            ),
            (
                "device",
                json!({"text":"Acme","replace":true,"scope":{"kind":"app","appId":"com.example.acme"}}),
            ),
        ] {
            let mut target = crate::targets::qa_fixture_target();
            target.kind = kind.into();
            let mutation = Mutation {
                operation: "type_text".into(),
                arguments,
                target,
                command_id: "typing".into(),
                timeout: Duration::from_secs(1),
            };
            assert_eq!(
                validate(&mutation).unwrap_err().code,
                "CAPABILITY_UNAVAILABLE"
            );
        }
    }

    #[test]
    fn unsupported_physical_operations_refuse_before_runner_dispatch() {
        let mut target = crate::targets::qa_fixture_target();
        target.kind = "device".into();
        for operation in ["key", "open_url"] {
            let mutation = Mutation {
                operation: operation.into(),
                arguments: json!({"key":"home","url":"https://example.com"}),
                target: target.clone(),
                command_id: "unsupported".into(),
                timeout: Duration::from_secs(1),
            };
            assert_eq!(
                validate(&mutation).unwrap_err().code,
                "CAPABILITY_UNAVAILABLE"
            );
        }
    }

    #[test]
    fn failed_and_uncertain_commands_are_never_successful_replies() {
        let failed = status(
            "failure",
            Err(Error::new("PLATFORM_FAILED", "Cannot launch app")),
        );
        let error = completed(failed.clone()).unwrap_err();
        assert_eq!(error.code, "PLATFORM_FAILED");
        assert_eq!(error.details.unwrap(), failed);
        let uncertain = status(
            "lost",
            Err(Error::new(
                "DEVICE_DISCONNECTED",
                "Disconnected after dispatch",
            )),
        );
        assert_eq!(uncertain["state"], "outcome_unknown");
        assert!(completed(uncertain).is_err());
        let runner = json!({"commandId":"pending","state":"dispatched"});
        assert!(completed(status("pending", Ok(runner))).is_err());
    }
}
