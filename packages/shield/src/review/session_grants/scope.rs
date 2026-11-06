//! Session-selector admission and matching through the shared prepared policy engine.
use super::{Error, Value, json};
use crate::policy::{CustomRules, Decision, Request};

/// A scope is an OR of closed, individually validated positive argv selectors.
pub(super) fn prepare(value: &Value) -> Result<Vec<CustomRules>, Error> {
    let selectors = value
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 32)
        .ok_or(Error::Invalid(
            "unsupported_scope: select 1-32 argv alternatives",
        ))?;
    let mut rules = Vec::new();
    for (index, selector) in selectors.iter().enumerate() {
        let fields = selector
            .as_object()
            .ok_or(Error::Invalid("invalid scope selector"))?;
        if fields
            .keys()
            .any(|k| !["conditions", "requirements"].contains(&k.as_str()))
        {
            return Err(Error::Invalid(
                "unsupported_scope: only conditions and requirements are accepted",
            ));
        }
        let c = &selector["conditions"];
        let executable = &c["executable"];
        let literal = |v: &Value| {
            v.as_str()
                .is_some_and(|s| !s.is_empty() && !s.contains('\0'))
        };
        let finite = literal(executable)
            || executable.as_object().is_some_and(|o| o.len() == 1)
                && executable["one_of"]
                    .as_array()
                    .is_some_and(|xs| !xs.is_empty() && xs.len() <= 32 && xs.iter().all(literal));
        let cwd = &c["cwd"];
        let rooted = cwd["exact"]["root"] == "project" || cwd["within"]["root"] == "project";
        let arguments = &c["arguments"];
        if c["domain"] != "argv"
            || !finite
            || !rooted
            || !["exact", "options"].iter().any(|k| arguments["kind"] == *k)
            || c.get("possible_effects").is_some()
            || !crate::json::fits(selector, 10_000)
        {
            return Err(Error::Invalid(
                "unsupported_scope: use literal executables, project cwd and exact argv or a closed options grammar",
            ));
        }
        // Unknown options can execute a different operation; a session grant
        // never opts into forward-compatible argument acceptance.
        if arguments["kind"] == "options"
            && (arguments["operands"] == true
                || arguments["parse"] == "stop_at_operand"
                || arguments["leading"]["parse"] == "stop_at_operand"
                || arguments
                    .get("unknown_options")
                    .is_some_and(|v| v != "refuse")
                || arguments
                    .get("long_abbreviations")
                    .is_some_and(|v| v != false)
                || arguments["leading"]
                    .get("long_abbreviations")
                    .is_some_and(|v| v != false))
        {
            return Err(Error::Invalid(
                "unsupported_scope: use bounded operands and intermixed parsing; unknown options and abbreviations are refused",
            ));
        }
        let mut rule = json!({"id":format!("session-{index}"),"decision":"allow","conditions":c});
        if let Some(requirements) = selector.get("requirements") {
            rule["requirements"] = requirements.clone();
            rule["on_unmet"] = json!({"decision":"deny"});
        }
        rules.push(CustomRules::from_rule_values(&[rule])?);
    }
    Ok(rules)
}

pub(super) fn matches(prepared: &[CustomRules], request: &Request) -> Result<bool, Error> {
    for selector in prepared {
        if selector.evaluate_observed(request)?.decision == Decision::Allow {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Resolve every claimed Deny override against the active, instance-qualified rule.
pub(super) fn overrides(value: &Value, rules: &Value) -> Result<Value, Error> {
    let references = value
        .as_array()
        .filter(|a| a.len() <= 32)
        .ok_or(Error::Invalid("invalid override_denies"))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut evidence = Vec::new();
    for reference in references {
        if reference.as_object().is_none_or(|o| o.len() != 3)
            || reference["branch"] != "met"
            || !seen.insert(reference.to_string())
        {
            return Err(Error::Invalid(
                "unsupported_scope: overrides require distinct rule_ref, branch met and rule_digest",
            ));
        }
        let rule = rules["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["rule_ref"] == reference["rule_ref"])
            .ok_or(Error::Missing)?;
        if rule["kind"] == "precondition"
            || rule["decision"] != "deny"
            || rule["rule_digest"] != reference["rule_digest"]
        {
            return Err(Error::Invalid(
                "unsupported_scope: override must identify a current met Deny rule and its rule_digest",
            ));
        }
        let source = if rule["source"].is_string() {
            rule["source"].clone()
        } else {
            let pack = rule["source"]["id"].as_str().ok_or(Error::Invalid(
                "unsupported_scope: pack identity unavailable",
            ))?;
            let identity = rule["source"]["source_identity"]
                .as_str()
                .ok_or(Error::Invalid(
                    "unsupported_scope: registry identity unavailable",
                ))?;
            json!(format!("{identity}/{pack}"))
        };
        evidence.push(json!({"rule_ref":reference["rule_ref"],"source":source,"reason":rule["reason"],"rule_digest":reference["rule_digest"]}));
    }
    Ok(json!(evidence))
}

pub(super) fn covered_denies(overrides: &Value, evaluation: &Value) -> bool {
    let explanation = evaluation.get("explanation").unwrap_or(evaluation);
    if explanation["requirement_checks"]
        .as_array()
        .is_some_and(|a| {
            a.iter()
                .any(|c| c["status"] != "met" && c["applied"] != false)
        })
    {
        return false;
    }
    let denies: Vec<_> = explanation["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["applied"] != false && m["decision"] == "deny")
        .collect();
    !denies.is_empty()
        && denies.iter().all(|m| {
            m["branch"] == "met"
                && m["precondition"] != true
                && overrides.as_array().into_iter().flatten().any(|r| {
                    let rr = &r["rule_ref"];
                    rr["rule_id"] == m["rule_id"] && r["source"] == m["source"]
                })
        })
}
