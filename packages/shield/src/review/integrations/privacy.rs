//! Private connections use opaque launchers or retain an exact native entry.
use super::{Error, Path, Plan, verify_mcp_connection};

pub(super) fn validate(root: &Path, plan: &Plan) -> Result<(), Error> {
    if let Some(desired) = &plan.desired {
        for (agent, owned) in &desired.connections {
            if !plan.affects(agent) {
                continue;
            }
            for row in owned["review"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["source"] == "local" && row["enabled"] == true)
            {
                if super::rebind::unchanged(
                    plan,
                    agent,
                    row["id"].as_str().unwrap_or_default(),
                    row["alias"].as_str().unwrap_or_default(),
                ) {
                    continue;
                }
                if row["projection"] == "existing native entry" {
                    let alias = row["alias"]
                        .as_str()
                        .ok_or(Error::Invalid("invalid native MCP alias"))?;
                    let transport = serde_json::from_value(row["settings"].clone())
                        .map_err(|_| Error::Invalid("invalid native MCP transport"))?;
                    // Retained native settings are never copied into host output.
                    // Recheck identity and competing scopes before publication.
                    verify_mcp_connection(root, agent, alias, &transport)?;
                    continue;
                }
                let id = row["id"]
                    .as_str()
                    .ok_or(Error::Invalid("invalid private MCP review record"))?;
                let binding: crate::mcp_connections::launcher::Binding =
                    serde_json::from_value(owned["launches"][id].clone()).map_err(|_| {
                        Error::Invalid("private MCP launch binding needs fresh reviewed setup")
                    })?;
                if !crate::mcp_connections::launcher::valid_token(&binding.token) {
                    return Err(Error::Invalid("invalid private MCP launch token"));
                }
                crate::mcp_connections::launcher::private_state(root)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_private_bindings_are_preserved_without_revalidation() {
        let root = assert_fs::TempDir::new().unwrap();
        let mut plan = super::super::setup::prepare_initial(
            root.path(),
            super::super::IntegrationChoice::agent("codex"),
        )
        .unwrap()
        .unwrap();
        plan.desired.as_mut().unwrap().connections.insert(
            "codex".into(),
            serde_json::json!({"review":[{"id":"local","source":"local","enabled":true}],"launches":{}}),
        );
        plan.targets = Some(vec!["claude".into()]);
        validate(root.path(), &plan).unwrap();
        plan.targets = Some(vec!["codex".into()]);
        assert!(matches!(
            validate(root.path(), &plan),
            Err(Error::Invalid(
                "private MCP launch binding needs fresh reviewed setup"
            ))
        ));
        plan.targets = None;
        assert!(validate(root.path(), &plan).is_err());
    }
}
