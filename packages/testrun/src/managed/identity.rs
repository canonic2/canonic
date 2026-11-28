//! Client identity: the optional host label, workspace and simulator device
//! set every CLI command and MCP server sends with its requests. Identity
//! grants nothing; every client reaches the same manager and devices.
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// The records key of the manager's device pool: one per data root, shared
/// by every client.
pub(crate) const POOL: &str = "manager";

/// A client's identity as it travels to the manager.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Identity {
    /// A host label such as `TESTRUN_TASK_ID` or `CODEX_THREAD_ID`: metadata
    /// for status and history.
    pub(crate) label: Option<String>,
    /// The repository root containing the client's working directory, which
    /// namespaces test builds.
    pub(crate) workspace: PathBuf,
    /// The simulator device set (`TESTRUN_SIMULATOR_DEVICE_SET`), or the default.
    #[serde(default)]
    pub(crate) device_set: Option<PathBuf>,
}

/// The inputs an identity is resolved from, gathered from the process
/// environment by [`Identity::from_environment`] and supplied directly in tests.
pub(crate) struct Inputs<'a> {
    pub(crate) label: Option<&'a str>,
    pub(crate) directory: &'a Path,
}

const MAX_LABEL: usize = 128;

impl Identity {
    pub(crate) fn from_environment() -> Result<Self> {
        let label = std::env::var("TESTRUN_TASK_ID")
            .ok()
            .or_else(|| std::env::var("CODEX_THREAD_ID").ok())
            .filter(|value| !value.is_empty());
        let directory = std::env::current_dir()
            .map_err(|_| Error::new("HOST_IO", "Cannot read the working directory"))?;
        Self::resolve(&Inputs {
            label: label.as_deref(),
            directory: &directory,
        })
    }

    pub(crate) fn resolve(inputs: &Inputs<'_>) -> Result<Self> {
        Ok(Self {
            label: inputs.label.map(validate).transpose()?,
            workspace: workspace(inputs.directory),
            device_set: crate::simulator::device_set::DeviceSet::configured()
                .map(|path| path.canonicalize().unwrap_or(path)),
        })
    }

    /// The key that namespaces this client's test builds: its workspace.
    pub(crate) fn workspace_key(&self) -> String {
        self.workspace.display().to_string()
    }
}

fn validate(label: &str) -> Result<String> {
    if label.len() > MAX_LABEL || label.chars().any(char::is_control) {
        return Err(Error::invalid(format!(
            "Host labels are at most {MAX_LABEL} characters without control characters"
        )));
    }
    Ok(label.to_owned())
}

/// The repository root containing `directory`, resolved through symlinks; the
/// directory itself when it is not inside a repository.
fn workspace(directory: &Path) -> PathBuf {
    let resolved = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    resolved
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .map_or_else(|| resolved.clone(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(label: Option<&str>, directory: &Path) -> Result<Identity> {
        Identity::resolve(&Inputs { label, directory })
    }

    #[test]
    fn workspace_is_the_repository_root_through_symlinks_or_the_directory() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("acme");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::create_dir_all(repository.join("app/src")).unwrap();
        let canonical = repository.canonicalize().unwrap();
        assert_eq!(
            resolve(None, &repository.join("app/src"))
                .unwrap()
                .workspace,
            canonical
        );
        #[cfg(unix)]
        {
            let link = root.path().join("acme-link");
            std::os::unix::fs::symlink(&repository, &link).unwrap();
            assert_eq!(
                resolve(None, &link.join("app")).unwrap().workspace,
                canonical
            );
        }
        let plain = root.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            resolve(None, &plain).unwrap().workspace,
            plain.canonicalize().unwrap()
        );
    }

    #[test]
    fn labels_are_metadata_and_invalid_ones_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let labelled = resolve(Some("thread-1"), directory.path()).unwrap();
        assert_eq!(labelled.label.as_deref(), Some("thread-1"));
        assert!(resolve(None, directory.path()).unwrap().label.is_none());
        for label in ["bad\tlabel", &"x".repeat(MAX_LABEL + 1)] {
            assert_eq!(
                resolve(Some(label), directory.path()).unwrap_err().code,
                "INVALID_ARGUMENT"
            );
        }
    }
}
