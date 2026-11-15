//! Source-key selection precedes rule projection and resolution. Parsed bodies
//! live only for this request; diagnostic captures retain their existing owner.
use super::{Context, Error, Value, Views, json, text};
use crate::review::{inspection, page};
use std::collections::{BTreeMap, BTreeSet};

struct Rule {
    key: Vec<u8>,
    id: String,
    reference: Value,
    pack: Option<usize>,
    custom: Option<usize>,
    precondition: bool,
}
struct Index {
    rules: Vec<Rule>,
    custom: Vec<Value>,
    shared: BTreeSet<usize>,
}

fn index(source: &Value, metadata: &Value) -> Result<Index, Error> {
    let mut custom = Vec::new();
    let mut custom_rules = BTreeMap::new();
    for field in ["rules", "local_rules"] {
        let Some(bytes) = source[field].as_str() else {
            continue;
        };
        let doc = crate::policy::inspect_retained_custom(bytes)?;
        for row in doc["rules"].as_array().into_iter().flatten() {
            let id = row["id"]
                .as_str()
                .ok_or(Error::Invalid("rule ID missing"))?
                .to_owned();
            let reference = json!({"scope":"custom","rule_id":id});
            let key = page::key(&json!([id, Value::Null, reference]));
            custom_rules.insert(
                id.clone(),
                Rule {
                    key,
                    id,
                    reference,
                    pack: None,
                    custom: Some(custom.len()),
                    precondition: false,
                },
            );
        }
        custom.push(doc);
    }
    let mut rules: Vec<_> = custom_rules.into_values().collect();
    let mut shared = BTreeSet::new();
    for (position, pack) in metadata["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let bytes = source["packs"][position]
            .as_str()
            .ok_or(Error::Invalid("pack bytes missing"))?;
        let doc: Value = crate::policy::parse(bytes)?;
        if ["invocations", "command_sets"]
            .iter()
            .any(|field| doc[field].as_object().is_some_and(|rows| !rows.is_empty()))
        {
            shared.insert(position);
        }
        for (field, precondition) in [("rules", false), ("preconditions", true)] {
            for row in doc[field].as_array().into_iter().flatten() {
                let id = row["id"]
                    .as_str()
                    .ok_or(Error::Invalid("rule ID missing"))?
                    .to_owned();
                let reference =
                    json!({"scope":"pack","pack_selector":pack["selector"],"rule_id":id});
                let key = page::key(&json!([id, pack["id"], reference]));
                rules.push(Rule {
                    key,
                    id,
                    reference,
                    pack: Some(position),
                    custom: None,
                    precondition,
                });
            }
        }
    }
    rules.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(Index {
        rules,
        custom,
        shared,
    })
}

fn definition_route(args: &Value, pack: &Value, field: &str) -> Value {
    let mut route = json!({"tool":"rule_list","section":field,"pack_selector":pack["selector"]});
    if let Some(candidate) = args.get("candidate_id") {
        route["candidate_id"] = candidate.clone();
    }
    route
}

fn shared_row(source: &Value, pack: &Value, position: usize, args: &Value) -> Result<Value, Error> {
    let doc: Value = crate::policy::parse(
        source["packs"][position]
            .as_str()
            .ok_or(Error::Invalid("pack bytes missing"))?,
    )?;
    let mut row = json!({"pack_selector":pack["selector"],"source":pack,"invocations":doc["invocations"].as_object().cloned().unwrap_or_default(),"command_sets":doc["command_sets"].as_object().cloned().unwrap_or_default()});
    for field in ["invocations", "command_sets"] {
        page::preview(&mut row, field, definition_route(args, pack, field));
    }
    Ok(row)
}

pub(super) fn rules(context: &Context, views: &mut Views, args: &Value) -> Result<Value, Error> {
    let section = text(args, "section")?;
    let pack_selector = text(args, "pack_selector")?;
    match section {
        None | Some("shared") if pack_selector.is_none() => {}
        Some("invocations" | "command_sets") if pack_selector.is_some() => {}
        _ => {
            return Err(Error::Invalid(
                "shared takes no pack_selector; invocation and command-set sections require pack_selector",
            ));
        }
    }
    let candidate = text(args, "candidate_id")?;
    let id = candidate.map_or_else(|| views.active_readonly(context), |id| Ok(id.to_owned()))?;
    let captured = views.get(&id).map_err(|error| {
        if args.get("cursor").is_some() && matches!(error, Error::CandidateUnavailable) {
            Error::InvalidCursor
        } else {
            error
        }
    })?;
    let source = inspection::source::rules(
        &captured.candidate,
        &captured.metadata,
        captured.revision(),
        None,
    )?;
    let query = page::query(context, "rule_list", args);
    let request =
        canonic_mcp_kit::page::Request::from_mcp(args, canonic_mcp_kit::page::Limits::STANDARD)
            .map_err(page::error)?;
    let after = page::after(&page::scope("rule_list", &query), &request)?;
    let mut result = json!({"policy_revision":captured.revision()});
    if let Some(section) = section {
        result["section"] = json!(section);
    }
    if candidate.is_some() {
        result["candidate_id"] = json!(id);
    }
    if let Some(field @ ("invocations" | "command_sets")) = section {
        let (position, _pack) = captured.metadata["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .find(|(_, pack)| pack["selector"] == pack_selector.unwrap())
            .ok_or(Error::Missing)?;
        let doc: Value = crate::policy::parse(
            source["packs"][position]
                .as_str()
                .ok_or(Error::Invalid("pack bytes missing"))?,
        )?;
        result["pack_selector"] = json!(pack_selector);
        result[field] = json!(doc[field].as_object().cloned().unwrap_or_default());
        return page::bounded("rule_list", &query, result, field);
    }
    let index = index(&source, &captured.metadata)?;
    if section == Some("shared") {
        let mut owners: Vec<_> = index
            .shared
            .iter()
            .map(|position| {
                let pack = &captured.metadata["installed"][*position];
                (page::key(&json!([pack["id"], pack["selector"]])), *position)
            })
            .collect();
        owners.sort_by(|a, b| a.0.cmp(&b.0));
        let rows = owners
            .into_iter()
            .filter(|(key, _)| after.as_ref().is_none_or(|last| key > last))
            .map(|(key, position)| {
                shared_row(
                    &source,
                    &captured.metadata["installed"][position],
                    position,
                    args,
                )
                .map(|row| (key, row))
            });
        let page = page::collect_results(
            "rule_list",
            &query,
            rows,
            "rule_list with section invocations or command_sets",
        )?;
        result["shared"] = json!(page.items);
        result["next_cursor"] = json!(page.next_cursor);
        result["total"] = json!(index.shared.len());
        return Ok(result);
    }
    let mut documents = BTreeMap::<usize, Value>::new();
    let rows = index
        .rules
        .iter()
        .filter(|rule| after.as_ref().is_none_or(|last| rule.key > *last))
        .map(|rule| {
            let mut row = if let Some(position) = rule.pack {
                let artifact = source["packs"][position]
                    .as_str()
                    .ok_or(Error::Invalid("pack bytes missing"))?;
                if let std::collections::btree_map::Entry::Vacant(entry) = documents.entry(position)
                {
                    entry.insert(crate::policy::parse(artifact)?);
                }
                let doc = &documents[&position];
                let field = if rule.precondition {
                    "preconditions"
                } else {
                    "rules"
                };
                let raw = doc[field]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|row| row["id"] == rule.id)
                    .ok_or(Error::Missing)?;
                let pack = &captured.metadata["installed"][position];
                let mut row = inspection::public_pack_rule(raw, &pack["profile"]);
                if rule.precondition {
                    row["kind"] = json!("precondition");
                }
                row["rule_ref"] = rule.reference.clone();
                row["source"] = pack.clone();
                row["profile"] = pack["profile"].clone();
                let resolution = views.resolution(&id, &source, position, artifact)?;
                if let Some(resolved) = resolution.get(&rule.id) {
                    row["resolution"] = resolved.clone();
                }
                row
            } else {
                let raw = index.custom[rule.custom.expect("custom source")]["rules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|row| row["id"] == rule.id)
                    .ok_or(Error::Missing)?;
                let mut row = raw.clone();
                row["rule_ref"] = rule.reference.clone();
                row["source"] = json!("custom");
                row["policy_schema_version"] = json!(1);
                row
            };
            row["rule_digest"] = json!(crate::policy::fingerprint(row.to_string().as_bytes()));
            Ok((rule.key.clone(), row))
        });
    let page = page::collect_results("rule_list", &query, rows, "rule_list with rule_ref")?;
    result["rules"] = json!(page.items);
    result["next_cursor"] = json!(page.next_cursor);
    result["total"] = json!(index.rules.len());
    if !index.shared.is_empty() {
        result["shared"] = json!([]);
        result["shared_total"] = json!(index.shared.len());
        let mut route = json!({"tool":"rule_list","section":"shared"});
        if let Some(candidate) = candidate {
            route["candidate_id"] = json!(candidate);
        }
        result["shared_list"] = route;
    }
    Ok(result)
}
