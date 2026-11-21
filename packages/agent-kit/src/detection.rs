//! Local CLI discovery for catalog-driven MCP setup defaults, without execution.
use super::{Context, Options, agents};
use std::path::PathBuf;

/// Explicit selections and removal never infer intent from installed CLIs.
pub(super) fn defaults(context: &Context, options: &Options) -> Vec<String> {
    if options.remove || !options.agents.is_empty() {
        return options.agents.clone();
    }
    let search_path: Vec<_> =
        std::env::var_os("PATH").map_or_default(|value| std::env::split_paths(&value).collect());
    defaults_on_path(context, options, &search_path)
}

/// Resolve install-picker defaults from an explicit PATH snapshot without execution.
/// Explicit agents remain authoritative and removal receives no inferred marks.
#[must_use]
pub fn defaults_on_path(
    context: &Context,
    options: &Options,
    search_path: &[PathBuf],
) -> Vec<String> {
    if options.remove || !options.agents.is_empty() {
        return options.agents.clone();
    }
    // In a Hub or Box project, that host publishes to each developer's
    // agents; Hub, Box's successor, wins where both are configured.
    if let Some(host) = agents::CONFIG_HOSTS
        .iter()
        .find(|host| agents::definition_path(host, context, options.scope, false).is_ok())
    {
        return vec![host.id.to_owned()];
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
