//! Shared project configuration and callable schemas.
use serde_json::{Value, json};
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384})
}
pub(crate) fn development(local: bool) -> Value {
    let task = object(
        json!({"command":{"type":"array","minItems":1,"maxItems":128,"items":text()},"cwd":text(),"timeout_ms":{"type":"integer","minimum":1000,"maximum":3600000},"effects":{"type":"array","maxItems":128,"items":text()},"expected_output":text()}),
        if local { &[] } else { &["command"] },
    );
    object(
        json!({"app_id":text(),"variant":text(),"expo":object(json!({"url":text(),"slug":text(),"connection_url":text(),"owner":text(),"connection_mode":{"enum":["lan","tunnel","forwarded"]}}),if local {&[]} else {&["url","slug"]}),"preflight":task,"prepare":task,"server":task,"checks":{"type":"object","maxProperties":128,"additionalProperties":task}}),
        if local { &[] } else { &["app_id"] },
    )
}
pub(crate) fn tools() -> Vec<Value> {
    let mut start = crate::builds::surface::tools()
        .into_iter()
        .find(|v| v["name"] == "build_start")
        .unwrap();
    start["name"] = json!("project_start");
    start["description"] = json!(
        "Run a declared project task or build; inspect and cancel its retained operation with build_status, build_log and build_cancel"
    );
    let properties = start["inputSchema"]["properties"].as_object_mut().unwrap();
    properties.remove("inline");
    properties.insert(
        "task".into(),
        json!({"enum":["preflight","prepare","server","check","build"]}),
    );
    properties.insert("check".into(), text());
    start["inputSchema"]["required"] = json!(["project", "profile", "task"]);
    let config = crate::builds::surface::config();
    let mut tools = vec![
        json!({"name":"project_list","description":"List configured applications and project-local profiles without starting runtimes","inputSchema":object(json!({"root":text(),"config":config,"offset":{"type":"integer","minimum":0,"maximum":128},"limit":{"type":"integer","minimum":1,"maximum":100}}),&["root"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}),
        json!({"name":"project_show","description":"Inspect a selected application's redacted configuration and source provenance","inputSchema":object(json!({"root":text(),"config":config,"project":text()}),&["root","project"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}),
        start,
        json!({"name":"project_status","description":"Inspect project configuration; legacy Expo profiles also inspect manifests and optional target app inventory","inputSchema":object(json!({"root":text(),"config":config,"project":text(),"profile":text(),"targetId":text()}),&["project","profile"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"project_open","description":"Open a legacy Expo profile on a connected simulator or Android target; unavailable adapters return a capability error","inputSchema":object(json!({"root":text(),"config":config,"project":text(),"profile":text(),"targetId":text()}),&["project","profile","targetId"]),"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}}),
    ];
    tools.extend(crate::configuration::onboarding::api::tools());
    for tool in &mut tools {
        if tool["name"] == "project_list" {
            crate::pagination::declare(tool, canonic_mcp_kit::page::Limits::STANDARD);
        }
    }
    tools
}
