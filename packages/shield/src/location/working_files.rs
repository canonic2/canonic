//! Trusted integration observations, never deserialized from tool input.
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkingFiles {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    /// Adapter-selected configuration folders; temporary access never opens them.
    pub protected: Vec<PathBuf>,
    /// The user's home folder, where HTTP clients read their configuration.
    pub home: Option<PathBuf>,
    /// Native HTTP configuration/proxy environment may change a literal fetch.
    pub http_environment: bool,
    /// The shell analyzer found the submitted request to be one plain sed
    /// invocation, so no other operation in it can create a file it edits.
    pub sole_operation: bool,
}

impl WorkingFiles {
    pub(crate) fn roots(&self, read: bool) -> impl Iterator<Item = &PathBuf> {
        self.write
            .iter()
            .chain(self.read.iter().filter(move |_| read))
    }
}
