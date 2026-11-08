//! Expand the human review only for changes shared by retained installations.
use super::{Error, IntegrationChoice, Registration, box_definition};
use std::collections::BTreeSet;

pub(super) fn selection(
    choice: IntegrationChoice,
    previous: Option<&Registration>,
    fresh: Option<&Registration>,
    approved: &[String],
) -> Result<(IntegrationChoice, Option<String>), Error> {
    let IntegrationChoice::Agents(selected) = choice else {
        return Ok((choice, None));
    };
    let Some((previous, fresh)) = previous.zip(fresh) else {
        return Ok((choice, None));
    };
    let defaults = crate::review::mcp::default_approved();
    let saved = previous.approved_tools.as_deref().unwrap_or(&defaults);
    let tools_changed =
        saved.iter().collect::<BTreeSet<_>>() != approved.iter().collect::<BTreeSet<_>>();
    let launcher_changed = previous.binary != fresh.binary;
    if !launcher_changed && !tools_changed {
        return Ok((choice, None));
    }
    let mut retained: BTreeSet<&str> = previous.agents.keys().map(String::as_str).collect();
    retained.extend(previous.connections.keys().map(String::as_str));
    if previous.box_definition.is_some() {
        retained.insert(box_definition::ID);
    }
    if previous.interceptor.is_some() {
        retained.insert("claude");
    }
    let expanded = selected | IntegrationChoice::bits(&retained.into_iter().collect::<Vec<_>>())?;
    if expanded == selected {
        return Ok((choice, None));
    }
    let reason = match (launcher_changed, tools_changed) {
        (true, true) => "The shared Shield launcher and tool approvals changed.",
        (true, false) => "The shared Shield launcher changed.",
        (false, true) => "The shared Shield tool approvals changed.",
        (false, false) => unreachable!("shared change checked above"),
    };
    Ok((
        IntegrationChoice::Agents(expanded),
        Some(format!(
            "{reason} This review includes every retained integration affected by those settings."
        )),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registration() -> Registration {
        let root = assert_fs::TempDir::new().unwrap();
        let mut registration = super::super::desired(root.path()).unwrap();
        registration.agents.insert(
            "claude".into(),
            serde_json::from_value(json!({"groups":{}})).unwrap(),
        );
        registration.agents.insert(
            "codex".into(),
            serde_json::from_value(json!({"groups":{}})).unwrap(),
        );
        registration
    }

    #[test]
    fn scope_ignores_advisory_binary_metadata_and_tool_order() {
        let mut previous = registration();
        let mut fresh = previous.clone();
        fresh.binary_digest = "changed".into();
        fresh.version = "another-version".into();
        let mut tools = crate::review::mcp::default_approved();
        previous.approved_tools = Some(tools.clone());
        tools.reverse();
        let (choice, reason) = selection(
            IntegrationChoice::agent("claude"),
            Some(&previous),
            Some(&fresh),
            &tools,
        )
        .unwrap();
        assert!(choice.contains("claude"));
        assert!(!choice.contains("codex"));
        assert!(reason.is_none());
    }

    #[test]
    fn scope_shared_launcher_and_tool_changes_include_omitted_installations() {
        let previous = registration();
        for change in ["path", "tools"] {
            let mut fresh = previous.clone();
            let mut tools = crate::review::mcp::default_approved();
            match change {
                "path" => fresh.binary = fresh.binary.with_file_name("another-shield"),
                "tools" => {
                    tools.pop();
                }
                _ => unreachable!(),
            }
            let (choice, reason) = selection(
                IntegrationChoice::agent("claude"),
                Some(&previous),
                Some(&fresh),
                &tools,
            )
            .unwrap();
            assert!(choice.contains("claude"));
            assert!(choice.contains("codex"));
            assert!(reason.is_some());
        }
    }

    #[test]
    fn scope_removal_never_migrates_omitted_installations() {
        let previous = registration();
        let mut fresh = previous.clone();
        fresh.binary = fresh.binary.with_file_name("another-shield");
        let selected =
            IntegrationChoice::RemoveAgents(IntegrationChoice::bits(&["claude"]).unwrap());
        let (choice, reason) = selection(selected, Some(&previous), Some(&fresh), &[]).unwrap();
        assert!(matches!(choice, IntegrationChoice::RemoveAgents(_)));
        assert!(choice.contains("claude"));
        assert!(!choice.contains("codex"));
        assert!(reason.is_none());
    }

    #[test]
    fn scope_entry_template_change_does_not_expand_nonshared_consumers() {
        let previous = registration();
        let mut fresh = previous.clone();
        fresh.entry = json!({"command":"updated-shared-template"});
        let tools = crate::review::mcp::default_approved();
        let (choice, reason) = selection(
            IntegrationChoice::agent("claude"),
            Some(&previous),
            Some(&fresh),
            &tools,
        )
        .unwrap();
        assert!(choice.contains("claude"));
        assert!(!choice.contains("codex"));
        assert!(reason.is_none());
    }
}
