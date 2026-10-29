//! Pure, conservative tool-effect hints from submitted arguments.

use crate::policy::Request;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
mod arguments;
mod docker;
mod git;
#[cfg(test)]
mod tests;

/// Reusable possible-effect predicates. These carry no allow/deny/profile values.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// Repository object/index/reference mutation.
    RepositoryWrite,
    /// Checkout/host file mutation.
    WorktreeWrite,
    /// Removal of repository, checkout, daemon, or image data.
    DataRemoval,
    /// Rewrite of existing references/history.
    HistoryRewrite,
    /// Transfer to/from a remote service.
    Network,
    /// Hooks, helpers, plugins, arbitrary processes or container commands.
    CodeExecution,
    /// Docker daemon objects/lifecycle mutation.
    DaemonWrite,
    /// Image construction from a build definition.
    ImageBuild,
    /// Host directories/devices exposed to a runtime.
    HostExposure,
    /// Elevated runtime privileges requested.
    PrivilegedRuntime,
    /// Configuration/credential changes or injection.
    Configuration,
}

/// Possible tool effects classified from literal executable names and arguments.
#[derive(Debug, Serialize)]
pub struct Hints {
    model: &'static str,
    tool_hint: &'static str,
    command: Option<String>,
    effects: BTreeSet<Effect>,
    complete: bool,
    unresolved: Vec<&'static str>,
    global_options: Vec<arguments::OptionValue>,
    command_path: Vec<String>,
    /// Inspectable possible nested execution with its own runtime namespace.
    pub nested: Vec<NestedExecution>,
    /// The exact request remains available; options/config/paths are never discarded.
    pub request: Request,
}

/// Retained indirect code with declared uncertainty; never a host launch request.
#[derive(Debug, Serialize)]
pub struct NestedExecution {
    /// Origin of the possible execution (alias or container argv).
    pub kind: &'static str,
    /// Shell source when explicitly supplied as an executable shell alias.
    pub source: Option<String>,
    /// Literal program and arguments in a different runtime namespace.
    pub argv: Vec<String>,
    /// Index in the original argv supplying this evidence.
    pub argument_index: usize,
    /// Runtime namespace; no mapping to host paths is asserted.
    pub namespace: &'static str,
}

impl Hints {
    /// Test a possible effect without choosing a permission or profile.
    #[must_use]
    pub fn may(&self, effect: Effect) -> bool {
        self.effects.contains(&effect)
    }

    pub(crate) fn symbolic(request: &Request, effects: &[Effect]) -> Self {
        Self {
            model: "symbolic-possible-effects-proof-v1",
            tool_hint: "symbolic",
            command: None,
            effects: effects.iter().copied().collect(),
            complete: false,
            unresolved: vec!["symbolic-comparison-only"],
            global_options: Vec::new(),
            command_path: Vec::new(),
            nested: Vec::new(),
            request: request.clone(),
        }
    }
}

/// Annotate recognizable Git/Docker names and retain the original request.
/// Unknown commands/options and implicit helpers retain incomplete coverage.
#[must_use]
pub fn classify(request: &Request) -> Option<Hints> {
    let name = request.executable.rsplit('/').next()?;
    if !matches!(name, "git" | "docker") {
        return None;
    }
    let mut hints = Hints {
        model: "possible-tool-effects-v2",
        tool_hint: if name == "git" { "git" } else { "docker" },
        command: None,
        effects: BTreeSet::new(),
        complete: false,
        unresolved: Vec::new(),
        global_options: Vec::new(),
        command_path: Vec::new(),
        nested: Vec::new(),
        request: request.clone(),
    };
    if name == "git" {
        git::classify(&mut hints);
    } else {
        docker::classify(&mut hints);
    }
    Some(hints)
}
