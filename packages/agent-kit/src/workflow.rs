//! Review and confirmation compose pure codecs with owned host-file publication.
use super::{
    Authorization, Context, Error, Options, Result, Scope, SetupPolicy, agents, inputs::Inputs,
    storage,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};
mod hooks;
mod portability;
#[cfg(test)]
mod tests;
pub use portability::relocate_project;

/// What a change owns in its file: the product's server entry, or the
/// permission rules setup added for its tools.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Server,
    Approvals,
    PromptHook,
    /// A whole Hub or Box server definition.
    Definition,
}

struct Change {
    kind: Kind,
    agents: Vec<String>,
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
    ledger: PathBuf,
    ledger_before: Option<String>,
    record: storage::Record,
    ownership_unchanged: bool,
    launch: Option<Value>,
}

/// A reviewed plan binds the exact configuration and ownership bytes read.
pub struct Plan {
    context: Context,
    scope: Scope,
    remove: bool,
    changes: Vec<Change>,
    other_scopes: Vec<Value>,
    /// Tools approved without asking, and the selected agents that receive them.
    approved: Vec<String>,
    approval_agents: Vec<String>,
    /// Unmarked tools that Claude Code still approves through permission rules
    /// setup did not write, with those rules.
    kept_by_other_rules: Vec<String>,
    other_rules: Vec<String>,
}

/// Host inspection metadata independent of a terminal UI representation.
#[derive(Clone, Debug)]
pub struct AgentStatus {
    pub id: String,
    pub detail: String,
    pub unavailable: Option<String>,
}

/// One context/scope/policy-bound lifetime for inspection and plan preparation.
/// It retains native-file snapshots only until the caller drops the session.
pub struct PreparationSession<'a> {
    context: &'a Context,
    scope: Scope,
    policy: SetupPolicy,
    inputs: Inputs,
}

impl<'a> PreparationSession<'a> {
    #[must_use]
    pub fn new(context: &'a Context, scope: Scope, policy: SetupPolicy) -> Self {
        Self {
            context,
            scope,
            policy,
            inputs: Inputs::default(),
        }
    }

    /// Inspect a host through this session's shared snapshots without writes.
    #[must_use]
    pub fn inspect(&self, id: &str, remove: bool) -> AgentStatus {
        inspect_inputs(self.context, id, self.scope, remove, &self.inputs)
    }

    /// Prepare an immutable plan from the same inputs used by inspection.
    ///
    /// # Errors
    /// Returns an error for mismatched scope or any failure described by [`prepare`].
    pub fn prepare(&self, options: &Options) -> Result<Plan> {
        if options.scope != self.scope {
            return Err(Error::invalid("Preparation session scope changed"));
        }
        prepare_inputs(self.context, options, &self.inputs, self.policy)
    }
}
impl Plan {
    /// Review metadata excludes unrelated configuration and credentials.
    #[must_use]
    pub fn review(&self) -> Value {
        let tool_approvals = (!self.remove && !self.approval_agents.is_empty()).then(|| {
            json!({"agents":self.approval_agents,"approved":self.approved,
                "ask":self.context.tools.iter().filter(|t| !self.approved.contains(&t.name) && !self.kept_by_other_rules.contains(&t.name)).map(|t| &t.name).collect::<Vec<_>>(),
                "approved_by_other_rules":self.kept_by_other_rules,"other_rules":self.other_rules})
        });
        json!({"product":self.context.product,"scope":self.scope,"root":self.context.root,
            "operation":if self.remove {"remove"} else {"install or repair"},
            "other_scopes":self.other_scopes,
            "tool_approvals":tool_approvals,
            "registrations":self.changes.iter().map(|c| {
                let mut row = json!({"agents":c.agents,"path":c.path,
                "change":if c.before == c.after {"unchanged"} else if self.remove {"remove"} else {"write"},
                "pending_recovery":c.record.pending,
                "format_note":if c.path.extension().is_some_and(|e| e == "yaml") {Some("YAML is rewritten as equivalent JSON; comments and formatting are not retained")} else {None}});
                match c.kind {
                    Kind::Server => row["entry"] = json!(c.record.desired),
                    Kind::Approvals => {
                        row["permission_rules"] = json!(setting_rules(c.record.desired.as_ref()));
                        row["prompt_hook"] = json!(hooks::settings_hook(c.record.desired.as_ref()));
                    }
                    Kind::PromptHook => row["prompt_hook"] = json!(c.record.desired),
                    Kind::Definition => {
                        row["definition"] = json!(c.after);
                        row["entry"] = json!(c.launch);
                    }
                }
                row
            }).collect::<Vec<_>>(),
            "launch_note":(self.scope == super::Scope::Project && !self.remove).then(|| format!(
                "Project files name {} for PATH lookup so they can be shared; agents must find it on PATH (installed at {})",
                self.context.product, self.context.program.display())),
            "host_action":"Trust/reload the agent and inspect its MCP server list; a file write does not establish a live connection"})
    }
    /// Apply only after caller confirmation; refuses concurrent file changes.
    ///
    /// # Errors
    /// Returns an error on lock contention, changed reviewed files or publication
    /// failure. A partial-publication error identifies completed and pending agents.
    pub fn apply(self) -> Result<Value> {
        let mut locks = Vec::new();
        // Stable path order avoids cross-process deadlocks and protects entries
        // shared by multiple products/surfaces using this library.
        let paths: BTreeSet<_> = self.changes.iter().map(|c| c.path.clone()).collect();
        for path in paths {
            locks.push(storage::lock(
                &self.context.state,
                &storage::identity(&path),
            )?);
        }
        for change in &self.changes {
            Self::validate(change)?;
        }
        let mut completed = Vec::new();
        for (index, change) in self.changes.iter().enumerate() {
            if let Err(error) = self.publish(change) {
                return Err(Error::Partial {
                    completed,
                    pending: self.changes[index..]
                        .iter()
                        .flat_map(|change| change.agents.clone())
                        .collect(),
                    source: Box::new(error),
                });
            }
            completed.extend(change.agents.clone());
        }
        let mut result = self.review();
        result["applied"] = json!(true);
        result["registration_verified"] = json!(true);
        result["connected"] = Value::Null;
        Ok(result)
    }
    fn validate(change: &Change) -> Result<()> {
        if storage::read(&change.path)? != change.before
            || storage::read(&change.ledger)? != change.ledger_before
        {
            return Err(Error::Conflict(format!(
                "Configuration changed after review: {}; run setup again",
                change.path.display()
            )));
        }
        Ok(())
    }
    fn publish(&self, change: &Change) -> Result<()> {
        // Apply already checked fresh bytes under the lock; a change that
        // writes nothing needs no second identical read.
        if change.before == change.after && change.ownership_unchanged && !self.remove {
            return Ok(());
        }
        Self::validate(change)?;
        let mut record = change.record.clone();
        record.pending = true;
        let pending =
            serde_json::to_string_pretty(&record).map_err(|e| Error::invalid(e.to_string()))?;
        storage::write(
            &change.ledger,
            &pending,
            true,
            change.ledger_before.as_deref(),
        )?;
        // Recheck host bytes after recording intent, before replacing the file.
        if storage::read(&change.path)? != change.before {
            return Err(Error::Conflict(
                "Host configuration changed during setup".into(),
            ));
        }
        if change.before != change.after {
            match &change.after {
                Some(after) => {
                    storage::write(&change.path, after, false, change.before.as_deref())?;
                }
                // Only a whole owned file is removed rather than edited.
                None if change.kind == Kind::Definition => {
                    storage::remove(&change.path)?;
                }
                None => {}
            }
        }
        if storage::read(&change.path)? != change.after {
            return Err(Error::invalid("Registration verification failed"));
        }
        if storage::read(&change.ledger)?.as_deref() != Some(&pending) {
            return Err(Error::Conflict(
                "Setup ownership changed during publication; review it before repair".into(),
            ));
        }
        if self.remove {
            storage::remove(&change.ledger)?;
            return Ok(());
        }
        record.previous = record.desired.clone();
        record.pending = false;
        storage::write(
            &change.ledger,
            &serde_json::to_string_pretty(&record).map_err(|e| Error::invalid(e.to_string()))?,
            true,
            Some(&pending),
        )
    }
}

/// Prepare every selected host before any writes. This function never prompts.
///
/// # Errors
/// Returns an error for invalid product or agent selections, unsupported scopes,
/// malformed host configuration, filesystem failures or ownership conflicts.
pub fn prepare(context: &Context, options: &Options) -> Result<Plan> {
    prepare_with_inputs(context, options, &Inputs::default())
}

/// Prepare registration using the embedding product's explicit policy.
///
/// # Errors
/// Returns the same preparation errors as [`prepare`], plus incompatible-route errors.
pub fn prepare_with_policy(
    context: &Context,
    options: &Options,
    policy: SetupPolicy,
) -> Result<Plan> {
    PreparationSession::new(context, options.scope, policy).prepare(options)
}

pub(crate) fn prepare_with_inputs(
    context: &Context,
    options: &Options,
    inputs: &Inputs,
) -> Result<Plan> {
    prepare_inputs(context, options, inputs, SetupPolicy::default())
}

fn prepare_inputs(
    context: &Context,
    options: &Options,
    inputs: &Inputs,
    policy: SetupPolicy,
) -> Result<Plan> {
    if context.product.is_empty()
        || context.product.len() > 64
        || !context
            .product
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(Error::invalid(
            "Setup product must be a simple name of at most 64 characters",
        ));
    }
    if options.agents.is_empty() {
        return Err(Error::selection(
            "Select agents with --agent, or run mcp setup in a terminal",
        ));
    }
    validate_policy(options, policy)?;
    let (approved, _) = approved(context, options, inputs);
    let mut plan = Plan {
        context: context.clone(),
        scope: options.scope,
        remove: options.remove,
        changes: Vec::new(),
        other_scopes: Vec::new(),
        approval_agents: if context.tools.is_empty() {
            Vec::new()
        } else {
            agents::APPROVALS
                .iter()
                .filter(|id| options.agents.iter().any(|a| a == *id))
                .map(|id| (*id).to_owned())
                .collect()
        },
        approved,
        kept_by_other_rules: Vec::new(),
        other_rules: Vec::new(),
    };
    let mut selected = BTreeSet::new();
    for id in &options.agents {
        if !selected.insert(id.as_str()) {
            continue;
        }
        if let Some(host) = agents::config_host(id) {
            plan.changes.extend(definition_change(
                host,
                context,
                options,
                &plan.approved,
                inputs,
            )?);
            continue;
        }
        let mut destination = agents::destination(context, id, options.scope)?;
        if policy.project_directory && options.scope == Scope::Project {
            agents::project_directory(&mut destination.entry, id, context);
        }
        if id == "codex" && !context.tools.is_empty() {
            agents::codex_approvals(&mut destination.entry, &context.tools, &plan.approved);
        }
        plan.other_scopes
            .extend(other_scope(context, id, options.scope, inputs));
        if let Some(existing) = plan.changes.iter_mut().find(|c| c.path == destination.path) {
            if !existing.agents.contains(id) {
                existing.agents.push(id.clone());
            }
            continue;
        }
        plan.changes
            .extend(server_change(context, options, destination, inputs)?);
        if id == "codex"
            && let Some(change) = hooks::codex(context, options, inputs)?
        {
            plan.changes.push(change);
        }
    }
    // Hooks need the settings change even without tools; other-rule coverage
    // only matters for tools, so a product without them never parses the file
    // unless it owns something there.
    if plan.approval_agents.iter().any(|id| id == "claude") {
        (plan.kept_by_other_rules, plan.other_rules) =
            claude_other_rules(context, options, &plan.approved, inputs)?;
    }
    if options.agents.iter().any(|id| id == "claude")
        && let Some(change) = claude_approvals(context, options, &plan.approved, inputs)?
    {
        plan.changes.push(change);
    }
    Ok(plan)
}

fn validate_policy(options: &Options, policy: SetupPolicy) -> Result<()> {
    if options.reset_tool_permissions && options.remove {
        return Err(Error::selection(
            "--reset-tool-permissions conflicts with --remove",
        ));
    }
    if options.yes && policy.authorization == Authorization::TerminalOnly {
        return Err(Error::selection(
            "Setup requires terminal confirmation; unattended authorization is unavailable",
        ));
    }
    if policy.exclusive_routes && !options.remove {
        let mut ids = options.agents.iter().map(String::as_str);
        if let Some(host) = ids.clone().find_map(agents::config_host)
            && ids.any(|a| a != host.id)
        {
            return Err(Error::selection(
                "Choose only --agent hub, only --agent box, or native agents alone; combined routes would publish competing server entries",
            ));
        }
    }
    Ok(())
}

/// The agent's registration in the scope not being configured, which the host
/// may load alongside or instead of this one. `None` when that file is absent.
fn other_scope(context: &Context, id: &str, scope: Scope, inputs: &Inputs) -> Option<Value> {
    let other_scope = if scope == Scope::Project {
        Scope::User
    } else {
        Scope::Project
    };
    let other = agents::destination(context, id, other_scope).ok()?;
    let status = match inputs.exists(&other.path) {
        Ok(true) => {
            match inputs
                .document(&other.path, other.format)
                .and_then(|doc| doc.entry(other.key, &context.product))
            {
                Ok(Some(_)) => "registration exists; host scope precedence applies",
                Ok(None) => "no registration",
                Err(_) => "invalid configuration; host loading may fail",
            }
        }
        Ok(false) => return None,
        Err(_) => "unreadable configuration; host loading is unverified",
    };
    Some(json!({"agent":id,"scope":other_scope,"path":other.path,"status":status}))
}

/// The server-entry change for one destination file, refusing entries setup
/// does not own. `None` when removing an entry that is already absent.
fn server_change(
    context: &Context,
    options: &Options,
    destination: agents::Destination,
    inputs: &Inputs,
) -> Result<Option<Change>> {
    let before = inputs.read(&destination.path)?;
    let desired = (!options.remove).then_some(destination.entry);
    // One edit yields both the observed entry and the reviewed result.
    let (current, after) = inputs
        .document(&destination.path, destination.format)?
        .edit(destination.key, &context.product, desired.as_ref())?;
    let ledger = ledger(context, &destination.path);
    let ledger_before = inputs.read(&ledger)?;
    let owned = ownership(context, &destination.path, ledger_before.as_deref())?;
    if let Some(record) = &owned {
        if current.is_some()
            && current != record.desired
            && !(record.pending && current == record.previous)
        {
            return Err(Error::Conflict(format!(
                "Owned {} entry was edited in {}; resolve it manually",
                context.product,
                destination.path.display()
            )));
        }
    } else if current.is_some() {
        return Err(Error::Conflict(format!(
            "Unowned {} entry in {}; preserve or rename it before setup",
            context.product,
            destination.path.display()
        )));
    }
    // An absent removal is a genuine no-op, including no ownership file.
    if options.remove && current.is_none() && owned.is_none() {
        return Ok(None);
    }
    let ownership_unchanged = owned
        .as_ref()
        .is_some_and(|r| !r.pending && r.desired == desired && r.previous == desired);
    let after = if options.remove && before.is_none() {
        None
    } else {
        Some(after)
    };
    let record = storage::Record {
        version: 1,
        path: destination.path.clone(),
        product: context.product.clone(),
        previous: current,
        desired,
        pending: owned.as_ref().is_some_and(|r| r.pending),
    };
    Ok(Some(Change {
        kind: Kind::Server,
        agents: destination.surfaces,
        path: destination.path,
        before,
        after,
        ledger,
        ledger_before,
        record,
        ownership_unchanged,
        launch: None,
    }))
}

/// The change to the product's Hub or Box definition: a whole file setup owns,
/// or `None` when removing one that is absent and unowned. An unowned file with
/// exactly the desired text is taken over; any other unowned file refuses.
fn definition_change(
    host: &agents::ConfigHost,
    context: &Context,
    options: &Options,
    approved: &[String],
    inputs: &Inputs,
) -> Result<Option<Change>> {
    let path = agents::definition_path(host, context, options.scope, options.remove)?;
    let before = inputs.read(&path)?;
    let ledger = ledger(context, &path);
    let ledger_before = inputs.read(&ledger)?;
    let owned = ownership(context, &path, ledger_before.as_deref())?;
    let current = before.clone().map(Value::String);
    let after = if options.remove {
        None
    } else {
        Some(agents::definition(host, context, options.scope, approved)?)
    };
    let desired = after.clone().map(Value::String);
    match &owned {
        Some(record)
            if current.is_some()
                && current != record.desired
                && !(record.pending && current == record.previous) =>
        {
            return Err(Error::Conflict(format!(
                "{} was edited after setup wrote it; restore or delete it, then run setup again",
                path.display()
            )));
        }
        None if current.is_some() && current != desired => {
            return Err(Error::Conflict(format!(
                "{} exists and was not written by setup; delete it or keep it instead of running setup",
                path.display()
            )));
        }
        _ => {}
    }
    if options.remove && current.is_none() && owned.is_none() {
        return Ok(None);
    }
    let ownership_unchanged = owned
        .as_ref()
        .is_some_and(|r| !r.pending && r.desired == desired && r.previous == desired);
    Ok(Some(Change {
        kind: Kind::Definition,
        launch: if options.remove {
            None
        } else {
            Some(
                json!({"command":agents::launch_command(context, options.scope)?,"args":context.arguments}),
            )
        },
        agents: vec![host.id.into()],
        record: storage::Record {
            version: 1,
            path: path.clone(),
            product: context.product.clone(),
            previous: current,
            desired,
            pending: owned.as_ref().is_some_and(|r| r.pending),
        },
        path,
        before,
        after,
        ledger,
        ledger_before,
        ownership_unchanged,
    }))
}

/// Tools left unapproved by this setup that Claude Code still approves through
/// permission rules setup did not write, such as one the user added or a
/// server-wide `mcp__<product>` rule, and those rules. Setup never removes them.
fn claude_other_rules(
    context: &Context,
    options: &Options,
    approved: &[String],
    inputs: &Inputs,
) -> Result<(Vec<String>, Vec<String>)> {
    let path = agents::claude_settings(context, options.scope);
    if !inputs.exists(&path)? {
        return Ok((Vec::new(), Vec::new()));
    }
    let current = inputs.document(&path, agents::Format::Json)?.allowed()?;
    let ledger_before = inputs.read(&ledger(context, &path))?;
    // As in claude_approvals: an interrupted publication may own either side.
    let owned: Vec<String> = ownership(context, &path, ledger_before.as_deref())?
        .into_iter()
        .flat_map(|record| [record.desired, record.previous.filter(|_| record.pending)])
        .flatten()
        .flat_map(|rules| setting_rules(Some(&rules)))
        .collect();
    let server = format!("mcp__{}", context.product);
    let others: Vec<String> = current
        .into_iter()
        .filter(|rule| !owned.contains(rule))
        .filter(|rule| {
            *rule == server
                || rule
                    .strip_prefix(&server)
                    .is_some_and(|rest| rest.starts_with("__"))
        })
        .collect();
    let covers = |tool: &str| {
        others.iter().any(|rule| {
            *rule == server
                || *rule == format!("{server}__*")
                || *rule == agents::claude_rule(&context.product, tool)
        })
    };
    let kept = context
        .tools
        .iter()
        .map(|tool| &tool.name)
        .filter(|tool| options.remove || !approved.contains(tool))
        .filter(|tool| covers(tool))
        .cloned()
        .collect();
    Ok((kept, others))
}

fn ledger(context: &Context, path: &std::path::Path) -> PathBuf {
    context
        .state
        .join(&context.product)
        .join(format!("{}.json", storage::identity(path)))
}

/// The product's ownership record for `path`, refusing one for another file.
fn ownership(
    context: &Context,
    path: &std::path::Path,
    ledger: Option<&str>,
) -> Result<Option<storage::Record>> {
    let Some(text) = ledger else {
        return Ok(None);
    };
    let record = serde_json::from_str::<storage::Record>(text)
        .map_err(|_| Error::invalid("Invalid setup ownership record; restore it before repair"))?;
    if record.version != 1 || record.path != path || record.product != context.product {
        return Err(Error::Conflict("Mismatched setup ownership record".into()));
    }
    Ok(Some(record))
}

/// Return approved tool names in catalog order and whether an explicit or saved
/// selection was found, including an empty selection. Explicit choices win;
/// otherwise reuse saved approvals unless resetting, then use catalog defaults.
fn approved(context: &Context, options: &Options, inputs: &Inputs) -> (Vec<String>, bool) {
    let selected = options.approved.clone().or_else(|| {
        if options.reset_tool_permissions {
            None
        } else {
            current_approvals(context, options, inputs)
        }
    });
    // Catalog order, so the review reads the same however the set was found.
    let approved = context
        .tools
        .iter()
        .filter(|t| {
            selected
                .as_ref()
                .map_or(t.approved, |s| s.contains(&t.name))
        })
        .map(|t| t.name.clone())
        .collect();
    (approved, selected.is_some())
}

/// Approvals an earlier setup left for the selected agents: Codex's owned
/// entry, or Claude Code's rules once setup owns any of them.
fn current_approvals(context: &Context, options: &Options, inputs: &Inputs) -> Option<Vec<String>> {
    if let Some(host) = options.agents.iter().find_map(|a| agents::config_host(a))
        && let Ok(path) = agents::definition_path(host, context, options.scope, options.remove)
        && inputs.exists(&ledger(context, &path)).unwrap_or(false)
        && let Ok(Some(source)) = inputs.read(&path)
    {
        // Setup's own format: one `    "tool": allow` line per approved tool.
        return Some(
            source
                .lines()
                .filter_map(|line| line.strip_prefix("    ")?.strip_suffix(": allow"))
                .filter_map(|name| serde_json::from_str(name).ok())
                .collect(),
        );
    }
    if options.agents.iter().any(|a| a == "codex")
        && let Ok(destination) = agents::destination(context, "codex", options.scope)
        && inputs.exists(&destination.path).unwrap_or(false)
        && let Ok(Some(entry)) = inputs
            .document(&destination.path, destination.format)
            .and_then(|doc| doc.entry(destination.key, &context.product))
        && let Some(tools) = entry["tools"].as_object()
    {
        return Some(
            tools
                .iter()
                .filter(|(_, t)| t["approval_mode"] == "approve")
                .map(|(name, _)| name.clone())
                .collect(),
        );
    }
    if options.agents.iter().any(|a| a == "claude") {
        let path = agents::claude_settings(context, options.scope);
        if !inputs.exists(&ledger(context, &path)).ok()? || !inputs.exists(&path).ok()? {
            return None;
        }
        let rules = inputs
            .document(&path, agents::Format::Json)
            .ok()?
            .allowed()
            .ok()?;
        return Some(
            context
                .tools
                .iter()
                .filter(|t| rules.contains(&agents::claude_rule(&context.product, &t.name)))
                .map(|t| t.name.clone())
                .collect(),
        );
    }
    None
}

/// Claude Code permission rules for the approved tools. Setup owns only the
/// rules it added; a rule the user already had is left alone and never removed.
fn claude_approvals(
    context: &Context,
    options: &Options,
    approved: &[String],
    inputs: &Inputs,
) -> Result<Option<Change>> {
    let path = agents::claude_settings(context, options.scope);
    let before = inputs.read(&path)?;
    let ledger = ledger(context, &path);
    let ledger_before = inputs.read(&ledger)?;
    let record = ownership(context, &path, ledger_before.as_deref())?;
    // An interrupted publication may have written either side of the record.
    let mut owned = record
        .as_ref()
        .map_or_else(Vec::new, |r| setting_rules(r.desired.as_ref()));
    if let Some(r) = record.as_ref().filter(|r| r.pending) {
        for rule in setting_rules(r.previous.as_ref()) {
            if !owned.contains(&rule) {
                owned.push(rule);
            }
        }
    }
    let desired: Vec<String> = if options.remove {
        Vec::new()
    } else {
        approved
            .iter()
            .map(|tool| agents::claude_rule(&context.product, tool))
            .collect()
    };
    if record.is_none() && (options.remove || (desired.is_empty() && context.prompt_hook.is_none()))
    {
        return Ok(None);
    }
    let source = before.as_deref().unwrap_or("{}\n");
    let stale: Vec<String> = owned
        .iter()
        .filter(|rule| !desired.contains(rule))
        .cloned()
        .collect();
    let (current, edited) = inputs
        .document(&path, agents::Format::Json)?
        .allow_list(&stale, &desired)?;
    let (owned_hook, previous_hook, edited) = hooks::claude(
        edited,
        record.as_ref(),
        context.prompt_hook.as_ref().filter(|_| !options.remove),
    )?;
    let now_owned: Vec<String> = desired
        .iter()
        .filter(|rule| owned.contains(rule) || !current.contains(rule))
        .cloned()
        .collect();
    let after = if before.is_none() && edited == source {
        None
    } else {
        Some(edited)
    };
    // Each side carries the hook shape only when it holds a hook: `previous`
    // keeps an observed owned hook until publication completes, even across
    // repeated interruptions, and dropping a hook records the rule list at once.
    let settings = |rules: &[String], hook: Option<Value>| match hook {
        Some(hook) => json!({"permission_rules":rules,"prompt_hook":hook}),
        None => json!(rules),
    };
    let desired = (!options.remove).then(|| settings(&now_owned, owned_hook));
    let previous = record.as_ref().map(|_| settings(&owned, previous_hook));
    let ownership_unchanged = record
        .as_ref()
        .is_some_and(|r| !r.pending && r.desired == desired && r.previous == desired);
    Ok(Some(Change {
        kind: Kind::Approvals,
        agents: vec!["claude".into()],
        record: storage::Record {
            version: 1,
            path: path.clone(),
            product: context.product.clone(),
            previous,
            desired,
            pending: record.is_some_and(|r| r.pending),
        },
        path,
        before,
        after,
        ledger,
        ledger_before,
        ownership_unchanged,
        launch: None,
    }))
}

fn setting_rules(value: Option<&Value>) -> Vec<String> {
    value
        .map(|v| {
            if v.is_array() {
                v
            } else {
                &v["permission_rules"]
            }
        })
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// A concise human report over the same metadata exposed to JSON callers.
#[must_use]
pub fn human(value: &Value) -> String {
    if value["cancelled_by_user"] == true {
        return "Setup cancelled".into();
    }
    let mut lines = vec![format!(
        "{} MCP setup · {} · {} scope",
        value["product"].as_str().unwrap_or(""),
        value["operation"].as_str().unwrap_or(""),
        value["scope"].as_str().unwrap_or("")
    )];
    if let Some(note) = value["launch_note"].as_str() {
        lines.push(format!("  {note}"));
    }
    let scope = if value["scope"] == "user" {
        Scope::User
    } else {
        Scope::Project
    };
    for row in value["registrations"].as_array().into_iter().flatten() {
        registration_lines(row, scope, &mut lines);
    }
    approval_lines(&value["tool_approvals"], &mut lines);
    for row in value["other_scopes"].as_array().into_iter().flatten() {
        if row["status"] != "no registration" {
            lines.push(format!(
                "  Other scope {}: {} ({})",
                row["agent"].as_str().unwrap_or(""),
                row["path"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("")
            ));
        }
    }
    if value["applied"] == true {
        lines.push(
            "Configuration verified. Trust/reload your agent and check its MCP server list.".into(),
        );
    }
    lines
        .join("\n")
        .chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// One registration's path, change, launch command and notes.
fn registration_lines(row: &Value, scope: Scope, lines: &mut Vec<String>) {
    lines.push(format!(
        "  {}: {} {}",
        row["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        row["change"].as_str().unwrap_or(""),
        row["path"].as_str().unwrap_or("")
    ));
    for hook in row["prompt_hook"]["hooks"].as_array().into_iter().flatten() {
        if let Some(command) = hook["command"].as_str() {
            lines.push(format!("    UserPromptSubmit: {command:?}"));
        }
    }
    if let Some(entry) = row.get("entry").filter(|v| !v.is_null()) {
        let argv: Vec<_> = if let Some(command) = entry["command"].as_array() {
            command.iter().filter_map(Value::as_str).collect()
        } else {
            entry["command"]
                .as_str()
                .or_else(|| entry["cmd"].as_str())
                .into_iter()
                .chain(
                    entry["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                )
                .collect()
        };
        lines.push(format!(
            "    Launch: {}",
            argv.iter()
                .map(|arg| format!("{arg:?}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        if let Some(directory) = entry["cwd"].as_str() {
            lines.push(format!("    Folder: {directory}"));
        }
    }
    if row["definition"].is_string()
        && let Some(host) = row["agents"][0].as_str().and_then(agents::config_host)
    {
        lines.push(format!("    {}", host.publish(scope)));
    }
    if let Some(rules) = row["permission_rules"].as_array() {
        lines.push(format!(
            "    Permission rules added by setup: {}",
            rules.len()
        ));
    }
    if let Some(note) = row["format_note"].as_str() {
        lines.push(format!("    {note}"));
    }
}

/// Which tools run without asking, for the agents setup writes approvals to.
fn approval_lines(approvals: &Value, lines: &mut Vec<String>) {
    if approvals["agents"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        let names = |key: &str| -> Vec<&str> {
            approvals[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect()
        };
        let (approved, ask) = (names("approved"), names("ask"));
        lines.push(format!(
            "  Tool permissions ({}): {} of {} tools run without asking",
            names("agents").join(", "),
            approved.len(),
            approved.len() + ask.len()
        ));
        lines.push(format!(
            "    Ask each time: {}",
            if ask.is_empty() {
                "none".to_owned()
            } else {
                ask.join(", ")
            }
        ));
        let kept = names("approved_by_other_rules");
        if !kept.is_empty() {
            lines.push(format!(
                "    Still run without asking in Claude Code, through rules setup did not write ({}): {}",
                names("other_rules").join(", "),
                kept.join(", ")
            ));
        }
    }
}

/// The permissions page: every tool, with the current or default approvals
/// marked. `None` means the user cancelled.
fn permissions(
    context: &Context,
    options: &Options,
    inputs: &Inputs,
) -> Result<Option<Vec<String>>> {
    let (marked, saved) = approved(context, options, inputs);
    let source = if saved {
        "Saved selection. Use --reset-tool-permissions to restore current defaults."
    } else {
        "Current product defaults."
    };
    let marked: Vec<&str> = marked.iter().map(String::as_str).collect();
    let choices = context
        .tools
        .iter()
        .map(|tool| {
            let mut choice =
                canonic_tui_kit::picker::Choice::new(tool.name.clone(), tool.name.clone());
            choice.detail.clone_from(&tool.description);
            choice
        })
        .collect();
    Ok(canonic_tui_kit::runtime::prompts::choose_many(
        "Tool permissions",
        &format!(
            "{source} Marked tools run without asking in Claude Code, Codex and the agents Hub or Box publishes to; unmarked tools ask each time. Other agents keep their own approval settings."
        ),
        choices,
        &marked,
        0,
        "continue",
    )?)
}

/// One multiple-choice list of agents with each one's registration status.
/// Space marks, Enter continues once one is marked; `None` changes nothing.
fn select_agents(
    context: &Context,
    options: &Options,
    inputs: &Inputs,
) -> Result<Option<Vec<String>>> {
    let choices = agents::CATALOG
        .iter()
        .map(|(id, _)| {
            let status = inspect_inputs(context, id, options.scope, options.remove, inputs);
            let mut choice = canonic_tui_kit::picker::Choice::new(&status.id, &status.id);
            choice.detail = status.detail;
            choice.disabled = status.unavailable;
            choice
        })
        .collect();
    let defaults = super::detection::defaults(context, options);
    let marked: Vec<&str> = defaults.iter().map(String::as_str).collect();
    Ok(canonic_tui_kit::runtime::prompts::setup::agents(
        choices,
        &marked,
        options.remove,
    )?)
}

fn inspect_inputs(
    context: &Context,
    id: &str,
    scope: Scope,
    remove: bool,
    inputs: &Inputs,
) -> AgentStatus {
    let mut choice = AgentStatus {
        id: id.to_owned(),
        detail: String::new(),
        unavailable: None,
    };
    if let Some(host) = agents::config_host(id) {
        match agents::definition_path(host, context, scope, remove) {
            Ok(path) => {
                choice.detail = match inputs.exists(&path) {
                    Ok(true) => {
                        format!("defined for {}; ownership checked on review", host.name)
                    }
                    Ok(false) => format!("{} publishes it to each developer's agents", host.name),
                    Err(_) => "unreadable definition".into(),
                };
            }
            Err(error) => choice.unavailable = Some(error.to_string()),
        }
        return choice;
    }
    match agents::destination(context, id, scope) {
        Ok(dest) => {
            choice.detail = match inputs.exists(&dest.path) {
                Ok(true) => match inputs
                    .document(&dest.path, dest.format)
                    .and_then(|doc| doc.entry(dest.key, &context.product))
                {
                    Ok(Some(_)) => "configured; ownership checked on review",
                    Ok(None) => "not registered",
                    Err(_) => "invalid configuration",
                },
                Ok(false) => "not registered",
                Err(_) => "unreadable configuration",
            }
            .into();
        }
        Err(error) => {
            choice.unavailable = Some(
                error
                    .to_string()
                    .lines()
                    .next()
                    .unwrap_or("unavailable in this scope")
                    .trim()
                    .to_owned(),
            );
        }
    }
    choice
}

/// Guided selection and confirmation. JSON and pipes never prompt; explicit
/// selection with `yes` can apply without terminal confirmation.
///
/// # Errors
/// Returns an error for invalid selections, unavailable confirmation, failed
/// catalog checks, terminal I/O, preparation conflicts or publication failures.
pub fn run(context: &Context, options: Options, machine: bool) -> Result<Value> {
    run_with_policy(context, options, machine, SetupPolicy::default())
}

/// Run shared setup under consumer-selected authorization and launch policy.
///
/// # Errors
/// Returns setup, confirmation, catalog and publication failures as [`run`] does.
pub fn run_with_policy(
    context: &Context,
    mut options: Options,
    machine: bool,
    policy: SetupPolicy,
) -> Result<Value> {
    let session = PreparationSession::new(context, options.scope, policy);
    if options.yes && options.agents.is_empty() {
        return Err(Error::selection(
            "--yes requires explicit --agent selections",
        ));
    }
    let interactive = !machine && io::stdin().is_terminal() && io::stdout().is_terminal();
    if options.agents.is_empty() {
        if !interactive {
            return Err(Error::ConfirmationRequired(
                "Agent selection requires a terminal; pass --agent and --dry-run to inspect without prompts".into(),
            ));
        }
        if !options.remove && !options.reset_tool_permissions {
            match canonic_tui_kit::runtime::prompts::setup::action()? {
                Some(canonic_tui_kit::runtime::prompts::setup::Action::Install) => {}
                Some(canonic_tui_kit::runtime::prompts::setup::Action::Remove) => {
                    options.remove = true;
                }
                None => return Ok(json!({"cancelled_by_user":true})),
            }
        }
        match select_agents(context, &options, &session.inputs)? {
            Some(agents) => options.agents = agents,
            None => return Ok(json!({"cancelled_by_user":true})),
        }
    }
    validate_policy(&options, policy)?;
    if interactive
        && !options.remove
        && !options.dry_run
        && options.approved.is_none()
        && !context.tools.is_empty()
        && agents::APPROVALS
            .iter()
            .any(|id| options.agents.iter().any(|a| a == id))
    {
        match permissions(context, &options, &session.inputs)? {
            Some(selected) => options.approved = Some(selected),
            None => return Ok(json!({"cancelled_by_user":true})),
        }
    }
    let plan = session.prepare(&options)?;
    drop(session);
    let review = plan.review();
    if options.dry_run {
        return Ok(review);
    }
    if !interactive && !options.yes {
        return Err(Error::ConfirmationRequired(
            if policy.authorization == Authorization::TerminalOnly {
                "Setup needs terminal confirmation to authorize registrations and tool approvals; --dry-run --json inspects selected changes without writes".into()
            } else {
                "Setup needs terminal confirmation or --yes; --dry-run --json inspects selected changes without writes".into()
            },
        ));
    }
    // Only a product's known check entry point is run; host settings cannot
    // select the executable used by this probe.
    if !options.remove {
        let _observed = policy
            .catalog_observer
            .map(|observe| observe(&context.program));
        super::check_catalog(context)?;
    }
    if options.yes {
        return plan.apply();
    }
    // Keep the complete review in terminal scrollback; a many-agent plan must
    // not be truncated to the picker's small inline region.
    writeln!(io::stdout().lock(), "{}", human(&review))?;
    if !canonic_tui_kit::runtime::prompts::setup::confirm(
        "The review above lists every configuration path and launch command. Cancel keeps your configuration.",
    )? {
        return Ok(json!({"cancelled_by_user":true}));
    }
    plan.apply()
}
