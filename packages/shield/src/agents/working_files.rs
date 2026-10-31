//! Native filesystem discovery helpers used explicitly by individual adapters.
use crate::location::WorkingFiles;
use crate::policy::portability::{Kind, Root};
use std::path::{Path, PathBuf};

pub(super) fn configuration(override_name: &str, default: &str) -> Option<PathBuf> {
    std::env::var_os(override_name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(default)))
}

/// The native temporary folders, as the platform names them.
fn temporary_candidates() -> Vec<PathBuf> {
    #[cfg(unix)]
    {
        vec![
            std::env::temp_dir(),
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
        ]
    }
    #[cfg(not(unix))]
    {
        vec![std::env::temp_dir()]
    }
}

/// The folders that differ between machines, from adapter scopes discovered
/// without a session: the project checkout, the adapters' configuration
/// folders, the native temporary roots and the home folder, each spelled both
/// as discovered and as resolved, such as `/tmp` and `/private/tmp`.
pub(super) fn machine_roots(scopes: &[WorkingFiles], project: &Path) -> Vec<Root> {
    let spellings = |paths: Vec<PathBuf>| {
        let mut all = Vec::new();
        for path in paths {
            all.extend(path.canonicalize().ok());
            all.push(path);
        }
        all.sort();
        all.dedup();
        all
    };
    let mut temporary: Vec<PathBuf> = scopes
        .iter()
        .flat_map(|scope| scope.roots(false).cloned())
        .collect();
    for candidate in temporary_candidates() {
        if candidate
            .canonicalize()
            .is_ok_and(|resolved| temporary.contains(&resolved))
        {
            temporary.push(candidate);
        }
    }
    let home = std::env::home_dir().filter(|home| home.is_absolute());
    let root = |kind, paths| Root {
        kind,
        paths: spellings(paths),
    };
    vec![
        root(Kind::Project, vec![project.to_path_buf()]),
        root(
            Kind::AgentConfiguration,
            scopes
                .iter()
                .flat_map(|scope| scope.protected.iter().cloned())
                .collect(),
        ),
        root(Kind::Temporary, temporary),
        root(Kind::Home, home.into_iter().collect()),
    ]
}

pub(super) fn temporary(config: Option<&Path>) -> WorkingFiles {
    let mut scope = temporary_in(temporary_candidates(), std::env::home_dir().as_deref());
    scope.http_environment = std::env::vars_os().any(|(name, value)| {
        name.to_str().is_some_and(|name| {
            crate::http_client::environment_entry_changes_request(name, !value.is_empty())
        })
    });
    // Keep a not-yet-created configuration root too. The protected-path guard
    // resolves its existing ancestors and aliases afresh for each request.
    if let Some(config) = config {
        match std::path::absolute(config) {
            Ok(path) => scope.protected.push(path),
            Err(_) => scope.write.clear(),
        }
    }
    scope
}

fn temporary_in(candidates: Vec<PathBuf>, home: Option<&Path>) -> WorkingFiles {
    let home = home
        .and_then(|home| home.canonicalize().ok())
        .filter(|home| home.is_dir());
    let mut write: Vec<_> = candidates
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| path.parent().is_some() && path.is_dir())
        .filter(|path| {
            home.as_ref().is_some_and(|home| {
                !home.starts_with(path)
                    && (!path.starts_with(home) || profile_temporary(path, home))
            })
        })
        .collect();
    write.sort();
    write.dedup();
    WorkingFiles {
        read: vec![],
        write,
        home,
        protected: vec![],
        http_environment: false,
        sole_operation: false,
    }
}

fn profile_temporary(path: &Path, home: &Path) -> bool {
    #[cfg(windows)]
    {
        let native = home.join("AppData/Local/Temp");
        path == native
            && native
                .canonicalize()
                .is_ok_and(|resolved| resolved == native)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, home);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn native_profile_temporary_is_narrow_and_excludes_other_home_folders() {
        let root = assert_fs::TempDir::new().unwrap();
        let home = root.path().canonicalize().unwrap().join("home");
        let temporary = home.join("AppData/Local/Temp");
        let credentials = home.join(".ssh");
        for directory in [&temporary, &credentials] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let scope = temporary_in(
            vec![home.clone(), temporary.clone(), credentials],
            Some(&home),
        );
        assert_eq!(scope.write, vec![temporary.canonicalize().unwrap()]);
    }

    #[test]
    fn native_temporary_discovery_deduplicates_aliases_and_rejects_broad_overrides() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let scratch = temp.path().join("scratch");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        let scope = temporary_in(
            vec![
                scratch.clone(),
                scratch.clone(),
                home.clone(),
                temp.path().into(),
                "/".into(),
            ],
            Some(&home),
        );
        assert_eq!(scope.write, vec![scratch.canonicalize().unwrap()]);
        assert_eq!(scope.read, Vec::<PathBuf>::new());
    }

    #[test]
    fn native_temporary_discovery_rejects_home_descendants_and_keeps_system_scratch() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let scratch = temp.path().join("system-scratch");
        let prefix_neighbor = temp.path().join("home-other");
        let descendants = [
            home.join(".ssh"),
            home.join(".claude"),
            home.join(".codex"),
            home.join("Code/other-project"),
            home.join(".cache/tmp"),
        ];
        for path in descendants.iter().chain([&scratch, &prefix_neighbor]) {
            std::fs::create_dir_all(path).unwrap();
        }
        let mut candidates = descendants.to_vec();
        candidates.extend([scratch.clone(), prefix_neighbor.clone()]);
        let scope = temporary_in(candidates, Some(&home));
        let mut expected = vec![
            scratch.canonicalize().unwrap(),
            prefix_neighbor.canonicalize().unwrap(),
        ];
        expected.sort();
        assert_eq!(scope.write, expected);
        assert_eq!(scope.read, Vec::<PathBuf>::new());
    }

    #[cfg(unix)]
    #[test]
    fn native_temporary_discovery_checks_physical_home_and_candidate_aliases() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let sensitive = home.join(".ssh");
        let scratch = temp.path().join("system-scratch");
        std::fs::create_dir_all(&sensitive).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        let home_alias = temp.path().join("home-alias");
        let sensitive_alias = temp.path().join("temporary-alias");
        let scratch_alias = temp.path().join("system-temporary-alias");
        std::os::unix::fs::symlink(&home, &home_alias).unwrap();
        std::os::unix::fs::symlink(&sensitive, &sensitive_alias).unwrap();
        std::os::unix::fs::symlink(&scratch, &scratch_alias).unwrap();
        let scope = temporary_in(vec![sensitive_alias, scratch_alias], Some(&home_alias));
        assert_eq!(scope.write, vec![scratch.canonicalize().unwrap()]);
        assert_eq!(scope.read, Vec::<PathBuf>::new());
    }

    #[test]
    fn native_temporary_discovery_requires_a_verified_home_boundary() {
        let temp = assert_fs::TempDir::new().unwrap();
        let file = temp.path().join("not-a-home-directory");
        std::fs::write(&file, "acme").unwrap();
        for home in [None, Some(temp.path().join("missing-home")), Some(file)] {
            let scope = temporary_in(vec![temp.path().into()], home.as_deref());
            assert_eq!(scope.write, Vec::<PathBuf>::new());
            assert!(scope.home.is_none());
        }
    }
}
