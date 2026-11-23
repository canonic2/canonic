//! Structural evidence for exact native review when policy cannot be loaded.
//! The empty evaluator is used only by the shell parser; no policy decision or
//! standing/session grant from it authorizes this request.
use super::super::{Input, NativeContext, Operation};
use super::{Path, Value, json};

pub(in crate::enforcement::protocol) fn inspect(
    root: &Path,
    input: &Input,
    native: NativeContext<'_>,
    message: &str,
) -> Result<Value, &'static str> {
    let context = crate::review::open_authorization(root).map_err(|_| "runtime-unavailable")?;
    if input.expected_project_id.as_deref() != Some(context.project_id.as_str()) {
        return Err("project-changed");
    }
    let owner = Path::new(&context.project_root);
    let cwd = Path::new(&input.cwd);
    if !cwd.is_absolute() {
        return Err("invalid-context");
    }
    let cwd = cwd.canonicalize().map_err(|_| "invalid-context")?;
    if !cwd.is_dir() || cwd.to_str().is_none() || !super::super::owns_context(owner, &cwd) {
        return Err("invalid-context");
    }
    let mut value = json!({"protocol_version":1,"request_id":input.request_id,
        "decision":"deny","disposition":"blocked","code":"policy-files-invalid",
        "policy_unavailable":true,"diagnostics":{"message":message},
        "identity":{"project_id":context.project_id,"project_root":context.project_root,
            "activation_id":"invalid-policy","policy_revision":"unavailable","candidate_digest":"unavailable"},
        "_observation":{"request":{"cwd":cwd}}});
    match &input.operation {
        Operation::Shell { .. } => {
            let operation = serde_json::to_value(&input.operation).map_err(|_| "invalid-input")?;
            value["_shell_analysis"] =
                super::paused_shell(owner, &cwd, &operation, native.working_files)?;
        }
        Operation::File {
            action,
            path,
            recursive,
            search,
        } => {
            value["_file_targets"] = json!([path]);
            value["_file_access"] = json!(action);
            if *recursive {
                value["_file_search"] = search.as_ref().map_or_else(
                    || json!({"names_only":false,"globs":[],"types":[]}),
                    |search| json!(search),
                );
            }
        }
        Operation::Web { .. } => {}
        _ => return Err("unsupported-operation"),
    }
    Ok(value)
}
