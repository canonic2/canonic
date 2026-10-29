//! Best-effort local CLI discovery for adapter-driven onboarding defaults.
use std::path::PathBuf;

/// Snapshot PATH once and let each registered adapter decide whether it is present.
pub(crate) fn installed() -> Vec<&'static str> {
    let search_path: Vec<_> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    super::ADAPTERS
        .iter()
        .filter(|adapter| adapter.is_installed(&search_path))
        .map(|adapter| adapter.spec().id)
        .collect()
}

/// Check executable files on supported Unix setup hosts without launching them.
/// Missing or inaccessible candidates are ordinary discovery misses.
pub(super) fn on_path(name: &str, search_path: &[PathBuf]) -> bool {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn adapters_detect_only_their_own_executable() {
        let root = assert_fs::TempDir::new().unwrap();
        let executable = root.path().join("claude");
        // Discovery must not execute this file, even though its mode is executable.
        std::fs::write(&executable, "not a runnable program").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths = vec![root.path().join("missing"), root.path().to_owned()];
        let claude = crate::agents::find("claude").unwrap();
        let codex = crate::agents::find("codex").unwrap();
        assert!(claude.is_installed(&paths));
        assert!(!codex.is_installed(&paths));
        symlink(&executable, root.path().join("codex")).unwrap();
        assert!(codex.is_installed(&paths));
        assert!(!claude.is_installed(&[]));
        // Reach the real fixture through a relative path without changing cwd.
        let upward: PathBuf = std::env::current_dir()
            .unwrap()
            .ancestors()
            .skip(1)
            .map(|_| "..")
            .collect();
        let relative = upward.join(
            root.path()
                .canonicalize()
                .unwrap()
                .strip_prefix("/")
                .unwrap(),
        );
        let relative_paths = vec![PathBuf::new(), PathBuf::from("."), relative];
        assert!(!claude.is_installed(&relative_paths));
        assert!(!codex.is_installed(&relative_paths));
    }

    #[test]
    fn discovery_ignores_nonexecutables_directories_and_broken_links() {
        let root = assert_fs::TempDir::new().unwrap();
        let paths = vec![root.path().to_owned()];
        let candidate = root.path().join("acme");
        std::fs::write(&candidate, "fixture").unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!on_path("acme", &paths));
        std::fs::create_dir(root.path().join("directory")).unwrap();
        assert!(!on_path("directory", &paths));
        symlink(root.path().join("missing"), root.path().join("broken")).unwrap();
        assert!(!on_path("broken", &paths));
    }
}
