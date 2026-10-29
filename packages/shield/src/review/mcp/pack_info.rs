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
        &["reference", "view", "rule_id", "query", "cursor", "limit"],
    )?;
    let reference = required(args, "reference")?;
    let view = view(args)?;
    let rule_id = text(args, "rule_id")?;
    if rule_id.is_some()
        && ["view", "limit", "query"]
            .iter()
            .any(|key| args.get(key).is_some())
    {
        return Err(Error::Invalid(
            "rule selection cannot coexist with view or paging",
        ));
    }
    if view != "rules"
        && rule_id.is_none()
        && ["limit", "query"].iter().any(|key| args.get(key).is_some())
    {
        return Err(Error::Invalid("summary does not support paging"));
    }
    let mut data = packs::info(context, reference)?;
    if view == "settings" {
        return Ok(
            json!({"view":"settings","reference":reference,"selected_release":data["selected_release"],
            "variables":data["variables"],"preconditions":data["preconditions"],"dependencies":data["dependencies"],"mcp_integrations":data["mcp_integrations"]}),
        );
    }
    if let Some(id) = rule_id {
        let rule = data["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(data["preconditions"].as_array().into_iter().flatten())
            .find(|rule| rule["id"] == id)
            .ok_or(Error::Missing)?;
        return Ok(
            json!({"reference":reference,"selected_release":data["selected_release"],
            "rule":rule,"invocations":data["invocations"],"command_sets":data["command_sets"]}),
        );
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
        return views.page("pack_info", &page_args(args), data, "rules");
    }
    summary(&data, reference)
}

pub(super) fn resume(views: &mut Views, args: &Value) -> Result<Value, Error> {
    required(args, "reference")?;
    if view(args)? != "rules" {
        return Err(Error::Invalid("only rule listings support cursors"));
    }
    views.resume("pack_info", &page_args(args))
}

fn view(args: &Value) -> Result<&str, Error> {
    match text(args, "view")?.unwrap_or("summary") {
        value @ ("summary" | "rules" | "settings") => Ok(value),
        _ => Err(Error::Invalid("invalid pack view")),
    }
}

fn page_args(args: &Value) -> Value {
    let mut result = args.clone();
    if result.get("limit").is_none() {
        result["limit"] = json!(10);
    }
    result
}

fn decision(value: &Value, profile: &str) -> Value {
    if value.is_string() {
        value.clone()
    } else {
        value[profile].clone()
    }
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
            increment(&mut met, &decision(&rule["profile_decisions"], profile));
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
        let outcome = decision(&rule["profile_decisions"], profile);
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
            rule["profile_decisions"],
            rule["on_unmet"],
            rule.get("requirements").is_some(),
            rule.get("when").is_some(),
            rule["fallback"]
        ])
        .to_string();
        let group = groups.entry(key).or_insert_with(|| {
            json!({
            "description":excerpt(rule.get("reason").unwrap_or(&rule["id"]),240),
            "profile_decisions":rule["profile_decisions"],"rule_count":0,"example_rule_ids":[],
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
            "default":value["default"],"values":value["values"]});
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

fn summary(data: &Value, reference: &str) -> Result<Value, Error> {
    let rules = data["rules"].as_array().map_or(&[][..], Vec::as_slice);
    let mut groups = permission_groups(rules)?;
    let group_count = groups.len();
    let mut missing = Vec::new();
    if group_count > 48 {
        missing.push("permission_groups");
        groups.truncate(48);
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
        "inspect_rules":{"reference":reference,"view":"rules","limit":10},
        "inspect_settings":{"reference":reference,"view":"settings"},
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
    bound_summary(&mut result)?;
    Ok(result)
}

fn bound_summary(result: &mut Value) -> Result<(), Error> {
    // Never replace a large summary with an unbounded full document.
    if result.to_string().len() > SUMMARY_BYTES {
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
        if result.to_string().len() <= SUMMARY_BYTES {
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
            if result.to_string().len() <= SUMMARY_BYTES {
                return Ok(());
            }
        }
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(())
}
