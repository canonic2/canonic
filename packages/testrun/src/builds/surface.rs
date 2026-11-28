//! Published build/configuration schemas and shared CLI request mapping.
use super::*;
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384})
}
fn env() -> Value {
    json!({"type":"object","maxProperties":128,"additionalProperties":{"type":"string","maxLength":16384}})
}
/// Optional absolute base file replacing `testrun.yml`/`.config/testrun.yml` selection.
pub(crate) fn config() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384,"description":"Absolute configuration file; default is testrun.yml, then .config/testrun.yml, in the root"})
}
fn tool(name: &str, description: &str, input: Value, read: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":!read}})
}
pub(crate) fn tools() -> Vec<Value> {
    let mut tools = Vec::new();
    for (name, description, read) in [
        (
            "config_init",
            "Create an empty testrun.yml, or the config file, and ignore its local overlay, preserving existing files",
            false,
        ),
        (
            "config_check",
            "Validate project build profiles without running tools or contacting devices",
            true,
        ),
        (
            "config_show",
            "Show redacted effective build profiles and source provenance",
            true,
        ),
        (
            "config_schema",
            "Return the shared or partial-local YAML configuration schema",
            true,
        ),
    ] {
        tools.push(tool(
            name,
            description,
            object(
                json!({"root":text(),"project":text(),"config":config(),"version":{"enum":[1,2]},"local":{"type":"boolean"}}),
                &[],
            ),
            read,
        ));
    }
    let overrides = object(
        json!({"configuration":text(),"variant":text(),"flavor":text(),"entry_point":text(),"mode":{"enum":["debug","profile","release"]},"timeout_ms":{"type":"integer","minimum":1000,"maximum":3600000},"launch_args":{"type":"array","maxItems":64,"items":text()},"launch_env":env()}),
        &[],
    );
    tools.push(tool("build_start","Submit a retained test-app build; use submissionId for safe retries and build_status to recover a lost reply",object(json!({"project":text(),"submissionId":{"type":"string","minLength":1,"maxLength":128},"profile":text(),"inline":crate::configuration::schema::profile(false),"env":env(),"bindings":env(),"baseline":env(),"overrides":overrides,"install":{"type":"boolean"},"launch":{"type":"boolean"},"replace":{"type":"boolean"},"boot":{"type":"boolean"},"targetId":text()}),&["project"]),false));
    for (name, description, read) in [
        (
            "build_status",
            "Inspect a retained build and each device-stage outcome without repeating work",
            true,
        ),
        (
            "build_cancel",
            "Cancel a build process or stop before the next device stage; dispatched device stages settle first",
            false,
        ),
    ] {
        tools.push(tool(
            name,
            description,
            object(json!({"operationId":text(),"submissionId":{"type":"string","minLength":1,"maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"integer","minimum":0,"maximum":4096}}), &[]),
            read,
        ));
    }
    let start = tools
        .iter_mut()
        .find(|tool| tool["name"] == "build_start")
        .unwrap();
    start["inputSchema"]["properties"]["prepare"] = json!({"type":"boolean","description":"Run the profile's declared preparation before building"});
    start["inputSchema"]["properties"]["root"] = text();
    start["inputSchema"]["properties"]["config"] = config();
    tools.push(tool("build_log", "Read a bounded private operation log; output may contain project secrets", object(json!({"operationId":text(),"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":65536}}), &["operationId"]),true));
    for tool in &mut tools {
        match tool["name"].as_str().unwrap_or_default() {
            "build_status" => {
                crate::pagination::declare(tool, canonic_mcp_kit::page::Limits::STANDARD)
            }
            "build_log" => crate::pagination::declare(tool, crate::pagination::BYTES),
            _ => {}
        }
    }
    tools
}
