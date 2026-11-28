//! Published tools for connected devices and their annotations.
use super::{schema, tool};
use serde_json::{Value, json};

/// Fields each `act` action accepts besides `action`.
pub(crate) fn action_fields(action: &str) -> Option<&'static [&'static str]> {
    Some(match action {
        "tap" => &["point", "element", "allowPartial"],
        "swipe" => &["from", "to", "durationMs"],
        "long_press" => &["point", "durationMs", "element", "allowPartial"],
        "key" => &["key", "modifiers"],
        "type_text" => &["text", "replace", "element"],
        "app_launch" | "app_restart" => &["appId", "readiness", "args", "env"],
        "app_terminate" => &["appId"],
        "open_url" => &["url"],
        "delay" => &["durationMs"],
        _ => return None,
    })
}

pub(crate) fn tools() -> Vec<Value> {
    let point = json!({"type":"object","properties":{"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1}},"required":["x","y"],"additionalProperties":false});
    let mut action = schema(
        json!({"action":{"enum":["tap","swipe","long_press","key","type_text","app_launch","app_terminate","app_restart","open_url","delay"]},"point":point,"from":point,"to":point,"durationMs":{"type":"integer","minimum":0,"maximum":crate::catalog::DELAY_MAX_MS},"key":super::key_schema(),"modifiers":{"type":"array","maxItems":4,"items":{"enum":["shift","control","alt","meta"]}},"text":{"type":"string"},"replace":{"type":"boolean"},"appId":{"type":"string","minLength":1},"url":{"type":"string","minLength":1}}),
        vec!["action"],
    );
    action["properties"]["durationMs"]["description"] = json!(
        "Delay: fixed wait in ms, without input or screenshot. Swipe/long_press: gesture ms (0..10000), defaulting to 150/500 respectively."
    );
    action["properties"]["point"]["description"] = json!(
        "Use observed display fractions, including ui_find centers; never assume screen dimensions."
    );
    action["properties"]["action"]["description"] = json!(
        "Physical iPhone HID: dismiss Control Center by tapping empty background or Command-H; bottom-edge swipes may page it. Close Spotlight with Escape twice, not Command-H."
    );
    action["properties"]["readiness"] = super::readiness();
    action["properties"]["args"] =
        json!({"type":"array","maxItems":64,"items":{"type":"string","maxLength":4096}});
    action["properties"]["env"] = json!({"type":"object","maxProperties":128,"additionalProperties":{"type":"string","maxLength":16384}});
    action["properties"]["element"] = super::element();
    action["properties"]["allowPartial"] = json!({"type":"boolean","default":false});
    let mut actions = json!({"type":"array","minItems":1,"maxItems":8,"items":action});
    actions["description"] = json!(super::ACTION_PACING);
    let profile = json!({"enum":["fast","evidence"],"default":"fast"});
    let mut tools = vec![
        tool(
            "connect",
            "Connect or recover a device and return a screenshot, preserving other connected targets",
            schema(
                json!({"targetId":{"type":"string","minLength":1},"boot":{"type":"boolean","default":false,"description":"Start a shut-down simulator or android:avd: emulator; disconnect stops it"},"window":{"type":"boolean","default":false,"description":"Show an emulator started with boot"},"controlPath":{"enum":["hid","xctest"],"description":"Physical iPhone: hid defaults to display input without element positions or field text. HID typing hides the on-screen keyboard until disconnect; disconnect before manual typing. Reconnect with xctest for app-scoped input/inspection and a visible keyboard; cover both paths when keyboard behavior matters."},"appId":{"type":"string","minLength":1,"description":"App inspected over xctest"},"setup":{"type":"boolean","default":false,"description":"Install the signed XCTest runner using the saved team or first-use signing selection; the user may need to trust the developer or enter a passcode"},"prepareSupport":{"type":"boolean","default":false},"runnerTeamId":{"type":"string","minLength":1,"description":"Signing team for this connection; host default: testrun signing setup"}}),
                vec!["targetId"],
            ),
            false,
        ),
        tool(
            "act",
            "Perform up to 8 ordered actions and return screenshots; uncertain input is never repeated",
            schema(
                json!({"actions":actions,"observeAfterEach":{"type":"boolean","default":true},"profile":profile,"settle":{"type":"boolean","default":true,"description":"iPhone HID: up to 2 s for visual change, then 300 ms stillness within 1.5 s; no-effect input may use the full change window. Android: screencaps until 300 ms stillness, within 1.5 s. This does not prove success. False skips waits."}}),
                vec!["actions"],
            ),
            false,
        ),
        tool(
            "observe",
            "Return a fresh screenshot of a connected target without sending input",
            schema(json!({"profile":profile}), vec![]),
            true,
        ),
        tool(
            "status",
            "Return each connected device's state and the outcome of the last action without repeating it",
            schema(json!({"actionId":{"type":"string","minLength":1}}), vec![]),
            true,
        ),
        tool(
            "disconnect",
            "With targetId, disconnect that device and stop owned resources; otherwise release this client, closing devices when no clients remain",
            schema(json!({}), vec![]),
            false,
        ),
        tool(
            "control_status",
            "Return whether the agent or a local viewer controls input, with the current controlVersion",
            schema(json!({}), vec![]),
            true,
        ),
        tool(
            "control_acquire",
            "Give input control to the agent or a local viewer; queued input from the previous owner is not sent",
            schema(
                json!({"owner":{"enum":["agent","viewer"]},"viewerId":{"type":"string","minLength":1,"maxLength":128},"expectedVersion":{"type":"integer","minimum":1}}),
                vec!["owner", "expectedVersion"],
            ),
            false,
        ),
        tool(
            "control_release",
            "Release input control while screenshots, previews and recordings continue",
            schema(
                json!({"expectedVersion":{"type":"integer","minimum":1}}),
                vec!["expectedVersion"],
            ),
            false,
        ),
    ];
    for tool in &mut tools {
        if tool["name"] == "connect" {
            tool["inputSchema"]["properties"]["androidCapture"] = json!({"enum":["screenshot","stream"],"description":"Experimental Android fast capture preference; defaults to screenshot. Stream may use fresh raw fallback. Disconnect before changing an active setting."});
        }
        if matches!(tool["name"].as_str(), Some("connect" | "disconnect")) {
            tool["annotations"]["idempotentHint"] = json!(true);
        }
    }
    tools
}
