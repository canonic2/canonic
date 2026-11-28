//! Shared initialization plans and terminal-free MCP onboarding.
use super::*;
use sha2::{Digest, Sha256};
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    path: PathBuf,
    kind: Option<String>,
    platforms: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Init {
    root: PathBuf,
    config: Option<PathBuf>,
    projects: Option<Vec<Selection>>,
    #[serde(default)]
    dry_run: bool,
    plan_digest: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discover {
    root: PathBuf,
    config: Option<PathBuf>,
    #[serde(default)]
    paths: Vec<PathBuf>,
    #[serde(rename = "cursor")]
    _cursor: Option<String>,
    #[serde(rename = "limit")]
    _limit: Option<u64>,
}
pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "project_discover" | "project_init" | "project_init_preview"
    )
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384})
}
fn config() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384,"description":"Absolute configuration file to create instead of root testrun.yml"})
}
fn identifier() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)?$"})
}
pub(crate) fn tools() -> Vec<Value> {
    let selection = object(
        json!({"path":text(),"kind":identifier(),"platforms":{"type":"array","maxItems":128,"items":identifier()}}),
        &["path"],
    );
    let mut tools = vec![
        json!({"name":"project_discover","description":"Detect application projects and preview starter configuration without running project code or writing files","inputSchema":object(json!({"root":text(),"config":config(),"paths":{"type":"array","maxItems":128,"items":text()}}),&["root"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}),
        json!({"name":"project_init","description":"Create selected projects with the planDigest from project_init_preview, preserving existing configuration","inputSchema":object(json!({"root":text(),"config":config(),"projects":{"type":"array","minItems":1,"maxItems":128,"items":selection},"dryRun":{"type":"boolean"},"planDigest":{"type":"string","pattern":"^[0-9a-f]{64}$","minLength":64,"maxLength":64}}),&["root"]),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}}),
    ];
    crate::pagination::declare(&mut tools[0], canonic_mcp_kit::page::Limits::STANDARD);
    let mut definitions = vec![
        canonic_mcp_kit::Tool::new(
            "project_init",
            "Initialize project configuration.",
            tools[1]["inputSchema"].clone(),
        )
        .with_annotations(canonic_mcp_kit::Annotations {
            open_world_hint: Some(false),
            ..canonic_mcp_kit::Annotations::default()
        }),
    ];
    canonic_mcp_kit::tool::preview::append_with_flag(&mut definitions, &["project_init"], "dryRun")
        .expect("initialization supports dry-run inspection");
    tools[1]["inputSchema"] = definitions[0].input_schema.clone();
    tools[1]["inputSchema"]["required"] = json!(["root", "planDigest"]);
    let mut preview = definitions.pop().expect("initialization preview");
    preview.input_schema["properties"]
        .as_object_mut()
        .expect("initialization properties")
        .remove("planDigest");
    tools.push(serde_json::to_value(preview).expect("preview schema"));
    tools
}
/// CLI and MCP use the same configuration, warnings, digest and creation review.
pub(crate) fn review(
    discovery: &Discovery,
    candidates: &[Candidate],
    config: Option<&Path>,
) -> Result<Value> {
    let configuration = if candidates.is_empty() {
        None
    } else {
        Some(super::configuration(candidates)?)
    };
    let (can_create, blocker) = match super::available(&discovery.root, config) {
        Ok(_) => (configuration.is_some() && !discovery.truncated, None),
        Err(error) if error.code == "CONFIG_EXISTS" => (false, Some(error)),
        Err(error) => return Err(error),
    };
    let destination = super::super::files::target(&discovery.root, config)?;
    let (ignore, entry) = super::super::files::ignore_target(
        &discovery.root,
        &destination.with_file_name(super::super::files::LOCAL),
    );
    // An explicit destination is part of the reviewed plan; the default keeps its identity.
    let mut identity = json!({"format":"testrun-init-plan-v1","root":discovery.root,"configuration":configuration});
    if config.is_some() {
        identity["config"] = json!(destination);
    }
    let digest = configuration.as_ref().map(|_| {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&identity).unwrap())
        )
    });
    Ok(
        json!({"dryRun":true,"root":discovery.root,"discovery":discovery,"configuration":configuration,"planDigest":digest,"canCreate":can_create,"blocker":blocker,"writes":[{"path":destination,"operation":"create"},{"path":ignore,"entry":entry,"operation":"append_local_ignore_if_missing"}],"warnings":discovery.warnings,"truncated":discovery.truncated}),
    )
}
fn absolute(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        return Err(Error::invalid("root must be an absolute directory"));
    }
    Ok(())
}
fn discover_page(args: &Value) -> Result<Value> {
    let request: Discover = serde_json::from_value(args.clone())
        .map_err(|_| Error::invalid("Invalid project discovery request"))?;
    absolute(&request.root)?;
    let page = super::detection::page(&request.root, &request.paths, args)?;
    let discovery = Discovery {
        root: PathBuf::from(page["root"].as_str().expect("canonical root")),
        candidates: serde_json::from_value(page["candidates"].clone())
            .map_err(|_| Error::new("INTERNAL", "Cannot encode discovery page"))?,
        warnings: serde_json::from_value(page["warnings"].clone())
            .map_err(|_| Error::new("INTERNAL", "Cannot encode discovery warnings"))?,
        truncated: page["incompleteManifests"] == true,
    };
    let mut result = review(&discovery, &discovery.candidates, request.config.as_deref())?;
    for field in [
        "nextCursor",
        "warnings_total",
        "scanDepth",
        "deeperProjects",
    ] {
        result[field] = page[field].clone();
    }
    Ok(result)
}

pub(crate) fn call(name: &str, args: &Value) -> Result<Value> {
    let definition = tools()
        .into_iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| Error::invalid("Unknown project onboarding operation"))?;
    crate::schema::validate(&definition["inputSchema"], args)?;
    let normalized = canonic_mcp_kit::tool::preview::normalize_with_flag(
        &["project_init"],
        canonic_mcp_kit::ToolCall {
            name: name.to_owned(),
            arguments: args.clone(),
            progress_token: None,
        },
        "dryRun",
    );
    let name = normalized.name.as_str();
    let args = &normalized.arguments;
    if name == "project_discover" {
        return discover_page(args);
    }
    let request: Init = serde_json::from_value(args.clone())
        .map_err(|_| Error::invalid("Invalid project initialization request"))?;
    absolute(&request.root)?;
    if !request.dry_run && request.plan_digest.is_none() {
        return Err(Error::new(
            "INIT_PLAN_REQUIRED",
            "Call project_init_preview, then create with the returned planDigest and the same selections",
        ));
    }
    let paths: Vec<_> = request
        .projects
        .as_ref()
        .into_iter()
        .flatten()
        .map(|p| p.path.clone())
        .collect();
    let mut discovery = discover(&request.root, &paths)?;
    if let Some(selections) = &request.projects {
        let mut candidates = Vec::new();
        for selection in selections {
            let directory = discovery
                .root
                .join(&selection.path)
                .canonicalize()
                .map_err(|_| {
                    Error::new("CONFIG_PATH", "Selected application root is unavailable")
                })?;
            let mut candidate = discovery
                .candidates
                .iter()
                .find(|candidate| discovery.root.join(&candidate.path) == directory)
                .cloned()
                .ok_or_else(|| Error::invalid("Selected application is not in discovery"))?;
            if let Some(kind) = &selection.kind {
                candidate.kind = kind.clone();
            }
            if let Some(platforms) = &selection.platforms {
                candidate.platforms = platforms.clone();
            }
            candidates.push(candidate);
        }
        discovery.candidates = candidates;
    }
    let plan = review(&discovery, &discovery.candidates, request.config.as_deref())?;
    if request.dry_run {
        return Ok(plan);
    }
    if discovery.truncated {
        return Err(Error::new(
            "DISCOVERY_LIMIT",
            "Discovery is incomplete; preview narrower explicit projects before creation",
        ));
    }
    let configuration = plan["configuration"].as_object().ok_or_else(|| {
        Error::new(
            "PROJECT_SELECTION_REQUIRED",
            "Select explicit application paths before creating configuration",
        )
    })?;
    if request.plan_digest.as_deref() != plan["planDigest"].as_str() {
        return Err(Error::new("INIT_PLAN_CHANGED","Project initialization selections changed; preview and review the new plan before creation").details(json!({"nextAction":"project_init_preview"})));
    }
    let mut result = super::publish_at(
        &discovery.root,
        request.config.as_deref(),
        &Value::Object(configuration.clone()),
    )?;
    result["dryRun"] = json!(false);
    result["planDigest"] = plan["planDigest"].clone();
    result["configuration"] = plan["configuration"].clone();
    result["warnings"] = json!(discovery.warnings);
    Ok(result)
}
