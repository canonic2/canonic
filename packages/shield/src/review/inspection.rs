//! Shared owner-qualified project rule inspection for CLI and MCP.
use super::{Context, Error, project};
use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(tag = "scope", rename_all = "lowercase", deny_unknown_fields)]
enum RuleRef {
    Custom {
        rule_id: String,
    },
    Pack {
        pack_selector: String,
        rule_id: String,
    },
}

impl RuleRef {
    fn selects(&self, row: &Value) -> bool {
        match self {
            Self::Custom { rule_id } => {
                row["rule_ref"]["scope"] == "custom" && row["id"] == *rule_id
            }
            Self::Pack {
                pack_selector,
                rule_id,
            } => {
                row["rule_ref"]["scope"] == "pack"
                    && row["rule_ref"]["pack_selector"] == *pack_selector
                    && row["id"] == *rule_id
            }
        }
    }
}

/// Lists or selects rules of the verified review state, for the operator CLI.
pub(super) fn rules(context: &Context, selector: Option<&Value>) -> Result<Value, Error> {
    let state = project::load(context)?;
    captured_rules(
        context,
        &state.candidates["project"],
        &state.operator,
        &state.policy_revision,
        selector,
    )
}

/// Lists or selects rules of one already validated captured candidate.
///
/// MCP supplies the selected retained capture; CLI uses the current review state.
/// This constructs no evaluator: a lookup reads only the requested pack.
pub(super) fn captured_rules(
    context: &Context,
    candidate: &Value,
    metadata: &Value,
    revision: &str,
    selector: Option<&Value>,
) -> Result<Value, Error> {
    let selected: Option<RuleRef> = selector
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| Error::Invalid("invalid rule reference"))
        })
        .transpose()?;
    let mut rows = if matches!(selected.as_ref(), Some(RuleRef::Pack { .. })) {
        Vec::new()
    } else {
        layered_custom_rows(candidate, &context.project_root)?
    };
    let mut shared = Vec::new();
    for (index, pack) in metadata["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if selected.as_ref().is_some_and(|s| !matches!(s, RuleRef::Pack { pack_selector, .. } if *pack_selector == pack["selector"])) {
            continue;
        }
        let artifact = candidate["packs"][index]
            .as_str()
            .ok_or(Error::Invalid("pack bytes missing"))?;
        let (doc, chosen) = if let Some(RuleRef::Pack { rule_id, .. }) = &selected {
            (
                Value::Null,
                crate::policy::inspect_pack_rule(artifact, rule_id)?,
            )
        } else {
            (crate::policy::parse::<Value>(artifact)?, None)
        };
        if selected.is_some() && chosen.is_none() {
            return Err(Error::Missing);
        }
        if !doc["invocations"].is_null() {
            shared.push(json!({"pack_selector":pack["selector"],"invocations":doc["invocations"],"command_sets":doc.get("command_sets").cloned().unwrap_or_else(|| json!({}))}));
        }
        let gates: Vec<Value> = doc["preconditions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|gate| {
                let mut gate = gate.clone();
                gate["kind"] = json!("precondition");
                gate
            })
            .collect();
        let rules = chosen.as_ref().map_or_else(
            || {
                doc["rules"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .chain(&gates)
                    .collect::<Vec<_>>()
            },
            |rule| vec![rule],
        );
        let resolution = resolution(artifact, candidate, index)?;
        for rule in rules {
            let mut rule = public_pack_rule(rule, &pack["profile"]);
            rule["rule_ref"] =
                json!({"scope":"pack","pack_selector":pack["selector"],"rule_id":rule["id"]});
            rule["source"] = pack.clone();
            rule["profile"] = pack["profile"].clone();
            if let Some(resolved) = rule["id"].as_str().and_then(|id| resolution.get(id)) {
                rule["resolution"] = resolved.clone();
            }
            rows.push(rule);
        }
    }
    if let Some(selector) = selected {
        return rows
            .into_iter()
            .find(|row| selector.selects(row))
            .ok_or(Error::Missing);
    }
    let mut result = json!({"policy_revision":revision,"rules":rows});
    if !shared.is_empty() {
        result["shared"] = json!(shared);
    }
    Ok(result)
}

/// Local rules replace shared rules with the same ID while retaining their position.
fn layered_custom_rows(candidate: &Value, project: &str) -> Result<Vec<Value>, Error> {
    let mut rows = Vec::new();
    let mut positions = std::collections::BTreeMap::<String, usize>::new();
    for source in [
        candidate["rules"].as_str(),
        candidate["local_rules"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        for row in custom_rows(source, project)? {
            let id = row["id"]
                .as_str()
                .ok_or(Error::Invalid("rule ID missing"))?;
            if let Some(&index) = positions.get(id) {
                rows[index] = row;
            } else {
                positions.insert(id.to_owned(), rows.len());
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

fn custom_rows(source: &str, project: &str) -> Result<Vec<Value>, Error> {
    let doc = project_rules(source, project)?;
    Ok(doc["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let mut row = row.clone();
            row["rule_ref"] = json!({"scope":"custom","rule_id":row["id"]});
            row["source"] = json!("custom");
            row["policy_schema_version"] = json!(1);
            row
        })
        .collect())
}

/// Variable values and overrides of the candidate's selection at `index`.
/// How a selection's entries resolve, with the layer that supplied each value.
fn resolution(
    artifact: &str,
    candidate: &Value,
    index: usize,
) -> Result<std::collections::BTreeMap<String, Value>, Error> {
    let settings = selection_settings(candidate, index)?;
    let mut resolution = crate::policy::inspect_resolution(artifact, &settings.effective())?;
    for entry in resolution.values_mut() {
        let layers: serde_json::Map<String, Value> = entry["variables"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, _)| (name.clone(), json!(settings.layer(name))))
            .collect();
        if !layers.is_empty() {
            entry["layers"] = Value::Object(layers);
        }
    }
    Ok(resolution)
}

/// One selection's values and overrides by layer.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Settings {
    /// The selection's own values: shared, or the local-only selection's.
    pub(super) shared: crate::policy::Binding,
    /// The local overlay of a shared selection; None for local-only selections.
    pub(super) local: Option<crate::configuration::layers::Overlay>,
}

impl Settings {
    pub(super) fn effective(&self) -> crate::policy::Binding {
        let mut binding = self.shared.clone();
        if let Some(overlay) = &self.local {
            overlay.apply(&mut binding);
        }
        binding
    }

    /// The layer that supplies a variable's value: local, project or default.
    pub(super) fn layer(&self, name: &str) -> &'static str {
        if self
            .local
            .as_ref()
            .is_some_and(|o| o.variables.contains_key(name))
        {
            "local"
        } else if self.shared.variables.contains_key(name)
            && !self
                .local
                .as_ref()
                .is_some_and(|o| o.unset.iter().any(|n| n == name))
        {
            "project"
        } else {
            "default"
        }
    }

    /// Change the effective binding in one layer: local edits become the
    /// overlay; project edits change the shared values and keep the overlay.
    pub(super) fn edit(
        &mut self,
        local: bool,
        change: impl FnOnce(&mut crate::policy::Binding) -> Result<(), Error>,
    ) -> Result<(), Error> {
        match (&mut self.local, local) {
            (Some(_), true) => {
                let mut effective = self.effective();
                change(&mut effective)?;
                self.local = Some(crate::configuration::layers::Overlay::between(
                    &self.shared,
                    &effective,
                ));
            }
            _ => change(&mut self.shared)?,
        }
        Ok(())
    }
}

/// The candidate's selection at `index`, by layer.
pub(super) fn selection_settings(candidate: &Value, index: usize) -> Result<Settings, Error> {
    let config = if candidate["config"].is_string() {
        crate::configuration::layers::merged(candidate)?
    } else {
        Value::Null
    };
    let selection = &config["packs"][index];
    let mut binding = json!({});
    for field in ["variables", "overrides"] {
        if !selection[field].is_null() {
            binding[field] = selection[field].clone();
        }
    }
    let invalid = |_| Error::Invalid("invalid pack variables");
    Ok(Settings {
        shared: serde_json::from_value(binding).map_err(invalid)?,
        local: if selection["layer"] == "local" {
            None
        } else if selection["local"].is_null() {
            Some(crate::configuration::layers::Overlay::default())
        } else {
            Some(serde_json::from_value(selection["local"].clone()).map_err(invalid)?)
        },
    })
}

pub(super) fn project_rules(source: &str, _project: &str) -> Result<Value, Error> {
    crate::policy::CustomRules::from_yaml(source)?;
    crate::policy::parse(source).map_err(Into::into)
}
pub(super) fn public_pack_rule(rule: &Value, profile: &Value) -> Value {
    let mut result = rule.clone();
    result["policy_schema_version"] = json!(1);
    result["selected_profile"] = profile.clone();
    result["effective_decision"] = if rule["decision"].is_string() {
        rule["decision"].clone()
    } else {
        rule["decision"][profile.as_str().unwrap_or("recommended")].clone()
    };
    if let Some(unmet) = rule.get("on_unmet") {
        result["effective_on_unmet_decision"] = if unmet["decision"].is_string() {
            unmet["decision"].clone()
        } else {
            unmet["decision"][profile.as_str().unwrap_or("recommended")].clone()
        };
    }
    result
}
