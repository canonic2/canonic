//! Compact approval evidence from retained pack artifacts and policy comparisons.
use serde_json::{Value, json};

pub(super) fn summary(proposal: &super::Proposal) -> Value {
    let comparison = &proposal.comparison;
    let before = packs(&proposal.base_candidate);
    let after = if proposal.base_candidate["packs"] == proposal.candidate["packs"] {
        before.clone()
    } else {
        packs(&proposal.candidate)
    };
    let mut summary = json!({"packs":{"before":before,"after":after},
        "comparison":{"relation":comparison["relation"],"complete":comparison["complete"],
            "coverage":comparison["coverage"],"incomplete_reason":comparison["incomplete_reason"],
            "expansion":witness(&comparison["expansion"]),"restriction":witness(&comparison["restriction"])}});
    // Prefixes let Codex skip its sandbox, so a proposal that changes them says so.
    if !comparison["codex_prefixes"].is_null() {
        summary["comparison"]["codex_prefixes"] = comparison["codex_prefixes"].clone();
    }
    // Files that lose their comments or layout are named before confirmation.
    if !comparison["formatting"].is_null() {
        summary["comparison"]["formatting"] = comparison["formatting"].clone();
    }
    summary
}

fn witness(value: &Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    json!({"request":value["request"],"before":value["before"]["decision"],"after":value["after"]["decision"]})
}

fn packs(candidate: &Value) -> Vec<Value> {
    candidate["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| crate::policy::parse::<Value>(source.as_str()?).ok())
        .map(|pack| {
            let command_members: usize = pack["command_sets"]
                .as_object()
                .into_iter()
                .flat_map(serde_json::Map::values)
                .filter_map(Value::as_object)
                .map(serde_json::Map::len)
                .sum();
            json!({"id":pack["id"],"version":pack["version"],
                "rule_count":pack["rules"].as_array().map_or(0, Vec::len),
                "invocation_groups":pack["invocations"].as_object().map_or(0, serde_json::Map::len),
                "command_members":command_members})
        })
        .collect()
}
