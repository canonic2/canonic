//! Native handoff uses current protected integration permission, never SDK grant state.
use super::{NativeApproval, NativeContext, Path, Value, json};
use crate::review::{
    integrations::{self, AskFallback},
    path_access,
};
mod paths;
mod recovery;
pub(crate) use paths::git_scope_check;
pub(crate) use paths::read_allowed as mandatory_read_allowed;
pub(super) use recovery::inspect as recovery;
#[cfg(test)]
mod tests;

/// Retain the ordinary native path and operator checks without consulting the
/// project's policy, grants, Ask fallback, or compiled cache. An empty policy
/// supplies only the existing shell analyzer's structural operation evidence;
/// none of its decisions authorize the request.
pub(crate) fn paused(
    cwd: &Path,
    operation: &Value,
    paused: &crate::review::mode::Paused,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Result<(), &'static str> {
    let root = paused.root.as_path();
    let cwd = cwd.canonicalize().map_err(|_| "invalid-context")?;
    if !cwd.is_dir() || !super::owns_context(root, &cwd) {
        return Err("invalid-context");
    }
    let guard = paths::Guard {
        cache: paused.cache.as_deref(),
        allow: paused.access,
        working_files,
    };
    let mut value = json!({});
    match operation["kind"].as_str() {
        Some("batch") => {
            let operations = operation["operations"]
                .as_array()
                .ok_or("invalid-host-input")?;
            for operation in operations {
                self::paused(&cwd, operation, paused, working_files)?;
            }
            if operation["unmodeled"] == true {
                return Err("unsupported-operation");
            }
            return Ok(());
        }
        Some("shell") => {
            value["_shell_analysis"] = paused_shell(root, &cwd, operation, working_files)?;
        }
        Some("file" | "read-text") => {
            let path = operation["path"].as_str().ok_or("invalid-host-input")?;
            let action = if operation["kind"] == "read-text" {
                "read"
            } else {
                operation["action"].as_str().ok_or("invalid-host-input")?
            };
            if !matches!(action, "read" | "write" | "remove") {
                return Err("invalid-host-input");
            }
            value["_file_targets"] = json!([path]);
            value["_file_access"] = json!(action);
            if operation["recursive"] == true {
                value["_file_search"] = operation
                    .get("search")
                    .filter(|search| search.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({"names_only":false,"globs":[],"types":[]}));
            }
        }
        Some("web" | "mcp") => return Ok(()),
        _ => return Err("unsupported-operation"),
    }
    if paths::operator_cli(&value["_shell_analysis"], &cwd) {
        return Err("operator-cli");
    }
    if opaque_shell(&value)
        || protected_removal(&value, &cwd, root, guard)
        || footprint(&value, root, &cwd, guard).finding.0 != paths::Protection::None
    {
        return Err("protected-access");
    }
    Ok(())
}

fn paused_shell(
    root: &Path,
    cwd: &Path,
    operation: &Value,
    working_files: Option<&crate::location::WorkingFiles>,
) -> Result<Value, &'static str> {
    let source = operation["source"].as_str().ok_or("invalid-host-input")?;
    let dialect =
        serde_json::from_value(operation["dialect"].clone()).map_err(|_| "unsupported-shell")?;
    let snapshot = crate::configuration::Candidate::from_yaml("schema_version: 1\npacks: []\n")
        .and_then(|candidate| candidate.capture(&[], None))
        .map_err(|_| "protection-failure")?;
    let evaluator =
        crate::configuration::Evaluator::new(vec![(crate::policy::Scope::Project, snapshot)])
            .map_err(|_| "protection-failure")?;
    let analysis = crate::shell::analyze_versioned_with_working_files(
        source,
        dialect,
        operation["dialect_version"].as_str(),
        cwd.to_str().ok_or("invalid-context")?,
        None,
        root.to_str().ok_or("invalid-context")?,
        &evaluator,
        &|| Ok(()),
        working_files,
    )
    .map_err(|_| "protection-failure")?;
    if !matches!(
        analysis["coverage"].as_str(),
        Some("single-literal-invocation" | "structured-literal-operations")
    ) || !crate::shell::supported_dialect(&analysis)
    {
        return Err("unsupported-shell");
    }
    Ok(analysis)
}

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
    let allow = path_access::current(root).map_err(|error| handoff_error(&error))?;
    let guard = paths::Guard {
        cache: cache.as_deref(),
        allow,
        working_files: native.working_files,
    };
    // Recovery always uses the exact Shield form, including on hosts with a
    // native Ask. No host fallback stands in for a review of unavailable policy.
    let native = if value["policy_unavailable"] == true {
        NativeContext {
            approval: NativeApproval::Fallback,
            ..native
        }
    } else {
        native
    };
    let result = gate(root, native, value, owner, cwd, guard);
    // A refused request records whether it touches state no approval may open,
    // so in-session approval is offered only for the groups a person can open.
    if result.is_ok() && value["disposition"] != "allowed" {
        value["fixed_protection"] = json!(fixed(value, owner, cwd, guard));
        value["deny_rules"] = deny_rules(value);
    }
    result
}

/// The IDs of the applied rules that denied the request, which a person
/// overriding the denial is shown.
fn deny_rules(value: &Value) -> Value {
    let evaluation = &value["_observation"]["evaluation"];
    let explanation = evaluation
        .get("explanation")
        .unwrap_or(&evaluation["evaluation"]);
    let mut rules: Vec<&str> = Vec::new();
    for found in explanation["matches"].as_array().into_iter().flatten() {
        if found["decision"] == "deny"
            && found["applied"].as_bool().unwrap_or(true)
            && let Some(id) = found["rule_id"].as_str()
            && !rules.contains(&id)
        {
            rules.push(id);
        }
    }
    json!(rules)
}

/// Opaque shell grammar has no verified footprint, so it may reach any path.
fn opaque_shell(value: &Value) -> bool {
    let shell = &value["_shell_analysis"];
    crate::shell::opaque_native(shell)
}

fn file_access(value: &Value) -> paths::Access {
    if value["_file_access"] == "read" {
        paths::Access::Read
    } else {
        paths::Access::Write
    }
}

/// The strictest protection the request's shell, file and search footprints reach.
fn footprint(
    value: &Value,
    owner: &Path,
    cwd: &Path,
    guard: paths::Guard<'_>,
) -> paths::Inspection {
    let mut result = paths::inspected(
        &value["_shell_analysis"],
        owner,
        cwd,
        guard,
        paths::Access::Write,
    );
    result.finding = [
        result.finding,
        paths::protection(
            &value["_file_targets"],
            owner,
            cwd,
            guard,
            file_access(value),
        ),
        match (
            value["_file_targets"][0].as_str(),
            value.get("_file_search"),
        ) {
            (Some(_), Some(search)) => paths::recursive_read(search),
            _ => (paths::Protection::None, None),
        },
    ]
    .into_iter()
    .max_by_key(|finding| finding.0)
    .expect("three findings");
    result
}

/// Whether the request still reaches protected state with every operator-openable
/// group open: review state, caches, the `.shield` folder or its ignore file, a
/// working-folder root or the operator CLI.
fn fixed(value: &Value, owner: &Path, cwd: &Path, guard: paths::Guard<'_>) -> bool {
    let open = opened(guard);
    opaque_shell(value)
        || paths::operator_cli(&value["_shell_analysis"], cwd)
        || protected_removal(value, cwd, owner, open)
        || footprint(value, owner, cwd, open).finding.0 >= paths::Protection::Blocked
}

/// The guard with every operator-openable group open: reading and changing
/// agent settings.
fn opened(guard: paths::Guard<'_>) -> paths::Guard<'_> {
    use crate::review::path_access::{Change, PathAccess, Read};
    paths::Guard {
        allow: PathAccess {
            agent_settings_read: Read::Allow,
            agent_settings_change: Change::Allow,
            ..PathAccess::default()
        },
        ..guard
    }
}

fn gate(
    root: &Path,
    native: NativeContext<'_>,
    value: &mut Value,
    owner: &Path,
    cwd: &Path,
    guard: paths::Guard<'_>,
) -> Result<(), &'static str> {
    let allow = guard.allow;
    // No native resource protection can be inferred from an opaque grammar.
    // A standing Ask fallback must not allow that grammar to edit protected
    // state without a verified footprint.
    if opaque_shell(value) {
        block(value, "unsupported-shell");
        return Ok(());
    }
    let inspection = footprint(value, owner, cwd, guard);
    let (protection, path) = inspection.finding;
    if !inspection.restorations.worktrees.is_empty() {
        value["git_restoration"] = json!(inspection.restorations);
    }
    // Preserve this consequence even when a policy Deny takes precedence, so
    // an in-session override cannot hide that Git may replace enforced policy.
    // A stricter finding in the same request, such as an agent-settings write,
    // can hide it; with the openable groups open it is the strictest left
    // whenever the request can still be approved.
    if value["git_restoration"]["policy_change"] == true
        || protection == paths::Protection::Undetermined
        || protection > paths::Protection::Undetermined
            && footprint(value, owner, cwd, opened(guard)).finding.0
                == paths::Protection::Undetermined
    {
        value["policy_change"] = json!(true);
    }
    // The operator CLI and a separately launched MCP server can activate policy;
    // agents use the registered MCP connection instead of starting either here.
    let blocked = if let Some(route) = paths::operator_route(&value["_shell_analysis"], cwd) {
        value["operator_route"] = json!(route);
        Some("operator-cli")
    } else if protection == paths::Protection::UnverifiedStashPaths {
        Some("git-stash-paths-unverified")
    } else if protection == paths::Protection::UnverifiedStashIndex {
        Some("git-stash-index-unverified")
    } else if protected_removal(value, cwd, owner, guard)
        || protection == paths::Protection::Blocked
    {
        Some("protected-access")
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
    // Policy Deny stands. Otherwise, uncertain policy writes and sensitive
    // reads require human approval regardless of policy Allow or Ask fallback.
    let approval = match protection {
        paths::Protection::Undetermined => Some("policy-file-unknown"),
        paths::Protection::Sensitive => Some("sensitive-read"),
        _ => None,
    };
    if let Some(code) = approval
        && value["decision"] != "deny"
    {
        value["decision"] = json!("ask");
        value["code"] = json!(code);
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
    // A command a Shield-written Codex prefix covers would run unsandboxed
    // without a prompt, so its Ask blocks whatever the configured fallback.
    let prefixed = value["_codex_covered"] == true
        && crate::agents::find(agent).is_some_and(|a| a.spec().prefix_rules_path.is_some());
    integrations::with_fallback(root, agent, allow, |configured| {
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
    .map_err(|error| handoff_error(&error))
}

fn handoff_error(error: &crate::review::Error) -> &'static str {
    if matches!(error, crate::review::Error::Locked) {
        "runtime-busy"
    } else {
        "handoff-failed"
    }
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
