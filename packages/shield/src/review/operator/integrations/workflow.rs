//! Shared onboarding and later confirmed integration configuration.
use super::super::terminal::Step;
use super::{
    AskFallback, Context, Error, IntegrationChoice, Path, Plan, Terminal, Value, apply,
    approved_tools, confirmed_fallback, json, pending, prepare_approving, review, state,
};

/// The agent page rows: Box first, then each native agent adapter.
pub(super) fn rows(
    box_description: &'static str,
    agent_description: &'static str,
) -> Vec<(&'static str, &'static str, &'static str)> {
    std::iter::once((
        super::box_definition::ID,
        super::box_definition::LABEL,
        box_description,
    ))
    .chain(
        crate::agents::ADAPTERS
            .iter()
            .map(|a| (a.spec().id, a.spec().label, agent_description)),
    )
    .collect()
}

/// The integration to set up. A confirmed repair resumes when no selection was
/// given; otherwise the agent page starts at `answer`, the earlier choice when
/// the operator came back to it, and records the new one there.
fn choice(
    tty: &mut Terminal,
    selected: Option<IntegrationChoice>,
    resumable: bool,
    answer: &mut Vec<String>,
) -> Result<IntegrationChoice, Error> {
    if selected.is_none() && resumable {
        return Ok(IntegrationChoice::Keep);
    }
    if matches!(selected, Some(IntegrationChoice::RemoveAgents(0))) {
        let rows = rows(
            "Remove the Box MCP definition Shield wrote",
            "Remove owned hooks, MCP and permission settings together",
        );
        let ids = tty.choose_many(
            "Remove agent integrations",
            "Select integrations to remove; an empty selection keeps all integrations.",
            &rows,
            &[],
            "Skip for now",
        )?;
        if ids.is_empty() {
            return Err(Error::Cancelled);
        }
        return Ok(IntegrationChoice::RemoveAgents(IntegrationChoice::bits(
            &ids,
        )?));
    }
    if let Some(selected) = selected {
        return Ok(selected);
    }
    let rows = rows(
        "Write .box/mcp/shield.yml; Box publishes it to each developer's agents",
        "Install or repair project hooks and Shield MCP",
    );
    let marked: Vec<&str> = answer.iter().map(String::as_str).collect();
    let ids = tty.choose_many(
        "Agent integrations",
        "Select agents to install or repair. Remove one with shield mcp setup --remove.",
        &rows,
        &marked,
        "Skip for now",
    )?;
    if ids.is_empty() {
        return Err(Error::Cancelled);
    }
    let bits = IntegrationChoice::bits(&ids)?;
    *answer = ids;
    Ok(IntegrationChoice::Agents(bits))
}

/// The agents the guided page marks first: those already set up, otherwise
/// Box alone in a Box project, otherwise the detected agent CLIs.
pub(super) fn preselected(root: &Path, meta: &Value) -> Result<Vec<String>, Error> {
    let registered: Vec<String> = super::registration(meta)?
        .map(|r| {
            r.box_definition
                .as_ref()
                .map(|_| super::box_definition::ID.to_owned())
                .into_iter()
                .chain(r.agents.keys().cloned())
                .collect()
        })
        .unwrap_or_default();
    Ok(if !registered.is_empty() {
        registered
    } else if super::box_definition::box_project(root) {
        vec![super::box_definition::ID.to_owned()]
    } else {
        detected()
    })
}

/// The agents whose CLIs are on `PATH`.
fn detected() -> Vec<String> {
    crate::agents::detection::installed()
        .iter()
        .map(|id| (*id).to_owned())
        .collect()
}

/// The permissions page: every Shield tool, with the saved or default
/// approvals marked. Marked tools run without asking. `back` says whether an
/// earlier step exists for Escape to return to.
fn permissions(tty: &mut Terminal, meta: &Value, back: bool) -> Result<Step<Vec<String>>, Error> {
    let tools = crate::review::mcp::tool_summaries();
    let rows: Vec<_> = tools
        .iter()
        .map(|(name, description)| (name.as_str(), name.as_str(), description.as_str()))
        .collect();
    let approved = approved_tools(meta)?;
    let marked: Vec<&str> = approved.iter().map(String::as_str).collect();
    tty.choose_many_step(
        "Tool permissions",
        "Marked tools run without asking; unmarked tools ask each time. Applying a policy change always asks for your approval.",
        &rows,
        &marked,
        "Every tool asks",
        back,
    )
}

/// The plan for `selected`, asking tool permissions for a new installation.
/// Escape on the permissions returns [`Step::Back`].
fn plan(
    root: &Path,
    meta: &Value,
    selected: IntegrationChoice,
    tty: &mut Terminal,
    initial: bool,
    back: bool,
) -> Result<Step<Option<Plan>>, Error> {
    let installs = matches!(selected, IntegrationChoice::Agents(_));
    // A pending plan resumes as confirmed; only a new installation asks.
    let approved = if installs && pending(meta)?.is_none() {
        match permissions(tty, meta, back)? {
            Step::Done(approved) => Some(approved),
            Step::Back => return Ok(Step::Back),
        }
    } else {
        None
    };
    // Only guided setup asks; `--agent box` keeps the default hand-over, which
    // the review states. Init has no earlier installations to hand over.
    let guided = back && !initial;
    let handover = approved.is_none() || !guided || handover(tty, meta, selected)?;
    let mut plan = prepare_approving(root, meta, selected, initial, approved, handover)?;
    with_fallbacks(meta, selected, plan.as_mut())?;
    Ok(Step::Done(plan))
}

/// Whether a Box install hands Shield's direct installations over to Box;
/// asked only when there are some, with hand-over as the default.
fn handover(tty: &mut Terminal, meta: &Value, selected: IntegrationChoice) -> Result<bool, Error> {
    let direct = super::handed_over(super::registration(meta)?.as_ref(), selected, true);
    if direct.is_empty() {
        return Ok(true);
    }
    let names: Vec<_> = direct.iter().map(|id| crate::agents::label(id)).collect();
    // Box does not publish Codex's prefix rules, so Shield keeps writing them.
    let handover_effect = if direct.iter().any(|id| id == "codex") {
        "Shield stops managing their hooks and MCP entries; it keeps Codex's prefix rules in step with your policy"
    } else {
        "Shield stops managing them and changes none of their files"
    };
    let answer = tty.choose(
        "Hand over to Box",
        &format!(
            "Shield also manages direct installations for {}. Handing them over keeps their hooks, MCP entries and settings in place for box config sync --adopt.",
            names.join(" and ")
        ),
        &[
            (
                "handover",
                "Hand over to Box",
                handover_effect,
            ),
            (
                "keep",
                "Keep direct installations",
                "Shield keeps managing them; removing them later deletes their entries",
            ),
        ],
        "handover",
    )?;
    Ok(answer == "handover")
}

/// Preserve confirmed Ask fallbacks of the selected agents; default new ones
/// to Allow so the final review includes their scope.
pub(super) fn with_fallbacks(
    meta: &Value,
    selected: IntegrationChoice,
    plan: Option<&mut Plan>,
) -> Result<(), Error> {
    let Some(desired) = plan.and_then(|plan| plan.desired.as_mut()) else {
        return Ok(());
    };
    if matches!(selected, IntegrationChoice::RemoveAgents(_)) {
        return Ok(());
    }
    for adapter in crate::agents::ADAPTERS {
        let spec = adapter.spec();
        // Box publishes every adapter's hooks, so it carries their fallbacks.
        let covered = selected.contains(spec.id) || selected.contains(super::box_definition::ID);
        if covered && spec.needs_ask_fallback {
            desired.ask_fallback.insert(
                spec.id.into(),
                confirmed_fallback(meta, spec.id)?.unwrap_or(AskFallback::Allow),
            );
        }
    }
    Ok(())
}

/// The review of a setup plan, including the agents' prefix rules.
fn setup_preview(root: &Path, plan: Option<&Plan>) -> Value {
    let mut review = preview(root, plan);
    let prefixes = super::prefix_rules::active(root).unwrap_or_default();
    let desired = plan.and_then(|plan| plan.desired.as_ref());
    super::prefix_rules::preview(root, &mut review, desired, &prefixes);
    review
}

/// The review `setup` would confirm, without writing. New installations use
/// the saved or default tool approvals instead of asking.
pub(super) fn review(
    context: &Context,
    selected: Option<IntegrationChoice>,
    json_mode: bool,
) -> Result<Value, Error> {
    let meta = crate::review::project::metadata(&crate::review::project::checkpoint(context)?);
    let root = Path::new(&context.project_root);
    let resumable = pending(&meta)?.is_some();
    let selected = match selected {
        Some(IntegrationChoice::RemoveAgents(0)) | None if !resumable => {
            let mut tty = Terminal::required(json_mode)?;
            choice(&mut tty, selected, false, &mut preselected(root, &meta)?)?
        }
        Some(selected) => selected,
        None => IntegrationChoice::Keep,
    };
    let mut plan = prepare_approving(root, &meta, selected, false, None, true)?;
    with_fallbacks(&meta, selected, plan.as_mut())?;
    let mut review = setup_preview(root, plan.as_ref());
    review["dry_run"] = json!(true);
    Ok(review)
}

/// The operator review of an integration plan: each agent's hooks and MCP
/// server, the Codex Ask fallback and every owned file's exact before/after.
pub(super) fn preview(root: &Path, plan: Option<&Plan>) -> Value {
    let Some(p) = plan else {
        return json!({"operation":"keep integrations"});
    };
    let box_row = match (
        p.previous.as_ref().and_then(|r| r.box_definition.as_ref()),
        p.desired.as_ref().and_then(|r| r.box_definition.as_ref()),
    ) {
        (None, None) => None,
        (None, Some(_)) => Some("added"),
        (Some(_), None) => Some("removed"),
        (Some(b), Some(a)) if b == a => Some("unchanged"),
        _ => Some("updated"),
    }
    .map(|change| json!({"agent":super::box_definition::ID,"change":change,"mcp_server":"shield"}));
    let direct = p.desired.as_ref().is_some_and(|r| !r.agents.is_empty());
    let native = crate::agents::ADAPTERS
        .iter()
        .map(|a| a.spec().id)
        .filter_map(|agent| {
            let before = p
                .previous
                .as_ref()
                .and_then(|r| r.agents.get(agent))
                .map(|i| json!(i));
            let after = p
                .desired
                .as_ref()
                .and_then(|r| r.agents.get(agent))
                .map(|i| json!(i));
            let change = match (&before, &after) {
                (None, None) => return None,
                (None, Some(_)) => "added",
                (Some(_), None) if p.handed_over.iter().any(|id| id == agent) => "handed over",
                (Some(_), None) => "removed",
                (Some(b), Some(a)) if b == a => "unchanged",
                _ => "updated",
            };
            let shown = after.as_ref().or(before.as_ref())?;
            let hooks: Vec<_> = shown["groups"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(event, _)| event.clone())
                .collect();
            Some(json!({"agent":agent,"change":change,"hooks":hooks,"mcp_server":"shield"}))
        });
    let agents: Vec<_> = box_row.iter().cloned().chain(native).collect();
    let box_next = box_row
        .as_ref()
        .filter(|row| row["change"] != "removed")
        .map(|_| {
            json!({"publish":"box setup (first time) or box config sync",
            "direct_installations":direct,"handed_over":p.handed_over})
        });
    let fallbacks: Value = crate::agents::ADAPTERS
        .iter()
        .filter(|a| a.spec().needs_ask_fallback)
        .map(|a| {
            let id = a.spec().id;
            let before = p.previous.as_ref().and_then(|r| r.ask_fallback.get(id));
            let after = p.desired.as_ref().and_then(|r| r.ask_fallback.get(id));
            (id.to_owned(), json!({"before":before,"after":after}))
        })
        .collect::<serde_json::Map<String, Value>>()
        .into();
    // Publication leaves an absent shared MCP file absent when nothing goes in it.
    let mut files: Vec<_> = (p.before.is_some() || !p.after.is_empty())
        .then(|| json!({"path":".mcp.json","before":p.before,"after":p.after}))
        .into_iter()
        .collect();
    files.extend(p.files.iter().map(|change| json!(change)));
    let tool_approvals = p
        .desired
        .as_ref()
        .and_then(|r| r.approved_tools.as_ref())
        .map(|approved| {
            let ask: Vec<_> = crate::review::mcp::tool_summaries()
                .into_iter()
                .map(|(name, _)| name)
                .filter(|name| !approved.contains(name))
                .collect();
            json!({"approved":approved,"ask":ask})
        });
    json!({
        "tool_approvals":tool_approvals,
        "operation":if p.desired.is_some() {"set up agent integrations"} else {"remove Shield integrations"},
        "project":root,
        "binary":p.desired.as_ref().or(p.previous.as_ref()).map(|r| &r.binary),
        "agents":agents,
        "box":box_next,
        "connections":p.desired.as_ref().map(|r|r.connections.iter().flat_map(|(agent,value)|value["review"].as_array().into_iter().flatten().map(move |row| {let mut row=row.clone();row["agent"]=json!(agent);row})).collect::<Vec<_>>()).unwrap_or_default(),
        "ask_fallbacks":fallbacks,
        "adopt_existing_entries":p.previous.is_some() && !root.join(".shield/review/state.json").exists(),
        "files":files,
    })
}

/// Where `shield init` is before its pack setup.
#[derive(Clone, Copy)]
enum Stage {
    Profile,
    Agents,
    Permissions,
}

/// The integration for the agents marked on the init agent page.
fn agent_choice(agents: &[String]) -> Result<IntegrationChoice, Error> {
    let bits = IntegrationChoice::bits(agents)?;
    Ok(if bits == 0 {
        IntegrationChoice::Keep
    } else {
        IntegrationChoice::Agents(bits)
    })
}

pub(super) fn onboard(
    root: &Path,
    selected: Option<IntegrationChoice>,
    json_mode: bool,
    cache_dir: Option<&Path>,
    baseline: super::Baseline,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    tty.progress("Setting up Shield. Nothing is activated until you confirm.")?;
    let has_baseline = !matches!(baseline, super::Baseline::Skip)
        && configured_packs(root)?
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["id"] == "canonic/baseline");
    let asks_profile = !has_baseline && matches!(baseline, super::Baseline::Choose);
    let fixed = selected;
    let meta = json!({});
    let (profile, plan) = onboarding_answers(root, &mut tty, &meta, fixed, asks_profile)?;
    let baseline = if has_baseline {
        None
    } else {
        match baseline {
            super::Baseline::Skip => None,
            super::Baseline::Selected(profile) => Some(profile),
            super::Baseline::Choose => Some(profile),
        }
    };
    let receipt = state::initialize(
        root,
        json_mode,
        &preview(root, plan.as_ref()),
        plan.as_ref().and_then(|plan| plan.desired.as_ref()),
        cache_dir,
        baseline,
    )?;
    finish(&review::open(root)?, &meta, plan.as_ref(), Some(receipt))
}

/// The packs existing project files configure, or null without a config file.
fn configured_packs(root: &Path) -> Result<Value, Error> {
    let config = root.join(".shield/config.yml");
    if !config.exists() {
        return Ok(Value::Null);
    }
    let captured =
        json!(
            super::super::project_files::capture(&config).map_err(|_| Error::Invalid(
                "invalid project configuration; review config.yml and local artifacts"
            ))?
        );
    let value = crate::configuration::layers::merged(&captured)
        .map_err(|_| Error::Invalid("invalid project configuration"))?;
    Ok(value["packs"].clone())
}

/// The baseline profile and integration plan chosen on the init pages.
fn onboarding_answers(
    root: &Path,
    tty: &mut Terminal,
    meta: &Value,
    fixed: Option<IntegrationChoice>,
    asks_profile: bool,
) -> Result<(crate::policy::Profile, Option<Plan>), Error> {
    let rows = rows(
        "Write .box/mcp/shield.yml; Box publishes it to each developer's agents",
        "Install project hooks and Shield MCP",
    );
    // Profile, agents, then tool permissions. Escape goes back one step with
    // the answers kept; on the first step shown it cancels.
    let mut profile = crate::policy::Profile::Recommended;
    let mut agents = preselected(root, meta)?;
    let mut stage = if asks_profile {
        Stage::Profile
    } else {
        Stage::Agents
    };
    loop {
        stage = match stage {
            Stage::Profile => {
                profile = tty
                    .choose_profile_step(
                        "Baseline protection",
                        "Baseline is installed by default and can be removed with shield pack remove baseline.",
                        profile,
                        false,
                    )?
                    .or_cancel()?;
                Stage::Agents
            }
            Stage::Agents if fixed.is_some() => Stage::Permissions,
            Stage::Agents => {
                let marked: Vec<&str> = agents.iter().map(String::as_str).collect();
                match tty.choose_many_step(
                    "Agent integrations",
                    "Box is preselected in a Box project, otherwise detected CLIs. Select agents. Enter with nothing selected: Skip for now. You can configure agents later with shield mcp setup.",
                    &rows,
                    &marked,
                    "Skip for now",
                    asks_profile,
                )? {
                    Step::Done(ids) => {
                        agents = ids;
                        Stage::Permissions
                    }
                    Step::Back if asks_profile => Stage::Profile,
                    Step::Back => return Err(Error::Cancelled),
                }
            }
            Stage::Permissions => {
                let selected = match fixed {
                    Some(selected) => selected,
                    None => agent_choice(&agents)?,
                };
                let earlier = fixed.is_none() || asks_profile;
                match plan(root, meta, selected, tty, true, earlier)? {
                    Step::Done(plan) => return Ok((profile, plan)),
                    Step::Back if fixed.is_none() => Stage::Agents,
                    Step::Back if asks_profile => Stage::Profile,
                    Step::Back => return Err(Error::Cancelled),
                }
            }
        };
    }
}

pub(super) fn setup(
    context: &Context,
    selected: Option<IntegrationChoice>,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let meta = crate::review::project::metadata(&crate::review::project::checkpoint(context)?);
    let root = Path::new(&context.project_root);
    let resumable = pending(&meta)?.is_some();
    // Escape on the permissions returns to the agent page when it was shown.
    let asked = selected.is_none() && !resumable;
    let mut answer = preselected(root, &meta)?;
    let plan = loop {
        let choice = choice(&mut tty, selected, resumable, &mut answer)?;
        match plan(root, &meta, choice, &mut tty, false, asked)? {
            Step::Done(plan) => break plan,
            Step::Back if asked => {}
            Step::Back => return Err(Error::Cancelled),
        }
    };
    if plan.is_some() {
        tty.confirm(&setup_preview(root, plan.as_ref()))?;
    }
    finish(context, &meta, plan.as_ref(), None)
}

/// Apply a confirmed plan, then bring prefix rules in line with the active
/// policy. `receipt` replaces the status receipt only when nothing was applied.
fn finish(
    context: &Context,
    meta: &Value,
    plan: Option<&Plan>,
    receipt: Option<Value>,
) -> Result<Value, Error> {
    if let Some(plan) = plan
        && let Err(error) = apply(context, meta, plan)
    {
        if pending(&crate::review::project::metadata(
            &crate::review::project::checkpoint(context)?,
        ))?
        .is_none()
        {
            return Err(error);
        }
        let mut receipt = crate::review::project::status(context)?;
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
        return Ok(receipt);
    }
    let synced = sync_prefix_rules(context);
    let mut receipt = match receipt {
        Some(receipt) if plan.is_none() => receipt,
        _ => crate::review::project::status(context)?,
    };
    if let Err(error) = synced {
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
    }
    Ok(receipt)
}

/// Write the active policy's prefix rules for installed agents, or remove them
/// with the agent. Without a readable activation they wait for the next one.
fn sync_prefix_rules(context: &Context) -> Result<(), Error> {
    let root = Path::new(&context.project_root);
    let Ok(prefixes) = super::prefix_rules::active(root) else {
        return Ok(());
    };
    super::prefix_rules::sync(
        root,
        &crate::review::project::metadata(&crate::review::project::checkpoint(context)?),
        &prefixes,
    )
}
