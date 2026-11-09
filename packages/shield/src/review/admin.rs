//! Frozen administrative reviews shared by operator and MCP adapters.
//!
//! Preparation never authorizes a change. MCP retains this server-owned value
//! while the enrolled host asks the human, and commits only that exact value
//! after its existing trusted confirmation exchange succeeds.
use super::{
    Checkpoint, Context, Error,
    integrations::{self, IntegrationChoice},
    model::State,
    project,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(in crate::review) mod policy;
pub(in crate::review) mod protection;
pub(in crate::review) mod registries;

#[derive(Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Input {
    #[serde(rename = "enable")]
    Enable {},
    #[serde(rename = "disable")]
    Disable {},
    #[serde(rename = "protection_set")]
    Protection {
        agent_settings_read: Option<super::path_access::Read>,
        agent_settings_change: Option<super::path_access::Change>,
        shield_config_change: Option<super::path_access::Change>,
    },
    #[serde(rename = "cache_prepare")]
    Cache { cache_dir: Option<PathBuf> },
    #[serde(rename = "registry_add")]
    RegistryAdd {
        name: String,
        location: String,
        #[serde(default)]
        scope: Scope,
    },
    #[serde(rename = "registry_remove")]
    RegistryRemove {
        name: String,
        #[serde(default)]
        scope: Scope,
    },
    #[serde(rename = "registry_default")]
    RegistryDefault {
        name: String,
        #[serde(default)]
        scope: Scope,
    },
    #[serde(rename = "policy_repair")]
    Repair {},
    #[serde(rename = "history_configure")]
    History {
        enabled: Option<bool>,
        limit: Option<usize>,
        #[serde(default)]
        purge: bool,
    },
    #[serde(rename = "policy_reject")]
    Reject {
        proposal_id: String,
        reason: Option<String>,
    },
    #[serde(rename = "mcp_setup")]
    Setup {
        #[serde(default)]
        agents: Vec<String>,
        #[serde(default)]
        remove: bool,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        scope: Scope,
        #[serde(default)]
        repair: bool,
        #[serde(default)]
        pending: Option<integrations::recovery::Action>,
    },
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Scope {
    #[default]
    Project,
    User,
}

enum Operation {
    Mode(super::mode::Prepared),
    Protection(protection::Prepared),
    Cache(Option<PathBuf>),
    Registry(registries::Prepared, Scope),
    Repair(Box<State>),
    History(policy::History),
    Reject(policy::Reject),
    Setup {
        metadata: Value,
        plan: Option<Box<integrations::Plan>>,
    },
    Recovery(Box<integrations::recovery::Prepared>),
}

/// A server-owned exact review. It contains no client-provided confirmation.
pub(super) struct Prepared {
    project_id: String,
    root: String,
    owner: String,
    review: Value,
    dry_run: bool,
    operation: Operation,
}

impl Prepared {
    pub(super) fn review(&self) -> Value {
        self.review.clone()
    }
    /// A model-visible setup preview never reads protected settings back to
    /// the caller. The exact sources remain private to the human review.
    pub(super) fn public_review(&self) -> Value {
        if !matches!(
            self.operation,
            Operation::Setup { .. } | Operation::Recovery(_)
        ) {
            return self.review();
        }
        let mut value = serde_json::Map::new();
        for field in [
            "operation",
            "project",
            "binary",
            "agents",
            "ask_fallbacks",
            "tool_approvals",
            "box",
            "adopt_existing_entries",
            "dry_run",
            "repair",
            "pending",
            "preserved_paths",
            "dropped_ownership",
            "effect",
            "connection_lifecycle",
        ] {
            if let Some(shown) = self.review.get(field) {
                value.insert(field.into(), shown.clone());
            }
        }
        let files = self.review["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|file| {
                let before = file["before"].as_str();
                let after = file["after"].as_str();
                let change = match (before, after) {
                    (None, Some(_)) => "added",
                    (Some(_), None) => "removed",
                    (before, after) if before == after => "unchanged",
                    _ => "updated",
                };
                json!({"path":file["path"],"change":change,
                "before_digest":before.map(|bytes|crate::policy::fingerprint(bytes.as_bytes())),
                "after_digest":after.map(|bytes|crate::policy::fingerprint(bytes.as_bytes()))})
            })
            .collect::<Vec<_>>();
        value.insert("files".into(), json!(files));
        value.insert(
            "protected_contents".into(),
            json!("Exact settings are shown only in the human confirmation form."),
        );
        Value::Object(value)
    }
    #[cfg(test)]
    pub(super) fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// The adapter must complete its trusted human-confirmation exchange first.
    pub(super) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        checkpoint()?;
        if context.project_id != self.project_id
            || context.project_root != self.root
            || context.retry_owner() != self.owner
        {
            return Err(Error::StaleSnapshot);
        }
        if self.dry_run {
            return Ok(self.public_review());
        }
        match self.operation {
            Operation::Mode(plan) => plan.commit(context, checkpoint),
            Operation::Protection(plan) => plan.commit(context, checkpoint),
            Operation::Cache(directory) => match directory {
                Some(directory) => {
                    super::cache::relocate_committed(context, &directory, checkpoint)
                }
                None => super::cache::prepare_committed(context, checkpoint),
            },
            Operation::Registry(plan, scope) => {
                plan.commit((scope == Scope::Project).then_some(context), checkpoint)
            }
            Operation::Repair(before) => super::publication::reconcile(context, &before),
            Operation::History(plan) => plan.commit(context, checkpoint),
            Operation::Reject(plan) => plan.commit(context, checkpoint),
            Operation::Setup { metadata, plan } => integrations::setup::finish_checked(
                context,
                &metadata,
                plan.as_deref(),
                None,
                checkpoint,
            ),
            Operation::Recovery(plan) => plan.commit(context, checkpoint),
        }
    }
}

pub(super) fn supports(name: &str) -> bool {
    matches!(
        name,
        "protection_set"
            | "enable"
            | "disable"
            | "cache_prepare"
            | "registry_add"
            | "registry_remove"
            | "registry_default"
            | "policy_repair"
            | "history_configure"
            | "policy_reject"
            | "mcp_setup"
    )
}

/// Capture everything shown to the human before obtaining host approval.
pub(super) fn prepare(
    context: &Context,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Prepared, Error> {
    checkpoint()?;
    if !supports(name) {
        return Err(Error::Invalid("unknown administrative operation"));
    }
    let input = decode(name, args)?;
    let mut dry_run = false;
    let (operation, review) = match input {
        Input::Enable {} | Input::Disable {} => {
            let plan = super::mode::Prepared::new(context, matches!(input, Input::Enable {}))?;
            let review = plan.review();
            (Operation::Mode(plan), review)
        }
        Input::Protection {
            agent_settings_read,
            agent_settings_change,
            shield_config_change,
        } => protection(
            context,
            agent_settings_read,
            agent_settings_change,
            shield_config_change,
        )?,
        Input::Cache { cache_dir } => {
            if cache_dir.as_ref().is_some_and(|path| !path.is_absolute()) {
                return Err(Error::Invalid("cache_dir must be an absolute directory"));
            }
            let review = json!({"operation":"prepare committed policy cache",
                "project":context.project_root,"cache_directory":cache_dir,
                "policy_authority":"unchanged"});
            (Operation::Cache(cache_dir), review)
        }
        Input::RegistryAdd {
            name,
            location,
            scope,
        } => registry(
            context,
            scope,
            registries::Change::Add {
                name: &name,
                location: &location,
            },
        )?,
        Input::RegistryRemove { name, scope } => {
            registry(context, scope, registries::Change::Remove { name: &name })?
        }
        Input::RegistryDefault { name, scope } => {
            registry(context, scope, registries::Change::Default { name: &name })?
        }
        Input::Repair {} => {
            let before = project::checkpoint(context)?;
            let review = json!({"operation":"restore project files from active policy","policy_revision":before.policy_revision,"manual_edits":"preserved; conflicts require manual review"});
            (Operation::Repair(Box::new(before)), review)
        }
        Input::History {
            enabled,
            limit,
            purge,
        } => {
            let plan = policy::History::new(context, enabled, limit, purge)?;
            let review = plan.review();
            (Operation::History(plan), review)
        }
        Input::Reject {
            proposal_id,
            reason,
        } => {
            let plan = policy::Reject::new(context, &proposal_id, reason.as_deref())?;
            let review = plan.review();
            (Operation::Reject(plan), review)
        }
        Input::Setup {
            agents,
            remove,
            dry_run: preview,
            scope,
            repair: repair_hooks,
            pending,
        } => {
            dry_run = preview;
            setup(
                context,
                &agents,
                remove,
                preview,
                scope,
                repair_hooks,
                pending,
            )?
        }
    };
    checkpoint()?;
    Ok(Prepared {
        project_id: context.project_id.clone(),
        root: context.project_root.clone(),
        owner: context.retry_owner(),
        review,
        dry_run,
        operation,
    })
}

fn decode(name: &str, args: &Value) -> Result<Input, Error> {
    let mut input = args
        .as_object()
        .ok_or(Error::Invalid("administrative arguments must be an object"))?
        .clone();
    if input.contains_key("operation") {
        return Err(Error::Invalid("unexpected operation argument"));
    }
    input.insert("operation".into(), json!(name));
    serde_json::from_value(Value::Object(input))
        .map_err(|_| Error::Invalid("invalid administrative arguments"))
}

fn protection(
    context: &Context,
    agent_settings_read: Option<super::path_access::Read>,
    agent_settings_change: Option<super::path_access::Change>,
    shield_config_change: Option<super::path_access::Change>,
) -> Result<(Operation, Value), Error> {
    if agent_settings_read.is_none()
        && agent_settings_change.is_none()
        && shield_config_change.is_none()
    {
        return Err(Error::Invalid("choose at least one protected file setting"));
    }
    let mut access = super::path_access::load(&project::metadata(&project::checkpoint(context)?))
        .unwrap_or_default();
    if let Some(value) = agent_settings_read {
        access.agent_settings_read = value;
    }
    if let Some(value) = agent_settings_change {
        access.agent_settings_change = value;
    }
    if let Some(value) = shield_config_change {
        access.shield_config_change = value;
    }
    let plan = protection::Prepared::new(context, access)?;
    let review = plan.review();
    Ok((Operation::Protection(plan), review))
}

fn setup(
    context: &Context,
    agents: &[String],
    remove: bool,
    dry_run: bool,
    scope: Scope,
    repair_hooks: bool,
    pending: Option<integrations::recovery::Action>,
) -> Result<(Operation, Value), Error> {
    if let Some(action) = pending {
        if !agents.is_empty() || remove || repair_hooks || scope != Scope::Project {
            return Err(Error::Invalid(
                "pending review or cancel cannot be combined with agent selection, removal, repair or user scope",
            ));
        }
        let metadata = project::metadata(&project::checkpoint(context)?);
        let plan =
            integrations::recovery::prepare(Path::new(&context.project_root), &metadata, action)?;
        let mut review = plan.review(Path::new(&context.project_root));
        review["dry_run"] = json!(dry_run);
        return Ok((Operation::Recovery(Box::new(plan)), review));
    }
    if scope != Scope::Project {
        return Err(Error::Invalid(
            "Shield integrations are project scoped; user scope is not supported",
        ));
    }
    if repair_hooks
        && (remove
            || agents.is_empty()
            || agents
                .iter()
                .any(|agent| !matches!(agent.as_str(), "claude" | "codex")))
    {
        return Err(Error::Invalid(
            "repair requires explicit Claude or Codex installation targets",
        ));
    }
    let metadata = project::metadata(&project::checkpoint(context)?);
    let root = Path::new(&context.project_root);
    let choice = if agents.is_empty() {
        if remove {
            return Err(Error::Invalid("choose agents to remove"));
        }
        if integrations::pending(&metadata)?.is_some() {
            IntegrationChoice::Keep
        } else {
            return Err(Error::Invalid("choose agents to set up"));
        }
    } else {
        let bits = IntegrationChoice::bits(agents)?;
        if remove {
            IntegrationChoice::RemoveAgents(bits)
        } else {
            IntegrationChoice::Agents(bits)
        }
    };
    let mut plan = integrations::setup::prepare_with(
        root,
        &metadata,
        choice,
        false,
        None,
        true,
        repair_hooks,
    )?;
    integrations::setup::with_fallbacks(&metadata, choice, plan.as_mut())?;
    let mut review = integrations::setup::setup_preview(root, plan.as_ref())?;
    review["dry_run"] = json!(dry_run);
    review["repair"] = json!(repair_hooks);
    review["connection_lifecycle"] =
        json!("Restart the agent connection after changing its Shield registration.");
    Ok((
        Operation::Setup {
            metadata,
            plan: plan.map(Box::new),
        },
        review,
    ))
}

fn registry(
    context: &Context,
    scope: Scope,
    change: registries::Change<'_>,
) -> Result<(Operation, Value), Error> {
    // Source paths belong to the selected project even when the registration
    // will be user-wide. Never reinterpret them against the MCP server's cwd.
    let location;
    let change = match change {
        registries::Change::Add {
            name,
            location: input,
        } if !input.starts_with("https://") && !Path::new(input).is_absolute() => {
            location = Path::new(&context.project_root).join(input);
            registries::Change::Add {
                name,
                location: location
                    .to_str()
                    .ok_or(Error::Invalid("registry path must be UTF-8"))?,
            }
        }
        change => change,
    };
    if matches!(change, registries::Change::Add { .. }) {
        context
            .progress
            .stage(super::progress::Stage::DiscoverRegistry);
    }
    let plan = registries::Prepared::new(
        (scope == Scope::Project).then_some(context),
        &context.fetcher,
        change,
    )?;
    let review = plan.review();
    Ok((Operation::Registry(plan, scope), review))
}

#[cfg(test)]
mod tests;
