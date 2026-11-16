//! Trusted integration observations, never deserialized from tool input.
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkingFiles {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    /// Adapter-selected skill libraries, editable without opening configuration.
    pub skills: Vec<PathBuf>,
    /// Adapter-selected configuration folders; temporary access never opens them.
    pub protected: Vec<PathBuf>,
    /// The user's home folder, where HTTP clients read their configuration.
    pub home: Option<PathBuf>,
    /// Native HTTP configuration/proxy environment may change a literal fetch.
    pub http_environment: bool,
    /// The shell analyzer found one plain text-filter invocation, so no other
    /// operation in the request can create an input first.
    pub sole_operation: bool,
    /// An earlier or concurrent shell operation may create or redirect inputs.
    pub filesystem_unknown: bool,
}

impl WorkingFiles {
    pub(crate) fn roots(&self, read: bool) -> impl Iterator<Item = &PathBuf> {
        self.write
            .iter()
            .chain(self.skills.iter())
            .chain(self.read.iter().filter(move |_| read))
    }
}
