//! Pack views: installed packs, one installed pack, catalog search and release details.
use super::{label, plural, policy, short};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write};

/// Rules listed by `info` before the rest are counted.
const MAX_SHOWN_RULES: usize = 10;

/// One installed pack: identity, version, profile and how it was selected.
fn installed(row: &Value, rows: &[Value]) -> String {
    let mut text = format!(
        "{} {} · {}",
        label(&row["id"]),
        label(&row["version"]),
        label(&row["profile"])
    );
    if row["direct"] == false {
        let parents: Vec<_> = row["incoming"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|edge| label(&edge["parent"]))
            .collect();
        if parents.is_empty() {
            text.push_str(" · dependency");
        } else {
            let _ = write!(text, " · dependency of {}", parents.join(", "));
        }
    }
    if row["pinned"] == true {
        text.push_str(" · pinned");
    }
    // Two installed packs with one name need their selectors to tell them apart.
    if rows.iter().filter(|other| other["id"] == row["id"]).count() > 1 {
        let _ = write!(
            text,
            " · from {} · {}",
            label(&row["source_alias"]),
            label(&row["selector"])
        );
    }
    text
}

/// `shield pack list`.
pub(super) fn list(value: &Value) -> String {
    let rows = value["packs"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "No packs installed. Find one with shield pack search, or add the baseline with shield pack add main/baseline.\n".into();
    }
    rows.iter().fold(String::new(), |mut output, row| {
        let _ = writeln!(output, "{}", installed(row, &rows));
        output
    })
}

/// `shield pack show`.
pub(super) fn show(value: &Value) -> String {
    let (row, artifact) = (&value["pack"], &value["artifact"]);
    let mut output = format!(
        "{} · from {}\n",
        installed(row, std::slice::from_ref(row)),
        label(&row["source_alias"])
    );
    if artifact["description"].is_string() {
        let _ = writeln!(output, "{}", label(&artifact["description"]));
    }
    let _ = write!(
        output,
        "{} · digest {}",
        rule_count(artifact),
        short(&row["digest"])
    );
    let dependencies: Vec<_> = row["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| format!("{} {}", label(&d["pack"]), label(&d["requirement"])))
        .collect();
    if !dependencies.is_empty() {
        let _ = write!(output, " · depends on {}", dependencies.join(", "));
    }
    output.push('\n');
    output
}

pub(super) fn rule_count(artifact: &Value) -> String {
    let bodies = artifact["rules"].as_array().map_or(0, Vec::len);
    let groups = artifact["invocations"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    if groups == 0 {
        return plural(bodies, "rule", "rules");
    }
    let members = artifact["command_sets"]
        .as_object()
        .into_iter()
        .flat_map(serde_json::Map::values)
        .filter_map(Value::as_object)
        .map(serde_json::Map::len)
        .sum();
    let mut summary = format!(
        "{} · {}",
        plural(bodies, "rule body", "rule bodies"),
        plural(groups, "invocation group", "invocation groups")
    );
    if members != 0 {
        let _ = write!(
            summary,
            " · {}",
            plural(members, "command member", "command members")
        );
    }
    summary
}

/// `shield pack search`: each matching pack once, with its versions.
pub(super) fn search(value: &Value) -> String {
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in value["packs"].as_array().into_iter().flatten() {
        versions
            .entry(label(&entry["id"]))
            .or_default()
            .push(label(&entry["version"]));
    }
    if versions.is_empty() {
        return format!("No packs match in registry {}.\n", label(&value["source"]));
    }
    let width = versions.keys().map(String::len).max().unwrap_or(0);
    let mut output = versions
        .iter()
        .fold(String::new(), |mut output, (id, list)| {
            let _ = writeln!(output, "{id:<width$}  {}", list.join(", "));
            output
        });
    let _ = writeln!(
        output,
        "Registry {}. Inspect one with shield pack info <pack>.",
        label(&value["source"])
    );
    output
}

/// `shield pack info`: the release `pack add` would choose, its rules and other versions.
pub(super) fn info(value: &Value) -> String {
    let selected = &value["selected_release"];
    let rules = value["rules"].as_array().cloned().unwrap_or_default();
    let mut output = format!(
        "{} {} · registry {} · {}\n",
        label(&selected["id"]),
        label(&selected["version"]),
        label(&value["source"]),
        rule_count(value)
    );
    if value["description"].is_string() {
        let _ = writeln!(output, "{}", label(&value["description"]));
    }
    let versions: Vec<_> = value["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| label(&entry["version"]))
        .collect();
    let _ = writeln!(
        output,
        "Versions {} · requires Shield {}",
        versions.join(", "),
        label(&value["compatibility"]["engines"]["shield"])
    );
    for rule in rules.iter().take(MAX_SHOWN_RULES) {
        let _ = writeln!(
            output,
            "  {}  {} · {}",
            label(&rule["id"]),
            policy::rule_conditions(rule),
            policy::decision(&rule["profile_decisions"])
        );
    }
    if rules.len() > MAX_SHOWN_RULES {
        let _ = writeln!(
            output,
            "  … {} more; use --json for all rules",
            rules.len() - MAX_SHOWN_RULES
        );
    }
    let _ = writeln!(
        output,
        "Add it with shield pack add {}.",
        label(&selected["id"])
    );
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn installed_packs_name_dependencies_and_disambiguate_duplicates() {
        let text = list(&json!({"packs":[
            {"id":"acme/parent","version":"0.1.0","profile":"strict","direct":true,"pinned":true},
            {"id":"acme/test","version":"0.1.0","profile":"strict","direct":false,"incoming":[{"parent":"acme/parent"}]},
            {"id":"acme/dup","version":"1.0.0","profile":"loose","direct":true,"source_alias":"a","selector":"pack:1"},
            {"id":"acme/dup","version":"2.0.0","profile":"loose","direct":true,"source_alias":"b","selector":"pack:2"}]}));
        assert_eq!(
            text,
            "acme/parent 0.1.0 · strict · pinned\nacme/test 0.1.0 · strict · dependency of acme/parent\nacme/dup 1.0.0 · loose · from a · pack:1\nacme/dup 2.0.0 · loose · from b · pack:2\n"
        );
    }

    #[test]
    fn search_groups_versions_and_explains_empty_results() {
        let text = search(&json!({"source":"acme","packs":[
            {"id":"acme/test","version":"0.1.0"},{"id":"acme/parent","version":"0.1.0"},{"id":"acme/parent","version":"0.2.0"}]}));
        assert_eq!(
            text,
            "acme/parent  0.1.0, 0.2.0\nacme/test    0.1.0\nRegistry acme. Inspect one with shield pack info <pack>.\n"
        );
        assert_eq!(
            search(&json!({"source":"acme","packs":[]})),
            "No packs match in registry acme.\n"
        );
    }
}
