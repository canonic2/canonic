//! Opt-in removal of a copied project cwd from exactly owned registrations.
use super::{Change, Kind, Plan, agents, ledger, ownership};
use crate::{Context, Result, Scope, document, storage};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Remove a setup-owned absolute project cwd without changing the launcher,
/// arguments, tools or permissions. Consumers opt in when inheriting the host's
/// project directory is their launch policy. Unowned and edited entries stay.
/// The old project's ownership record is preserved, including for a live copy.
///
/// # Errors
/// Returns an error on unsafe paths, malformed owned configuration, concurrent
/// changes or recoverable partial publication. No catalog process is started.
pub fn relocate_project(context: &Context, previous_root: &Path) -> Result<Vec<PathBuf>> {
    if !context.root.is_absolute() || !previous_root.is_absolute() {
        return Err(crate::Error::invalid(
            "Project migration roots must be absolute",
        ));
    }
    let mut candidates = Vec::new();
    for (id, _) in agents::catalog() {
        let Ok(destination) = agents::destination(context, id, Scope::Project) else {
            continue;
        };
        let Ok(relative) = destination.path.strip_prefix(&context.root) else {
            continue;
        };
        let previous = previous_root.join(relative);
        let old_ledger = ledger(context, &previous);
        let new_ledger = ledger(context, &destination.path);
        if storage::read(&new_ledger)?.is_some() || storage::read(&old_ledger)?.is_some() {
            candidates.push((destination, previous, old_ledger));
        }
    }
    let mut locks = Vec::new();
    let previous_paths: BTreeSet<_> = candidates
        .iter()
        .filter(|(destination, previous, _)| destination.path != *previous)
        .map(|(_, previous, _)| previous)
        .collect();
    for path in previous_paths {
        locks.push(storage::lock(&context.state, &storage::identity(path))?);
    }
    let mut plan = Plan {
        context: context.clone(),
        scope: Scope::Project,
        remove: false,
        changes: Vec::new(),
        other_scopes: Vec::new(),
        approved: Vec::new(),
        approval_agents: Vec::new(),
        kept_by_other_rules: Vec::new(),
        other_rules: Vec::new(),
    };
    for (destination, previous, old_ledger) in candidates {
        if plan
            .changes
            .iter()
            .any(|change| change.path == destination.path)
        {
            continue;
        }
        if let Some(change) =
            registration(context, destination, &previous, &old_ledger, previous_root)?
        {
            plan.changes.push(change);
        }
    }
    let paths = plan
        .changes
        .iter()
        .map(|change| change.path.clone())
        .collect();
    if !plan.changes.is_empty() {
        plan.apply()?;
    }
    Ok(paths)
}

fn registration(
    context: &Context,
    destination: agents::Destination,
    previous: &Path,
    old_ledger: &Path,
    previous_root: &Path,
) -> Result<Option<Change>> {
    let new_ledger = ledger(context, &destination.path);
    let ledger_before = storage::read(&new_ledger)?;
    let owned =
        if let Some(record) = ownership(context, &destination.path, ledger_before.as_deref())? {
            Some(record)
        } else {
            ownership(context, previous, storage::read(old_ledger)?.as_deref())?
        };
    let Some(record) = owned else { return Ok(None) };
    let Some(before) = storage::read(&destination.path)? else {
        return Ok(None);
    };
    let document = document::Prepared::parse(before.as_str(), destination.format)?;
    let current = document.entry(destination.key, &context.product)?;
    // Migrate the observed entry, never the record's intent: an interrupted
    // setup's other changes stay unapplied, and an absent entry stays absent.
    let Some(root) = previous_root.to_str() else {
        return Ok(None);
    };
    let Some(observed) = current
        .as_ref()
        .filter(|entry| entry["cwd"].as_str() == Some(root))
    else {
        return Ok(None);
    };
    let Some(desired) = without_cwd(observed) else {
        return Ok(None);
    };
    // A pending record whose file still holds its previous entry is accepted
    // only when that interrupted change involved nothing but cwd.
    let recovering = record.pending
        && current == record.previous
        && record
            .desired
            .as_ref()
            .is_some_and(|intent| without_cwd(intent).as_ref() == Some(&desired));
    if current != record.desired && !recovering {
        return Ok(None);
    }
    let (_, after) = document.edit(destination.key, &context.product, Some(&desired))?;
    Ok(Some(Change {
        kind: Kind::Server,
        agents: destination.surfaces,
        path: destination.path.clone(),
        before: Some(before),
        after: Some(after),
        ledger: new_ledger,
        ledger_before,
        record: storage::Record {
            version: 1,
            path: destination.path,
            product: context.product.clone(),
            previous: current,
            desired: Some(desired),
            pending: record.pending,
        },
        ownership_unchanged: false,
        launch: None,
    }))
}

fn without_cwd(entry: &serde_json::Value) -> Option<serde_json::Value> {
    let mut entry = entry.clone();
    entry.as_object_mut()?.remove("cwd");
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, SetupPolicy, Tool, prepare_with_policy};
    use serde_json::json;
    use std::{collections::BTreeMap, fs};

    fn installed() -> (tempfile::TempDir, Context, Options, SetupPolicy) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let context = Context {
            product: "acme".into(),
            program: root.join("acme"),
            arguments: vec!["serve".into()],
            launch_command: None,
            check_arguments: Vec::new(),
            root: root.join("original"),
            home: root.join("home"),
            state: root.join("state"),
            agent_homes: BTreeMap::new(),
            tools: vec![Tool::new("inspect", "Inspect", true)],
            prompt_hook: None,
        };
        fs::create_dir_all(&context.root).unwrap();
        fs::create_dir_all(&context.home).unwrap();
        let options = Options {
            agents: vec!["codex".into()],
            ..Options::default()
        };
        let policy = SetupPolicy {
            project_directory: true,
            ..SetupPolicy::default()
        };
        prepare_with_policy(&context, &options, policy)
            .unwrap()
            .apply()
            .unwrap();
        (temp, context, options, policy)
    }

    #[test]
    fn portable_registration_preserves_approvals_and_the_original_copy() {
        let (_temp, mut context, options, policy) = installed();
        let old = context.root.clone();
        let original = fs::read_to_string(old.join(".codex/config.toml")).unwrap();
        context.root = old.with_file_name("copy");
        fs::create_dir_all(context.root.join(".codex")).unwrap();
        fs::write(context.root.join(".codex/config.toml"), &original).unwrap();
        assert_eq!(relocate_project(&context, &old).unwrap().len(), 1);
        let copied = fs::read_to_string(context.root.join(".codex/config.toml")).unwrap();
        let (entry, _) =
            document::edit(&copied, crate::Format::Toml, "mcp_servers", "acme", None).unwrap();
        let entry = entry.unwrap();
        assert!(entry.get("cwd").is_none());
        assert_eq!(entry["tools"]["inspect"]["approval_mode"], json!("approve"));
        assert_eq!(
            fs::read_to_string(old.join(".codex/config.toml")).unwrap(),
            original
        );
        assert_eq!(
            relocate_project(&context, &old).unwrap(),
            Vec::<PathBuf>::new()
        );
        let portable = SetupPolicy {
            project_directory: false,
            ..policy
        };
        prepare_with_policy(&context, &options, portable)
            .unwrap()
            .apply()
            .unwrap();
    }

    #[test]
    fn portable_registration_removes_the_cwd_of_a_project_that_never_moved() {
        let (_temp, context, _, _) = installed();
        let path = context.root.join(".codex/config.toml");
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(context.root.to_str().unwrap())
        );
        assert_eq!(relocate_project(&context, &context.root).unwrap().len(), 1);
        let (entry, _) = document::edit(
            &fs::read_to_string(&path).unwrap(),
            crate::Format::Toml,
            "mcp_servers",
            "acme",
            None,
        )
        .unwrap();
        let entry = entry.unwrap();
        assert!(entry.get("cwd").is_none());
        assert_eq!(entry["tools"]["inspect"]["approval_mode"], json!("approve"));
        // Repeating it changes nothing.
        assert_eq!(
            relocate_project(&context, &context.root).unwrap(),
            Vec::<PathBuf>::new()
        );
    }

    fn entry(text: &str) -> Option<serde_json::Value> {
        document::edit(text, crate::Format::Toml, "mcp_servers", "acme", None)
            .unwrap()
            .0
    }

    /// Replace the ownership record with an interrupted publication from
    /// `previous` to `desired`.
    fn interrupt(
        context: &Context,
        previous: Option<serde_json::Value>,
        desired: Option<serde_json::Value>,
    ) {
        let path = context.root.join(".codex/config.toml");
        let ledger = ledger(context, &path);
        let before = storage::read(&ledger).unwrap();
        let record = storage::Record {
            version: 1,
            path,
            product: context.product.clone(),
            previous,
            desired,
            pending: true,
        };
        storage::write(
            &ledger,
            &serde_json::to_string(&record).unwrap(),
            true,
            before.as_deref(),
        )
        .unwrap();
    }

    #[test]
    fn portable_registration_does_not_complete_a_pending_installation() {
        let (_temp, context, _, _) = installed();
        let path = context.root.join(".codex/config.toml");
        let installed = entry(&fs::read_to_string(&path).unwrap());
        interrupt(&context, None, installed);
        let configuration = "model = \"example\"\n";
        fs::write(&path, configuration).unwrap();
        assert_eq!(
            relocate_project(&context, &context.root).unwrap(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), configuration);
    }

    #[test]
    fn portable_registration_recovers_only_cwd_changes_from_pending_setup() {
        for cwd_only in [false, true] {
            let (_temp, context, _, _) = installed();
            let path = context.root.join(".codex/config.toml");
            let configuration = fs::read_to_string(&path).unwrap();
            let observed = entry(&configuration).unwrap();
            let mut intent = observed.clone();
            if cwd_only {
                intent.as_object_mut().unwrap().remove("cwd");
            } else {
                intent["tools"]["inspect"]["approval_mode"] = json!("prompt");
            }
            interrupt(&context, Some(observed), Some(intent));
            let changed = relocate_project(&context, &context.root).unwrap();
            let after = fs::read_to_string(&path).unwrap();
            if cwd_only {
                assert_eq!(changed.len(), 1);
                let migrated = entry(&after).unwrap();
                assert!(migrated.get("cwd").is_none());
                assert_eq!(
                    migrated["tools"]["inspect"]["approval_mode"],
                    json!("approve")
                );
            } else {
                assert_eq!(changed, Vec::<PathBuf>::new());
                assert_eq!(after, configuration);
            }
        }
    }

    #[test]
    fn portable_registration_preserves_edited_and_unowned_entries() {
        for edited in [true, false] {
            let (_temp, mut context, _, _) = installed();
            let old = context.root.clone();
            let original = fs::read_to_string(old.join(".codex/config.toml")).unwrap();
            context.root = old.with_file_name("copy");
            fs::create_dir_all(context.root.join(".codex")).unwrap();
            let path = context.root.join(".codex/config.toml");
            let mut configuration = original.clone();
            if edited {
                let (entry, _) = document::edit(
                    &configuration,
                    crate::Format::Toml,
                    "mcp_servers",
                    "acme",
                    None,
                )
                .unwrap();
                let mut entry = entry.unwrap();
                entry["command"] = json!("manual-launcher");
                configuration = document::edit(
                    &configuration,
                    crate::Format::Toml,
                    "mcp_servers",
                    "acme",
                    Some(&entry),
                )
                .unwrap()
                .1;
            } else {
                context.state = context.state.join("another-user");
            }
            fs::write(&path, &configuration).unwrap();
            assert_eq!(
                relocate_project(&context, &old).unwrap(),
                Vec::<PathBuf>::new()
            );
            assert_eq!(fs::read_to_string(path).unwrap(), configuration);
            assert_eq!(
                fs::read_to_string(old.join(".codex/config.toml")).unwrap(),
                original
            );
        }
    }
}
