//! Native handoff uses current protected integration permission, never SDK grant state.
use super::{NativeApproval, NativeContext, Path, Value, json};
use crate::review::{
    integrations::{self, AskFallback},
    path_access,
};
mod paths;

pub(super) fn resolve(
    root: &Path,
    native: NativeContext<'_>,
    value: &mut Value,
) -> Result<(), &'static str> {
    let owner = value["identity"]["project_root"]
        .as_str()
        .map_or(root, Path::new)
        .to_path_buf();
    let cwd = value["_observation"]["request"]["cwd"]
        .as_str()
        .map_or(owner.as_path(), Path::new)
        .to_path_buf();
    let (owner, cwd) = (owner.as_path(), cwd.as_path());
    let cache = crate::review::compiled_cache_root(owner).map_err(|_| "runtime-unavailable")?;
    // An unreadable or invalid choice blocks; the handoff below rechecks it.
    let allow = path_access::current(root).map_err(|_| "handoff-failed")?;
    let guard = paths::Guard {
        cache: cache.as_deref(),
        allow,
        working_files: native.working_files,
    };
    let file_access = if value["_file_access"] == "read" {
        paths::Access::Read
    } else {
        paths::Access::Write
    };
    let (protection, path) = [
        paths::protection(
            &value["_shell_analysis"],
            owner,
            cwd,
            guard,
            paths::Access::Write,
        ),
        paths::protection(&value["_file_targets"], owner, cwd, guard, file_access),
        match (
            value["_file_targets"][0].as_str(),
            value.get("_file_search"),
        ) {
            (Some(path), Some(search)) => paths::recursive_read(search, path, owner, cwd, guard),
            _ => (paths::Protection::None, None),
        },
    ]
    .into_iter()
    .max_by_key(|finding| finding.0)
    .expect("three findings");
    // The operator CLI and a separately launched MCP server can activate policy;
    // agents use the registered MCP connection instead of starting either here.
    let blocked = if paths::operator_cli(&value["_shell_analysis"], cwd) {
        Some("operator-cli")
    } else if protected_removal(value, cwd, owner, guard)
        || protection == paths::Protection::Blocked
    {
        Some("protected-access")
    } else if protection == paths::Protection::SearchBlocked {
        Some("protected-search")
    } else if protection == paths::Protection::Unverified && value["decision"] != "deny" {
        Some("search-footprint-unavailable")
    } else {
        None
    };
    if protection != paths::Protection::None
        && let Some(path) = path
    {
        // Lets the agent message name what tripped the check and why.
        value["protected_group"] = json!(paths::group(&path, cwd, guard));
        value["protected_path"] = json!(path);
    }
    if let Some(code) = blocked {
        block(value, code);
        return Ok(());
    }
    if value["decision"] == "deny"
        && let Some(found) = unapproved_outside(value, owner, cwd, native.working_files)
    {
        // Names the path, and the folder the agent may propose when it is narrow.
        value["outside_path"] =
            json!({"path":found.path,"access":found.access,"folder":found.folder});
    }
    // A policy Deny stands; otherwise the user decides on reading agent settings.
    if protection == paths::Protection::Sensitive && value["decision"] != "deny" {
        value["decision"] = json!("ask");
        value["code"] = json!("sensitive-read");
        value["disposition"] = json!(if native.approval == NativeApproval::Prompt {
            "approval_required"
        } else {
            "blocked"
        });
        return Ok(());
    }
    resolve_fallback(root, native, allow, value)
}

fn resolve_fallback(
    root: &Path,
    native: NativeContext<'_>,
    allow: path_access::PathAccess,
    value: &mut Value,
) -> Result<(), &'static str> {
    let agent = native.integration;
    // Only a valid policy Ask can use fallback. Unsupported tools/resource checks
    // and failures are not Ask, even when the host cannot present a prompt.
    let asked = value["code"] == "approval-required"
        && value["_observation"]["evaluation"]["decision"] == "ask";
    let activation = value["identity"]["activation_id"]
        .as_str()
        .ok_or("runtime-unavailable")?
        .to_owned();
    // A command a Shield-written Codex prefix covers would run unsandboxed
    // without a prompt, so its Ask blocks whatever the configured fallback.
    let prefixed = value["_codex_covered"] == true
        && crate::agents::find(agent).is_some_and(|a| a.spec().prefix_rules_path.is_some());
    integrations::with_fallback(root, &activation, agent, allow, |configured| {
        let fallback = if prefixed {
            AskFallback::Block
        } else {
            configured
        };
        // The agent is told why an allowing fallback did not apply.
        if asked && prefixed && configured == AskFallback::Allow {
            value["ask_fallback_reason"] = json!("codex-prefix-rules");
        }
        value["integration"] = json!(agent);
        value["host_context_complete"] = json!(native.context_complete);
        value["ask_fallback"] = if asked && native.approval == NativeApproval::Fallback {
            json!(fallback)
        } else {
            Value::Null
        };
        value["disposition"] = json!(disposition(value, native, fallback, asked));
    })
    .map_err(|_| "handoff-failed")
}

fn block(value: &mut Value, code: &str) {
    value["decision"] = json!("deny");
    value["code"] = json!(code);
    value["disposition"] = json!("blocked");
}

fn protected_removal(value: &Value, cwd: &Path, owner: &Path, guard: paths::Guard<'_>) -> bool {
    value["_file_access"] == "remove"
        && value["_file_targets"].as_array().is_some_and(|targets| {
            targets
                .iter()
                .filter_map(Value::as_str)
                .any(|path| paths::removal(&cwd.join(path), owner, guard))
        })
}

fn unapproved_outside(
    value: &Value,
    owner: &Path,
    cwd: &Path,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Option<paths::Outside> {
    outside(value, owner, cwd).filter(|found| {
        working_files.is_none_or(|scope| {
            !scope
                .roots(found.access == "read")
                .any(|root| found.path.starts_with(root))
        })
    })
}

fn disposition(
    value: &mut Value,
    native: NativeContext<'_>,
    fallback: AskFallback,
    asked: bool,
) -> &'static str {
    // A host that can prompt asks the user about a tool Shield cannot model,
    // so history records the prompt rather than a block.
    if value["code"] == "unsupported-operation" && native.approval == NativeApproval::Prompt {
        return "approval_required";
    }
    if asked {
        value["decision"] = json!("ask");
        if native.approval != NativeApproval::Fallback {
            "approval_required"
        } else if fallback == AskFallback::Allow {
            "allowed"
        } else {
            "blocked"
        }
    } else if value["decision"] == "allow" {
        // Native hooks decide submitted requests using available host context.
        // Missing vendor fields are evidence limits, not another approval policy.
        "allowed"
    } else {
        "blocked"
    }
}

/// The outside path behind a location refusal: the file tool's own path, or an
/// outside argument of the shell command a failed location check names.
fn outside(value: &Value, owner: &Path, cwd: &Path) -> Option<paths::Outside> {
    let located = |checks: &Value| {
        checks
            .as_array()
            .into_iter()
            .flatten()
            .find(|check| check["code"] == "location_outside")
            .cloned()
    };
    let evaluation = &value["_observation"]["evaluation"];
    let explanation = evaluation
        .get("explanation")
        .unwrap_or(&evaluation["evaluation"]);
    let check = located(&explanation["requirement_checks"]);
    if check.is_some()
        && let Some(path) = value["_file_targets"][0].as_str()
    {
        let reader = if value["_file_access"] == "read" {
            "cat"
        } else {
            "tee"
        };
        return paths::outside(
            &json!({"request":{"executable":reader,"arguments":[path]}}),
            owner,
            cwd,
        );
    }
    // Without a location check, a rule that denies a read, such as a catch-all
    // for `cat`, is explained by the outside path it was given.
    let (reference, reads_only) = match check {
        Some(check) => (check["operation_ref"].as_str()?.to_owned(), false),
        None => (
            explanation["guidance"][0]["operation_ref"]
                .as_str()?
                .to_owned(),
            true,
        ),
    };
    value["_shell_analysis"]["operations"]
        .as_array()?
        .iter()
        .filter(|operation| {
            format!(
                "shell:{}:{}-{}",
                operation["source_unit"], operation["span"]["start"], operation["span"]["end"]
            ) == reference
        })
        .filter_map(|operation| match file_effect(operation) {
            Some(file) => paths::outside(&file, owner, cwd),
            None => paths::outside(operation, owner, cwd),
        })
        .find(|found| !reads_only || found.access == "read")
}

/// A redirection or retained typed file effect as the write or read it performs.
/// Use the recognized download destination, never guess from its raw options.
fn file_effect(operation: &Value) -> Option<Value> {
    let request = if operation["coverage"] == "file-redirection" {
        &operation["normalized_request"]
    } else if operation["policy_request"]["kind"] == "file" {
        &operation["policy_request"]
    } else {
        return None;
    };
    let (reader, path) = match request["destination"].as_str() {
        Some(path) => ("tee", path),
        None => ("cat", request["sources"][0].as_str()?),
    };
    Some(json!({"cwd":operation["cwd"],"request":{"executable":reader,"arguments":[path]}}))
}
