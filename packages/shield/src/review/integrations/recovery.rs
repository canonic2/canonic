//! Exact human reviews for conflicting or cancelled confirmed setup publication.
use super::{
    Plan, Registration, files, legacy, pending, read_file, reconcile, registration, setup,
};
use crate::review::{Checkpoint, Context, Error, project, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(in crate::review) enum Action {
    Review,
    Cancel,
}

pub(in crate::review) struct Prepared {
    metadata: Value,
    original: Plan,
    plan: Option<Plan>,
    action: Action,
    preserved: Vec<String>,
    registration: Option<Registration>,
}

pub(in crate::review) fn prepare(
    root: &Path,
    metadata: &Value,
    action: Action,
) -> Result<Prepared, Error> {
    let original = pending(metadata)?.ok_or(Error::Invalid(
        "there is no interrupted setup to review or cancel",
    ))?;
    let committed = registration(metadata)?;
    let ownership = super::validate_pending_ownership(metadata);
    if action == Action::Review {
        ownership.as_ref().map_err(|_| Error::StaleSnapshot)?;
    }
    let rebased = if ownership.is_err() {
        Err(Error::StaleSnapshot)
    } else {
        rebase(root, &original, action)
    };
    let (plan, preserved) = match (action, rebased) {
        (_, Ok((plan, preserved))) if preserved.is_empty() => (Some(plan), preserved),
        (Action::Review, Ok(_)) => {
            return Err(Error::Invalid(
                "an opaque integration asset changed; call mcp_setup with pending cancel to preserve it without importing ownership",
            ));
        }
        (Action::Review, Err(error)) => return Err(error),
        (Action::Cancel, _) => (None, affected_paths(&original)),
    };
    let mut registered = committed.clone();
    if plan.is_none() {
        drop_affected(&mut registered, &preserved);
    }
    let mut prepared = Prepared {
        metadata: metadata.clone(),
        original,
        plan,
        action,
        preserved,
        registration: registered,
    };
    if action == Action::Cancel
        && let Some(plan) = prepared.plan.as_mut()
    {
        plan.previous.clone_from(&committed);
        plan.desired.clone_from(&committed);
    }
    let invalid_legacy = prepared
        .plan
        .as_ref()
        .is_some_and(|plan| legacy::validate_plan(plan).is_err());
    if action == Action::Review && invalid_legacy {
        return Err(Error::Invalid(
            "a legacy interceptor asset changed; call mcp_setup with pending cancel to preserve the edited asset without importing ownership",
        ));
    }
    let private_cancel = action == Action::Cancel
        && prepared
            .plan
            .as_ref()
            .is_some_and(|plan| super::privacy::validate(root, plan).is_err());
    if action == Action::Cancel
        && (private_cancel
            || invalid_legacy
            || prepared.plan.is_some()
                && serde_json::to_vec(&prepared.review(root))
                    .map_err(|_| Error::Invalid("cancellation review encoding"))?
                    .len()
                    > 24 * 1024)
    {
        prepared.plan = None;
        prepared.preserved = affected_paths(&prepared.original);
        prepared.registration = committed;
        drop_affected(&mut prepared.registration, &prepared.preserved);
    }
    Ok(prepared)
}

impl Prepared {
    pub(in crate::review) fn review(&self, root: &Path) -> Value {
        let mut review = setup::preview(root, self.plan.as_ref());
        review["operation"] = json!(if self.action == Action::Cancel {
            "cancel interrupted agent setup"
        } else {
            "review conflicting agent setup"
        });
        review["pending"] = json!(self.action);
        review["preserved_paths"] = json!(self.preserved);
        if self.plan.is_none() {
            let previous = registration(&self.metadata).ok().flatten();
            let after = self.registration.as_ref();
            review["dropped_ownership"] = json!({
                "agents":previous.as_ref().into_iter().flat_map(|registered|registered.agents.keys()).filter(|agent|after.is_none_or(|registered|!registered.agents.contains_key(*agent))).collect::<Vec<_>>(),
                "private_launch_agents":previous.as_ref().into_iter().flat_map(|registered|registered.connections.keys()).filter(|agent|after.is_none_or(|registered|!registered.connections.contains_key(*agent))).collect::<Vec<_>>(),
                "legacy_interceptor":previous.as_ref().is_some_and(|registered|registered.interceptor.is_some()) && after.is_none_or(|registered|registered.interceptor.is_none()),
                "host":previous.as_ref().and_then(|registered|registered.definition.as_ref()).filter(|_|after.is_none_or(|registered|registered.definition.is_none())).map(|definition|definition.host().id)
            });
        }
        review["effect"] = if self.plan.is_none() {
            json!(
                "Cancel the pending publication without editing any host file. Unreadable, oversized or manually conflicting files are preserved. Affected registration and private launcher authority are dropped; preserved files are not claimed healthy or managed."
            )
        } else if self.action == Action::Cancel {
            json!(
                "Restore the prior committed integration fragments while preserving unrelated settings. The pending plan stays durable until that restoration finishes."
            )
        } else {
            json!(
                "Publish the originally approved integration fragments against the exact current files shown in this new review. Unrelated settings stay intact."
            )
        };
        review
    }

    pub(in crate::review) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        crate::review::project::checkpoint(context)?;
        let publication =
            crate::review::publication::ProjectionLease::acquire(Path::new(&context.project_root))?;
        if let Some(plan) = &self.plan {
            Store::open(Path::new(&context.project_root))?.update_checkpoint(
                context,
                checkpoint,
                |state| {
                    if state.operator["integration"] != self.metadata["integration"]
                        || state.operator["integration_pending"]
                            != self.metadata["integration_pending"]
                    {
                        return Err(Error::StaleSnapshot);
                    }
                    files::validate_plan_recovery(Path::new(&context.project_root), plan)?;
                    if plan.touches_shared() {
                        reconcile::reconcile(
                            ".mcp.json",
                            plan.before.as_deref(),
                            super::shared_after(plan),
                            read_file(Path::new(&context.project_root))?.as_deref(),
                        )?;
                    }
                    checkpoint()?;
                    state.operator["integration_pending"] = json!(plan);
                    record_review(state, context);
                    checkpoint()?;
                    Ok(json!({}))
                },
            )?;
            let mut metadata = self.metadata.clone();
            metadata["integration_pending"] = json!(plan);
            let mut receipt = setup::finish_guarded(
                context,
                &metadata,
                Some(plan),
                None,
                checkpoint,
                &publication,
            )?;
            receipt["pending_resolution"] = json!(self.action);
            return Ok(receipt);
        }
        checkpoint()?;
        Store::open(Path::new(&context.project_root))?.update_checkpoint(
            context,
            checkpoint,
            |state| {
                if state.operator["integration"] != self.metadata["integration"]
                    || state.operator["integration_pending"] != self.metadata["integration_pending"]
                {
                    return Err(Error::StaleSnapshot);
                }
                let previous = registration(&state.operator)?;
                crate::review::session_grants::preserve_integration(
                    state,
                    previous.as_ref(),
                    self.registration.as_ref(),
                )?;
                state.operator["integration"] = json!(self.registration);
                state.operator["integration_pending"] = Value::Null;
                if let Some(metadata) = state.operator.as_object_mut() {
                    metadata.remove("integration_publication");
                }
                let digest = crate::json::fingerprint(&json!(self.original))
                    .map_err(|_| Error::Invalid("cancelled setup encoding"))?;
                state.operator["integration_cancelled"] =
                    json!({"plan_digest":digest,"preserved_paths":self.preserved});
                record_review(state, context);
                checkpoint()?;
                Ok(json!({}))
            },
        )?;
        let mut receipt = project::status(context)?;
        receipt["pending_resolution"] = json!("cancel");
        receipt["preserved_paths"] = json!(self.preserved);
        receipt["host_files_changed"] = json!(false);
        receipt["next_action"] = json!(
            "The interrupted publication is cancelled. Preserved host files remain unverified; review mcp_setup with explicit agents before relying on those integrations."
        );
        Ok(receipt)
    }
}

pub(in crate::review) fn record_review(state: &mut crate::review::model::State, context: &Context) {
    let review_id = state.id("integration-review");
    let marker = json!({"project_id":state.project_id,"review_id":review_id});
    state.operator["integration_review"] = marker.clone();
    let mut event = state.event(
        "integration_review_confirmed",
        context,
        json!({}),
        json!({}),
        json!({"review_channel":marker}),
    );
    event["operator"]["mechanism"] = json!(if context.surface == "mcp" {
        "host-elicitation"
    } else {
        "operator-cli"
    });
    state.record(event);
}

fn rebase(root: &Path, original: &Plan, action: Action) -> Result<(Plan, Vec<String>), Error> {
    let current = if original.touches_shared() {
        read_file(root)?
    } else {
        None
    };
    let mode = if action == Action::Review {
        reconcile::Conflict::ReplaceOwned
    } else {
        reconcile::Conflict::KeepCurrent
    };
    let (before, after) = match action {
        Action::Review => (original.before.as_deref(), super::shared_after(original)),
        Action::Cancel => (super::shared_after(original), original.before.as_deref()),
    };
    let (merged, conflict) = if original.touches_shared() {
        reconcile::resolve(".mcp.json", before, after, current.as_deref(), mode)?
    } else {
        (None, false)
    };
    let mut preserved = if conflict {
        vec![".mcp.json".into()]
    } else {
        vec![]
    };
    let mut changes = vec![];
    for file in &original.files {
        let current = if legacy::asset_path(&file.path) {
            files::read_legacy(root, &file.path)?
        } else {
            files::read(root, &file.path)?
        };
        if action == Action::Cancel && legacy::asset_path(&file.path) && file.before != file.after {
            preserved.push(file.path.clone());
            continue;
        }
        let (before, after) = if action == Action::Review {
            (file.before.as_deref(), file.after.as_deref())
        } else {
            (file.after.as_deref(), file.before.as_deref())
        };
        let (after, conflict) =
            reconcile::resolve(&file.path, before, after, current.as_deref(), mode)?;
        if conflict {
            preserved.push(file.path.clone());
        }
        if current != after {
            changes.push(files::Change {
                path: file.path.clone(),
                before: current,
                after,
            });
        }
    }
    let after = merged.unwrap_or_else(|| {
        if current.is_some() {
            "{}\n".into()
        } else {
            String::new()
        }
    });
    Ok((
        Plan {
            targets: original.targets.clone(),
            scope_reason: original.scope_reason.clone(),
            policy_revision: original.policy_revision.clone(),
            before: current,
            after,
            previous: original.previous.clone(),
            desired: if action == Action::Review {
                original.desired.clone()
            } else {
                original.previous.clone()
            },
            files: changes,
            handed_over: if action == Action::Review {
                original.handed_over.clone()
            } else {
                vec![]
            },
        },
        preserved,
    ))
}

fn affected_paths(plan: &Plan) -> Vec<String> {
    let mut paths: BTreeSet<String> = plan
        .files
        .iter()
        .filter(|file| file.before != file.after)
        .map(|file| file.path.clone())
        .collect();
    if plan.before.as_deref() != super::shared_after(plan) {
        paths.insert(".mcp.json".into());
    }
    paths.into_iter().collect()
}

fn drop_affected(registration: &mut Option<Registration>, paths: &[String]) {
    let Some(registered) = registration else {
        return;
    };
    for adapter in crate::agents::ADAPTERS {
        let spec = adapter.spec();
        if paths.iter().any(|path| {
            path == spec.hooks_path
                || spec.mcp_path == Some(path.as_str())
                || spec.prefix_rules_path == Some(path.as_str())
                || (path == ".mcp.json" && spec.mcp_path.is_none())
        }) {
            registered.agents.remove(spec.id);
            registered.connections.remove(spec.id);
            registered.ask_fallback.remove(spec.id);
        }
    }
    if paths.iter().any(|path| legacy::asset_path(path)) {
        registered.interceptor = None;
    }
    if let Some(host) = registered
        .definition
        .as_ref()
        .map(super::host_definition::Definition::host)
        && paths
            .iter()
            .any(|path| [host.path, host.local_path].contains(&path.as_str()))
    {
        registered.definition = None;
        let retained: BTreeSet<String> = registered
            .connections
            .keys()
            .filter(|agent| registered.covers(agent))
            .cloned()
            .collect();
        registered.connections.retain(|agent, _| {
            retained.contains(agent)
                || crate::agents::find(agent).is_none_or(|adapter| !adapter.supports_box_hooks())
        });
    }
    if registered.agents.is_empty()
        && registered.interceptor.is_none()
        && registered.definition.is_none()
    {
        *registration = None;
    }
}
