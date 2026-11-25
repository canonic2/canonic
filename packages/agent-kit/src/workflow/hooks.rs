//! Reviewed native prompt-hook edits, sharing setup's publication and ownership.
use super::{Change, Context, Error, Inputs, Kind, Options, Result, ledger, ownership, storage};
use crate::Format;
use crate::document::Prepared;
use serde_json::Value;

const EVENT: &str = "UserPromptSubmit";

#[derive(Default)]
pub(super) struct HookEdit {
    pub(super) owned: Option<Value>,
    pub(super) observed: Option<Value>,
    groups: Option<Vec<Value>>,
}

impl HookEdit {
    pub(super) fn groups_for_edit(&self) -> Option<(&'static str, &[Value])> {
        self.groups.as_deref().map(|groups| (EVENT, groups))
    }
}

pub(super) fn settings_hook(value: Option<&Value>) -> Option<&Value> {
    value
        .and_then(|v| v.get("prompt_hook"))
        .filter(|v| !v.is_null())
}

pub(super) fn claude(
    document: &Prepared,
    record: Option<&storage::Record>,
    desired: Option<&Value>,
) -> Result<HookEdit> {
    let previous = settings_hook(record.and_then(|r| r.desired.as_ref()));
    let pending = settings_hook(
        record
            .filter(|r| r.pending)
            .and_then(|r| r.previous.as_ref()),
    );
    let previous: Vec<_> = [previous, pending].into_iter().flatten().collect();
    if desired.is_none() && previous.is_empty() {
        return Ok(HookEdit::default());
    }
    edit(document, &previous, desired)
}

/// Own newly inserted groups, leaving an equivalent pre-existing group unowned.
fn edit(document: &Prepared, previous: &[&Value], desired: Option<&Value>) -> Result<HookEdit> {
    if let Some(desired) = desired {
        crate::agents::prompt_handlers(desired)?;
    }
    let mut groups = document.hook_groups(EVENT)?;
    let observed = groups
        .iter()
        .find(|group| previous.contains(group))
        .cloned();
    let original_count = groups.len();
    groups.retain(|group| !previous.contains(&group) || Some(group) == desired);
    let removed = groups.len() != original_count;
    for group in previous {
        refuse_modified_group(groups.iter().filter(|g| Some(*g) != desired), group)?;
    }
    let owned = if let Some(desired) = desired {
        let present = groups.contains(desired);
        if !present {
            refuse_modified_group(&groups, desired)?;
            groups.push(desired.clone());
        }
        (!present || previous.contains(&desired)).then(|| desired.clone())
    } else {
        None
    };
    Ok(HookEdit {
        owned,
        observed,
        groups: (removed || groups.len() != original_count).then_some(groups),
    })
}

fn refuse_modified_group<'a>(
    groups: impl IntoIterator<Item = &'a Value>,
    group: &Value,
) -> Result<()> {
    let commands: Vec<_> = group["hooks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| h["command"].as_str())
        .collect();
    if groups.into_iter().any(|g| {
        g["hooks"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|h| h["command"].as_str().is_some_and(|c| commands.contains(&c)))
    }) {
        return Err(Error::Conflict(
            "UserPromptSubmit hook command exists with different fields; align the existing group with the requested definition or remove it before setup"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn codex(
    context: &Context,
    options: &Options,
    inputs: &Inputs,
) -> Result<Option<Change>> {
    // Native Codex homes follow the same override as its MCP configuration.
    let destination = crate::agents::destination(context, "codex", options.scope)?;
    let path = destination.path.with_file_name("hooks.json");
    let ledger = ledger(context, &path);
    let ledger_before = inputs.read(&ledger)?;
    let owned = ownership(context, &path, ledger_before.as_deref())?;
    if owned.is_none() && (options.remove || context.prompt_hook.is_none()) {
        return Ok(None);
    }
    // Codex also loads inline `[[hooks.UserPromptSubmit]]` from the same layer's
    // config.toml. An equal inline group already runs, so it stays unowned and
    // is not duplicated; the same command with other fields conflicts.
    let inline = if inputs.exists(&destination.path)? {
        inputs
            .document(&destination.path, Format::Toml)?
            .hook_groups(EVENT)?
    } else {
        Vec::new()
    };
    let requested = context.prompt_hook.as_ref().filter(|_| !options.remove);
    if let Some(group) = requested {
        refuse_modified_group(inline.iter().filter(|g| *g != group), group)?;
    }
    let requested = requested.filter(|group| !inline.contains(group));
    if owned.is_none() && requested.is_none() {
        return Ok(None);
    }
    let before = inputs.read(&path)?;
    let previous = owned.as_ref().and_then(|r| r.desired.as_ref());
    let pending = owned
        .as_ref()
        .filter(|r| r.pending)
        .and_then(|r| r.previous.as_ref());
    let previous_groups: Vec<_> = [previous, pending].into_iter().flatten().collect();
    let document = inputs.document(&path, Format::Json)?;
    let update = edit(&document, &previous_groups, requested)?;
    let edited = document.edit_settings(None, update.groups_for_edit())?;
    if before.as_deref() == Some(&edited) && owned.is_none() {
        return Ok(None);
    }
    let desired = update.owned.filter(|_| !options.remove);
    let ownership_unchanged = owned
        .as_ref()
        .is_some_and(|r| !r.pending && r.desired == desired && r.previous == desired);
    let after = if before.is_none() && options.remove {
        None
    } else {
        Some(edited)
    };
    Ok(Some(Change {
        kind: Kind::PromptHook,
        agents: vec!["codex".into()],
        record: storage::Record {
            version: 1,
            path: path.clone(),
            product: context.product.clone(),
            previous: update.observed,
            desired,
            pending: owned.is_some_and(|r| r.pending),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scope, prepare};
    use serde_json::json;
    use std::{collections::BTreeMap, fs};

    /// Persist the plan's pending Claude settings record without replacing the
    /// host file, as publication does before an interruption.
    fn interrupt_claude_settings(context: &Context, options: &Options) {
        let plan = prepare(context, options).unwrap();
        let change = plan
            .changes
            .iter()
            .find(|c| c.kind == Kind::Approvals)
            .unwrap();
        let mut record = change.record.clone();
        record.pending = true;
        storage::write(
            &change.ledger,
            &serde_json::to_string(&record).unwrap(),
            true,
            change.ledger_before.as_deref(),
        )
        .unwrap();
    }

    #[test]
    fn claude_hook_removal_keeps_ownership_across_repeated_interruptions() {
        for remove in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let base = temp.path().canonicalize().unwrap();
            let group = json!({"hooks":[{"type":"command","command":"acme hook","timeout":5}]});
            let mut context = Context {
                product: "acme".into(),
                program: base.join("acme"),
                arguments: vec!["serve".into()],
                launch_command: None,
                check_arguments: Vec::new(),
                root: base.join("project"),
                home: base.join("home"),
                state: base.join("state"),
                agent_homes: BTreeMap::new(),
                tools: Vec::new(),
                prompt_hook: Some(group.clone()),
            };
            fs::create_dir_all(&context.root).unwrap();
            fs::create_dir_all(&context.home).unwrap();
            let mut options = Options {
                agents: vec!["claude".into()],
                ..Options::default()
            };
            prepare(&context, &options).unwrap().apply().unwrap();
            // Either a removal or a consumer that stops supplying the hook.
            if remove {
                options.remove = true;
            } else {
                context.prompt_hook = None;
            }
            interrupt_claude_settings(&context, &options);
            interrupt_claude_settings(&context, &options);
            let path = crate::agents::claude_settings(&context, Scope::Project);
            let installed: Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(installed["hooks"][EVENT], json!([group]));
            prepare(&context, &options).unwrap().apply().unwrap();
            let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(value["hooks"][EVENT], json!([]), "remove={remove}");
        }
    }

    #[test]
    fn prompt_hook_repair_retains_the_observed_group_if_publication_is_interrupted_again() {
        let old = json!({"hooks":[{"type":"command","command":"acme hook old"}]});
        let new = json!({"hooks":[{"type":"command","command":"acme hook user-prompt"}]});
        let source = json!({"hooks":{"UserPromptSubmit":[old]}}).to_string();
        let document = Prepared::parse(source, Format::Json).unwrap();
        let update = edit(&document, &[&new, &old], Some(&new)).unwrap();
        assert_eq!(update.observed, Some(old.clone()));
        // A second pending record must still accept the unchanged on-disk old
        // group if replacement did not happen before another interruption.
        let update = edit(
            &document,
            &[
                update.owned.as_ref().unwrap(),
                update.observed.as_ref().unwrap(),
            ],
            Some(&new),
        )
        .unwrap();
        assert_eq!(update.owned, Some(new.clone()));
        assert_eq!(update.observed, Some(old));
        assert_eq!(update.groups.as_deref(), Some([new].as_slice()));
    }
}
