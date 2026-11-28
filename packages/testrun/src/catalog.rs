//! Pure tool definitions and action admission catalog shared by all entry points.
//!
//! [`tools`] is the published MCP catalog. [`internal_tools`] defines the
//! session-level operations the task controller, CLI and batches run on an
//! explicit run, goal and session; they are not published.
use serde_json::{Value, json};
mod managed;
pub(crate) use managed::action_fields;

pub(crate) const BATCH_TOOLS: &[&str] = &[
    "tap",
    "swipe",
    "long_press",
    "key",
    "type_text",
    "app_launch",
    "app_terminate",
    "app_restart",
    "open_url",
    "delay",
    "screenshot",
    "ui_snapshot",
    "preview_start",
    "preview_stop",
    "preview_status",
    "recording_start",
    "recording_stop",
    "recording_status",
];

/// Batch steps an Android session supports.
pub(crate) fn android_batch_tool(name: &str) -> bool {
    BATCH_TOOLS.contains(&name) && !name.starts_with("preview_") && !name.starts_with("recording_")
}

/// Session-level operations that the published catalog also exposes for the
/// task's connected device, without run, goal or session references.
const CONNECTED_READS: &[&str] = &[
    "app_list",
    "clipboard_read",
    "ui_snapshot",
    "ui_find",
    "ui_read",
    "wait",
    "preview_start",
    "preview_stop",
    "preview_status",
    "recording_start",
    "recording_stop",
    "recording_status",
];

/// Longest single delay step in an act or batch call.
pub(crate) const DELAY_MAX_MS: u64 = 60_000;

/// Arguments of a delay step, which exists only inside act and batch calls.
pub(crate) fn delay_schema() -> Value {
    schema(
        json!({"durationMs":{"type":"integer","minimum":0,"maximum":DELAY_MAX_MS}}),
        vec!["durationMs"],
    )
}

fn string() -> Value {
    json!({"type":"string","minLength":1})
}

fn key_schema() -> Value {
    json!({"type":"string","minLength":1,"description":"enter, escape, backspace, tab, space, delete, home/end (line navigation), page_up, page_down, arrow_left, arrow_right, arrow_up, arrow_down or a-z. Android: home_screen, back, app_switch, volume_up, volume_down, power. Physical iPhone HID Home Screen: h with modifiers [\"meta\"] (Command-H). XCTest has no chords; see the target's key capability for modifiers."})
}

const ACTION_PACING: &str = "Batch only decided actions. Pace animations with delay actions (durationMs); delay does not prove readiness. If the next action depends on the screen, inspect the returned image or observe before deciding.";
fn schema(properties: Value, required: Vec<&str>) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn tool(name: &str, description: &str, input: Value, read_only: bool) -> Value {
    let mut tool = json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"idempotentHint":read_only,"openWorldHint":true}});
    match name {
        "target_list" | "app_list" | "status" | "ui_find" => {
            crate::pagination::declare(&mut tool, canonic_mcp_kit::page::Limits::STANDARD)
        }
        "ui_snapshot" => crate::pagination::declare(&mut tool, crate::pagination::BYTES),
        _ => {}
    }
    tool
}
fn target_id() -> Value {
    schema(json!({"targetId":string()}), vec!["targetId"])
}

/// The published MCP catalog.
#[cfg(any(unix, test))]
pub fn tools() -> Vec<Value> {
    catalog().to_vec()
}
/// The published MCP catalog, built once per process: it is static data.
pub(crate) fn catalog() -> &'static [Value] {
    static CATALOG: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(build)
}
/// One catalog tool by name.
pub(crate) fn tool_named(name: &str) -> Option<&'static Value> {
    catalog().iter().find(|tool| tool["name"] == name)
}
fn build() -> Vec<Value> {
    let mut result = vec![
        tool(
            "target_list",
            "List iOS simulators, physical iPhones and Android devices and emulators with their capabilities",
            schema(
                json!({"kind":{"enum":["simulator","device","android"]},"state":string(),"cursor":string(),"probe":{"type":"boolean","default":false},"limit":{"type":"integer","minimum":1,"maximum":200}}),
                vec![],
            ),
            true,
        ),
        tool(
            "doctor",
            "Check whether a physical iPhone or Android device is ready for testing, without connecting or sending input",
            target_id(),
            true,
        ),
        tool(
            "developer_mode_reveal",
            "Show the Developer Mode setting on a physical iPhone; the user turns it on and confirms the restart",
            target_id(),
            false,
        ),
        tool(
            "developer_support_install",
            "Download and install iPhone developer support on an unlocked USB device; no manual setup step",
            target_id(),
            false,
        ),
    ];
    result.extend(managed::tools());
    result.push(tool(
        "app_install",
        "Install a local .app or .apk on a device; replace updates an installed app without uninstalling it",
        schema(
            json!({"targetId":string(),"path":string(),"artifactRef":string(),"replace":{"type":"boolean","default":false},"playProtect":{"enum":["fail","dont_send","send"],"default":"fail","description":"Android: answer to a Google Play Protect security-check prompt shown during the install; fail stops and reports it"}}),
            vec![],
        ),
        false,
    ));
    result.push(tool(
        "app_uninstall",
        "Uninstall an app and its data from a device",
        schema(json!({"targetId":string(),"appId":string()}), vec!["appId"]),
        false,
    ));
    let internal = internal_tools();
    for name in CONNECTED_READS {
        let mut definition = internal
            .iter()
            .find(|tool| tool["name"] == *name)
            .cloned()
            .expect("Connected reads have session definitions");
        let input = &mut definition["inputSchema"];
        for field in ["runId", "goalId", "sessionId"] {
            input["properties"].as_object_mut().unwrap().remove(field);
        }
        input["required"]
            .as_array_mut()
            .unwrap()
            .retain(|field| !matches!(field.as_str(), Some("runId" | "goalId" | "sessionId")));
        definition["description"] = json!(connected_description(name));
        result.push(definition);
    }
    result.extend(crate::playback::schema::tools());
    result.extend(crate::network::schema::tools());
    for tool in &mut result {
        if takes_held_target(tool["name"].as_str().unwrap_or_default()) {
            tool["inputSchema"]["properties"]["targetId"] = json!({
                "type":"string","minLength":1,
                "description":"Device to use; may be omitted while one device is connected"
            });
        }
    }
    result.extend(crate::browser::surface::tools());
    result.extend(crate::builds::surface::tools());
    result.extend(crate::project::schema::tools());
    result.extend(crate::diagnostics::tools());
    crate::browser::api::extend(&mut result);
    result.extend(crate::documentation::tools());
    declare_pages(&mut result);
    result
}

fn declare_pages(tools: &mut [Value]) {
    for tool in tools {
        match tool["name"].as_str().unwrap_or_default() {
            "target_list"
            | "app_list"
            | "project_list"
            | "browser_list"
            | "status"
            | "browser_status"
            | "ui_find"
            | "build_status"
            | "project_discover"
            | "diagnostics_query"
            | "diagnostics_project" => {
                crate::pagination::declare(tool, canonic_mcp_kit::page::Limits::STANDARD)
            }
            "ui_snapshot" | "diagnostics_read" | "build_log" => {
                crate::pagination::declare(tool, crate::pagination::BYTES)
            }
            _ => {}
        }
    }
}

/// Device tools that take an optional `targetId` naming a held target. Tools
/// that require one, or name their device through a preview or recording ID,
/// do not.
fn takes_held_target(name: &str) -> bool {
    !matches!(
        name,
        "target_list"
            | "doctor"
            | "developer_mode_reveal"
            | "developer_support_install"
            | "connect"
            | "app_install"
            | "app_uninstall"
            | "preview_status"
            | "preview_stop"
            | "recording_status"
            | "recording_stop"
    )
}

fn connected_description(name: &str) -> &'static str {
    match name {
        "app_list" => {
            "List launchable apps and home-screen names; search by name or ID, or use all for every installed app"
        }
        "clipboard_read" => "Return an iOS simulator's clipboard text",
        "ui_snapshot" => "Return the accessibility tree of the device's screen",
        "ui_find" => {
            "Find screen elements by identifier, role, label or value; return frames and display size in matching units, plus {x, y} display-fraction tap centers"
        }
        "ui_read" => "Return one element found by query or by elementId from an earlier snapshot",
        "wait" => {
            "Wait until an element or app appears, disappears or changes, without sending input"
        }
        "preview_start" => "Start a live video preview of the connected iOS device",
        "preview_stop" => "Stop a live preview",
        "preview_status" => "Return a live preview's health, size and frame age",
        "recording_start" => "Start recording the connected device's screen to an MP4",
        "recording_stop" => "Stop a recording and save the MP4",
        _ => "Return a recording's progress or its finished file",
    }
}

/// A definition by name: session-level operations first, then the published catalog.
#[cfg(any(unix, test))]
pub(crate) fn definition(name: &str) -> Option<Value> {
    internal_tools()
        .into_iter()
        .chain(tools())
        .find(|tool| tool["name"] == name)
}

/// Session-level operations on an explicit run, goal and session.
#[expect(
    clippy::too_many_lines,
    reason = "Declarative session schemas are maintained together"
)]
pub(crate) fn internal_tools() -> Vec<Value> {
    let mut result = vec![
        tool(
            "host_info",
            "Read host architecture and adapter protocol",
            schema(json!({}), vec![]),
            true,
        ),
        tool(
            "run.begin",
            "Begin a QA run with declared goals and private evidence",
            schema(
                json!({"goalIds":{"type":"array","items":string(),"minItems":1,"maxItems":100},"label":{"type":"string","maxLength":256}}),
                vec!["goalIds"],
            ),
            false,
        ),
        tool(
            "goal.begin",
            "Validate ownership of a declared run goal",
            schema(
                json!({"runId":string(),"goalId":string()}),
                vec!["runId", "goalId"],
            ),
            true,
        ),
        tool(
            "run.end",
            "Close run sessions and persist its private summary",
            schema(json!({"runId":string()}), vec!["runId"]),
            false,
        ),
        tool(
            "session_open",
            "Open an explicit target for a run goal",
            schema(
                json!({"runId":string(),"goalId":string(),"target":schema(json!({"targetId":string(),"generation":string()}),vec!["targetId","generation"]),"mode":{"enum":["control","observe"]},"androidCapture":{"enum":["screenshot","stream"]},"controlPath":{"enum":["hid","xctest"]},"requireCapabilities":{"type":"array","items":string()},"boot":{"type":"boolean","default":false},"window":{"type":"boolean","default":false},"setup":{"type":"boolean","default":false},"runnerTeamId":string(),"timeoutMs":{"type":"integer","minimum":1,"maximum":900000}}),
                vec!["runId", "goalId", "target", "mode"],
            ),
            false,
        ),
        tool(
            "batch",
            "Prevalidate 1..8 ordered steps, stop on failure and return a final screenshot",
            schema(
                json!({
                    "runId":string(),"goalId":string(),"sessionId":string(),"leaseId":string(),
                    "commandId":{"type":"string","minLength":1,"maxLength":96},
                    "display":schema(json!({"displayId":{"const":"main"},"geometryVersion":string()}),vec!["displayId","geometryVersion"]),
                    "scope":ui_scope(),"observe":{"type":"boolean","default":true},
                    "observeAfterEach":{"type":"boolean","default":false},
                    "observationProfile":{"enum":["evidence","fast"]},
                    "settle":{"type":"boolean","default":true},
                    "timeoutMs":{"type":"integer","minimum":1,"maximum":120000},
                    "steps":{"type":"array","minItems":1,"maxItems":8,"items":schema(json!({
                        "tool":{"type":"string","enum":BATCH_TOOLS},
                        "arguments":{"type":"object"}
                    }),vec!["tool","arguments"])}
                }),
                vec![
                    "runId",
                    "goalId",
                    "sessionId",
                    "leaseId",
                    "commandId",
                    "steps",
                ],
            ),
            false,
        ),
    ];
    for (name, readonly) in [
        ("session_status", true),
        ("session_close", false),
        ("app_list", true),
        ("clipboard_read", true),
        ("screenshot", true),
        ("preview_start", false),
        ("ui_snapshot", true),
        ("ui_find", true),
        ("ui_read", true),
        ("wait", true),
        ("tap", false),
        ("long_press", false),
        ("swipe", false),
        ("key", false),
        ("type_text", false),
        ("preview_stop", false),
        ("preview_status", true),
        ("recording_start", false),
        ("recording_status", true),
        ("recording_stop", false),
        ("command_status", true),
        ("app_launch", false),
        ("app_terminate", false),
        ("app_restart", false),
        ("open_url", false),
    ] {
        let mut properties = json!({"runId":string(),"goalId":string(),"sessionId":string()});
        let mut required = vec!["runId", "goalId", "sessionId"];
        if name == "command_status" {
            properties["commandId"] = string();
            required.push("commandId");
        }
        if matches!(name, "preview_stop" | "preview_status") {
            properties["previewId"] = string();
            required.push("previewId");
        }
        if name == "preview_start" {
            properties["format"] = json!({"enum":["h264","jpeg","hevc"]});
        }
        if name == "app_list" {
            properties["all"] = json!({"type":"boolean","default":false,"description":"Also list apps that cannot be opened, such as Android system packages without a home-screen icon"});
            let mut search = element_query()["properties"]["label"].clone();
            search["description"] = json!("Find apps whose home-screen name or app ID matches");
            properties["name"] = search;
        }
        if name == "screenshot" {
            properties["profile"] = json!({"enum":["evidence","fast"]});
            properties["display"] = schema(
                json!({"displayId":{"const":"main"},"geometryVersion":string()}),
                vec!["displayId", "geometryVersion"],
            );
            properties["name"] = json!({"type":"string","minLength":1,"maxLength":128});
            properties["format"] = json!({"enum":["png","jpeg"]});
            properties["scale"] = json!({"type":"number","exclusiveMinimum":0,"maximum":1});
            properties["privacy"] = json!({"enum":["local","shareable"],"default":"local"});
            properties["includeImage"] = json!({"type":"boolean","default":true});
            properties["region"] = schema(
                json!({"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"width":{"type":"number","exclusiveMinimum":0,"maximum":1},"height":{"type":"number","exclusiveMinimum":0,"maximum":1}}),
                vec!["x", "y", "width", "height"],
            );
        }
        if name == "recording_start" {
            properties["maxDurationMs"] = json!({"type":"integer","minimum":100,"maximum":600000});
        }
        if matches!(name, "recording_status" | "recording_stop") {
            properties["recordingId"] = string();
            required.push("recordingId");
        }
        if matches!(name, "ui_snapshot" | "ui_find") {
            properties["scope"] = ui_scope();
            properties["allowPartial"] = json!({"type":"boolean","default":false});
            required.push("scope");
            if name == "ui_snapshot" {
                properties["maxBytes"] = json!({"type":"integer","minimum":1024,"maximum":65536});
            } else {
                properties["query"] = element_query();
                properties["limit"] = json!({"type":"integer","minimum":1,"maximum":200});
                required.push("query");
            }
        }
        if matches!(name, "ui_read" | "wait") {
            properties["allowPartial"] = json!({"type":"boolean","default":false});
            if name == "ui_read" {
                properties["element"] = element();
                required.push("element");
            } else {
                properties["conditions"] = json!({"type":"array","minItems":1,"maxItems":8,"description":"Wait ends when any condition holds","items":schema(json!({"kind":{"enum":["exists","gone","value","state","foreground"]},"query":element_query(),"value":element_query()["properties"]["value"],"state":{"enum":["enabled","focused","selected","checked","clickable","scrollable"]},"equals":{"type":"boolean"},"appId":string()}),vec!["kind"])});
                properties["timeoutMs"] =
                    json!({"type":"integer","minimum":0,"maximum":30000,"default":10000});
                required.push("conditions");
            }
        }
        if matches!(
            name,
            "app_launch"
                | "app_terminate"
                | "app_restart"
                | "open_url"
                | "tap"
                | "long_press"
                | "swipe"
                | "key"
                | "type_text"
        ) {
            properties["leaseId"] = string();
            properties["commandId"] = string();
            properties["timeoutMs"] = json!({"type":"integer","minimum":1,"maximum":900000});
            required.extend(["leaseId", "commandId"]);
            if matches!(name, "tap" | "long_press" | "swipe" | "key" | "type_text") {
                properties["display"] = schema(
                    json!({"displayId":{"const":"main"},"geometryVersion":string()}),
                    vec!["displayId", "geometryVersion"],
                );
                required.push("display");
                properties["scope"] = ui_scope();
                if name == "key" {
                    properties["key"] = key_schema();
                    properties["modifiers"] = json!({"type":"array","maxItems":4,"items":{"enum":["shift","control","alt","meta"]}});
                    required.push("key");
                } else if name == "type_text" {
                    properties["text"] = json!({"type":"string","maxLength":65536});
                    properties["replace"] = json!({"type":"boolean","default":false});
                    properties["element"] = element();
                    required.push("text");
                } else {
                    let point = schema(
                        json!({"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1}}),
                        vec!["x", "y"],
                    );
                    if name == "swipe" {
                        properties["from"] = point.clone();
                        properties["to"] = point;
                        required.extend(["from", "to"]);
                    } else {
                        properties["point"] = point;
                        properties["element"] = element();
                        properties["allowPartial"] = json!({"type":"boolean","default":false});
                    }
                    if name != "tap" {
                        properties["durationMs"] =
                            json!({"type":"integer","minimum":0,"maximum":10000});
                        required.push("durationMs");
                    }
                }
            } else {
                let field = if name == "open_url" { "url" } else { "appId" };
                properties[field] = string();
                required.push(field);
                if matches!(name, "app_launch" | "app_restart") {
                    properties["args"] = json!({"type":"array","items":{"type":"string","maxLength":4096},"maxItems":64});
                    properties["env"] = json!({"type":"object","maxProperties":128,"additionalProperties":{"type":"string","maxLength":16384}});
                    properties["readiness"] = readiness();
                }
            }
        }
        if crate::managed::selected_operation(name) {
            required.retain(|key| !matches!(*key, "runId" | "goalId" | "sessionId"));
        }
        result.push(tool(name, name, schema(properties, required), readonly));
    }
    result
}
fn element_query() -> Value {
    let text = schema(
        json!({"text":{"type":"string","maxLength":8192},"mode":{"enum":["exact","substring","regex"]},"caseSensitive":{"type":"boolean","default":false}}),
        vec!["text", "mode"],
    );
    let mut query = schema(
        json!({"identifier":string(),"role":string(),"label":text,"value":text}),
        vec![],
    );
    query["minProperties"] = json!(1);
    query
}
/// An element chosen by a fresh query or by `snapshotId` and `elementId` from a snapshot.
pub(crate) fn element() -> Value {
    schema(
        json!({"query":element_query(),"snapshotId":string(),"elementId":string()}),
        vec![],
    )
}
/// Observed launch readiness: the app's window in the foreground, optionally
/// with an accessibility element matching `element`.
pub(crate) fn readiness() -> Value {
    let mut value = schema(
        json!({"element":element_query(),"timeoutMs":{"type":"integer","minimum":100,"maximum":30000,"default":10000}}),
        vec![],
    );
    value["description"] = json!(
        "Wait until the app's window is in the foreground and, with element, a matching accessibility element is shown"
    );
    value
}
fn ui_scope() -> Value {
    schema(
        json!({"kind":{"enum":["display","app","system"]},"appId":string()}),
        vec!["kind"],
    )
}
