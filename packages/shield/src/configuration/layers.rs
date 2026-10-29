//! The uncommitted local layer: merging `config.local.yml` over the shared
//! configuration into one candidate document, and splitting it back into files.
//!
//! The merged document is the shared one, with local-only selections marked
//! `layer: local`, personal values and overrides of shared selections in a
//! `local` overlay, shared selections the local layer disables under
//! `disabled`, and local rules and roots under `local_rules` and
//! `local_access_paths`. Without a local file it is the shared document itself.
use super::AccessPaths;
use crate::policy::{self, Error, Override, Profile};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// Personal changes to one shared selection.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Overlay {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) variables: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) unset: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) overrides: BTreeMap<String, Override>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) clear_overrides: Vec<String>,
}

impl Overlay {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Apply this overlay to a shared binding.
    pub(crate) fn apply(&self, binding: &mut policy::Binding) {
        for name in &self.unset {
            binding.variables.remove(name);
        }
        binding.variables.extend(self.variables.clone());
        for id in &self.clear_overrides {
            binding.overrides.remove(id);
        }
        binding.overrides.extend(self.overrides.clone());
    }

    /// The overlay that turns `shared` into `effective`.
    pub(crate) fn between(shared: &policy::Binding, effective: &policy::Binding) -> Self {
        let mut overlay = Self::default();
        for (name, value) in &effective.variables {
            if shared.variables.get(name) != Some(value) {
                overlay.variables.insert(name.clone(), value.clone());
            }
        }
        overlay.unset = shared
            .variables
            .keys()
            .filter(|name| !effective.variables.contains_key(*name))
            .cloned()
            .collect();
        for (id, change) in &effective.overrides {
            if shared.overrides.get(id) != Some(change) {
                overlay.overrides.insert(id.clone(), change.clone());
            }
        }
        overlay.clear_overrides = shared
            .overrides
            .keys()
            .filter(|id| !effective.overrides.contains_key(*id))
            .cloned()
            .collect();
        overlay
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalDocument {
    schema_version: u8,
    #[serde(default)]
    packs: Vec<LocalEntry>,
    #[serde(default)]
    rules: Option<String>,
    #[serde(default)]
    access_paths: AccessPaths,
    #[serde(default)]
    mcp: crate::mcp_connections::Connections,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalEntry {
    id: String,
    #[serde(default)]
    registry_identity: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    profile: Option<Profile>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    variables: BTreeMap<String, Value>,
    #[serde(default)]
    overrides: BTreeMap<String, Override>,
    #[serde(default)]
    unset: Vec<String>,
    #[serde(default)]
    clear_overrides: Vec<String>,
    #[serde(default)]
    disabled: bool,
}

fn same(entry: &Value, id: &str, registry: Option<&str>) -> bool {
    entry["id"] == id && registry.is_none_or(|r| entry["registry_identity"] == r)
}

/// Merge the local document, when present, over the shared one.
///
/// # Errors
/// Refuses invalid local documents, entries that match no shared selection yet
/// lack a complete selection, entries that match several, entries naming a
/// selection an earlier entry already named, and shared
/// selections whose version, profile or path a local entry tries to change.
pub(crate) fn merge(shared: &str, local: Option<&str>) -> Result<String, Error> {
    let Some(local) = local else {
        return Ok(shared.to_owned());
    };
    let mut document: Value = policy::parse(shared)?;
    let local: LocalDocument = policy::parse(local)?;
    if local.schema_version != 1 {
        return Err(Error::Contract(
            "unsupported local configuration schema version",
        ));
    }
    if local.packs.len() > 64 {
        return Err(Error::Limit);
    }
    let packs = document["packs"]
        .as_array_mut()
        .ok_or(Error::Contract("configuration packs required"))?;
    // Rows a local entry already named, shared or local-only. Disabled rows
    // leave the list after every entry has matched, so indices stay stable.
    let mut named = Vec::new();
    let mut disabled = Vec::new();
    for entry in local.packs {
        let matches: Vec<usize> = packs
            .iter()
            .enumerate()
            .filter(|(_, p)| same(p, &entry.id, entry.registry_identity.as_deref()))
            .map(|(i, _)| i)
            .collect();
        if matches.iter().any(|index| named.contains(index)) {
            return Err(Error::Contract(
                "several local entries name one selection; combine them into one entry",
            ));
        }
        match matches.as_slice() {
            [index] => {
                named.push(*index);
                if entry.version.is_some() || entry.profile.is_some() || entry.path.is_some() {
                    return Err(Error::Contract(
                        "local entries for shared selections change only variables, overrides or disabled",
                    ));
                }
                if entry.disabled {
                    disabled.push(*index);
                    continue;
                }
                let overlay = Overlay {
                    variables: entry.variables,
                    unset: entry.unset,
                    overrides: entry.overrides,
                    clear_overrides: entry.clear_overrides,
                };
                if !overlay.is_empty() {
                    packs[*index]["local"] = json!(overlay);
                }
            }
            [] => {
                let (Some(version), Some(profile), Some(path)) =
                    (entry.version, entry.profile, entry.path)
                else {
                    return Err(Error::Contract(
                        "a local entry names no shared selection and is not a complete selection",
                    ));
                };
                if entry.disabled || !entry.unset.is_empty() || !entry.clear_overrides.is_empty() {
                    return Err(Error::Contract(
                        "local-only selections hold their own variables and overrides",
                    ));
                }
                let mut selection = json!({"id":entry.id,"version":version,"profile":profile,"path":path,"layer":"local"});
                if let Some(registry) = entry.registry_identity {
                    selection["registry_identity"] = json!(registry);
                }
                if !entry.variables.is_empty() {
                    selection["variables"] = json!(entry.variables);
                }
                if !entry.overrides.is_empty() {
                    selection["overrides"] = json!(entry.overrides);
                }
                named.push(packs.len());
                packs.push(selection);
            }
            _ => {
                return Err(Error::Contract(
                    "a local entry matches several shared selections; add registry_identity",
                ));
            }
        }
    }
    // In local entry order; removal runs from the end of the list.
    let rows: Vec<Value> = disabled.iter().map(|index| packs[*index].clone()).collect();
    disabled.sort_unstable();
    for index in disabled.into_iter().rev() {
        packs.remove(index);
    }
    if !rows.is_empty() {
        document["disabled"] = json!(rows);
    }
    extra_layers(&mut document, local.rules, &local.access_paths, &local.mcp);
    Ok(document.to_string())
}

fn extra_layers(
    document: &mut Value,
    rules: Option<String>,
    access_paths: &AccessPaths,
    mcp: &crate::mcp_connections::Connections,
) {
    if let Some(rules) = rules {
        document["local_rules"] = json!(rules);
    }
    if !access_paths.read.is_empty() || !access_paths.write.is_empty() {
        document["local_access_paths"] = json!(access_paths);
    }
    if !mcp.is_empty() {
        document["local_mcp"] = json!(mcp);
    }
}

/// The merged configuration of a retained candidate: its shared `config`
/// source with its optional `local_config` source applied.
///
/// # Errors
/// Refuses candidates without a configuration source and invalid layers.
pub(crate) fn merged(candidate: &Value) -> Result<Value, Error> {
    let shared = candidate["config"]
        .as_str()
        .ok_or(Error::Contract("candidate configuration missing"))?;
    policy::parse(&merge(shared, candidate["local_config"].as_str())?)
}

/// Split a merged document into the shared document and, when it has local
/// content, the local document. `previous` holds the shared and local sources
/// the layers replace: a selection the local layer disables returns to its
/// place in the shared list, local entries keep their order, and an entry
/// written without `registry_identity` stays without it while one shared
/// selection has its id. Splitting a merged pair yields the same layers.
///
/// # Errors
/// Refuses documents that are not configuration objects, and invalid
/// previous sources.
pub(crate) fn split(
    merged: &str,
    previous: (Option<&str>, Option<&str>),
) -> Result<(String, Option<String>), Error> {
    let rows = |source: Option<&str>| -> Result<Vec<Value>, Error> {
        Ok(source
            .map(policy::parse::<Value>)
            .transpose()?
            .and_then(|document| document["packs"].as_array().cloned())
            .unwrap_or_default())
    };
    let (shared_before, local_before) = (rows(previous.0)?, rows(previous.1)?);
    let mut document: Value = policy::parse(merged)?;
    let fields = document
        .as_object_mut()
        .ok_or(Error::Contract("configuration object required"))?;
    let disabled = fields.remove("disabled");
    let rules = fields.remove("local_rules");
    let access_paths = fields.remove("local_access_paths");
    let mcp = fields.remove("local_mcp");
    // Local entries, each marked when it only names a shared selection.
    let mut entries: Vec<(Value, bool)> = Vec::new();
    let mut shared = Vec::new();
    for mut selection in fields
        .remove("packs")
        .and_then(|p| p.as_array().cloned())
        .unwrap_or_default()
    {
        let row = selection.as_object_mut().ok_or(Error::Syntax)?;
        if row.remove("layer").is_some() {
            entries.push((Value::Object(row.clone()), false));
            continue;
        }
        if let Some(Value::Object(overlay)) = row.remove("local") {
            let mut entry = identity(row);
            entry.extend(overlay);
            entries.push((Value::Object(entry), true));
        }
        shared.push(Value::Object(row.clone()));
    }
    let shared_position = |row: &Value| {
        shared_before.iter().position(|before| {
            before["id"] == row["id"] && before["registry_identity"] == row["registry_identity"]
        })
    };
    for selection in disabled
        .and_then(|d| d.as_array().cloned())
        .unwrap_or_default()
    {
        let row = selection.as_object().ok_or(Error::Syntax)?;
        let mut entry = identity(row);
        entry.insert("disabled".into(), json!(true));
        entries.push((Value::Object(entry), true));
        // Before the first selection that came after it, or a new one.
        let at = shared_position(&selection).map_or(shared.len(), |place| {
            shared
                .iter()
                .position(|row| shared_position(row).is_none_or(|other| other > place))
                .unwrap_or(shared.len())
        });
        shared.insert(at, selection);
    }
    let entries = as_written(entries, &local_before, &shared);
    fields.insert("packs".into(), json!(shared));
    let shared = document.to_string();
    if entries.is_empty() && rules.is_none() && access_paths.is_none() && mcp.is_none() {
        return Ok((shared, None));
    }
    let mut local = json!({"schema_version":1,"packs":entries});
    if let Some(mcp) = mcp {
        local["mcp"] = mcp;
    }
    if let Some(rules) = rules {
        local["rules"] = rules;
    }
    if let Some(Value::Object(mut paths)) = access_paths {
        // The merge writes both lists; a layer keeps only those it has.
        paths.retain(|_, list| list.as_array().is_none_or(|list| !list.is_empty()));
        local["access_paths"] = Value::Object(paths);
    }
    Ok((shared, Some(local.to_string())))
}

/// Local entries in the previous local file's order, new ones after it. An
/// entry that only names a shared selection drops the `registry_identity` its
/// previous entry was written without, while the id stays unambiguous.
fn as_written(mut entries: Vec<(Value, bool)>, before: &[Value], shared: &[Value]) -> Vec<Value> {
    let previous = |entry: &Value| {
        before.iter().position(|written| {
            written["id"] == entry["id"]
                && (written["registry_identity"].is_null()
                    || written["registry_identity"] == entry["registry_identity"])
        })
    };
    // A stable sort keeps new entries in merged order.
    entries.sort_by_key(|(entry, _)| previous(entry).unwrap_or(usize::MAX));
    entries
        .into_iter()
        .map(|(mut entry, names_shared)| {
            let unwritten =
                previous(&entry).is_some_and(|index| before[index]["registry_identity"].is_null());
            let unique = shared.iter().filter(|row| row["id"] == entry["id"]).count() == 1;
            if names_shared
                && unwritten
                && unique
                && let Some(fields) = entry.as_object_mut()
            {
                fields.remove("registry_identity");
            }
            entry
        })
        .collect()
}

fn identity(row: &Map<String, Value>) -> Map<String, Value> {
    let mut entry = Map::new();
    entry.insert("id".into(), row["id"].clone());
    if let Some(registry) = row.get("registry_identity").filter(|r| !r.is_null()) {
        entry.insert("registry_identity".into(), registry.clone());
    }
    entry
}
