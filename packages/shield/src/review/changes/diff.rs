//! Structured differences between the active policy and a candidate: packs
//! added, removed, updated, re-profiled, switched between pinned and tracking
//! or between direct and dependency selections, pack variable values and rule
//! overrides, custom rules added, removed or edited, and access path changes.
//! Changes to the uncommitted local layer carry `"layer":"local"`. Reviews and
//! receipts carry them.
use serde_json::{Value, json};

/// Changes from `before` (installed rows and project candidate) to `after`.
pub(in crate::review) fn between(
    before_meta: &Value,
    before_candidate: &Value,
    after_meta: &Value,
    after_candidate: &Value,
) -> Value {
    let mut changes = packs(&before_meta["installed"], &after_meta["installed"]);
    changes.extend(settings(before_candidate, after_candidate));
    changes.extend(rules(&before_candidate["rules"], &after_candidate["rules"]));
    for mut change in rules(
        &before_candidate["local_rules"],
        &after_candidate["local_rules"],
    ) {
        change["layer"] = json!("local");
        changes.push(change);
    }
    let config =
        |candidate: &Value| crate::configuration::layers::merged(candidate).unwrap_or_default();
    let (old, new) = (config(before_candidate), config(after_candidate));
    for (field, layer) in [("mcp", "project"), ("local_mcp", "local")] {
        let ids: std::collections::BTreeSet<_> = [&old[field], &new[field]]
            .into_iter()
            .filter_map(Value::as_object)
            .flat_map(serde_json::Map::keys)
            .collect();
        for id in ids {
            if old[field][id] != new[field][id] {
                let mut change = json!({"kind":"mcp","id":id,"layer":layer,"change":"changed"});
                if layer != "local" {
                    change["before"] = old[field][id].clone();
                    change["after"] = new[field][id].clone();
                }
                changes.push(change);
            }
        }
    }
    for (field, layer) in [
        ("access_paths", None),
        ("local_access_paths", Some("local")),
    ] {
        if old[field] != new[field] {
            let mut change = json!({"kind":"access_paths","change":"changed","before":old[field],"after":new[field]});
            if let Some(layer) = layer {
                change["layer"] = json!(layer);
            }
            changes.push(change);
        }
    }
    let disabled = |config: &Value| config["disabled"].as_array().cloned().unwrap_or_default();
    for entry in disabled(&new) {
        if !disabled(&old).iter().any(|e| e["id"] == entry["id"]) {
            changes.push(json!({"kind":"pack","change":"disabled","id":entry["id"],"version":entry["version"],"layer":"local"}));
        }
    }
    json!(changes)
}

/// A selection's values and overrides by layer. Local unsets and cleared
/// overrides appear as `{"unset":true}`.
fn layered(row: &Value, field: &str) -> (Value, Value) {
    if row["layer"] == "local" {
        return (json!({}), row[field].clone());
    }
    let mut local = row["local"][field].clone();
    let removed = if field == "variables" {
        "unset"
    } else {
        "clear_overrides"
    };
    for name in row["local"][removed].as_array().into_iter().flatten() {
        if let Some(name) = name.as_str() {
            local[name] = json!({"unset":true});
        }
    }
    (row[field].clone(), local)
}

/// Overrides in effect for a selection: shared ones, cleared or replaced locally.
fn effective_overrides(row: &Value) -> Value {
    let (mut shared, local) = layered(row, "overrides");
    if !shared.is_object() {
        shared = json!({});
    }
    for (id, change) in local.as_object().into_iter().flatten() {
        if change == &json!({"unset":true}) {
            shared.as_object_mut().map(|s| s.remove(id));
        } else {
            shared[id] = change.clone();
        }
    }
    shared
}

fn packs(before: &Value, after: &Value) -> Vec<Value> {
    let key = |row: &Value| (row["source_identity"].clone(), row["id"].clone());
    let rows = |value: &Value| value.as_array().cloned().unwrap_or_default();
    let mode = |row: &Value| {
        if row["pinned"] == true {
            "pinned"
        } else {
            "tracking"
        }
    };
    let role = |row: &Value| {
        if row["direct"] == false {
            "dependency"
        } else {
            "direct"
        }
    };
    let (before, after) = (rows(before), rows(after));
    let mut changes = Vec::new();
    for row in &after {
        let pack = |change: &str| {
            let mut entry = json!({"kind":"pack","change":change,"id":row["id"],"version":row["version"],
                "profile":row["profile"],"dependency":row["direct"] == false,"mode":mode(row),"digest":row["digest"]});
            if row["layer"] == "local" {
                entry["layer"] = json!("local");
            }
            entry
        };
        match before.iter().find(|old| key(old) == key(row)) {
            None => changes.push(pack("added")),
            Some(old) => {
                if old["version"] != row["version"] {
                    let mut change = pack("updated");
                    change["from"] = old["version"].clone();
                    changes.push(change);
                }
                if old["profile"] != row["profile"] {
                    let mut change = pack("profile");
                    change["from"] = old["profile"].clone();
                    changes.push(change);
                }
                if mode(old) != mode(row) {
                    let mut change = pack("mode");
                    change["from"] = json!(mode(old));
                    changes.push(change);
                }
                if role(old) != role(row) {
                    let mut change = pack("role");
                    change["role"] = json!(role(row));
                    change["from"] = json!(role(old));
                    changes.push(change);
                }
            }
        }
    }
    for old in before
        .iter()
        .filter(|old| !after.iter().any(|row| key(row) == key(old)))
    {
        changes.push(
            json!({"kind":"pack","change":"removed","id":old["id"],"version":old["version"],
            "profile":old["profile"],"dependency":old["direct"] == false,"mode":mode(old)}),
        );
    }
    changes
}

/// Variable values and overrides of selections present in both candidates;
/// an override that loosens its entry is flagged as widening.
fn settings(before: &Value, after: &Value) -> Vec<Value> {
    let selections = |candidate: &Value| {
        crate::configuration::layers::merged(candidate)
            .ok()
            .and_then(|config| config["packs"].as_array().cloned())
            .unwrap_or_default()
    };
    let key = |row: &Value| (row["registry_identity"].clone(), row["id"].clone());
    let old_rows = selections(before);
    let mut changes = Vec::new();
    for (index, row) in selections(after).iter().enumerate() {
        let Some((old_index, old)) = old_rows
            .iter()
            .enumerate()
            .find(|(_, old)| key(old) == key(row))
        else {
            continue;
        };
        let old_artifact = before["packs"][old_index].as_str().unwrap_or_default();
        let artifact = after["packs"][index].as_str().unwrap_or_default();
        let profile = |selection: &Value| {
            serde_json::from_value::<crate::policy::Profile>(selection["profile"].clone())
                .unwrap_or_default()
        };
        let (old_effective, new_effective) = (effective_overrides(old), effective_overrides(row));
        for (field, kind) in [("variables", "variable"), ("overrides", "override")] {
            let (old_shared, old_local) = layered(old, field);
            let (new_shared, new_local) = layered(row, field);
            for (layer, was_map, now_map) in [
                (None, &old_shared, &new_shared),
                (Some("local"), &old_local, &new_local),
            ] {
                let names: std::collections::BTreeSet<&String> = [was_map, now_map]
                    .iter()
                    .filter_map(|m| m.as_object())
                    .flat_map(serde_json::Map::keys)
                    .collect();
                for name in names {
                    let (was, now) = (&was_map[name], &now_map[name]);
                    if was == now {
                        continue;
                    }
                    let change = match (was.is_null(), now.is_null()) {
                        (true, _) => "set",
                        (_, true) => "removed",
                        _ => "changed",
                    };
                    let mut entry = json!({"kind":kind,"change":change,"id":row["id"],"name":name,"before":was,"after":now});
                    if let Some(layer) = layer {
                        entry["layer"] = json!(layer);
                    }
                    if kind == "override" {
                        let parse = |v: &Value| {
                            serde_json::from_value::<crate::policy::Override>(v.clone()).ok()
                        };
                        entry["widens"] = json!(crate::policy::override_widens(
                            old_artifact,
                            artifact,
                            name,
                            profile(old),
                            profile(row),
                            parse(&old_effective[name]).as_ref(),
                            parse(&new_effective[name]).as_ref()
                        ));
                    }
                    changes.push(entry);
                }
            }
        }
    }
    changes
}

fn rules(before: &Value, after: &Value) -> Vec<Value> {
    let parse = |source: &Value| {
        source
            .as_str()
            .and_then(|s| serde_saphyr::from_str::<Value>(s).ok())
            .and_then(|document| document["rules"].as_array().cloned())
            .unwrap_or_default()
    };
    let (before, after) = (parse(before), parse(after));
    let mut changes = Vec::new();
    for rule in &after {
        match before.iter().find(|old| old["id"] == rule["id"]) {
            None => changes.push(json!({"kind":"rule","change":"added","rule":rule})),
            Some(old) if old != rule => {
                changes.push(json!({"kind":"rule","change":"edited","rule":rule,"before":old}));
            }
            Some(_) => {}
        }
    }
    for old in before
        .iter()
        .filter(|old| !after.iter().any(|rule| rule["id"] == old["id"]))
    {
        changes.push(json!({"kind":"rule","change":"removed","rule":old}));
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, version: &str, profile: &str, direct: bool) -> Value {
        json!({"source_identity":"acme-source","id":id,"version":version,"profile":profile,"direct":direct,"digest":"0".repeat(64)})
    }

    #[test]
    fn local_mcp_change_records_omit_values_for_add_edit_and_remove() {
        let candidate = |profile: Option<&str>| {
            let mut candidate = json!({"config":"schema_version: 1\npacks: []\n"});
            if let Some(profile) = profile {
                candidate["local_config"] = json!(json!({"schema_version":1,"mcp":{"cloud":{
                    "transport":{"type":"stdio","command":"/acme/private/connector","args":["--profile",profile],"env":{"ACME_REGION":"acme-private-region"}},
                    "agents":{"claude":"personal_cloud"}}}}).to_string());
            }
            candidate
        };
        let empty = candidate(None);
        let before = candidate(Some("acme-personal-before"));
        let after = candidate(Some("acme-personal-after"));
        for (old, new) in [(&empty, &before), (&before, &after), (&after, &empty)] {
            let changes = between(&json!({}), old, &json!({}), new);
            assert_eq!(
                changes,
                json!([{"kind":"mcp","id":"cloud","layer":"local","change":"changed"}])
            );
        }
        assert_eq!(between(&json!({}), &after, &json!({}), &after), json!([]));
    }

    #[test]
    fn packs_rules_and_access_paths_report_each_kind_of_change() {
        let before_meta = json!({"installed":[row("acme/git","1.0.0","recommended",true),row("acme/old","1.0.0","loose",true)]});
        let after_meta = json!({"installed":[row("acme/git","1.1.0","strict",true),row("acme/new","2.0.0","recommended",false)]});
        let rule = |id: &str, reason: &str| json!({"id":id,"decision":"allow","reason":reason});
        let document = |rules: Vec<Value>| json!({"rules":rules}).to_string();
        let before = json!({"config":json!({"access_paths":{"read":["/srv/acme"]}}).to_string(),"rules":document(vec![rule("keep","Acme"),rule("edit","Old"),rule("drop","Acme")])});
        let after = json!({"config":json!({"access_paths":{"read":["/srv/other"]}}).to_string(),"rules":document(vec![rule("keep","Acme"),rule("edit","New"),rule("add","Acme")])});
        let changes = between(&before_meta, &before, &after_meta, &after);
        let summary: Vec<_> = changes
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                format!(
                    "{} {} {}",
                    c["kind"].as_str().unwrap(),
                    c["change"].as_str().unwrap(),
                    c["id"].as_str().or(c["rule"]["id"].as_str()).unwrap_or("")
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                "pack updated acme/git",
                "pack profile acme/git",
                "pack added acme/new",
                "pack removed acme/old",
                "rule edited edit",
                "rule added add",
                "rule removed drop",
                "access_paths changed ",
            ]
        );
        assert_eq!(changes[2]["dependency"], true);
        assert_eq!(changes[1]["from"], "recommended");
        assert_eq!(
            between(&before_meta, &before, &before_meta, &before),
            json!([])
        );
    }

    #[test]
    fn pinned_tracking_and_dependency_direct_switches_are_reported() {
        let mut pinned = row("acme/git", "1.0.0", "recommended", true);
        pinned["pinned"] = json!(true);
        let before = json!({"installed":[pinned, row("acme/lib","1.0.0","recommended",false)]});
        let after = json!({"installed":[row("acme/git","1.0.0","recommended",true), row("acme/lib","1.0.0","recommended",true)]});
        let candidate = json!({});
        let changes = between(&before, &candidate, &after, &candidate);
        let changes = changes.as_array().unwrap();
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert_eq!(
            (
                &changes[0]["change"],
                &changes[0]["id"],
                &changes[0]["mode"],
                &changes[0]["from"]
            ),
            (
                &json!("mode"),
                &json!("acme/git"),
                &json!("tracking"),
                &json!("pinned")
            )
        );
        assert_eq!(
            (
                &changes[1]["change"],
                &changes[1]["id"],
                &changes[1]["role"],
                &changes[1]["from"]
            ),
            (
                &json!("role"),
                &json!("acme/lib"),
                &json!("direct"),
                &json!("dependency")
            )
        );
        let back = between(&after, &candidate, &before, &candidate);
        assert_eq!(back[0]["mode"], "pinned");
        assert_eq!(back[1]["role"], "dependency");
    }

    #[test]
    fn variable_values_and_overrides_report_changes_and_flag_widening() {
        let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[
            {"id":"lint","decision":"deny","conditions":{}},
            {"id":"read","decision":"allow","conditions":{}}
        ]}).to_string();
        let candidate = |selection: Value| {
            let mut entry = json!({"id":"acme/tools","registry_identity":"acme"});
            for (key, value) in selection.as_object().unwrap() {
                entry[key] = value.clone();
            }
            json!({"config":json!({"packs":[entry]}).to_string(),"packs":[pack]})
        };
        let before =
            candidate(json!({"variables":{"tool":"make","stale":"x"},"overrides":{"read":"deny"}}));
        let after =
            candidate(json!({"variables":{"tool":"just","new":"y"},"overrides":{"lint":"ask"}}));
        let meta = json!({"installed":[]});
        let changes = between(&meta, &before, &meta, &after);
        let summary: Vec<_> = changes
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                format!(
                    "{} {} {} {}",
                    c["kind"].as_str().unwrap(),
                    c["change"].as_str().unwrap(),
                    c["name"].as_str().unwrap(),
                    c["widens"]
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                "variable set new null",
                "variable removed stale null",
                "variable changed tool null",
                "override set lint true",
                "override removed read true",
            ]
        );
        let narrowing = between(
            &meta,
            &after,
            &meta,
            &candidate(
                json!({"variables":{"tool":"just","new":"y"},"overrides":{"lint":"deny","read":"off"}}),
            ),
        );
        assert!(
            narrowing
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["widens"] == false),
            "{narrowing}"
        );
    }

    #[test]
    fn override_review_uses_selected_profiles_and_both_artifacts() {
        let pack = |decision: Value| {
            json!({
                "schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0",
                "description":"Acme","license":"MIT","maintainers":["Acme"],
                "engines":{"shield":"*"},"capabilities":["structured-rules-v1"],
                "rules":[{"id":"inspect","decision":decision,"conditions":{}}]
            })
            .to_string()
        };
        let candidate = |profile: &str, overrides: Value, source: &str| {
            json!({
                "config":json!({"packs":[{
                    "id":"acme/tools","profile":profile,"overrides":overrides
                }]}).to_string(),"packs":[source]
            })
        };
        let source = pack(json!({"loose":"allow","recommended":"ask","strict":"deny"}));
        for (profile, widens) in [("loose", true), ("recommended", false), ("strict", false)] {
            let before = candidate(profile, json!({"inspect":"ask"}), &source);
            let after = candidate(profile, json!({}), &source);
            let changes = settings(&before, &after);
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0]["widens"], widens, "{changes:?}");
        }
        let before = candidate("recommended", json!({}), &pack(json!("deny")));
        let after = candidate(
            "recommended",
            json!({"inspect":"ask"}),
            &pack(json!("allow")),
        );
        assert_eq!(settings(&before, &after)[0]["widens"], true);
        let before = candidate("strict", json!({"inspect":"ask"}), &source);
        let after = candidate("loose", json!({}), &source);
        assert_eq!(settings(&before, &after)[0]["widens"], true);
        let mut gated: Value = serde_json::from_str(&source).unwrap();
        gated["preconditions"] = json!([{
            "id":"gate","conditions":{"domain":"argv","executable":"acme"},
            "requires_any":[{"arguments":{"kind":"exact","values":["inspect"]}}],
            "on_unmet":{"decision":{"loose":"ask","recommended":"ask","strict":"deny"}}
        }]);
        let gated = gated.to_string();
        let before = candidate("loose", json!({"gate":{"on_unmet":"deny"}}), &gated);
        let after = candidate("loose", json!({}), &gated);
        assert_eq!(settings(&before, &after)[0]["widens"], true);
    }
}
