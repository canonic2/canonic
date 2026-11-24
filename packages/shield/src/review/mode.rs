//! Native interception mode, the `enforcement` key of the policy files,
//! changed by an exact human-reviewed action.
use super::{Checkpoint, Context, Error, integrations, project, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The mode as earlier checkpoints recorded it; read once to move it into
/// `config.local.yml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mode {
    pub(crate) enabled: bool,
    generation: u64,
}

impl Mode {
    pub(crate) fn with(enabled: bool) -> Self {
        Self {
            enabled,
            generation: 0,
        }
    }
}

/// Whether native checks are on, read with the project's state. Invalid state
/// is an error, never an implicit request to bypass protection.
pub(crate) fn current(root: &Path) -> Result<bool, Error> {
    Ok(Store::open(root)?.read_checkpoint()?.settings.enabled())
}

/// Mandatory path protection retained during a policy pause. These values come
/// from the same checked authority checkpoint as the mode, never tool input.
pub(crate) struct Paused {
    pub(crate) root: PathBuf,
    pub(crate) cache: Option<PathBuf>,
    pub(crate) access: super::path_access::PathAccess,
}

pub(super) fn paused(state: &super::model::State) -> Option<Paused> {
    if state.settings.enabled() {
        return None;
    }
    Some(Paused {
        root: PathBuf::from(&state.project_root),
        // The folder the policy files' compiled plans live in.
        cache: super::files::cache_for(&state.operator).ok(),
        access: super::settings::path_access(&state.settings),
    })
}

pub(super) struct Prepared {
    root: String,
    project_id: String,
    before: bool,
    enabled: bool,
    metadata: Value,
    files: Vec<integrations::files::Change>,
    /// The policy files this change was reviewed against, which hold the mode.
    policy_files: Option<String>,
}

impl Prepared {
    pub(super) fn new(context: &Context, enabled: bool) -> Result<Self, Error> {
        let state = project::checkpoint(context)?;
        let metadata = project::metadata(&state);
        let root = Path::new(&context.project_root);
        let files = if enabled == state.settings.enabled() {
            Vec::new()
        } else {
            let prefixes = if enabled {
                crate::runtime::Project::open(root)
                    .and_then(|project| project.snapshot())
                    .map_err(|_| {
                        Error::Invalid(
                            "active policy is unavailable; repair it before enabling Shield",
                        )
                    })?
                    .codex_prefixes()
                    .to_vec()
            } else {
                Vec::new()
            };
            integrations::prefix_rules::plan(root, &metadata, &prefixes)?
        };
        Ok(Self {
            root: context.project_root.clone(),
            project_id: context.project_id.clone(),
            before: state.settings.enabled(),
            enabled,
            metadata,
            files,
            policy_files: super::files::digest(root).ok(),
        })
    }

    /// Whether the mode, policy or files this review saw changed since.
    fn stale(&self, state: &super::model::State, root: &Path) -> bool {
        state.settings.enabled() != self.before
            || project::metadata(state) != self.metadata
            || super::files::digest(root).ok() != self.policy_files
    }

    pub(super) fn review(&self) -> Value {
        json!({"operation":if self.enabled {"enable Shield"} else {"disable Shield"},
            "project":self.root,
            "before":self.before,"enabled":self.enabled,"scope":"project",
            "effect":if self.enabled {"Native hooks resume active policy checks."} else {"Native hooks pause policy decisions while retaining protected-file and operator-command guards. Other requests defer to the agent's own permissions and sandbox; Shield does not approve them."},
            "state_trust":self.enabled.then_some("Resuming trusts the saved local policy and authority state, including any edits made while paused outside intercepted requests. Hooks do not authenticate those edits or isolate same-user processes."),
            "preserved":"Policy, packs, history, grants and agent registrations are retained. Shield MCP remains available and administrative changes still require human confirmation. Explicit checked execution and SDK consumers continue to enforce policy.",
            "files":self.files.iter().map(|file|json!({"path":file.path,"before":file.before,"after":file.after,"change":if self.enabled {"restore policy permission rules"} else {"remove policy permission rules"}})).collect::<Vec<_>>(),
            "restart":"Restart connected agents to refresh cached permission rules. Native hooks read the setting on their next invocation."})
    }

    pub(super) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        if context.project_root != self.root || context.project_id != self.project_id {
            return Err(Error::StaleSnapshot);
        }
        let root = Path::new(&context.project_root);
        let store = Store::open(root)?;
        store.read_checkpoint()?.check(context)?;
        let publication = super::publication::ProjectionLease::acquire(root)?;
        publication.check(context)?;
        let changed = self.before != self.enabled;
        if !self.enabled {
            // Under the writer and following the files, as the review was.
            store.update_checkpoint_if_changed(context, checkpoint, |state| {
                if self.stale(state, root) {
                    return Err(Error::StaleSnapshot);
                }
                Ok(false)
            })?;
            integrations::files::validate_before(root, &self.files)?;
            integrations::files::publish(root, &self.files)?;
        }
        let expected_files: Vec<_> = self
            .files
            .iter()
            .cloned()
            .map(|mut file| {
                if !self.enabled {
                    file.before.clone_from(&file.after);
                }
                file
            })
            .collect();
        store.update_checkpoint_if_changed(context, checkpoint, |state| {
            if self.stale(state, root) {
                return Err(Error::StaleSnapshot);
            }
            integrations::files::validate_before(root, &expected_files)?;
            // Remove derived permissions before bypassing native checks. A
            // failed file write then leaves enforcement on with fewer grants.
            if changed {
                super::settings::write(context, &super::settings::enforcement(self.enabled))?;
                state.settings.enforcement = super::settings::enforcement(self.enabled)
                    .values()
                    .enforcement;
            }
            Ok(false)
        })?;
        let mut receipt = json!({"schema_version":1,"enabled":self.enabled,"changed":changed,
            "restart_required":!self.files.is_empty(),"scope":"project"});
        if self.enabled && !self.files.is_empty() {
            // Enable checks before restoring derived grants. Failure remains
            // restrictive, and a competing toggle cannot restore these files.
            let repair = (|| {
                let state = store.read_checkpoint()?;
                state.check(context)?;
                if state.settings.enabled() != self.enabled
                    || project::metadata(&state) != self.metadata
                {
                    return Err(Error::StaleSnapshot);
                }
                integrations::files::publish(root, &self.files)
            })();
            if let Err(error) = repair {
                receipt["repair_needed"] = json!(true);
                receipt["setup_error"] = json!(error.to_string());
                receipt["repair_route"] = json!("shield mcp setup");
            }
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (assert_fs::TempDir, Context) {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = super::super::mcp::initialize_fixture(root.path(), &bundle);
        (root, context)
    }

    #[test]
    fn disabling_writes_the_local_file_and_the_checkpoint_keeps_no_copy() {
        let (root, context) = crate::review::tests::enrolled();
        std::fs::write(
            root.path().join(".shield/config.yml"),
            "schema_version: 1\npacks: []\n",
        )
        .unwrap();
        Prepared::new(&context, false)
            .unwrap()
            .commit(&context, &|| Ok(()))
            .unwrap();
        let local = std::fs::read_to_string(root.path().join(".shield/config.local.yml")).unwrap();
        assert!(local.contains("enforcement: paused"), "{local}");
        let stored: Value = serde_json::from_slice(
            &std::fs::read(root.path().join(".shield/state/state.json")).unwrap(),
        )
        .unwrap();
        assert_ne!(stored["enforcement"]["enabled"], false, "{stored}");
        // The next hook read follows the file.
        assert!(!current(root.path()).unwrap());
    }

    #[test]
    fn shield_mode_is_local_preserves_authority_and_rejects_stale_reviews() {
        let (root, context) = fixture();
        let before = project::checkpoint(&context).unwrap();
        let stale = Prepared::new(&context, false).unwrap();
        let plan = Prepared::new(&context, false).unwrap();
        assert!(current(root.path()).unwrap());
        assert_eq!(plan.commit(&context, &|| Ok(())).unwrap()["enabled"], false);
        assert!(!current(root.path()).unwrap());
        Prepared::new(&context, true)
            .unwrap()
            .commit(&context, &|| Ok(()))
            .unwrap();
        assert!(matches!(
            stale.commit(&context, &|| Ok(())),
            Err(Error::StaleSnapshot)
        ));
        let after = project::checkpoint(&context).unwrap();
        assert_eq!(before.policy_revision, after.policy_revision);
        assert_eq!(before.operator, after.operator);
        assert_eq!(before.read_requests.len(), after.read_requests.len());
        assert_eq!(before.run_requests.len(), after.run_requests.len());
        let state_path = root.path().join(".shield/state/state.json");
        let unchanged = std::fs::read(&state_path).unwrap();
        assert_eq!(
            Prepared::new(&context, true)
                .unwrap()
                .commit(&context, &|| Ok(()))
                .unwrap()["changed"],
            false
        );
        assert_eq!(std::fs::read(state_path).unwrap(), unchanged);
        assert_eq!(project::status(&context).unwrap()["enabled"], true);
    }

    #[test]
    fn shield_mode_defaults_to_enabled_but_malformed_state_cannot_disable_it() {
        let (root, context) = fixture();
        let path = root.path().join(".shield/state/state.json");
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("enforcement");
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(current(root.path()).unwrap());
        value["enforcement"] = json!({"enabled":"false","generation":0});
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(current(root.path()).is_err());
        assert!(Prepared::new(&context, false).is_err());
    }
}
