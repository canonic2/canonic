//! Shared public verbs translated once into the retained browser runtime.
use crate::error::{Error, Result};
use serde_json::{Value, json};
mod certificate_paths;
mod tags;

/// Pinned `recordHar.path` is written relative to the caller. The archive
/// export refuses `..`, so relative paths resolve lexically like trace exports.
fn bind_record_har(
    options: Option<&mut Value>,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<()> {
    let Some(field) = options
        .and_then(|options| options.get_mut("recordHar"))
        .and_then(|record| record.get_mut("path"))
    else {
        return Ok(());
    };
    let Some(path) = field.as_str() else {
        return Ok(());
    };
    if std::path::Path::new(path).is_absolute() {
        return Ok(());
    }
    let directory = match base {
        Some(directory) => directory,
        None => base.insert(cwd().map_err(|_| {
            Error::new("HOST_IO", "Cannot bind the relative recordHar path")
                .details(json!({"admitted":false,"dispatched":false}))
        })?),
    };
    let absolute =
        crate::browser::tracing::export::client_absolute(path, directory).map_err(|_| {
            Error::new(
                "HAR_EXPORT_PATH_INVALID",
                "recordHar path requires a bounded host file path",
            )
            .details(json!({"admitted":false,"dispatched":false}))
        })?;
    *field = json!(absolute);
    Ok(())
}
/// Bind selected host paths before crossing into the detached manager.
/// Application values and unrelated calls never require a working-directory read.
pub(crate) fn prepare_host_paths(name: &str, args: &mut Value) -> Result<()> {
    prepare_host_paths_with(name, args, std::env::current_dir)
}
fn prepare_host_paths_with(
    name: &str,
    args: &mut Value,
    mut cwd: impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<()> {
    let mut base = None;
    if name == "browser_connect" {
        if let Some(profile) = args.get_mut("profile") {
            bind_file_path(
                profile.get_mut("path"),
                &mut base,
                &mut cwd,
                HostFile::Profile,
            )?;
        }
        if let Some(attachment) = args.get_mut("attachment") {
            for key in ["credentialPath", "caPath"] {
                bind_file_path(
                    attachment.get_mut(key),
                    &mut base,
                    &mut cwd,
                    HostFile::Attachment,
                )?;
            }
        }
        if let Some(fixtures) = args.get_mut("mediaFixtures") {
            for key in ["microphonePath", "cameraPath"] {
                bind_file_path(fixtures.get_mut(key), &mut base, &mut cwd, HostFile::Media)?;
            }
        }
        bind_record_har(args.get_mut("context"), &mut base, &mut cwd)?;
        bind_file_path(
            args.get_mut("context")
                .and_then(|options| options.get_mut("storageState")),
            &mut base,
            &mut cwd,
            HostFile::StorageState,
        )?;
        return certificate_paths::bind(args.get_mut("context"), &mut base, &mut cwd);
    }
    if name != "browser_send" {
        return Ok(());
    }
    let Some(request) = args.get_mut("request") else {
        return Ok(());
    };
    if request["operation"] == "batch" {
        if let Some(requests) = request.get_mut("requests").and_then(Value::as_array_mut) {
            // Invalid batches retain their normal schema refusal without host reads.
            if requests.len() > 8 {
                return Ok(());
            }
            for request in requests {
                prepare_request_paths(request, &mut base, &mut cwd)?;
            }
        }
        Ok(())
    } else {
        prepare_request_paths(request, &mut base, &mut cwd)
    }
}

#[derive(Clone, Copy)]
enum HostFile {
    Media,
    StorageState,
    RouteFulfill,
    Certificate,
    Attachment,
    Profile,
    CaptureOutput,
}

/// Bind a host filename without collapsing parent components across symlinks.
fn bind_file_path(
    field: Option<&mut Value>,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
    kind: HostFile,
) -> Result<()> {
    let Some(field) = field else {
        return Ok(());
    };
    let Some(path) = field.as_str() else {
        return Ok(());
    };
    let (code, invalid, cwd_failure) = match kind {
        HostFile::Media => (
            "MEDIA_FIXTURE_PATH_INVALID",
            "Media fixtures require a bounded host file path",
            "Cannot bind the relative media fixture path",
        ),
        HostFile::StorageState => (
            "HTTP_STORAGE_STATE_PATH_INVALID",
            "storageState path requires a bounded host file path",
            "Cannot bind the relative storageState path",
        ),
        HostFile::RouteFulfill => (
            "ROUTE_FILE_PATH_INVALID",
            "route_fulfill path requires a bounded host file path",
            "Cannot bind the relative route_fulfill path",
        ),
        HostFile::Certificate => (
            "HTTP_CLIENT_CERTIFICATE_PATH_INVALID",
            "Certificate files require a bounded host path",
            "Cannot bind the selected certificate file",
        ),
        HostFile::Attachment => (
            "ATTACHMENT_FILE_PATH_INVALID",
            "Attachment files require a bounded host path",
            "Cannot bind the selected attachment file",
        ),
        HostFile::Profile => (
            "PROFILE_SELECTION_INVALID",
            "The borrowed profile requires a bounded host path",
            "Cannot bind the selected borrowed profile",
        ),
        HostFile::CaptureOutput => (
            "CAPTURE_OUTPUT_PATH_INVALID",
            "Screenshot output requires a bounded local file path",
            "Cannot bind the relative screenshot output path",
        ),
    };
    let error = |code, message| {
        Error::new(code, message).details(json!({"admitted":false,"dispatched":false}))
    };
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || matches!(kind, HostFile::CaptureOutput)
            && (path.contains("://") || path.chars().any(char::is_control))
    {
        return Err(error(code, invalid));
    }
    if std::path::Path::new(path).is_absolute() {
        return Ok(());
    }
    let directory = match base {
        Some(directory) => directory,
        None => base.insert(cwd().map_err(|_| error("HOST_IO", cwd_failure))?),
    };
    let absolute = directory.join(path);
    *field = json!(
        absolute
            .to_str()
            .filter(|absolute| absolute.len() <= 4096)
            .ok_or_else(|| error(code, invalid))?
    );
    Ok(())
}

fn prepare_request_paths(
    request: &mut Value,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<()> {
    if request["operation"] == "page_screenshot" {
        return bind_file_path(
            request
                .get_mut("capture")
                .and_then(|options| options.get_mut("outputPath")),
            base,
            cwd,
            HostFile::CaptureOutput,
        );
    }
    if matches!(
        request["operation"].as_str(),
        Some("http_context_create" | "context_create")
    ) {
        certificate_paths::bind(request.get_mut("options"), base, cwd)?;
    }
    if request["operation"] == "context_create" {
        bind_record_har(request.get_mut("options"), base, cwd)?;
        bind_file_path(
            request
                .get_mut("options")
                .and_then(|options| options.get_mut("storageState")),
            base,
            cwd,
            HostFile::StorageState,
        )?;
    }
    if matches!(
        request["operation"].as_str(),
        Some("page_add_script_tag" | "page_add_style_tag")
    ) {
        return tags::rewrite(request, base, cwd);
    }
    if matches!(
        request["operation"].as_str(),
        Some("http_context_create" | "http_context_storage_state" | "storage_export")
    ) {
        let name = if request["operation"] == "http_context_create" {
            "storageState"
        } else {
            "path"
        };
        let field = request
            .get_mut("options")
            .and_then(|options| options.get_mut(name));
        return bind_file_path(field, base, cwd, HostFile::StorageState);
    }
    let field = match request["operation"].as_str() {
        Some("trace_start") => request
            .get_mut("options")
            .and_then(|options| options.get_mut("filePath")),
        Some("trace_stop") => request.get_mut("filePath"),
        Some("callback_resolve") => {
            let field = request
                .get_mut("response")
                .filter(|response| response["kind"] == "route_fulfill")
                .and_then(|response| response.get_mut("path"));
            return bind_file_path(field, base, cwd, HostFile::RouteFulfill);
        }
        _ => None,
    };
    let Some(field) = field else {
        return Ok(());
    };
    let Some(path) = field.as_str() else {
        return Ok(());
    };
    if std::path::Path::new(path).is_absolute() {
        return Ok(());
    }
    let directory = match base {
        Some(directory) => directory,
        None => base.insert(cwd().map_err(|_| {
            Error::new("HOST_IO", "Cannot bind the relative trace export path")
                .details(json!({"admitted":false,"dispatched":false}))
        })?),
    };
    let absolute =
        crate::browser::tracing::export::client_absolute(path, directory).map_err(|code| {
            Error::new(code, "Trace export requires a bounded host file path")
                .details(json!({"admitted":false,"dispatched":false}))
        })?;
    *field = json!(absolute);
    Ok(())
}

fn rename(value: &mut Value, from: &str, to: &str) {
    if let Some(fields) = value.as_object_mut()
        && let Some(field) = fields.remove(from)
    {
        fields.insert(to.into(), field);
    }
}

fn schema_rename(schema: &mut Value, from: &str, to: &str) {
    rename(&mut schema["properties"], from, to);
    if let Some(required) = schema["required"].as_array_mut() {
        for field in required {
            if field.as_str() == Some(from) {
                *field = json!(to);
            }
        }
    }
}

/// Only executable discriminators change; locators, conditions and serialized
/// application values retain their own types and never undergo recursive edits.
fn action_schema(mut schema: Value) -> Value {
    let operation = schema["properties"]["operation"]["const"]
        .as_str()
        .unwrap()
        .to_owned();
    if matches!(
        operation.as_str(),
        "page_action"
            | "context_clock"
            | "context_credentials"
            | "page_clipboard"
            | "page_debugger"
            | "web_mcp_debug"
            | "web_mcp"
            | "browser_extension"
            | "browser_pwa"
            | "context_debugger"
            | "network_route"
    ) {
        schema_rename(&mut schema, "action", "input");
        for variant in schema["properties"]["input"]["oneOf"]
            .as_array_mut()
            .unwrap()
        {
            if operation == "page_action" && variant["properties"]["kind"]["const"] == "at_source" {
                schema_rename(variant, "action", "input");
                for input in variant["properties"]["input"]["oneOf"]
                    .as_array_mut()
                    .expect("Wrapped page input variants")
                {
                    schema_rename(input, "kind", "action");
                }
            }
            schema_rename(variant, "kind", "action");
        }
    }
    if operation == "page_input" {
        for variant in schema["properties"]["actions"]["items"]["oneOf"]
            .as_array_mut()
            .unwrap()
        {
            schema_rename(variant, "kind", "action");
        }
    }
    schema_rename(&mut schema, "operation", "action");
    schema
}

fn request(mut action: Value) -> Result<Value> {
    if action.get("operation").is_some() {
        return Err(Error::invalid("Use action, not operation, inside actions"));
    }
    let kind = action["action"].as_str().unwrap_or_default().to_owned();
    rename(&mut action, "action", "operation");
    if matches!(
        kind.as_str(),
        "page_action"
            | "context_clock"
            | "context_credentials"
            | "page_clipboard"
            | "page_debugger"
            | "web_mcp_debug"
            | "web_mcp"
            | "browser_extension"
            | "browser_pwa"
            | "context_debugger"
            | "network_route"
    ) {
        rename(&mut action, "input", "action");
        if kind == "page_action" && action["action"]["action"] == "at_source" {
            rename(&mut action["action"], "input", "action");
            rename(&mut action["action"]["action"], "action", "kind");
            // Move the wrapper discriminator only after preserving its command.
            action["action"]["kind"] = json!("at_source");
        } else {
            rename(&mut action["action"], "action", "kind");
        }
    }
    if kind == "page_input"
        && let Some(actions) = action["actions"].as_array_mut()
    {
        for action in actions {
            rename(action, "action", "kind");
        }
    }
    Ok(action)
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

/// Dispatch selects a platform branch using the explicit target. Union schemas
/// describe both branches; runtime validation enforces that selection too.
pub(crate) fn extend(tools: &mut Vec<Value>) {
    let schemas = super::contract::schemas();
    let target = json!({"type":"string","minLength":1,"description":"Explicit browser: targetId from connect; browser commands never open one-shot targets"});
    let requests = schemas["request"]["$defs"]["browserRequest"]["oneOf"]
        .as_array()
        .unwrap();
    let mut definitions = schemas["request"]["$defs"].clone();
    definitions
        .as_object_mut()
        .unwrap()
        .remove("browserRequest");
    let screenshot = requests
        .iter()
        .find(|schema| schema["properties"]["operation"]["const"] == "page_screenshot")
        .unwrap();
    let actions: Vec<Value> = requests
        .iter()
        .filter(|schema| {
            !matches!(
                schema["properties"]["operation"]["const"].as_str(),
                Some("input_release" | "batch")
            )
        })
        .cloned()
        .map(action_schema)
        .collect();
    let variants = [
        (
            "connect",
            schemas["launch"].clone(),
            "Connect a device, launch a browser or attach to an explicit endpoint, then use status to watch browser preparation",
        ),
        (
            "act",
            object(
                json!({"targetId":target,"actions":{"type":"array","minItems":1,"maxItems":8,"items":{"oneOf":actions}},"actionId":{"type":"string","format":"uuid"},"timeoutMs":{"type":"integer","minimum":1,"maximum":900000,"default":30000}}),
                &["targetId", "actions"],
            ),
            "Perform ordered mobile actions or submit an ordered browser batch; inspect status before repeating uncertain work",
        ),
        (
            "observe",
            object(
                json!({"targetId":target,"page":screenshot["properties"]["page"],"capture":screenshot["properties"]["capture"]}),
                &["targetId", "page"],
            ),
            "Return a fresh device screenshot or submit a browser screenshot for an explicit page",
        ),
        (
            "status",
            object(
                json!({"targetId":target,"actionId":{"type":"string","format":"uuid"},"waitMs":{"type":"integer","minimum":0,"maximum":30000,"default":0},"includeDetails":{"type":"boolean","default":false},"limit":crate::pagination::properties(super::runtime::STATUS_OPERATION_LIMITS)["limit"]}),
                &["targetId"],
            ),
            "Inspect device or compact browser state. For a browser, use includeDetails for resources or actionId for one full result.",
        ),
    ];
    for (name, mut browser, description) in variants {
        browser["$defs"] = definitions.clone();
        let tool = tools.iter_mut().find(|tool| tool["name"] == name).unwrap();
        let mobile = tool["inputSchema"].take();
        tool["inputSchema"] = json!({"type":"object","anyOf":[mobile,browser],"$defs":definitions});
        tool["description"] = json!(description);
        // Browser connect allocates on every call, unlike mobile recovery.
        if name == "connect" {
            tool["annotations"]["idempotentHint"] = json!(false);
        }
    }
    for name in ["control_status", "control_acquire", "control_release"] {
        let tool = tools.iter_mut().find(|tool| tool["name"] == name).unwrap();
        let mobile = tool["inputSchema"].take();
        let mut browser = mobile.clone();
        browser["properties"]["targetId"] = target.clone();
        browser["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("targetId"));
        if name == "control_acquire" {
            // This branch is also extracted as a standalone owner-control schema.
            browser["properties"]["page"] = definitions["browserReference"].clone();
            browser["properties"]["page"]["description"] = json!(
                "Required for a browser viewer; exact owned page to control. Omit for agent ownership."
            );
            browser["if"] = json!({"properties":{"owner":{"const":"viewer"}}});
            browser["then"] = json!({"required":["page","viewerId"]});
            browser["else"] =
                json!({"not":{"anyOf":[{"required":["page"]},{"required":["viewerId"]}]}});
        }
        tool["inputSchema"] = json!({"type":"object","anyOf":[mobile,browser]});
    }
    tools.push(json!({"name":"cancel","description":"Request cancellation of a retained browser action; dispatched effects are never undone or replayed","inputSchema":object(json!({"targetId":target,"actionId":{"type":"string","format":"uuid"}}), &["targetId","actionId"]),"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}}));
}

pub(crate) fn selected(name: &str, args: &Value) -> bool {
    (name == "connect" && args.get("engine").is_some())
        || (matches!(
            name,
            "act"
                | "observe"
                | "status"
                | "cancel"
                | "control_status"
                | "control_acquire"
                | "control_release"
        ) && args["targetId"]
            .as_str()
            .is_some_and(|target| target.starts_with("browser:")))
}

/// Negotiate new workflows before owner admission. Application JSON inside
/// evaluation/fixture values never selects capabilities.
pub(crate) fn requires_workflows(args: &Value) -> bool {
    fn options(options: &Value) -> bool {
        options.as_object().is_some_and(|options| {
            options.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "viewport" | "deviceScaleFactor" | "acceptDownloads" | "hasTouch"
                )
            })
        })
    }
    fn workflow(value: &Value) -> bool {
        match value["operation"].as_str() {
            Some("batch") => value["requests"]
                .as_array()
                .is_some_and(|requests| requests.iter().any(workflow)),
            Some(
                "storage_register" | "storage_export" | "storage_apply" | "storage_forget"
                | "diagnostics_wait" | "events_read" | "events_wait",
            ) => true,
            Some("context_create") => options(&value["options"]),
            _ => false,
        }
    }
    args.get("engine").is_some() && options(&args["context"]) || workflow(&args["request"])
}

/// A shared tool's browser argument schema, with its definitions, prepared
/// once per process.
fn browser_arguments(name: &str) -> Result<Value> {
    let definition =
        crate::catalog::tool_named(name).ok_or_else(|| Error::invalid("Unknown shared tool"))?;
    let mut schema = definition["inputSchema"]["anyOf"].as_array().map_or_else(
        || definition["inputSchema"].clone(),
        |variants| variants[1].clone(),
    );
    if let Some(definitions) = definition["inputSchema"].get("$defs") {
        schema["$defs"] = definitions.clone();
    }
    Ok(schema)
}

/// Validate before translating; unknown or conflicting fields cannot disappear.
pub(crate) fn translate(name: &str, mut args: Value) -> Result<(String, Value)> {
    super::transfer::fixture_path_guidance(name, &args)?;
    static SCHEMAS: std::sync::LazyLock<crate::schema::PreparedTools> =
        std::sync::LazyLock::new(Default::default);
    SCHEMAS
        .get(name, super::transfer::argument_byte_limit(name), || {
            browser_arguments(name)
        })?
        .validate(&args)?;
    if name != "connect" {
        super::runtime::connection(args["targetId"].as_str().unwrap())?;
    }
    let destination = match name {
        "connect" => "browser_connect",
        "status" => "browser_status",
        "cancel" => "browser_cancel",
        "control_status" => "browser_control_status",
        "control_acquire" => "browser_control_acquire",
        "control_release" => "browser_control_release",
        "act" => {
            let actions = args.as_object_mut().unwrap().remove("actions").unwrap();
            let requests = actions
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .map(request)
                .collect::<Result<Vec<_>>>()?;
            args["request"] = json!({"operation":"batch","requests":requests});
            "browser_send"
        }
        "observe" => {
            let page = args.as_object_mut().unwrap().remove("page").unwrap();
            let capture = args
                .as_object_mut()
                .unwrap()
                .remove("capture")
                .unwrap_or(json!({}));
            args["request"] = json!({"operation":"page_screenshot","page":page,"capture":capture});
            "browser_send"
        }
        _ => return Err(Error::invalid("Unknown shared browser verb")),
    };
    rename(&mut args, "actionId", "operationId");
    Ok((destination.into(), args))
}

pub(crate) fn result(mut value: Value) -> Value {
    // Keep the complete retained record; expose its public polling identity too.
    if value["operation"]["id"].is_string() {
        value["actionId"] = value["operation"]["id"].clone();
    }
    value
}

pub(crate) fn validate_mobile(name: &str, args: &Value) -> Result<()> {
    if name == "cancel" {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "cancel requires an explicit browser: targetId and actionId; mobile cancellation is not exposed by this command",
        ));
    }
    if matches!(
        name,
        "connect"
            | "act"
            | "observe"
            | "status"
            | "control_status"
            | "control_acquire"
            | "control_release"
    ) {
        let tool = crate::catalog::tool_named(name).unwrap();
        crate::schema::validate(&tool["inputSchema"]["anyOf"][0], args)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
