//! Non-authorizing normalized evaluations and immutable before/after comparisons.
use super::{
    Checkpoint, Context, Error, fields, required, text,
    views::{Captured, Views},
};
use crate::{
    configuration,
    policy::{
        self,
        normalized::{Context as RequestContext, Input},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) fn evaluate(
    context: &Context,
    views: &mut Views,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    fields(args, &["input", "context", "candidate_id"])?;
    let (input, request_context) = request(context, args)?;
    let id = match text(args, "candidate_id")? {
        Some(id) => id.into(),
        None => views.active(context)?,
    };
    let captured = views.get(&id)?;
    let mut value = evaluate_captured(context, &captured, &input, &request_context, checkpoint)?;
    value["candidate_id"] = json!(id);
    value["policy_revision"] = json!(captured.evaluator()?.revision());
    Ok(value)
}
pub(super) fn compare(
    context: &Context,
    views: &mut Views,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    fields(
        args,
        &[
            "before_candidate_id",
            "after_candidate_id",
            "domain",
            "input",
            "context",
        ],
    )?;
    let before_id = required(args, "before_candidate_id")?;
    let after_id = required(args, "after_candidate_id")?;
    let before = views.get(before_id)?;
    let after = views.get(after_id)?;
    checkpoint()?;
    if required(args, "domain")? != "sample"
        && (args.get("input").is_some() || args.get("context").is_some())
    {
        return Err(Error::Invalid("only sample accepts input/context"));
    }
    let mut value = match required(args, "domain")? {
        "sample" => {
            let (input, ctx) = request(context, args)?;
            let a = evaluate_captured(context, &before, &input, &ctx, checkpoint)?;
            let b = evaluate_captured(context, &after, &input, &ctx, checkpoint)?;
            json!({"domain":"sample","complete":false,"relation":"unknown","coverage":"one request witness; no whole-policy proof","context":ctx,"witnesses":[{"input":input,"relation":if a["decision"]==b["decision"] {"equivalent"}else if rank(&b["decision"])>rank(&a["decision"]) {"restrictive"}else{"expansive"},"before":a,"after":b}]})
        }
        "literal_argv" => {
            json!(configuration::compare_policy(
                &snapshots(&before)?,
                &snapshots(&after)?
            )?)
        }
        "supported_semantic" => {
            json!({"domain":"supported_semantic","complete":false,"relation":"unknown","coverage":"no complete semantic comparison model is implemented"})
        }
        _ => return Err(Error::Invalid("invalid comparison domain")),
    };
    checkpoint()?;
    value["before_candidate_id"] = json!(before_id);
    value["after_candidate_id"] = json!(after_id);
    value["domain"] = args["domain"].clone();
    value["engine_version"] = json!(env!("CARGO_PKG_VERSION"));
    value["model"] = json!("normalized-request-v1");
    Ok(value)
}
fn snapshots(captured: &Captured) -> Result<Vec<(policy::Scope, configuration::Snapshot)>, Error> {
    crate::review::snapshots(&BTreeMap::from([("project".into(), captured.source()?)]))
}
fn request(context: &Context, args: &Value) -> Result<(Input, RequestContext), Error> {
    let input: Input = serde_json::from_value(args["input"].clone())
        .map_err(|_| Error::Invalid("invalid normalized input"))?;
    let mut ctx: RequestContext = serde_json::from_value(args["context"].clone())
        .map_err(|_| Error::Invalid("invalid request context"))?;
    input.validate(&ctx, &context.project_root)?;
    let cwd = Path::new(&ctx.cwd).canonicalize()?;
    if !cwd.is_dir() || !cwd.starts_with(&context.project_root) {
        return Err(Error::Invalid(
            "cwd must be a directory inside registered project",
        ));
    }
    ctx.cwd = cwd.to_string_lossy().into_owned();
    Ok((input, ctx))
}
fn evaluate_captured(
    context: &Context,
    captured: &Captured,
    input: &Input,
    ctx: &RequestContext,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    let executable = resolve(input, ctx);
    let mut value = captured.evaluator()?.evaluate_normalized(
        input,
        ctx,
        &context.project_root,
        executable.as_deref(),
        &crate::review::model::now(),
        checkpoint,
    )?;
    inspection_refs(&mut value, &captured.metadata);
    Ok(value)
}
fn resolve(input: &Input, ctx: &RequestContext) -> Option<String> {
    let executable = match input {
        Input::Argv { executable, .. } => executable.as_str(),
        _ => return None,
    };
    let resolved = if Path::new(executable).is_absolute() {
        Path::new(executable).canonicalize().ok()
    } else if executable.contains('/') {
        Path::new(&ctx.cwd).join(executable).canonicalize().ok()
    } else {
        ctx.environment
            .as_ref()
            .and_then(|e| e.get("PATH"))
            .and_then(|path| {
                std::env::split_paths(path).find_map(|p| {
                    let directory = if p.is_absolute() {
                        p
                    } else {
                        Path::new(&ctx.cwd).join(p)
                    };
                    launch_path(&directory.join(executable))
                })
            })
    };
    resolved
        .and_then(|p| launch_path(&p))
        .map(|p| p.to_string_lossy().into_owned())
}
fn launch_path(path: &Path) -> Option<std::path::PathBuf> {
    let path = path.canonicalize().ok()?;
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}
fn rank(value: &Value) -> u8 {
    match value.as_str() {
        Some("allow") => 0,
        Some("deny") => 2,
        _ => 1,
    }
}
/// The installed `pack:` selector of an evaluation source naming a pack by
/// `publisher/name` or `identity/publisher/name`.
fn installed_selector(metadata: &Value, source: &str) -> Option<Value> {
    metadata["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| {
            p["id"] == source
                || format!(
                    "{}/{}",
                    p["source_identity"].as_str().unwrap_or_default(),
                    p["id"].as_str().unwrap_or_default()
                ) == source
        })
        .map(|p| p["selector"].clone())
}
/// Give every rule reference in an evaluation, from matches, requirement
/// checks and guidance alike, the installed `pack:` selector of its pack.
fn inspection_refs(value: &mut Value, metadata: &Value) {
    match value {
        Value::Object(fields) => {
            if let (Some(source), Some(rule)) = (
                fields.get("source").and_then(Value::as_str),
                fields.get("rule_id").and_then(Value::as_str),
            ) {
                let reference = if source == "custom:project" {
                    json!({"scope":"custom","rule_id":rule})
                } else {
                    installed_selector(metadata, source).map_or(
                        Value::Null,
                        |selector| json!({"scope":"pack","pack_selector":selector,"rule_id":rule}),
                    )
                };
                fields.insert("rule_ref".into(), reference);
            } else if let Some(reference) = fields.get_mut("rule_ref")
                && reference["scope"] == "pack"
                && let Some(source) = reference["pack_selector"].as_str()
                && !source.starts_with("pack:")
            {
                match installed_selector(metadata, source) {
                    Some(selector) => reference["pack_selector"] = selector,
                    None => *reference = Value::Null,
                }
            }
            for v in fields.values_mut() {
                inspection_refs(v, metadata);
            }
        }
        Value::Array(rows) => {
            for v in rows {
                inspection_refs(v, metadata);
            }
        }
        _ => {}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn every_pack_rule_reference_names_the_installed_selector() {
        let selector = "pack:123e4567-e89b-42d3-a456-426614174000";
        let metadata = json!({"installed":[{"id":"acme/tools","source_identity":"acme-identity","selector":selector}]});
        let pack = |name: &str| json!({"scope":"pack","pack_selector":name,"rule_id":"build"});
        let custom = json!({"scope":"custom","rule_id":"local"});
        let mut evaluation = json!({
            "matches":[{"source":"acme/tools","rule_id":"build"}],
            "requirement_checks":[{"rule_ref":pack("acme/tools")},{"rule_ref":custom}],
            "explanation":{"guidance":[
                {"rule_ref":pack("acme-identity/acme/tools")},
                {"rule_ref":pack(selector)},
                {"rule_ref":pack("acme/missing")}
            ]}
        });
        inspection_refs(&mut evaluation, &metadata);
        let installed = pack(selector);
        assert_eq!(evaluation["matches"][0]["rule_ref"], installed);
        assert_eq!(evaluation["requirement_checks"][0]["rule_ref"], installed);
        assert_eq!(evaluation["requirement_checks"][1]["rule_ref"], custom);
        let guidance = &evaluation["explanation"]["guidance"];
        assert_eq!(guidance[0]["rule_ref"], installed);
        assert_eq!(guidance[1]["rule_ref"], installed);
        assert_eq!(guidance[2]["rule_ref"], Value::Null);
    }

    #[test]
    fn executable_observation_uses_request_cwd_for_relative_path_entries() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("bin")).unwrap();
        std::os::unix::fs::symlink("/bin/echo", root.path().join("bin/acme")).unwrap();
        let input: Input = serde_json::from_value(
            json!({"kind":"argv","executable":"acme","arguments":[],"original_input":{}}),
        )
        .unwrap();
        let ctx = RequestContext {
            working_files: None,
            cwd: root.path().to_string_lossy().into_owned(),
            environment: Some(BTreeMap::from([("PATH".into(), "bin".into())])),
        };
        assert_eq!(
            resolve(&input, &ctx),
            Some(
                Path::new("/bin/echo")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }
}
