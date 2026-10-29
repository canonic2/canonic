//! Guided project custom rules and confirmed proposal review.
use super::{
    state,
    terminal::{Step, Terminal},
};
use crate::review::packs;
use crate::{
    cli::public::RuleCommand,
    review::{self, Context, Error},
};
use serde_json::{Value, json};
fn rules(candidate: &Value, context: &Context) -> Result<Value, Error> {
    candidate["rules"].as_str().map_or_else(
        || Ok(crate::policy::rules::document(&[])?),
        |s| review::inspection::project_rules(s, &context.project_root),
    )
}
pub(super) fn mutate(
    context: &Context,
    input: &RuleCommand,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let before = crate::review::project::load(context)?;
    let candidate = &before.candidates["project"];
    let mut document = rules(candidate, context)?;
    let rows = document["rules"]
        .as_array_mut()
        .ok_or(Error::Invalid("custom rules missing"))?;
    match input {
        RuleCommand::List => return Err(Error::Invalid("invalid operator command")),
        RuleCommand::Remove { id } => {
            let index = rows
                .iter()
                .position(|r| r["id"] == *id)
                .ok_or(Error::Missing)?;
            rows.remove(index);
        }
        RuleCommand::Add | RuleCommand::Edit { .. } => {
            let id = match input {
                RuleCommand::Edit { id } => id.clone(),
                _ => tty.prompt("Rule id")?,
            };
            let index = rows.iter().position(|r| r["id"] == id);
            if matches!(input, RuleCommand::Edit { .. }) && index.is_none() {
                return Err(Error::Missing);
            }
            if matches!(input, RuleCommand::Add) && index.is_some() {
                return Err(Error::Invalid("rule id already exists"));
            }
            let row = guided_rule(&id, index.map(|i| &rows[i]), &mut tty)?;
            super::super::changes::portable_edit(context, index.map(|i| &rows[i]), &row)?;
            if let Some(index) = index {
                rows[index] = row;
            } else {
                rows.push(row);
            }
        }
    }
    document = crate::policy::rules::document_with(
        document["rules"]
            .as_array()
            .ok_or(Error::Invalid("custom rules missing"))?,
        document["directory_options"]
            .as_array()
            .map_or(&[][..], Vec::as_slice),
    )?;
    crate::policy::CustomRules::from_yaml(&document.to_string())?;
    let meta = crate::review::project::metadata(&before);
    let bytes: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack bytes missing"))?;
    let next = packs::rebuild(
        candidate,
        meta["installed"]
            .as_array()
            .ok_or(Error::Invalid("installed packs missing"))?,
        &bytes,
        Some(&document.to_string()),
    )?;
    state::review_candidate(context, &before, &next, &meta, &mut tty)?;
    crate::review::publication::commit(context, &before, next, meta, None)
}

fn guided_rule(id: &str, existing: Option<&Value>, tty: &mut Terminal) -> Result<Value, Error> {
    if existing.is_none() {
        return create_rule(id, tty);
    }
    edit_conditions(existing.ok_or(Error::Missing)?, tty)
}

/// Matching, its entries, then the decision. Escape on the decision returns to
/// matching with the earlier choices kept; Escape on matching cancels.
fn create_rule(id: &str, tty: &mut Terminal) -> Result<Value, Error> {
    let mut domain = "exact_request".to_owned();
    let mut decision = "ask".to_owned();
    loop {
        let Step::Done(chosen) = tty.choose_step("Rule matching", "Choose how much of the request this rule binds. The complete request is the narrowest choice.", &[
            ("exact_request", "Complete request", "Bind normalized input, vendor context and environment"),
            ("argv", "Exact command arguments", "Broader vendor/environment scope; bind project cwd"),
            ("structured", "Structured rule", "Enter a complete v1 rule with optional requirements and guidance"),
        ], &domain, false)? else {
            return Err(Error::Cancelled);
        };
        domain = chosen;
        if let Some(rule) = rule_entries(id, &domain, &mut decision, tty)? {
            return Ok(rule);
        }
    }
}

/// The entries for one matching choice; `None` when the decision went back.
fn rule_entries(
    id: &str,
    domain: &str,
    decision: &mut String,
    tty: &mut Terminal,
) -> Result<Option<Value>, Error> {
    if domain == "structured" {
        let rule: Value = serde_json::from_str(&tty.prompt("Complete structured rule JSON (conditions, optional requirements/on_unmet and guidance)")?).map_err(|_| Error::Invalid("rule must be JSON"))?;
        if rule["id"] != id {
            return Err(Error::Invalid("rule identity must match"));
        }
        return Ok(Some(rule));
    }
    let conditions = match domain {
        "" | "exact_request" => {
            let input: Value = serde_json::from_str(
                &tty.prompt("Complete normalized input JSON including original_input")?,
            )
            .map_err(|_| Error::Invalid("normalized input must be JSON"))?;
            let context: Value = serde_json::from_str(&tty.prompt(
                "Exact context JSON with canonical cwd and available environment (or null)",
            )?)
            .map_err(|_| Error::Invalid("context must be JSON"))?;
            let executable =
                tty.prompt("Observed canonical executable path, or null when unavailable")?;
            json!({"domain":"exact_request","input":input,"context":context,"executable":if executable == "null" { Value::Null } else { json!(executable) }})
        }
        "argv" => {
            let executable = tty.prompt("Executable (exact name or canonical path)")?;
            let arguments: Vec<String> =
                serde_json::from_str(&tty.prompt("Exact arguments JSON array")?)
                    .map_err(|_| Error::Invalid("arguments must be a JSON string array"))?;
            json!({"domain":"argv","executable":executable,"arguments":{"kind":"exact","values":arguments},"cwd":{"exact":{"root":"project"}}})
        }
        _ => return Err(Error::Invalid("invalid rule domain")),
    };
    let Step::Done(chosen) = tty.choose_step(
        "Rule decision",
        "The final review confirms this policy change.",
        &[
            (
                "ask",
                "Ask",
                "Require approval or the configured native fallback",
            ),
            ("deny", "Deny", "Block matching requests"),
            (
                "allow",
                "Allow",
                "Permit matching requests under this rule's scope",
            ),
        ],
        decision,
        true,
    )?
    else {
        return Ok(None);
    };
    *decision = chosen;
    let reason = tty.prompt("Reason")?;
    let expiration = tty.prompt("Expiration UTC timestamp (empty means no expiration)")?;
    let mut rule = json!({"id":id,"conditions":conditions,"decision":decision});
    if !reason.is_empty() {
        rule["reason"] = json!(reason);
    }
    if !expiration.is_empty() {
        rule["expires_at"] = json!(expiration);
    }
    Ok(Some(rule))
}

fn edit_conditions(existing: &Value, tty: &mut Terminal) -> Result<Value, Error> {
    let mut row = existing.clone();
    tty.display(&json!({"existing_rule":row,"editing":"Empty answers keep existing values and complete conditions"}))?;
    let complete =
        tty.prompt("Complete structured rule JSON (empty continues editing existing fields)")?;
    if !complete.is_empty() {
        let updated: Value =
            serde_json::from_str(&complete).map_err(|_| Error::Invalid("rule must be JSON"))?;
        if updated["id"] != row["id"] {
            return Err(Error::Invalid("rule edit cannot change identity"));
        }
        return Ok(updated);
    }
    if row["conditions"]["domain"] == "argv" {
        let executable = tty.prompt("Executable (empty keeps existing)")?;
        let arguments =
            tty.prompt("Arguments JSON array (sets exact matching; empty keeps existing)")?;
        if !executable.is_empty() {
            row["conditions"]["executable"] = json!(executable);
        }
        if !arguments.is_empty() {
            let values: Vec<String> = serde_json::from_str(&arguments)
                .map_err(|_| Error::Invalid("arguments must be a JSON string array"))?;
            row["conditions"]["arguments"] = json!({"kind":"exact","values":values});
        }
    } else {
        let conditions = tty.prompt("Complete conditions JSON (empty keeps existing)")?;
        if !conditions.is_empty() {
            row["conditions"] = serde_json::from_str(&conditions)
                .map_err(|_| Error::Invalid("conditions must be JSON"))?;
        }
        let expiration = tty.prompt("Expiration UTC timestamp or null (empty keeps existing)")?;
        if !expiration.is_empty() {
            if expiration == "null" {
                row.as_object_mut()
                    .ok_or(Error::Invalid("rule object required"))?
                    .remove("expires_at");
            } else {
                row["expires_at"] = json!(expiration);
            }
        }
    }
    let decision = tty.choose(
        "Rule decision",
        "Keep the current decision or review a replacement.",
        &[
            (
                "keep",
                "Keep current decision",
                "Preserve this rule's decision",
            ),
            (
                "ask",
                "Ask",
                "Require approval or the configured native fallback",
            ),
            ("deny", "Deny", "Block matching requests"),
            (
                "allow",
                "Allow",
                "Permit matching requests under the rule's scope",
            ),
        ],
        "keep",
    )?;
    let reason = tty.prompt("Reason (empty keeps existing)")?;
    if decision != "keep" {
        row["decision"] = json!(decision);
    }
    if !reason.is_empty() {
        row["reason"] = json!(reason);
    }
    Ok(row)
}
pub(super) fn policy(context: &Context, json_mode: bool) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let before = crate::review::project::load(context)?;
    let value = review::project::policy(context)?;
    let mut shown = value.clone();
    shown["operation"] = json!("inspect policy");
    tty.display(&shown)?;
    let pending = before
        .proposals
        .iter()
        .filter(|p| p.status == "pending")
        .count();
    let waiting = if pending == 1 {
        "Review the pending proposal".to_owned()
    } else {
        format!("Review {pending} pending proposals")
    };
    let mut choices = vec![
        ("inspect", "Finish inspection", "Make no changes"),
        (
            "manual",
            "Review edited project files",
            "Validate and activate your configuration edits",
        ),
        (
            "repair",
            "Repair project files",
            "Restore active policy projections",
        ),
        (
            "history",
            "Request history settings",
            "Configure retention and optional history clearing",
        ),
    ];
    // Waiting proposals come first and are the default.
    if pending > 0 {
        choices.insert(
            0,
            (
                "review",
                waiting.as_str(),
                "Accept or reject agents' proposed changes",
            ),
        );
    }
    // Escape in the history settings returns to this menu; Escape here cancels.
    let mut action = if pending > 0 { "review" } else { "inspect" }.to_owned();
    let mut history = History {
        enabled: before.history_enabled,
        limit: before.history_limit,
        purge: false,
    };
    loop {
        action = tty
            .choose_step(
                "Policy",
                "Inspect the active policy or review a specific change.",
                &choices,
                &action,
                false,
            )?
            .or_cancel()?;
        return match action.as_str() {
            // The policy was already shown above the choice.
            "inspect" | "" => {
                Ok(json!({"inspected":true,"policy_revision":value["policy_revision"]}))
            }
            "manual" => manual(context, &before, &mut tty),
            "repair" => {
                tty.confirm(&json!({"operation":"restore project files from active policy","policy_revision":before.policy_revision,"manual_edits":"preserved; conflicts require manual review"}))?;
                let mut receipt = crate::review::publication::reconcile(context, &before)?;
                receipt["restored_project_files"] = json!(true);
                Ok(receipt)
            }
            "review" => review_proposals(context, &mut tty),
            "history" => match history_menu(&mut history, &mut tty)? {
                Step::Done(()) => {
                    let History {
                        enabled,
                        limit,
                        purge,
                    } = history;
                    tty.confirm(&json!({"operation":"history settings","enabled":enabled,"limit":limit,"purge":purge}))?;
                    review::history_settings(context, Some(enabled), Some(limit), purge, &|| Ok(()))
                }
                Step::Back => continue,
            },
            _ => Err(Error::Invalid("invalid policy action")),
        };
    }
}

/// Each pending proposal in turn, oldest first. Accepting activates it at once,
/// so later proposals made against the older policy show as stale.
fn review_proposals(context: &Context, tty: &mut Terminal) -> Result<Value, Error> {
    let ids: Vec<String> = crate::review::project::load(context)?
        .proposals
        .iter()
        .filter(|p| p.status == "pending")
        .map(|p| p.id.clone())
        .collect();
    let mut reviewed = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let before = crate::review::project::load(context)?;
        let Some(proposal) = before
            .proposals
            .iter()
            .find(|p| &p.id == id && p.status == "pending")
        else {
            continue;
        };
        let stale = proposal.policy_revision != before.policy_revision;
        let mut change = crate::review::publication::change_review(
            context,
            &before,
            &proposal.candidate,
            &proposal.candidate_metadata,
        )?;
        state::terminal_mcp_details(
            &mut change["changes"],
            &before.candidates["project"],
            &proposal.candidate,
        )?;
        let row = proposal.public(false);
        let requested = row["links"]["source_entry_id"].as_str().and_then(|entry| {
            review::observations::requested(std::path::Path::new(&context.project_root), entry)
        });
        let card = super::presentation::card(&row, &change, requested.as_ref(), stale);
        let choice = tty.decide(&card, ids.len() - index - 1)?;
        let result = match choice.as_str() {
            "accept" if !stale => crate::review::publication::commit(
                context,
                &before,
                proposal.candidate.clone(),
                proposal.candidate_metadata.clone(),
                Some(id),
            )?,
            "reject" => review::transition(context, id, "reject", None, &|| Ok(()))?,
            _ => json!({"proposal_id":id,"status":"pending"}),
        };
        reviewed
            .push(json!({"proposal_id":id,"choice":choice,"summary":card.summary,"result":result}));
    }
    Ok(json!({"reviewed":reviewed}))
}

/// Request history answers, kept while the operator goes back and forth.
#[derive(Clone, Copy)]
struct History {
    enabled: bool,
    limit: usize,
    purge: bool,
}

/// Recording, the retained limit, then clearing. Escape on clearing returns to
/// recording with the answers kept; Escape on recording returns [`Step::Back`].
fn history_menu(history: &mut History, tty: &mut Terminal) -> Result<Step<()>, Error> {
    loop {
        let Step::Done(enabled) = tty.choose_step(
            "Record request history",
            "History records decisions and available outcomes; it is separate from authorization.",
            &[
                ("yes", "Enabled", "Keep bounded local request observations"),
                (
                    "no",
                    "Disabled",
                    "Do not retain optional request observations",
                ),
            ],
            if history.enabled { "yes" } else { "no" },
            true,
        )?
        else {
            return Ok(Step::Back);
        };
        history.enabled = match enabled.as_str() {
            "yes" => true,
            "no" => false,
            _ => return Err(Error::Invalid("yes or no required")),
        };
        history.limit = loop {
            let answer = tty.prompt(&format!(
                "Retained request limit [1..10000] (default {})",
                history.limit
            ))?;
            if answer.is_empty() {
                break history.limit;
            }
            if let Ok(limit) = answer.parse::<usize>()
                && (1..=10000).contains(&limit)
            {
                break limit;
            }
            tty.note(
                "Invalid retention limit",
                "Enter a whole number from 1 to 10000, or press Enter to keep the current limit.",
            )?;
        };
        let purge = tty.choose_step(
            "Clear existing history",
            "This affects optional request history only. Authorization records remain intact.",
            &[
                (
                    "no",
                    "Keep history",
                    "Preserve existing request observations",
                ),
                (
                    "yes",
                    "Clear history",
                    "Delete retained optional request observations",
                ),
            ],
            if history.purge { "yes" } else { "no" },
            true,
        )?;
        if let Step::Done(purge) = purge {
            history.purge = purge == "yes";
            return Ok(Step::Done(()));
        }
    }
}

/// The edited `config.yml` and optional `config.local.yml` as adopted, with
/// their merged configuration. Project rules live in `rules.yml`; pointing
/// `config.yml` there edits the file and keeps its comments.
fn adopted_config(directory: &std::path::Path) -> Result<(String, Option<String>, Value), Error> {
    let read = |name: &str, message: &'static str| {
        crate::application::read(&directory.join(name)).map_err(|_| Error::Invalid(message))
    };
    let source = read("config.yml", "project config unreadable")?;
    let mut shared: Value =
        crate::policy::parse(&source).map_err(|_| Error::Invalid("invalid project config"))?;
    let source = if shared["rules"] == "rules.yml" {
        source
    } else {
        shared["rules"] = json!("rules.yml");
        crate::policy_files::write(Some(&source), &shared)?.text
    };
    let local = if directory.join("config.local.yml").exists() {
        Some(read("config.local.yml", "local config unreadable")?)
    } else {
        None
    };
    let config: Value = crate::policy::parse(&crate::configuration::layers::merge(
        &source,
        local.as_deref(),
    )?)
    .map_err(|_| Error::Invalid("invalid project config"))?;
    Ok((source, local, config))
}

fn manual(
    context: &Context,
    before: &crate::review::model::State,
    tty: &mut Terminal,
) -> Result<Value, Error> {
    let fingerprint =
        crate::review::publication::projections::projection_fingerprint(context, before)?;
    let directory = crate::review::project::root(context).join(".shield");
    let read = |name: &str, message: &'static str| {
        crate::application::read(&directory.join(name)).map_err(|_| Error::Invalid(message))
    };
    let (source, local, config) = adopted_config(&directory)?;
    let mut meta = crate::review::project::metadata(before);
    // Installed and locally disabled selections are both known releases.
    let known: Vec<Value> = meta["installed"]
        .as_array()
        .into_iter()
        .chain(meta["disabled"].as_array())
        .flatten()
        .cloned()
        .collect();
    let mut selected = Vec::new();
    let mut bytes = Vec::new();
    let mut disabled_rows = Vec::new();
    let mut disabled_bytes = Vec::new();
    for (list, rows, artifacts) in [
        ("packs", &mut selected, &mut bytes),
        ("disabled", &mut disabled_rows, &mut disabled_bytes),
    ] {
        for row in config[list].as_array().into_iter().flatten() {
            let original = known
                .iter()
                .find(|p| {
                    p["id"] == row["id"]
                        && (row["registry_identity"].is_null()
                            || p["source_identity"] == row["registry_identity"])
                        && p["version"] == row["version"]
                })
                .ok_or(Error::Invalid("use shield pack add for a new pack release"))?;
            let path = row["path"]
                .as_str()
                .ok_or(Error::Invalid("pack path missing"))?;
            let artifact = contained(&directory, path)?;
            if crate::policy::fingerprint(artifact.as_bytes()) != original["digest"] {
                return Err(Error::Invalid(
                    "installed artifact bytes changed; reacquire the pack",
                ));
            }
            let mut selection = original.clone();
            selection["profile"] = row["profile"].clone();
            if let Some(fields) = selection.as_object_mut() {
                fields.remove("layer");
            }
            if row["layer"] == "local" {
                selection["layer"] = json!("local");
            }
            rows.push(selection);
            artifacts.push(artifact);
        }
    }
    meta["installed"] = json!(selected);
    if disabled_rows.is_empty() {
        if let Some(fields) = meta.as_object_mut() {
            fields.remove("disabled");
        }
    } else {
        meta["disabled"] = json!(disabled_rows);
    }
    let rules = read("rules.yml", "manual rules unreadable")?;
    let local_rules = config["local_rules"]
        .as_str()
        .map(|path| contained(&directory, path))
        .transpose()?;
    let candidate = json!(crate::configuration::Bundle::capture_layers(
        &source,
        local.as_deref(),
        &bytes,
        Some(&rules),
        local_rules.as_deref(),
        &disabled_bytes,
    )?);
    let candidates = std::collections::BTreeMap::from([("project".into(), candidate.clone())]);
    let mut comparison = json!(crate::configuration::compare_policy(
        &review::snapshots(&before.candidates)?,
        &review::snapshots(&candidates)?,
    )?);
    if let Some(prefixes) = review::codex_prefix_changes(&before.candidates, &candidates)? {
        comparison["codex_prefixes"] = prefixes;
    }
    let mut view = json!({"operation":"adopt manual project edits",
        "changes":crate::review::changes::diff::between(&before.operator, &before.candidates["project"], &meta, &candidate),
        "candidate":candidate,"comparison":comparison,"packs":meta["installed"]});
    state::terminal_mcp_details(
        &mut view["changes"],
        &before.candidates["project"],
        &candidate,
    )?;
    tty.confirm(&view)?;
    crate::review::publication::commit_reviewed(
        context,
        before,
        candidate,
        meta,
        None,
        Some(&fingerprint),
    )
}

/// A project file named by a configuration path, refusing paths that leave .shield.
fn contained(directory: &std::path::Path, path: &str) -> Result<String, Error> {
    crate::configuration::Candidate::from_yaml(
        &json!({"schema_version":1,"packs":[],"rules":path}).to_string(),
    )?;
    let resolved = directory.join(path).canonicalize()?;
    if !resolved.starts_with(directory.canonicalize()?) {
        return Err(Error::Invalid("manual artifact escapes project"));
    }
    crate::application::read(&resolved).map_err(|_| Error::Invalid("project artifact unreadable"))
}
