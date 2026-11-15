//! Guided project custom rules and confirmed proposal review.
use super::{
    state,
    terminal::{Step, Terminal},
};
use crate::{
    cli::public::RuleCommand,
    review::{self, Context, Error},
};
use serde_json::{Value, json};
fn rules(candidate: &Value, local: bool) -> Result<Value, Error> {
    candidate[if local { "local_rules" } else { "rules" }]
        .as_str()
        .map_or_else(
            || Ok(crate::policy::rules::document(&[])?),
            |s| crate::policy::normalize_retained_custom(s).map_err(Error::from),
        )
}
pub(super) fn mutate(
    context: &Context,
    input: &RuleCommand,
    json_mode: bool,
) -> Result<Value, Error> {
    use crate::review::changes::{Change, Layer};
    let mut tty = Terminal::required(json_mode)?;
    let (before, base) = crate::review::project::load_prepared(context)?;
    let local = match input {
        RuleCommand::Add { local }
        | RuleCommand::Edit { local, .. }
        | RuleCommand::Remove { local, .. } => *local,
        RuleCommand::List { .. } => return Err(Error::Invalid("invalid operator command")),
    };
    let layer = Some(if local { Layer::Local } else { Layer::Project });
    let document = rules(&before.candidates["project"], local)?;
    let rows = document["rules"]
        .as_array()
        .ok_or(Error::Invalid("custom rules missing"))?;
    let change = match input {
        RuleCommand::Remove { id, .. } => Change::RuleRemove {
            rule_id: id.clone(),
            layer,
        },
        RuleCommand::Add { .. } | RuleCommand::Edit { .. } => {
            let id = match input {
                RuleCommand::Edit { id, .. } => id.clone(),
                _ => tty.prompt("Rule id")?,
            };
            let existing = rows.iter().find(|r| r["id"] == id);
            if matches!(input, RuleCommand::Edit { .. }) && existing.is_none() {
                return Err(Error::Missing);
            }
            if matches!(input, RuleCommand::Add { .. }) && existing.is_some() {
                return Err(Error::Invalid("rule id already exists"));
            }
            let row = guided_rule(&id, existing, &mut tty)?;
            let rule =
                serde_json::from_value(row).map_err(|_| Error::Invalid("invalid custom rule"))?;
            if matches!(input, RuleCommand::Edit { .. }) {
                Change::RuleEdit {
                    rule_id: id,
                    rule,
                    layer,
                }
            } else {
                Change::RuleAdd { rule, layer }
            }
        }
        RuleCommand::List { .. } => return Err(Error::Invalid("invalid operator command")),
    };
    let (next, meta, _) = crate::review::changes::custom_rule(context, &before, &change, None)?;
    state::write_reviewed(context, &before, next, meta, &mut tty, &base)
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
    let (before, base) = crate::review::project::load_prepared(context)?;
    let value = review::project::policy_prepared(context, &before, &base)?;
    let mut shown = value.clone();
    shown["operation"] = json!("inspect policy");
    tty.display(&shown)?;
    let pending = before
        .proposals
        .iter()
        .filter(|p| p.status == "pending")
        .count();
    let waiting = if pending == 1 {
        "Review the live change".to_owned()
    } else {
        format!("Select one of {pending} live changes")
    };
    let mut choices = vec![
        ("inspect", "Finish inspection", "Make no changes"),
        (
            "history",
            "Request history settings",
            "Configure retention and optional history clearing",
        ),
    ];
    // Live interactions are optional direct reviews, never a backlog default.
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
    let mut action = "inspect".to_owned();
    let mut history = History {
        enabled: before.settings.history_enabled,
        limit: before.settings.history_limit,
        purge: false,
    };
    loop {
        action = tty
            .choose_step(
                "Policy",
                "Inspect the policy or review a specific change.",
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
            "review" => review_live_change(context, &mut tty),
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

/// Select one live interaction explicitly; closed history cannot be activated.
fn review_live_change(context: &Context, tty: &mut Terminal) -> Result<Value, Error> {
    let state = crate::review::project::checkpoint(context)?;
    let mut choices = vec![("cancel", "Cancel review", "Make no changes")];
    for row in state.proposals.iter().filter(|p| p.status == "pending") {
        choices.push((
            row.id.as_str(),
            row.operation.as_str(),
            "Review this live interaction",
        ));
    }
    let id = tty.choose(
        "Live policy changes",
        "Select the current change you want to review.",
        &choices,
        "cancel",
    )?;
    if id == "cancel" {
        return Ok(json!({"cancelled":true}));
    }
    let (mut before, base) = crate::review::project::load_prepared(context)?;
    let Some(proposal) = before
        .proposals
        .iter_mut()
        .find(|p| p.id == id && p.status == "pending")
    else {
        return Err(Error::StaleSnapshot);
    };
    crate::review::storage::Store::open(std::path::Path::new(&context.project_root))?
        .hydrate_proposal(proposal)?;
    let obsolete = proposal.policy_revision != before.policy_revision;
    let proposal = before
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    let (mut change, plan) = crate::review::publication::prepare_review(
        context,
        &before,
        &proposal.candidate,
        &proposal.candidate_metadata,
        Some(&base),
    )?;
    state::terminal_mcp_details(
        &mut change["changes"],
        &before.candidates["project"],
        &proposal.candidate,
    )?;
    let row = proposal.public();
    let requested = row["links"]["source_entry_id"].as_str().and_then(|entry| {
        review::observations::requested(std::path::Path::new(&context.project_root), entry)
    });
    let card = super::presentation::card(&row, &change, requested.as_ref(), obsolete);
    let choice = match tty.decide(&card) {
        Ok(choice) => choice,
        Err(error) => {
            let _ = review::transition(context, &id, "withdraw", &|| Ok(()));
            return Err(error);
        }
    };
    let result = match choice.as_str() {
        "accept" if !obsolete => crate::review::files::write::write(
            context,
            crate::review::files::write::Edit {
                before: &before,
                candidate: proposal.candidate.clone(),
                meta: proposal.candidate_metadata.clone(),
                // A review prepared before proposals recorded their files is
                // prepared again by its agent.
                base: proposal
                    .reviewed_paths
                    .clone()
                    .ok_or(Error::StaleSnapshot)?,
                evaluator: Some(&plan),
                reviewed: Some(&proposal.files),
            },
            Some(&id),
            "operator-cli",
        )?,
        "reject" => review::transition(context, &id, "reject", &|| Ok(()))?,
        _ => review::transition(context, &id, "withdraw", &|| Ok(()))?,
    };
    Ok(
        json!({"reviewed":[{"proposal_id":id,"choice":choice,"summary":card.summary,"result":result}]}),
    )
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
