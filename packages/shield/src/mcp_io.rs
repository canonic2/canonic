//! Owns MCP process startup: the enrolled project context, diagnostics and
//! the stdio connection's lifetime.
use crate::{
    cli::{McpServe, ReviewContext},
    mcp,
};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

#[cfg(test)]
mod tests;

pub(crate) fn run(
    project: Option<&Path>,
    input: &McpServe,
    log: &Arc<crate::diagnostics::Log>,
) -> ExitCode {
    let options = mcp::options(
        Duration::from_millis(input.request_timeout_ms),
        Some(log.clone()),
    );
    if input.skills_only {
        return match mcp::serve_static(
            io::BufReader::new(io::stdin()),
            io::stdout().lock(),
            &options,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session = session_dir(input.agent.as_deref());
    let target = match serve_project(project, &cwd, session.as_deref()) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("Cannot start Shield MCP server: {error}");
            return ExitCode::FAILURE;
        }
    };
    let requested =
        project.map_or_else(|| target.clone(), |project| absolute_project(project, &cwd));
    let context = ReviewContext {
        project: target.clone(),
        agent: input.agent.clone(),
        session_id: input.session_id.clone(),
        tool_use_id: input.tool_use_id.clone(),
    };
    log.event("mcp_start", || serde_json::json!({"session_id":context.session_id,"agent":context.agent,"request_timeout_ms":input.request_timeout_ms}));
    // Starting a runtime connection, unlike CLI inspection, may reuse this
    // developer's exact reviewed source receipt before first-use diagnostics.
    if valid_attribution(&context)
        && let Err(error) = crate::review::reuse_reviewed_policy(&target)
    {
        log.event(
            "reviewed_policy_reuse_failed",
            || serde_json::json!({"error_kind":error.code()}),
        );
    }
    if valid_attribution(&context)
        && let Ok(probe) = crate::review::bootstrap::inspect(&target)
        && probe.bootstrap_required()
    {
        let attribution = ReviewContext {
            project: requested,
            ..context
        };
        return match mcp::serve_bootstrap(
            probe.project_root(),
            attribution,
            io::BufReader::new(io::stdin()),
            io::stdout().lock(),
            &options,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let context = match review_context(&context) {
        Ok(context) => context,
        Err(error) => {
            if matches!(
                error,
                crate::review::Error::StateFormat { .. }
                    | crate::review::Error::StateCorrupt { .. }
            ) {
                eprintln!("{error}");
            } else {
                eprintln!("Cannot start Shield MCP server: enrolled project required");
            }
            return ExitCode::FAILURE;
        }
    };
    let Ok(server) = mcp::Server::for_review(context).map(|server| server.with_log(log.clone()))
    else {
        eprintln!("Cannot start Shield project review server");
        return ExitCode::FAILURE;
    };
    match mcp::serve(
        server,
        io::BufReader::new(io::stdin()),
        io::stdout().lock(),
        &options,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// A validated enrollment or integration-review receipt retains human forms
/// across connections, without importing native registration or caller claims.
pub(crate) fn review_context(
    input: &ReviewContext,
) -> Result<crate::review::Context, crate::review::Error> {
    let mut context = crate::review::cli::context(input)?;
    context.confirmed_mcp_forms = crate::agents::review::resolve(context.agent.as_deref())
        .confirmation()
        == crate::agents::review::Confirmation::Terminal
        && crate::review::bootstrap::forms_reviewed_at(Path::new(&context.project_root));
    Ok(context)
}

fn valid_attribution(context: &ReviewContext) -> bool {
    [&context.agent, &context.session_id, &context.tool_use_id]
        .into_iter()
        .all(|value| {
            value.as_ref().is_none_or(|value| {
                !value.is_empty() && value.len() <= 4096 && !value.contains('\0')
            })
        })
}

/// Resolve portable launchers from cwd and keep historical absolute launchers
/// on the current checkout instead of opening their old runtime.
pub(crate) fn project_path(
    project: Option<&Path>,
    cwd: &Path,
) -> Result<PathBuf, crate::review::Error> {
    let cwd = cwd.canonicalize()?;
    let nearest = nearest_project(&cwd);
    let Some(project) = project.filter(|path| *path != Path::new(".")) else {
        return Ok(nearest.unwrap_or(&cwd).into());
    };
    let selected = absolute_project(project, &cwd);
    if let Some(nearest) = nearest
        && nearest != selected
    {
        if recorded_launch(
            &nearest.join(".shield/review/state.json"),
            &selected,
            nearest,
        ) || crate::review::bootstrap::accepts_launch(nearest, &selected)
        {
            return Ok(nearest.into());
        }
        return Ok(selected);
    }
    Ok(selected)
}

/// The agent's session folder from this process's host-set environment.
fn session_dir(agent: Option<&str>) -> Option<PathBuf> {
    crate::agents::find(agent?)?
        .session_dir(&serde_json::Value::Null, &|name| std::env::var_os(name))
}

/// The project an MCP server serves, chosen like a native hook's: the nearest
/// project holding the folder the host started the session in (the agent's
/// session folder, else the server's launch folder), so the server and hooks
/// agree after a move or in a worktree that inherited an absolute `--project`.
/// Outside every project the launch selection stands.
pub(crate) fn serve_project(
    project: Option<&Path>,
    cwd: &Path,
    session: Option<&Path>,
) -> Result<PathBuf, crate::review::Error> {
    match session_project(session.unwrap_or(cwd)) {
        Some(found) => Ok(found),
        None => project_path(project, cwd),
    }
}

/// The nearest folder at or above a canonical `cwd` that holds Shield project files.
fn nearest_project(cwd: &Path) -> Option<&Path> {
    cwd.ancestors().find(|path| {
        [".shield/config.yml", ".shield/review"].iter().any(|name| {
            match fs::symlink_metadata(path.join(name)) {
                Ok(_) => true,
                Err(error) => error.kind() != io::ErrorKind::NotFound,
            }
        })
    })
}

/// The project holding the folder a host started its session in, found the way
/// Git finds a repository. A native hook uses it instead of its `--project`:
/// after a move, or in a worktree that inherited ignored agent settings, an
/// absolute `--project` names a folder that does not hold the session.
pub(crate) fn session_project(session: &Path) -> Option<PathBuf> {
    nearest_project(&session.canonicalize().ok()?).map(Path::to_path_buf)
}

fn absolute_project(project: &Path, cwd: &Path) -> PathBuf {
    let selected = if project.is_absolute() {
        project.into()
    } else {
        cwd.join(project)
    };
    selected.canonicalize().unwrap_or(selected)
}

fn recorded_launch(path: &Path, requested: &Path, current: &Path) -> bool {
    recorded_claim(path).is_some_and(|value| {
        if value["operator"]["portable_launch_roots"]
            .as_array()
            .is_some_and(|roots| {
                roots.iter().take(2).any(|root| {
                    root.as_str()
                        .is_some_and(|root| Path::new(root) == requested)
                })
            })
        {
            return true;
        }
        // These claims select a runtime only. Its checkpoint or private policy
        // receipt still has to pass the ordinary authority validation.
        value["project_root"]
            .as_str()
            .is_some_and(|root| Path::new(root) != current)
            && (value["project_root"]
                .as_str()
                .is_some_and(|root| Path::new(root) == requested)
                || crate::review::bootstrap::launch_matches_metadata(current, requested))
    })
}

fn recorded_claim(path: &Path) -> Option<serde_json::Value> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 * 1024
    {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let value = crate::json::parse(&bytes).ok()?;
    Some(value)
}
