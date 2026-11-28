//! Public browser tool definitions and strict envelopes above lifecycle models.
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineInput {
    pub engine: super::contract::Engine,
}

pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "browser_list"
            | "browser_doctor"
            | "browser_connect"
            | "browser_status"
            | "browser_send"
            | "browser_cancel"
            | "browser_disconnect"
            | "browser_control_status"
            | "browser_control_acquire"
            | "browser_control_release"
    )
}
/// Owner-only routes behind shared tools, not additional MCP tools.
pub(crate) fn definition(name: &str) -> Option<Value> {
    if let Some(public) = name.strip_prefix("browser_control_") {
        let tool = crate::catalog::tool_named(&format!("control_{public}"))?;
        let schema = tool["inputSchema"]["anyOf"][1].clone();
        return Some(json!({"name":name,"inputSchema":schema}));
    }
    cached().iter().find(|tool| tool["name"] == name).cloned()
}
/// Validates `args` for browser tool `name`, whose schema is prepared once
/// per process: every browser call takes this path.
pub(crate) fn validate(name: &str, args: &Value) -> crate::error::Result<()> {
    super::transfer::fixture_path_guidance(name, args)?;
    static SCHEMAS: std::sync::LazyLock<crate::schema::PreparedTools> =
        std::sync::LazyLock::new(Default::default);
    SCHEMAS
        .get(name, super::transfer::argument_byte_limit(name), || {
            let mut tool = definition(name)
                .ok_or_else(|| crate::error::Error::invalid("Unknown browser tool"))?;
            Ok(tool["inputSchema"].take())
        })?
        .validate(args)
}
pub(crate) fn tools() -> Vec<Value> {
    cached().to_vec()
}
/// The browser tools, built once per process: they are static data.
fn cached() -> &'static [Value] {
    static TOOLS: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    TOOLS.get_or_init(build)
}
fn build() -> Vec<Value> {
    let schema = super::contract::schemas();
    let mut request = schema["request"].clone();
    request.as_object_mut().unwrap().remove("$defs");
    let target = json!({"type":"string","minLength":1,"description":"Browser to use, as browser_connect returned it; may be omitted while one browser is connected or a reference or operation names it"});
    [
        ("browser_list","List Chrome, Firefox and WebKit availability and missing private dependencies without downloading or launching",object(json!({}),&[]),true),
        ("browser_doctor","Check one browser's host support and private dependency inventory without downloading or launching",object(json!({"engine":{"enum":["chrome","firefox","webkit"]}}),&["engine"]),true),
        ("browser_connect","Launch a browser with an isolated or selected profile, or attach to an explicit running browser endpoint",schema["launch"].clone(),false),
        ("browser_status","Get compact browser status. Use includeDetails for resources or operationId for one full result. waitMs waits for that operation, or preparation and the oldest pending operation on one browser.",object(json!({"targetId":target,"operationId":{"type":"string","format":"uuid"},"waitMs":{"type":"integer","minimum":0,"maximum":30000,"default":0},"includeDetails":{"type":"boolean","default":false}}),&[]),true),
        ("browser_send","Submit browser lifecycle, page control, environment settings, evaluation, diagnostics, previews, recordings, screenshots or registered transfers; return an operation ID",object(json!({"targetId":target,"request":request,"operationId":{"type":"string","format":"uuid"},"timeoutMs":{"type":"integer","minimum":1,"maximum":900000,"default":30000}}),&["request"]),false),
        ("browser_cancel","Request cancellation of a browser operation without replaying or undoing dispatched work",object(json!({"targetId":target,"operationId":{"type":"string","format":"uuid"}}),&["operationId"]),false),
        ("browser_disconnect","Close one browser for every client, keeping the other browsers and devices",object(json!({"targetId":target}),&[]),false),
    ].into_iter().map(|(name,description,mut input,read)|{
        if name=="browser_send" {input["$defs"]=schema["request"]["$defs"].clone();}
        let mut tool=json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":true}});
        if matches!(name,"browser_list"|"browser_status") {crate::pagination::declare(&mut tool,canonic_mcp_kit::page::Limits::STANDARD);}
        if name == "browser_status" {
            tool["inputSchema"]["properties"]["limit"]["description"] = json!(
                "Maximum browsers per page. For one selected browser, compact status uses this as the operation summary limit (default 3, maximum 16)."
            );
        }
        tool
    }).collect()
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

#[cfg(test)]
#[test]
fn fixture_path_points_to_cli_without_admitting_a_host_read() {
    let registration = json!({"operation":"fixture_register","fixture":{"path":"private.txt"}});
    let errors = [
        validate("browser_send", &json!({"request":registration})).unwrap_err(),
        validate(
            "browser_send",
            &json!({"request":{"operation":"batch","requests":[registration]}}),
        )
        .unwrap_err(),
        super::api::translate(
            "act",
            json!({"actions":[{"action":"fixture_register","fixture":registration["fixture"]}]}),
        )
        .unwrap_err(),
    ];
    for error in errors {
        assert!(
            error
                .message
                .contains("testrun browser fixture-register PATH")
        );
        assert!(!error.message.contains("private.txt"));
    }
}
