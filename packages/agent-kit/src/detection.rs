//! Local CLI discovery for catalog-driven MCP setup defaults, without execution.
use super::{Context, Options, agents};
use std::path::PathBuf;

/// Explicit selections and removal never infer intent from installed CLIs.
pub(super) fn defaults(context: &Context, options: &Options) -> Vec<String> {
    if options.remove || !options.agents.is_empty() {
        return options.agents.clone();
    }
    let search_path: Vec<_> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    defaults_on_path(context, options, &search_path)
}

/// An explicit search path keeps discovery fixtures independent of the host.
pub(super) fn defaults_on_path(
    context: &Context,
    options: &Options,
    search_path: &[PathBuf],
) -> Vec<String> {
    if options.remove || !options.agents.is_empty() {
        return options.agents.clone();
    }
    // In a Box project, Box publishes to each developer's agents.
    if agents::box_definition_path(context, options.scope).is_ok() {
        return vec![agents::BOX.to_owned()];
    }
    agents::CATALOG
        .iter()
        .filter(|(id, names)| {
            names.iter().any(|name| on_path(name, search_path))
                && agents::destination(context, id, options.scope).is_ok()
        })
        .map(|(id, _)| (*id).to_owned())
        .collect()
}

/// Unix setup hosts use regular executable files, including symlink targets.
/// Other platforms remain manual until native executable rules are supplied.
fn on_path(name: &str, search_path: &[PathBuf]) -> bool {
    search_path.iter().any(|directory| {
        if !directory.is_absolute() {
            return false;
        }
        let path = directory.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            false
        }
    })
}
