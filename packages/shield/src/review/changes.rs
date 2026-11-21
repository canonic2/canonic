//! Complete project candidates shared by MCP proposal capture and operator commit.
pub(super) mod diff;
pub(in crate::review) mod historical;
pub(super) mod preparation;
pub(super) mod project_files;
use super::{Checkpoint, Context, Error, model::Proposal, snapshots, storage::Store};
use crate::{configuration, policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    RuleAdd {
        rule: policy::resolution::normalized::Rule,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        layer: Option<Layer>,
    },
    RuleEdit {
        rule_id: String,
        rule: policy::resolution::normalized::Rule,
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
    /// Captured application workflow data; never deserialized from tool arguments.
    #[serde(skip)]
    pub(super) prepared: Option<Prepared>,
    /// Private workflow binding; never accepted from model-supplied JSON.
    #[serde(skip)]
    pub(super) expected_policy_revision: Option<String>,
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
pub(super) type CandidateChange = (Value, Value, Option<configuration::Snapshot>);
/// An installation candidate whose exact artifacts were captured before user input.
#[derive(Serialize, Deserialize)]
pub(super) struct Prepared {
    pub(super) binding: String,
    pub(super) candidate: Value,
    pub(super) metadata: Value,
    pub(super) comparison: Option<Value>,
}

pub(super) fn prepared_binding(state: &super::model::State) -> String {
    policy::fingerprint(
        json!({"candidates":super::map_digest(&state.candidates),"metadata":state.operator})
            .to_string()
            .as_bytes(),
    )
}
/// Reserve handles for a base and candidate with their metadata, their
/// [`super::candidate_identity`] digests and, for a fresh review, their plans.
type Reservation<'a> = &'a mut dyn FnMut(
    (&Value, &Value),
    (&Value, &Value),
    (&str, &str),
    Option<PreparedPair>,
) -> Result<(), Error>;

/// An explicitly keyed completed captured operation can replay without acquiring
/// its inputs again. Pending operations still bind the complete captured image.
pub(super) fn applied_retry(context: &Context, input: &Input) -> Result<bool, Error> {
    validate_input(input)?;
    let Some(key) = &input.idempotency_key else {
        return Ok(false);
    };
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    state.check_operations()?;
    let request = policy::fingerprint(json!(input).to_string().as_bytes());
    Ok(state.proposals.iter().any(|row| {
        row.retry_owner == context.retry_owner()
            && row.retry_key == *key
            && row.status == "applied"
            && row
                .signature
                .split_once(':')
                .is_some_and(|(original, _)| original == request)
    }))
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep ordered capture, reservation and writer-locked revalidation together with their request-owned spans"
)]
pub(super) fn propose(
    context: &Context,
    input: &Input,
    checkpoint: Checkpoint<'_>,
    reserve: Reservation<'_>,
    present: &dyn Fn(&Proposal) -> Value,
) -> Result<Value, Error> {
    let _trace = crate::diagnostics::trace::span("proposal.prepare");
    validate_input(input)?;
    let changes = input.changes()?;
    let request_signature = policy::fingerprint(json!(input).to_string().as_bytes());
    let mut signature = request_signature.clone();
    if let Some(prepared) = &input.prepared {
        let captured = policy::fingerprint(
            json!([
                input,
                prepared.binding,
                prepared.candidate,
                prepared.metadata
            ])
            .to_string()
            .as_bytes(),
        );
        signature = format!("{request_signature}:{captured}");
    }
    let mut retry_key = input.retry_key();
    if input.idempotency_key.is_none() && input.prepared.is_some() {
        retry_key = format!(
            "proposal-{}",
            &policy::fingerprint(signature.as_bytes())[..32]
        );
    }
    let store = Store::open(Path::new(&context.project_root))?;
    let mut before = store.read_following_files()?;
    before.check(context)?;
    before.check_operations()?;
    if input
        .expected_policy_revision
        .as_ref()
        .is_some_and(|revision| revision != &before.policy_revision)
    {
        return Err(Error::StaleSnapshot);
    }
    if before.candidates.len() != 1 {
        return Err(Error::Invalid(
            "operator workflow requires project-only policy",
        ));
    }
    if let Some(row) = before
        .proposals
        .iter_mut()
        .find(|p| p.retry_owner == context.retry_owner() && p.retry_key == retry_key)
    {
        let applied_replay = input.idempotency_key.is_some()
            && row.status == "applied"
            && row
                .signature
                .split_once(':')
                .is_some_and(|(original, _)| original == request_signature);
        if row.signature != signature && !applied_replay {
            return Err(Error::Invalid("idempotency key reused with changed input"));
        }
        // Hydration checked the stored digests against these candidates.
        store.hydrate_proposal(row)?;
        reserve(
            (&row.base_candidate, &row.base_metadata),
            (&row.candidate, &row.candidate_metadata),
            (&row.base_digest, &row.candidate_digest),
            None,
        )?;
        return Ok(present(row));
    }
    super::web_reviews::check_proposal(&before, context, &input.review_ids)?;
    checkpoint()?;
    context
        .progress
        .stage(super::progress::Stage::PreparePolicy);
    let (mut before, base_plan, base_snapshots) = Store::read_for_comparison(before)?;
    let (candidate, meta, captured) = if let Some(prepared) = &input.prepared {
        if prepared.binding != prepared_binding(&before) {
            return Err(Error::StaleSnapshot);
        }
        (prepared.candidate.clone(), prepared.metadata.clone(), None)
    } else {
        let _trace = crate::diagnostics::trace::span("proposal.changes");
        prepare_all(
            context,
            &mut before,
            &changes,
            checkpoint,
            base_snapshots.first().map(|(_, snapshot)| snapshot),
        )?
    };
    // Applying writes the files only while they still hold what this review
    // was prepared from.
    let reviewed_paths = {
        let _trace = crate::diagnostics::trace::span("proposal.reviewed_paths");
        Some(super::files::write::base(
            context, &before, &candidate, &meta,
        )?)
    };
    let reviewed_files: Option<String> = None;
    // The proposal is the file texts it writes. Both sides are rebuilt from
    // them here, as every later load rebuilds them and refuses texts that
    // no longer match these digests.
    let files = {
        let _trace = crate::diagnostics::trace::span("proposal.file_texts");
        super::files::write::texts(context, &before, &candidate, &meta)?
    };
    let root = Path::new(&context.project_root);
    let prepared_candidate = candidate;
    let (candidate, base_candidate) = {
        let _trace = crate::diagnostics::trace::span("proposal.rebuild_sides");
        (
            super::files::write::bundle(root, &files, true)?,
            super::files::write::bundle(root, &files, false)?,
        )
    };
    // The prepared plan and comparison describe the stored candidate only
    // when the texts rebuild it exactly.
    let rebuilt_exactly = candidate == prepared_candidate;
    let captured = captured.filter(|_| rebuilt_exactly);
    let mut after = BTreeMap::new();
    {
        let _trace = crate::diagnostics::trace::span("proposal.historical");
        historical::validate(&base_candidate, &candidate)?;
    }
    after.insert("project".into(), candidate.clone());
    let next_snapshots = {
        let _trace = crate::diagnostics::trace::span("proposal.capture_next");
        match captured {
            Some(snapshot) => vec![(policy::Scope::Project, snapshot)],
            None => snapshots(&after)?,
        }
    };
    let mut comparison = {
        let _trace = crate::diagnostics::trace::span("proposal.compare");
        if let Some(comparison) = input
            .prepared
            .as_ref()
            .and_then(|prepared| prepared.comparison.as_ref())
            .filter(|_| rebuilt_exactly)
        {
            comparison.clone()
        } else {
            context
                .progress
                .stage(super::progress::Stage::ComparePolicy);
            json!(configuration::compare_policy(
                &base_snapshots,
                &next_snapshots
            )?)
        }
    };
    let next_plan = configuration::Evaluator::new(next_snapshots)?;
    context
        .progress
        .stage(super::progress::Stage::PrepareReview);
    compare_prefixes(&base_plan, &next_plan, &mut comparison);
    if let Some(summary) =
        diff::override_permission_review(&base_candidate, &candidate, &comparison)
    {
        comparison["override_permission_review"] = summary;
    }
    if let Some(lines) = super::formatting_changes(&base_candidate, &candidate) {
        comparison["formatting"] = lines;
    }
    // Each side is hashed once: the handles, the presentation and the
    // proposal row all bind these identities.
    let presentation_before = super::candidate_identity(&base_candidate);
    let presentation_after = super::candidate_identity(&candidate);
    {
        let _trace = crate::diagnostics::trace::span("proposal.reserve");
        reserve(
            (&base_candidate, &before.operator),
            (&candidate, &meta),
            (&presentation_before, &presentation_after),
            Some((base_plan, next_plan)),
        )?;
    }
    checkpoint()?;
    if files.values().all(|texts| texts.before == texts.after) && meta == before.operator {
        return Ok(
            json!({"unchanged":true,"committed":false,"policy_revision":before.policy_revision}),
        );
    }
    // The complete base was validated above. Under the writer lock, bind to
    // those exact source bytes and metadata instead of hydrating every retained
    // proposal and preparing the same active policy again.
    let presentation_ref = super::model::presentation::capture(
        &before,
        (&base_candidate, &candidate),
        (&presentation_before, &presentation_after),
        &meta,
        &comparison,
        reviewed_files.is_some(),
    )
    .ok()
    .flatten()
    .unwrap_or(Value::Null);
    store.update_checkpoint(context, checkpoint, |state| {
        state.check_operations()?;
        if state.policy_revision != before.policy_revision
            || state.operator != before.operator
            || super::map_digest(&state.candidates) != super::map_digest(&before.candidates)
        {
            return Err(Error::StaleSnapshot);
        }
        if let Some(row) = state.proposals.iter_mut().find(|p| p.retry_owner == context.retry_owner() && p.retry_key == retry_key) {
            let applied_replay = input.idempotency_key.is_some() && row.status == "applied"
                && row.signature.split_once(':').is_some_and(|(original, _)| original == request_signature);
            return if row.signature == signature || applied_replay {
                // A concurrent identical request may have won after our first
                // read. Reserve that retained row, not this request's candidate.
                store.hydrate_proposal(row)?;
                reserve((&row.base_candidate, &row.base_metadata), (&row.candidate, &row.candidate_metadata), (&row.base_digest, &row.candidate_digest), None)?;
                Ok(present(row))
            } else { Err(Error::Invalid("idempotency key reused with changed input")) };
        }
        if input.supersedes_proposal_id.as_ref().is_some_and(|id| !state.proposals.iter().any(|p| p.id == *id)) { return Err(Error::Missing); }
        super::web_reviews::check_proposal(state, context, &input.review_ids)?;
        for previous in &mut state.proposals {
            if super::proposal_lifecycle::owns(previous, context) {
                super::proposal_lifecycle::close(previous, "withdrawn", "superseded");
            }
        }
        super::proposal_lifecycle::reclaim(state);
        capacity(&state.proposals)?;
        let id = state.id("proposal");
        super::web_reviews::attach(state, context, &input.review_ids, &id)?;
        let (kind, rule_id) = match changes.as_slice() {
            [change] => (json!(change)["kind"].as_str().unwrap_or_default().to_owned(), match change { Change::RuleAdd { rule, .. } | Change::RuleEdit { rule, .. } => rule.id.clone(), Change::RuleRemove { rule_id, .. } => rule_id.clone(), _ => String::new() }),
            _ => ("batch".to_owned(), String::new()),
        };
        let event = state.event("proposal_created", context, json!({"proposal_id":id,"source_entry_id":input.source_entry_id,"supersedes_proposal_id":input.supersedes_proposal_id}),json!({"agent_reason":input.agent_reason}),json!({"operation":kind,"policy_revision":state.policy_revision}));
        let mut event = event;
        event["links"]["review_ids"] = json!(input.review_ids);
        event["details"]["presentation"] =
            super::model::presentation::digest(&presentation_ref);
        event["details"]["before_candidate"] = json!(presentation_before);
        event["details"]["after_candidate"] = json!(presentation_after);
        let row = Proposal { id, created_at:event["recorded_at"].as_str().ok_or(Error::Invalid("timestamp unavailable"))?.into(), status:"pending".into(), provenance:context.provenance(), links:event["links"].clone(), reasons:event["reasons"].clone(), policy_revision:state.policy_revision.clone(),target_scope:"project".into(),operation:kind,rule_id,candidate_digest:presentation_after.clone(),base_digest:presentation_before.clone(),files:files.clone(),candidate,base_candidate:base_candidate.clone(),retired_candidate:None,retired_base_candidate:None,base_metadata:before.operator.clone(),candidate_metadata:meta,receipt:Value::Null,comparison,presentation:presentation_ref,operator:None,retry_key:retry_key.clone(),retry_owner:context.retry_owner(),signature,reviewed_files, reviewed_paths, interaction:Some(super::proposal_lifecycle::Interaction::new(context)?), closed_reason:None };
        let result = {
            let _trace = crate::diagnostics::trace::span("proposal.present");
            present(&row)
        };
        state.proposals.push(row);
        state.record(event);
        Ok(result)
    })
}

pub(super) fn compare_prefixes(
    before: &configuration::Evaluator,
    after: &configuration::Evaluator,
    comparison: &mut Value,
) {
    let old: std::collections::BTreeSet<_> = before.codex_prefixes().iter().collect();
    let new: std::collections::BTreeSet<_> = after.codex_prefixes().iter().collect();
    if old != new {
        comparison["codex_prefixes"] = json!({"added":new.difference(&old).collect::<Vec<_>>(),"removed":old.difference(&new).collect::<Vec<_>>()});
    }
}

/// The candidate after every change in order, each prepared against the
/// candidate and pack state the previous one produced, so a batch commits as
/// one generation.
fn prepare_all(
    context: &Context,
    state: &mut super::model::State,
    changes: &[&Change],
    checkpoint: Checkpoint<'_>,
    patterns: Option<&configuration::Snapshot>,
) -> Result<CandidateChange, Error> {
    // Each step reads the state the previous one produced; the loaded
    // candidate and pack state are restored afterwards, whatever the outcome.
    let candidate = state.candidates.get("project").cloned();
    let operator = state.operator.clone();
    let mut steps = || {
        let mut prepared = None;
        for change in changes {
            checkpoint()?;
            let previous = prepared
                .as_ref()
                .and_then(|(_, _, snapshot): &CandidateChange| snapshot.as_ref())
                .or(patterns);
            let (candidate, meta, snapshot) = prepare(context, state, change, previous)?;
            state.candidates.insert("project".into(), candidate.clone());
            state.operator = meta.clone();
            prepared = Some((candidate, meta, snapshot));
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
    patterns: Option<&configuration::Snapshot>,
) -> Result<CandidateChange, Error> {
    if matches!(
        change,
        Change::PackAdd { .. }
            | Change::PackUpdate { .. }
            | Change::PackRemove { .. }
            | Change::PackConfigure { .. }
    ) {
        return super::packs::prepare_change(context, before, change)
            .map(|(candidate, metadata)| (candidate, metadata, None));
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
        return access_path(
            before,
            add,
            *access,
            path,
            *layer == Some(Layer::Local),
            patterns,
        );
    }
    custom_rule(context, before, change, patterns)
}

/// A candidate with one custom rule added, edited or removed in its layer.
pub(super) fn custom_rule(
    context: &Context,
    before: &super::model::State,
    change: &Change,
    patterns: Option<&configuration::Snapshot>,
) -> Result<CandidateChange, Error> {
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
        |s| policy::normalize_retained_custom(s).map_err(Error::from),
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
    let (bundle, snapshot) = if local {
        config["local_rules"] = json!("rules.local.yml");
        recapture(
            candidate,
            &config,
            candidate["rules"].as_str(),
            Some(&source),
            patterns,
        )?
    } else {
        config["rules"] = json!("rules.yml");
        recapture(
            candidate,
            &config,
            Some(&source),
            candidate["local_rules"].as_str(),
            patterns,
        )?
    };
    Ok((json!(bundle), before.operator.clone(), Some(snapshot)))
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
    policy::rules::portability::check(rule, &roots).map_err(Error::NotPortable)
}

/// Capture a changed configuration with the candidate's other artifacts, each
/// file edited from the candidate's source.
fn recapture(
    candidate: &Value,
    config: &Value,
    rules: Option<&str>,
    local_rules: Option<&str>,
    patterns: Option<&configuration::Snapshot>,
) -> Result<(configuration::Bundle, configuration::Snapshot), Error> {
    let artifacts = |field: &str| -> Result<Vec<String>, Error> {
        if candidate[field].is_null() {
            return Ok(Vec::new());
        }
        serde_json::from_value(candidate[field].clone())
            .map_err(|_| Error::Invalid("pack artifacts missing"))
    };
    Ok(configuration::Bundle::revise_prepared(
        candidate,
        config,
        &artifacts("packs")?,
        rules,
        local_rules,
        &artifacts("disabled")?,
        patterns,
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
    patterns: Option<&configuration::Snapshot>,
) -> Result<CandidateChange, Error> {
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
    let (bundle, snapshot) = recapture(
        candidate,
        &config,
        candidate["rules"].as_str(),
        candidate["local_rules"].as_str(),
        patterns,
    )?;
    Ok((json!(bundle), before.operator.clone(), Some(snapshot)))
}

pub(super) fn validate_input(input: &Input) -> Result<(), Error> {
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

#[cfg(test)]
mod prepared_binding_tests {
    use super::*;

    #[test]
    fn prepared_binding_matches_every_reader_of_the_files_but_rejects_drift() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
        super::super::mcp::initialize_fixture(root.path(), &bundle);
        let store = Store::open(root.path()).unwrap();
        let mut compact = store.read_following_files().unwrap();
        let mut hydrated = store.read().unwrap();
        let binding = prepared_binding(&hydrated);
        assert_eq!(binding, prepared_binding(&compact));
        compact.operator["default"] = json!("other");
        assert_ne!(binding, prepared_binding(&compact));
        hydrated.candidates.get_mut("project").unwrap()["config"] =
            json!("schema_version: 1\npacks: []\n\n");
        assert_ne!(binding, prepared_binding(&hydrated));
    }
}
