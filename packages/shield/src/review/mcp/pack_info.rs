//! Compact registry pack review with explicit access to complete rule evidence.
use super::{Context, Error, fields, required, text, views::Views};
use crate::review::packs;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const PROFILES: [&str; 3] = ["loose", "recommended", "strict"];
const SUMMARY_BYTES: usize = 16 * 1024;

pub(super) fn inspect(context: &Context, views: &mut Views, args: &Value) -> Result<Value, Error> {
    fields(
        args,
        &[
            "reference",
            "view",
            "section",
            "rule_id",
            "query",
            "cursor",
            "limit",
        ],
    )?;
    let reference = required(args, "reference")?;
    let view = view(args)?;
    let rule_id = text(args, "rule_id")?;
    if rule_id.is_some()
        && ((args.get("view").is_some() && view != "rules")
            || ["limit", "query", "cursor"]
                .iter()
                .any(|key| args.get(key).is_some()))
    {
        return Err(Error::Invalid(
            "rule_id accepts view=rules or no view, without query, cursor or limit",
        ));
    }

    let mut data = packs::info(context, reference)?;
    data["reference"] = json!(reference);
    for field in [
        "variables",
        "mcp_integrations",
        "invocations",
        "command_sets",
    ] {
        if data[field].is_null() {
            data[field] = json!({});
        }
    }
    for field in ["preconditions", "dependencies"] {
        if data[field].is_null() {
            data[field] = json!([]);
        }
    }
    if view == "settings" {
        let mut settings = json!({"view":"settings","reference":reference,"selected_release":data["selected_release"],
            "variables":data["variables"],"preconditions":data["preconditions"],"dependencies":data["dependencies"],"mcp_integrations":data["mcp_integrations"],"invocations":data["invocations"],"command_sets":data["command_sets"],"setup":setup(&data,reference)});
        if let Some(section) = text(args, "section")? {
            if ![
                "variables",
                "preconditions",
                "dependencies",
                "mcp_integrations",
                "invocations",
                "command_sets",
            ]
            .contains(&section)
            {
                return Err(Error::Invalid("invalid settings section"));
            }
            for field in [
                "variables",
                "preconditions",
                "dependencies",
                "mcp_integrations",
                "invocations",
                "command_sets",
            ] {
                if field != section {
                    settings.as_object_mut().expect("settings").remove(field);
                }
            }
            return views.page(context, "pack_info", args, settings, section);
        }
        for field in [
            "variables",
            "preconditions",
            "dependencies",
            "mcp_integrations",
            "invocations",
            "command_sets",
        ] {
            crate::review::page::preview(
                &mut settings,
                field,
                json!({"tool":"pack_info","reference":reference,"view":"settings","section":field}),
            );
        }
        canonic_mcp_kit::page::Request::from_mcp(args, canonic_mcp_kit::page::Limits::STANDARD)
            .map_err(crate::review::page::error)?;
        if args.get("cursor").is_some() {
            return Err(Error::InvalidCursor);
        }
        return Ok(settings);
    }
    if let Some(id) = rule_id {
        let rule = data["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(data["preconditions"].as_array().into_iter().flatten())
            .find(|rule| rule["id"] == id)
            .ok_or(Error::Missing)?;
        let rule = if rule.get("decision").is_some() {
            packs::rule_view(rule.clone())
        } else {
            rule.clone()
        };
        let mut result = json!({"reference":reference,"selected_release":data["selected_release"],
            "rule":rule,"rules":[rule],"invocations":data["invocations"],"command_sets":data["command_sets"]});
        referenced_definitions(&mut result);
        result.as_object_mut().expect("rule result").remove("rules");
        return Ok(result);
    }
    if view == "rules" {
        // Shared invocation definitions belong to an explicit rule inspection,
        // never the initial summary. Cursor pages retain the acquired bytes.
        if let Some(query) = text(args, "query")? {
            let query = query.to_lowercase();
            data["rules"]
                .as_array_mut()
                .ok_or(Error::Invalid("pack rules missing"))?
                .retain(|rule| {
                    ["id", "reason"].iter().any(|field| {
                        rule[field]
                            .as_str()
                            .is_some_and(|value| value.to_lowercase().contains(&query))
                    })
                });
        }
        for field in [
            "variables",
            "preconditions",
            "dependencies",
            "packs",
            "mcp_integrations",
        ] {
            data.as_object_mut()
                .ok_or(Error::Invalid("invalid pack info"))?
                .remove(field);
        }
        let mut result = views.page(context, "pack_info", &page_args(args), data, "rules")?;
        referenced_definitions(&mut result);
        return Ok(result);
    }
    let mut result = summary(&data, reference)?;
    if text(args, "section")? == Some("permission_groups") {
        result["permission_groups"] = json!(permission_groups(
            data["rules"].as_array().map_or(&[][..], Vec::as_slice)
        )?);
        return views.page(context, "pack_info", args, result, "permission_groups");
    }
    if args.get("cursor").is_some() {
        return Err(Error::InvalidCursor);
    }
    canonic_mcp_kit::page::Request::from_mcp(args, canonic_mcp_kit::page::Limits::STANDARD)
        .map_err(crate::review::page::error)?;
    Ok(result)
}

fn view(args: &Value) -> Result<&str, Error> {
    match text(args, "view")?.unwrap_or("summary") {
        value @ ("summary" | "rules" | "settings") => Ok(value),
        _ => Err(Error::Invalid("invalid pack view")),
    }
}

fn page_args(args: &Value) -> Value {
    args.clone()
}

fn decision(value: &Value, profile: &str) -> Value {
    if value.is_string() {
        value.clone()
    } else {
        value[profile].clone()
    }
}

fn declared(rule: &Value) -> &Value {
    rule.get("profile_decisions").unwrap_or(&rule["decision"])
}

fn counts() -> Value {
    json!({"allow":0,"ask":0,"deny":0})
}

fn increment(value: &mut Value, outcome: &Value) {
    if let Some(key @ ("allow" | "ask" | "deny")) = outcome.as_str() {
        value[key] = json!(value[key].as_u64().unwrap_or_default() + 1);
    }
}

fn excerpt(value: &Value, length: usize) -> Value {
    value.as_str().map_or(Value::Null, |text| {
        let mut short: String = text.chars().take(length).collect();
        if short.len() < text.len() {
            short.push('…');
        }
        json!(short)
    })
}

fn profile_summary(rules: &[Value]) -> Value {
    let mut profiles = json!({});
    for profile in PROFILES {
        let mut met = counts();
        let mut unmet = counts();
        for rule in rules {
            increment(&mut met, &decision(declared(rule), profile));
            increment(
                &mut unmet,
                &decision(&rule["on_unmet"]["decision"], profile),
            );
        }
        profiles[profile] = json!({"rule_outcomes":met,"unmet_requirement_outcomes":unmet,
            "examples":profile_examples(rules,profile)});
    }
    profiles
}

// Examples explain the pack's declared boundaries, not effective permission.
// Sample each outcome independently of the group listing's order and limit.
fn profile_examples(rules: &[Value], profile: &str) -> Value {
    let mut examples = json!({"allow":[],"ask":[],"deny":[]});
    for rule in rules {
        let outcome = decision(declared(rule), profile);
        let Some(rows) = outcome
            .as_str()
            .and_then(|key| examples.get_mut(key))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        let description = excerpt(
            rule.get("reason")
                .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()))
                .unwrap_or(&rule["id"]),
            160,
        );
        if rows.len() >= 4 || rows.iter().any(|row| row["description"] == description) {
            continue;
        }
        rows.push(json!({"rule_id":rule["id"],"description":description,
            "conditional":rule.get("requirements").is_some() || rule.get("when").is_some(),
            "fallback":rule["fallback"] == true,
            "on_unmet_decision":decision(&rule["on_unmet"]["decision"],profile)}));
    }
    examples
}

// Repeated executable paths and wrapper variants share the pack author's
// explanation and outcomes. Keep their counts without repeating argv grammars.
fn permission_groups(rules: &[Value]) -> Result<Vec<Value>, Error> {
    let mut groups: BTreeMap<String, Value> = BTreeMap::new();
    for rule in rules {
        let key = json!([
            rule["reason"],
            declared(rule),
            rule["on_unmet"],
            rule.get("requirements").is_some(),
            rule.get("when").is_some(),
            rule["fallback"]
        ])
        .to_string();
        let group = groups.entry(key).or_insert_with(|| {
            json!({
            "id":crate::policy::fingerprint(json!([rule["reason"],declared(rule),rule["on_unmet"],rule.get("requirements").is_some(),rule.get("when").is_some(),rule["fallback"]]).to_string().as_bytes()),
            "description":excerpt(rule.get("reason").unwrap_or(&rule["id"]),240),
            "profile_decisions":declared(rule),"rule_count":0,"example_rule_ids":[],
            "conditional":rule.get("requirements").is_some() || rule.get("when").is_some()})
        });
        if rule["fallback"] == true {
            group["fallback"] = json!(true);
        }
        if !rule["on_unmet"].is_null() {
            group["on_unmet_decisions"] = rule["on_unmet"]["decision"].clone();
        }
        group["rule_count"] = json!(group["rule_count"].as_u64().unwrap_or_default() + 1);
        let ids = group["example_rule_ids"]
            .as_array_mut()
            .ok_or(Error::Invalid("invalid rule summary"))?;
        if ids.is_empty() {
            ids.push(rule["id"].clone());
        }
    }
    Ok(groups.into_values().collect())
}

fn settings_summary(data: &Value, missing: &mut Vec<&'static str>) -> Value {
    let mut variables = json!({});
    let definitions = data["variables"].as_object();
    for (name, value) in definitions.into_iter().flatten().take(8) {
        variables[name] = json!({"type":value["type"],"description":excerpt(&value["description"],160),
            "default":value["default"],"values":value["values"],"required":value["required"] == true,
            "match":value["match"],"setup":value["setup"]});
    }
    if definitions.is_some_and(|items| items.len() > 8) {
        missing.push("variables");
    }
    let preconditions = data["preconditions"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let guards: Vec<_> = preconditions
        .iter()
        .take(8)
        .map(|rule| {
            json!({
        "id":rule["id"],"when":rule["when"],
        "on_unmet_decisions":rule["on_unmet"]["decision"],
        "message":excerpt(&rule["on_unmet"]["message"],160)})
        })
        .collect();
    if preconditions.len() > 8 {
        missing.push("preconditions");
    }
    json!({"variables":variables,"precondition_count":preconditions.len(),
        "mcp_integrations":data["mcp_integrations"].as_object().map(|definitions|definitions.keys().collect::<Vec<_>>()).unwrap_or_default(),
        "preconditions":guards,"dependencies":data["dependencies"]})
}

fn setup(data: &Value, reference: &str) -> Value {
    let names: Vec<_> = data["variables"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name)
        .collect();
    let mut result = json!({"settings_available":!names.is_empty(),
    "install_arguments":{"reference":reference},
    "message":if names.is_empty() {
        "No settings are declared. Installation cannot add a requested target restriction; report the gap instead of proposing unrestricted permissions."
    } else {
        "settings_arguments_template is a template, not ready-to-call arguments. Replace its placeholder with only declared names needed for the user's request; required missing values are collected automatically. Omit unrequested optional settings. Unsupported form types need terminal installation. Local storage does not restrict targets."
    }});
    if !names.is_empty() {
        result["settings_arguments_template"] =
            json!({"reference":reference,"variables":["<declared-variable-name>"]});
    }
    result
}

fn summary(data: &Value, reference: &str) -> Result<Value, Error> {
    let rules = data["rules"].as_array().map_or(&[][..], Vec::as_slice);
    let mut groups = permission_groups(rules)?;
    let group_count = groups.len();
    let mut missing = Vec::new();
    if group_count > 16 {
        missing.push("permission_groups");
        groups.truncate(16);
    }
    let settings = settings_summary(data, &mut missing);
    let mut result = json!({"view":"summary","reference":reference,"source":data["source"],
        "source_identity":data["source_identity"],"selected_release":data["selected_release"],
        "description":excerpt(&data["description"],512),"compatibility":data["compatibility"],
        "release_count":data["packs"].as_array().map_or(0,Vec::len),
        "rule_count":rules.len(),"profiles":profile_summary(rules),
        "permission_group_count":group_count,"permission_groups":groups,
        "missing":missing,"truncated":!missing.is_empty(),
        "scope":"Declared rule outcomes. Effective permissions also depend on conditions, requirements, variable bindings, preconditions and other active policy.",
        "inspect_rules":{"reference":reference,"view":"rules","limit":50},
        "inspect_settings":{"reference":reference,"view":"settings"},
        "setup":setup(data,reference),
        "inspect_rule":{"reference":reference,"rule_id":"<example_rule_id>"}});
    result
        .as_object_mut()
        .ok_or(Error::Invalid("invalid summary"))?
        .extend(
            settings
                .as_object()
                .ok_or(Error::Invalid("invalid settings summary"))?
                .clone(),
        );
    for field in [
        "variables",
        "preconditions",
        "dependencies",
        "mcp_integrations",
        "invocations",
        "command_sets",
    ] {
        crate::review::page::preview(
            &mut result,
            field,
            json!({"tool":"pack_info","reference":reference,"view":"settings","section":field}),
        );
        result[format!("{field}_total")] = json!(
            data[field]
                .as_array()
                .map(Vec::len)
                .or_else(|| data[field].as_object().map(serde_json::Map::len))
                .unwrap_or_default()
        );
    }
    result["permission_groups_total"] = json!(group_count);
    result["permission_groups_list"] =
        json!({"tool":"pack_info","reference":reference,"section":"permission_groups"});
    bound_summary(&mut result)?;
    Ok(result)
}

fn bound_summary(result: &mut Value) -> Result<(), Error> {
    // Never replace a large summary with an unbounded full document.
    if !crate::json::fits(&result, SUMMARY_BYTES) {
        for profile in PROFILES {
            result["profiles"][profile]
                .as_object_mut()
                .map(|row| row.remove("examples"));
        }
        result["missing"]
            .as_array_mut()
            .ok_or(Error::Invalid("invalid summary"))?
            .push(json!("profile_examples"));
        result["truncated"] = json!(true);
        if crate::json::fits(&result, SUMMARY_BYTES) {
            return Ok(());
        }
        for field in [
            "variables",
            "preconditions",
            "dependencies",
            "permission_groups",
        ] {
            result
                .as_object_mut()
                .ok_or(Error::Invalid("invalid summary"))?
                .remove(field);
            let missing = result["missing"]
                .as_array_mut()
                .ok_or(Error::Invalid("invalid summary"))?;
            if !missing.contains(&json!(field)) {
                missing.push(json!(field));
            }
            result["truncated"] = json!(true);
            if crate::json::fits(&result, SUMMARY_BYTES) {
                return Ok(());
            }
        }
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(())
}

/// Carry shared definitions only when this page actually references them.
fn referenced_definitions(data: &mut Value) {
    let mut invocations = std::collections::BTreeSet::new();
    let mut commands = std::collections::BTreeSet::new();
    fn visit(
        value: &Value,
        invocations: &mut std::collections::BTreeSet<String>,
        commands: &mut std::collections::BTreeSet<String>,
    ) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if key == "invocation" {
                        if let Some(name) = value.as_str() {
                            invocations.insert(name.to_owned());
                        }
                    }
                    if key == "command_set" {
                        if let Some(name) = value.as_str() {
                            commands.insert(name.to_owned());
                        }
                    }
                    visit(value, invocations, commands);
                }
            }
            Value::Array(rows) => {
                for row in rows {
                    visit(row, invocations, commands);
                }
            }
            _ => {}
        }
    }
    visit(&data["rules"], &mut invocations, &mut commands);
    let mut visited = std::collections::BTreeSet::new();
    loop {
        let pending: Vec<_> = invocations.difference(&visited).cloned().collect();
        if pending.is_empty() {
            break;
        }
        for name in pending {
            if let Some(definition) = data["invocations"].get(&name) {
                visit(definition, &mut invocations, &mut commands);
            }
            visited.insert(name);
        }
    }
    let reference = data["reference"].clone();
    for (field, names) in [("invocations", invocations), ("command_sets", commands)] {
        if let Some(definitions) = data[field].as_object_mut() {
            definitions.retain(|name, _| names.contains(name));
        }
        if field == "command_sets" {
            data["command_set_members_total"] = json!(
                data[field]
                    .as_object()
                    .into_iter()
                    .flat_map(serde_json::Map::values)
                    .filter_map(Value::as_object)
                    .map(serde_json::Map::len)
                    .sum::<usize>()
            );
        }
        crate::review::page::preview(
            data,
            field,
            json!({"tool":"pack_info","reference":reference,"view":"settings","section":field}),
        );
    }
}
