//! Complete project candidates shared by MCP proposal capture and operator commit.
pub(super) mod diff;
use super::{Checkpoint, Context, Error, model::Proposal, snapshots, storage::Store};
use crate::{configuration, policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    RuleAdd {
        rule: policy::normalized::Rule,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    RuleEdit {
        rule_id: String,
        rule: policy::normalized::Rule,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    RuleRemove {
        rule_id: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    PackAdd {
        reference: String,
        #[serde(default, deserialize_with = "present")]
        profile: Option<policy::Profile>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    PackUpdate {
        #[serde(default, deserialize_with = "present")]
        reference: Option<String>,
    },
    PackRemove {
        reference: String,
    },
    PackConfigure {
        reference: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        set: BTreeMap<String, Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unset: Vec<String>,
        #[serde(
            default,
            rename = "override",
            skip_serializing_if = "BTreeMap::is_empty"
        )]
        overrides: BTreeMap<String, policy::Override>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        clear_override: Vec<String>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    AccessPathAdd {
        access: Access,
        path: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    AccessPathRemove {
        access: Access,
        path: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
}

/// The configuration layer a change writes: shared project files, or the
/// developer's uncommitted local configuration.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Layer {
    Project,
    Local,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Access {
    Read,
    Write,
}

/// The most changes one proposal applies together.
const MAX_BATCH: usize = 16;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// One change, or `changes` for several applied together.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) change: Option<Change>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) changes: Option<Vec<Change>>,
    #[serde(default, deserialize_with = "present")]
    pub(super) idempotency_key: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub(super) agent_reason: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub(super) source_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) review_ids: Vec<String>,
    #[serde(default, deserialize_with = "present")]
    pub(super) supersedes_proposal_id: Option<String>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

impl Input {
    /// The submitted changes in order: exactly one of `change` or a nonempty,
    /// bounded `changes`.
    fn changes(&self) -> Result<Vec<&Change>, Error> {
        match (&self.change, &self.changes) {
            (Some(change), None) => Ok(vec![change]),
            (None, Some(changes)) if (1..=MAX_BATCH).contains(&changes.len()) => {
                Ok(changes.iter().collect())
            }
            _ => Err(Error::Invalid("propose either one change or 1-16 changes")),
        }
    }

    /// The retry key: the agent's own, or one derived from the submitted
    /// input, so an identical retry finds its proposal and a changed one is new.
    fn retry_key(&self) -> String {
        self.idempotency_key.clone().unwrap_or_else(|| {
            let digest = policy::fingerprint(json!(self).to_string().as_bytes());
            format!("proposal-{}", &digest[..32])
        })
    }
}

/// Live pending proposals a project retains.
const MAX_PENDING_PROPOSALS: usize = 128;
/// Proposals a project retains in every status.
const MAX_RETAINED_PROPOSALS: usize = 1024;

/// Refuse a new proposal when either bound is reached, with the counts.
fn capacity(proposals: &[Proposal]) -> Result<(), Error> {
    let pending = proposals.iter().filter(|p| p.status == "pending").count();
    let (kind, limit) = if pending >= MAX_PENDING_PROPOSALS {
        (super::CapacityKind::Proposals, MAX_PENDING_PROPOSALS)
    } else if proposals.len() >= MAX_RETAINED_PROPOSALS {
        (
            super::CapacityKind::RetainedProposals,
            MAX_RETAINED_PROPOSALS,
        )
    } else {
        return Ok(());
    };
    Err(Error::Capacity {
        kind,
        pending,
        retained: proposals.len(),
        limit,
    })
}

type PreparedPair = (configuration::Evaluator, configuration::Evaluator);
type Reservation<'a> =
    &'a mut dyn FnMut(&Value, &Value, &Value, &Value, Option<PreparedPair>) -> Result<(), Error>;

pub(super) fn propose(
    context: &Context,
    input: &Input,
    checkpoint: Checkpoint<'_>,
    reserve: Reservation<'_>,
    present: &dyn Fn(&Proposal) -> Value,
) -> Result<Value, Error> {
    validate_input(input)?;
    let changes = input.changes()?;
    let signature = policy::fingerprint(json!(input).to_string().as_bytes());
    let retry_key = input.retry_key();
    let store = Store::open(Path::new(&context.project_root))?;
    let (mut before, base_plan) = store.read_evaluated()?;
    before.check(context)?;
    if before.candidates.len() != 1 {
        return Err(Error::Invalid(
            "operator workflow requires project-only policy",
        ));
    }
    if let Some(row) = before
        .proposals
        .iter()
        .find(|p| p.retry_owner == context.retry_owner() && p.retry_key == retry_key)
    {
        if row.signature != signature {
            return Err(Error::Invalid("idempotency key reused with changed input"));
        }
        reserve(
            &row.base_candidate,
            &row.base_metadata,
            &row.candidate,
            &row.candidate_metadata,
            None,
        )?;
        return Ok(present(row));
    }
    capacity(&before.proposals)?;
    checkpoint()?;
    let (candidate, meta) = prepare_all(context, &mut before, &changes, checkpoint)?;
    let mut after = BTreeMap::new();
    after.insert("project".into(), candidate.clone());
    let base_snapshots = snapshots(&before.candidates)?;
    let next_snapshots = snapshots(&after)?;
    let mut comparison = json!(configuration::compare_policy(
        &base_snapshots,
        &next_snapshots
    )?);
    let next_plan = configuration::Evaluator::new(next_snapshots)?;
    let old: std::collections::BTreeSet<_> = base_plan.codex_prefixes().iter().collect();
    let new: std::collections::BTreeSet<_> = next_plan.codex_prefixes().iter().collect();
    if old != new {
        let prefixes = json!({"added":new.difference(&old).collect::<Vec<_>>(),"removed":old.difference(&new).collect::<Vec<_>>()});
        comparison["codex_prefixes"] = prefixes;
    }
    if let Some(lines) = super::formatting_changes(&before.candidates["project"], &candidate) {
        comparison["formatting"] = lines;
    }
    reserve(
        &before.candidates["project"],
        &before.operator,
        &candidate,
        &meta,
        Some((base_plan, next_plan)),
    )?;
    checkpoint()?;
    // The complete base was validated above. Under the writer lock, bind to
    // those exact source bytes and metadata instead of hydrating every retained
    // proposal and preparing the same active policy again.
    store.update_checkpoint(context, checkpoint, |state| {
        if state.policy_revision != before.policy_revision
            || state.operator != before.operator
            || super::storage::sources::map_digest(&state.candidates)
                != super::storage::sources::map_digest(&before.candidates)
        {
            return Err(Error::StaleSnapshot);
        }
        if let Some(row) = state.proposals.iter_mut().find(|p| p.retry_owner == context.retry_owner() && p.retry_key == retry_key) {
            return if row.signature == signature {
                // A concurrent identical request may have won after our first
                // read. Reserve that retained row, not this request's candidate.
                row.base_candidate = store.captured_candidate(&row.base_candidate)?;
                row.candidate = store.captured_candidate(&row.candidate)?;
                reserve(&row.base_candidate, &row.base_metadata, &row.candidate, &row.candidate_metadata, None)?;
                Ok(present(row))
            } else { Err(Error::Invalid("idempotency key reused with changed input")) };
        }
        if input.supersedes_proposal_id.as_ref().is_some_and(|id| !state.proposals.iter().any(|p| p.id == *id)) { return Err(Error::Missing); }
        capacity(&state.proposals)?;
        let id = state.id("proposal");
        let (kind, rule_id) = match changes.as_slice() {
            [change] => (json!(change)["kind"].as_str().unwrap_or_default().to_owned(), match change { Change::RuleAdd { rule, .. } | Change::RuleEdit { rule, .. } => rule.id.clone(), Change::RuleRemove { rule_id, .. } => rule_id.clone(), _ => String::new() }),
            _ => ("batch".to_owned(), String::new()),
        };
        let event = state.event("proposal_created", context, json!({"proposal_id":id,"source_entry_id":input.source_entry_id,"supersedes_proposal_id":input.supersedes_proposal_id}),json!({"agent_reason":input.agent_reason}),json!({"operation":kind,"policy_revision":state.policy_revision}));
        let mut event = event;
        event["links"]["review_ids"] = json!(input.review_ids);
        let row = Proposal { id, created_at:event["recorded_at"].as_str().ok_or(Error::Invalid("timestamp unavailable"))?.into(), status:"pending".into(), provenance:context.provenance(), links:event["links"].clone(), reasons:event["reasons"].clone(), policy_revision:state.policy_revision.clone(),target_scope:"project".into(),operation:kind,rule_id,candidate,base_candidate:before.candidates["project"].clone(),base_metadata:before.operator.clone(),candidate_metadata:meta,receipt:Value::Null,comparison,operator:None,retry_key:retry_key.clone(),retry_owner:context.retry_owner(),signature };
        let result = present(&row);
        state.proposals.push(row);
        state.record(event);
        Ok(result)
    })
}

/// The candidate after every change in order, each prepared against the
/// candidate and pack state the previous one produced, so a batch commits as
/// one generation.
fn prepare_all(
    context: &Context,
    state: &mut super::model::State,
    changes: &[&Change],
    checkpoint: Checkpoint<'_>,
) -> Result<(Value, Value), Error> {
    // Each step reads the state the previous one produced; the loaded
    // candidate and pack state are restored afterwards, whatever the outcome.
    let candidate = state.candidates.get("project").cloned();
    let operator = state.operator.clone();
    let mut steps = || {
        let mut prepared = None;
        for change in changes {
            checkpoint()?;
            let (candidate, meta) = prepare(context, state, change)?;
            state.candidates.insert("project".into(), candidate.clone());
            state.operator = meta.clone();
            prepared = Some((candidate, meta));
        }
        prepared.ok_or(Error::Invalid("proposal has no changes"))
    };
    let result = steps();
    if let Some(candidate) = candidate {
        state.candidates.insert("project".into(), candidate);
    }
    state.operator = operator;
    result
}

fn prepare(
    context: &Context,
    before: &super::model::State,
    change: &Change,
) -> Result<(Value, Value), Error> {
    if matches!(
        change,
        Change::PackAdd { .. }
            | Change::PackUpdate { .. }
            | Change::PackRemove { .. }
            | Change::PackConfigure { .. }
    ) {
        return super::packs::prepare_change(context, before, change);
    }
    if let Change::AccessPathAdd {
        access,
        path,
        layer,
    }
    | Change::AccessPathRemove {
        access,
        path,
        layer,
    } = change
    {
        let add = matches!(change, Change::AccessPathAdd { .. });
        return access_path(before, add, *access, path, *layer == Some(Layer::Local));
    }
    custom_rule(context, before, change)
}

/// A candidate with one custom rule added, edited or removed in its layer.
fn custom_rule(
    context: &Context,
    before: &super::model::State,
    change: &Change,
) -> Result<(Value, Value), Error> {
    let candidate = &before.candidates["project"];
    let local = matches!(
        change,
        Change::RuleAdd {
            layer: Some(Layer::Local),
            ..
        } | Change::RuleEdit {
            layer: Some(Layer::Local),
            ..
        } | Change::RuleRemove {
            layer: Some(Layer::Local),
            ..
        }
    );
    let field = if local { "local_rules" } else { "rules" };
    let mut document: Value = candidate[field].as_str().map_or_else(
        || Ok(policy::rules::document(&[])?),
        |s| super::inspection::project_rules(s, &context.project_root),
    )?;
    let rows = document["rules"]
        .as_array_mut()
        .ok_or(Error::Invalid("rules missing"))?;
    match change {
        Change::RuleAdd { rule, .. } => {
            rule.validate()?;
            portable_edit(context, None, &json!(rule))?;
            if rows.iter().any(|r| r["id"] == rule.id) {
                return Err(Error::Invalid("rule already exists"));
            }
            rows.push(json!(rule));
        }
        Change::RuleEdit { rule_id, rule, .. } => {
            rule.validate()?;
            if *rule_id != rule.id {
                return Err(Error::Invalid("rule edit cannot change identity"));
            }
            let row = rows
                .iter_mut()
                .find(|r| r["id"] == *rule_id)
                .ok_or(Error::Missing)?;
            let rule = json!(rule);
            portable_edit(context, Some(&*row), &rule)?;
            *row = rule;
        }
        Change::RuleRemove { rule_id, .. } => {
            let index = rows
                .iter()
                .position(|r| r["id"] == *rule_id)
                .ok_or(Error::Missing)?;
            rows.remove(index);
        }
        _ => return Err(Error::Invalid("rule change required")),
    }
    document = policy::rules::document_with(
        document["rules"]
            .as_array()
            .ok_or(Error::Invalid("rules missing"))?,
        document["directory_options"]
            .as_array()
            .map_or(&[][..], Vec::as_slice),
    )?;
    policy::CustomRules::from_yaml(&document.to_string())?;
    // Re-capture so the lock's custom-rule digest follows the new bytes.
    let mut config = configuration::layers::merged(candidate)?;
    let source = document.to_string();
    let bundle = if local {
        config["local_rules"] = json!("rules.local.yml");
        recapture(
            candidate,
            &config,
            candidate["rules"].as_str(),
            Some(&source),
        )?
    } else {
        config["rules"] = json!("rules.yml");
        recapture(
            candidate,
            &config,
            Some(&source),
            candidate["local_rules"].as_str(),
        )?
    };
    Ok((json!(bundle), before.operator.clone()))
}

/// Refuses a new rule, or an edit of its `existing` version that changes what
/// it matches, when it names this machine's folders, so a rule approved once
/// matches on every checkout. An edit that keeps conditions and requirements,
/// such as a new reason, is not blocked by paths the rule already had.
pub(super) fn portable_edit(
    context: &Context,
    existing: Option<&Value>,
    rule: &Value,
) -> Result<(), Error> {
    if existing.is_some_and(|existing| {
        ["conditions", "requirements"]
            .into_iter()
            .all(|field| existing[field] == rule[field])
    }) {
        return Ok(());
    }
    let roots = crate::agents::machine_roots(Path::new(&context.project_root));
    policy::portability::check(rule, &roots).map_err(Error::NotPortable)
}

/// Capture a changed configuration with the candidate's other artifacts, each
/// file edited from the candidate's source.
fn recapture(
    candidate: &Value,
    config: &Value,
    rules: Option<&str>,
    local_rules: Option<&str>,
) -> Result<configuration::Bundle, Error> {
    let artifacts = |field: &str| -> Result<Vec<String>, Error> {
        if candidate[field].is_null() {
            return Ok(Vec::new());
        }
        serde_json::from_value(candidate[field].clone())
            .map_err(|_| Error::Invalid("pack artifacts missing"))
    };
    Ok(configuration::Bundle::revise(
        candidate,
        config,
        &artifacts("packs")?,
        rules,
        local_rules,
        &artifacts("disabled")?,
    )?)
}

/// Adds or removes one root in the project's `access_paths`, or the local
/// layer's, keeping packs and rules.
fn access_path(
    before: &super::model::State,
    add: bool,
    access: Access,
    path: &str,
    local: bool,
) -> Result<(Value, Value), Error> {
    let candidate = &before.candidates["project"];
    let mut config = configuration::layers::merged(candidate)?;
    let key = match access {
        Access::Read => "read",
        Access::Write => "write",
    };
    let list = &mut config[if local {
        "local_access_paths"
    } else {
        "access_paths"
    }][key];
    if list.is_null() {
        *list = json!([]);
    }
    let rows = list
        .as_array_mut()
        .ok_or(Error::Invalid("access paths invalid"))?;
    match (add, rows.iter().position(|row| row == path)) {
        (true, None) => rows.push(json!(path)),
        (false, Some(index)) => {
            rows.remove(index);
        }
        (true, Some(_)) => return Err(Error::Invalid("access path already present")),
        (false, None) => return Err(Error::Missing),
    }
    let bundle = recapture(
        candidate,
        &config,
        candidate["rules"].as_str(),
        candidate["local_rules"].as_str(),
    )?;
    Ok((json!(bundle), before.operator.clone()))
}

fn validate_input(input: &Input) -> Result<(), Error> {
    if input.review_ids.len() > 16
        || input
            .review_ids
            .iter()
            .any(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        || input.agent_reason.as_ref().is_some_and(|s| s.len() > 4096)
        || [
            &input.idempotency_key,
            &input.source_entry_id,
            &input.supersedes_proposal_id,
        ]
        .into_iter()
        .flatten()
        .any(|s| s.is_empty() || s.len() > 128 || !s.is_ascii() || s.contains('\0'))
    {
        return Err(Error::Invalid("invalid proposal fields"));
    }
    Ok(())
}
