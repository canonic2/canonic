//! Persisted maintenance scope cannot widen while a reviewed plan resumes.
use super::{Error, Plan, Registration, agents, box_definition, legacy};
use std::collections::BTreeSet;

#[cfg(test)]
mod status_tests;

impl Plan {
    /// Equivalent maintenance never authorizes a different executable payload,
    /// regardless of which automatic or explicit resume path reaches it.
    pub(super) fn check_maintenance_binary(&self) -> Result<(), Error> {
        if self.scope_reason.as_deref() == Some(super::portable::ADOPT_REASON) {
            let Some(desired) = self.desired.as_ref().filter(|_| self.previous.is_none()) else {
                return Err(Error::StaleSnapshot);
            };
            if super::binary_digest(&std::env::current_exe()?.canonicalize()?)?
                != desired.binary_digest
            {
                return Err(Error::Invalid(
                    "automatic maintenance executable changed; review setup",
                ));
            }
            return Ok(());
        }
        if !matches!(
            self.scope_reason.as_deref(),
            Some(super::portable::REASON | super::rebind::REASON)
        ) {
            return Ok(());
        }
        let Some(expected) = self
            .previous
            .as_ref()
            .zip(self.desired.as_ref())
            .filter(|(before, after)| before.binary_digest == after.binary_digest)
            .map(|(_, after)| after.binary_digest.as_str())
        else {
            return Err(Error::StaleSnapshot);
        };
        if super::binary_digest(&std::env::current_exe()?.canonicalize()?)? != expected {
            return Err(Error::Invalid(
                "automatic maintenance executable changed; review setup",
            ));
        }
        Ok(())
    }
    pub(in crate::review) fn affects(&self, agent: &str) -> bool {
        self.targets.as_ref().is_none_or(|targets| {
            targets.iter().any(|target| target == agent)
                || (targets.iter().any(|target| target == box_definition::ID)
                    && agents::find(agent).is_some_and(agents::AgentAdapter::supports_box_hooks))
        })
    }

    pub(in crate::review) fn touches_shared(&self) -> bool {
        self.targets.is_none()
            || agents::ADAPTERS
                .iter()
                .any(|adapter| self.affects(adapter.spec().id) && adapter.spec().mcp_path.is_none())
    }
}

pub(super) fn validate(plan: &Plan) -> Result<(), Error> {
    let Some(targets) = &plan.targets else {
        return Ok(());
    };
    let unique: BTreeSet<_> = targets.iter().collect();
    if targets.is_empty()
        || unique.len() != targets.len()
        || targets
            .iter()
            .any(|id| id != box_definition::ID && agents::find(id).is_none())
    {
        return Err(Error::Invalid("invalid integration maintenance targets"));
    }
    for adapter in agents::ADAPTERS {
        let id = adapter.spec().id;
        if plan.affects(id) {
            continue;
        }
        let old = plan.previous.as_ref();
        let new = plan.desired.as_ref();
        if old.and_then(|r| r.agents.get(id)) != new.and_then(|r| r.agents.get(id))
            || old.and_then(|r| r.connections.get(id)) != new.and_then(|r| r.connections.get(id))
            || old.and_then(|r| r.ask_fallback.get(id)) != new.and_then(|r| r.ask_fallback.get(id))
            || (id == "claude"
                && old.and_then(|r| r.interceptor.as_ref())
                    != new.and_then(|r| r.interceptor.as_ref()))
        {
            return Err(Error::Invalid(
                "maintenance changed an omitted agent's ownership",
            ));
        }
        if old.is_some_and(|r| r.covers(id) || r.connections.contains_key(id))
            && shared_changed(old, new)
        {
            return Err(Error::Invalid(
                "shared integration settings require broader review",
            ));
        }
        if id == "claude"
            && old.is_some_and(|r| r.agents.contains_key(id) || r.interceptor.is_some())
            && old.map(|r| &r.entry) != new.map(|r| &r.entry)
        {
            return Err(Error::Invalid(
                "maintenance changed an omitted shared MCP entry",
            ));
        }
    }
    if !plan.affects(box_definition::ID) {
        let old = plan
            .previous
            .as_ref()
            .and_then(|r| r.box_definition.as_ref());
        let new = plan
            .desired
            .as_ref()
            .and_then(|r| r.box_definition.as_ref());
        if old != new
            || (old.is_some() && shared_changed(plan.previous.as_ref(), plan.desired.as_ref()))
        {
            return Err(Error::Invalid("maintenance changed omitted Box settings"));
        }
    }
    if !plan.touches_shared() && (plan.before.is_some() || !plan.after.is_empty()) {
        return Err(Error::Invalid(
            "maintenance captured an omitted shared MCP file",
        ));
    }
    if plan
        .files
        .iter()
        .any(|change| !allowed_path(plan, &change.path))
    {
        return Err(Error::Invalid(
            "maintenance captured an omitted integration file",
        ));
    }
    Ok(())
}

fn shared_changed(old: Option<&Registration>, new: Option<&Registration>) -> bool {
    let Some((old, new)) = old.zip(new) else {
        return old.is_some() != new.is_some();
    };
    let defaults = crate::review::mcp::default_approved();
    old.binary != new.binary
        || old
            .approved_tools
            .as_deref()
            .unwrap_or(&defaults)
            .iter()
            .collect::<BTreeSet<_>>()
            != new
                .approved_tools
                .as_deref()
                .unwrap_or(&defaults)
                .iter()
                .collect::<BTreeSet<_>>()
}

fn allowed_path(plan: &Plan, path: &str) -> bool {
    if [box_definition::PATH, box_definition::LOCAL_PATH].contains(&path) {
        return plan.affects(box_definition::ID);
    }
    if legacy::asset_path(path) {
        return plan.affects("claude");
    }
    agents::ADAPTERS.iter().any(|adapter| {
        let spec = adapter.spec();
        plan.affects(spec.id)
            && (spec.hooks_path == path
                || spec.mcp_path == Some(path)
                || spec.prefix_rules_path == Some(path)
                || spec.settings_paths.contains(&path))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan() -> Plan {
        let root = assert_fs::TempDir::new().unwrap();
        let mut registration = super::super::setup::desired(root.path()).unwrap();
        for agent in ["claude", "codex"] {
            registration.agents.insert(
                agent.into(),
                serde_json::from_value(json!({"groups":{}})).unwrap(),
            );
        }
        Plan {
            policy_revision: None,
            before: None,
            after: String::new(),
            previous: Some(registration.clone()),
            desired: Some(registration),
            files: Vec::new(),
            handed_over: Vec::new(),
            targets: Some(vec!["codex".into()]),
            scope_reason: None,
        }
    }

    #[test]
    fn maintenance_scope_preserves_omitted_records_and_ignores_advisory_binary_metadata() {
        let mut plan = plan();
        plan.desired.as_mut().unwrap().binary_digest = "replacement".into();
        plan.desired.as_mut().unwrap().version = "replacement".into();
        assert!(validate(&plan).is_ok());
        assert!(!plan.affects("claude"));
        assert!(!plan.touches_shared());
        plan.desired.as_mut().unwrap().agents.remove("claude");
        assert!(validate(&plan).is_err());
    }

    #[test]
    fn maintenance_scope_refuses_forged_targets_files_and_shared_changes() {
        let mut invalid = plan();
        invalid.targets = Some(vec!["codex".into(), "codex".into()]);
        assert!(validate(&invalid).is_err());
        invalid.targets = Some(vec!["unknown".into()]);
        assert!(validate(&invalid).is_err());
        let mut changed = plan();
        changed.desired.as_mut().unwrap().binary = std::path::PathBuf::from("another-shield");
        assert!(validate(&changed).is_err());
        let mut changed = plan();
        changed.files.push(super::super::files::Change {
            path: ".claude/settings.local.json".into(),
            before: None,
            after: Some("{}".into()),
        });
        assert!(validate(&changed).is_err());
        let mut changed = plan();
        changed.before = Some("{}".into());
        assert!(validate(&changed).is_err());
    }

    #[test]
    fn maintenance_scope_refuses_omitted_approval_connection_and_fallback_changes() {
        let mut changed = plan();
        changed.desired.as_mut().unwrap().approved_tools = Some(Vec::new());
        assert!(validate(&changed).is_err());
        let mut changed = plan();
        changed
            .desired
            .as_mut()
            .unwrap()
            .connections
            .insert("claude".into(), json!({"entries":{}}));
        assert!(validate(&changed).is_err());
        let mut changed = plan();
        changed
            .desired
            .as_mut()
            .unwrap()
            .ask_fallback
            .insert("claude".into(), super::super::AskFallback::Allow);
        assert!(validate(&changed).is_err());
    }

    #[test]
    fn maintenance_historical_full_scope_and_explicit_box_scope_remain_available() {
        let mut historic = plan();
        historic.targets = None;
        assert!(historic.affects("claude"));
        assert!(historic.touches_shared());
        assert!(validate(&historic).is_ok());
        let mut boxed = plan();
        boxed.targets = Some(vec![box_definition::ID.into()]);
        assert!(boxed.affects("claude"));
        assert!(boxed.touches_shared());
        assert!(validate(&boxed).is_ok());
    }
}
