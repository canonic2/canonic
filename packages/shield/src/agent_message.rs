//! Bounded rule feedback shared by native hooks, SDK receipts and MCP previews.
use serde_json::{Value, json};

pub(crate) fn cache_repair(project: &str) -> Option<String> {
    if !std::path::Path::new(project).is_absolute() || project.chars().any(char::is_control) {
        return None;
    }
    Some(format!(
        "shield cache prepare --project '{}'",
        project.replace('\'', "'\\''")
    ))
}

pub(crate) fn failure(explanation: &Value) -> Option<String> {
    let check = explanation["requirement_checks"].as_array()?.first()?;
    if let Some(field) = check["evidence"]["fields"]
        .as_array()
        .and_then(|fields| fields.iter().find(|field| field["code"] == check["code"]))
        && let Some(pointer) = field["pointer"].as_str()
    {
        let pointer = field["evidence_pointer"].as_str().unwrap_or(pointer);
        if field["source"] == "reviewed-connection" {
            return Some(
                if ["field_missing", "evidence_unavailable"]
                    .iter()
                    .any(|code| check["code"] == *code)
                {
                    format!("Required connection evidence is unavailable: {pointer}.")
                } else {
                    format!("Connection setting {pointer} does not meet the rule.")
                },
            );
        }
        if check["code"] == "field_missing" {
            return Some(format!("Missing required field: {pointer}."));
        }
        let expected = &field["expected"]["text"];
        if let Some(value) = expected
            .as_str()
            .or_else(|| expected.get("exact").and_then(Value::as_str))
        {
            return Some(format!("{pointer} must be {value}."));
        }
        return Some(format!("The value of {pointer} does not meet the rule."));
    }
    if check["code"] == "required_option_missing" {
        for (key, join) in [("missing_options", ", "), ("required_any", " or ")] {
            let names: Vec<_> = check["evidence"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !names.is_empty() {
                return Some(format!("Missing required option: {}.", names.join(join)));
            }
        }
    }
    if check["code"] == "option_value_mismatch"
        && let Some(option) = check["evidence"]["mismatched_options"][0].as_object()
        && let Some(name) = option.get("name").and_then(Value::as_str)
    {
        let listed = |key| -> Vec<&str> {
            option
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect()
        };
        let (expected, excluded) = (listed("expected"), listed("excluded"));
        return Some(match (expected.as_slice(), excluded.as_slice()) {
            ([one], _) => format!("{name} must be {one}."),
            ([_, _, ..], _) => format!("{name} must be one of {}.", expected.join(", ")),
            ([], [one]) => format!("{name} must not be {one}."),
            ([], [_, _, ..]) => format!("{name} must not be any of {}.", excluded.join(", ")),
            ([], []) => format!("The value of {name} does not meet the rule."),
        });
    }
    Some(
        match check["code"].as_str()? {
            "required_option_missing" => "A required option is missing.",
            "forbidden_option_present" => "A forbidden option is present.",
            "option_value_missing" => "An option requires a value.",
            "option_value_mismatch" => "An option value does not meet the rule.",
            "repeated_option" => "The rule forbids this repeated option.",
            "unknown_option" => "The rule does not permit this option.",
            "ambiguous_option" => {
                "An abbreviated option matches several options; spell it in full."
            }
            "location_outside" => "An affected path is outside the permitted location.",
            "location_excluded" => "An affected path is excluded by the rule.",
            "evidence_unavailable" => "A required fact could not be verified.",
            "field_missing" => "A required field is missing.",
            _ => "A rule requirement is unmet.",
        }
        .into(),
    )
}

/// The alternative sharing the most options with the request, so `rg --files`
/// is offered the `--files` form; the rule's own order breaks ties.
fn closest<'a>(explanation: &Value, guidance: &'a Value) -> Option<&'a str> {
    let used: Vec<&str> = explanation["requirement_checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| check["operation_ref"] == guidance["operation_ref"])
        .flat_map(|check| {
            check["evidence"]["options"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|option| option["name"].as_str())
        .collect();
    let shared = |alternative: &str| {
        alternative
            .split_whitespace()
            .filter(|word| word.starts_with('-') && used.contains(word))
            .count()
    };
    guidance["alternatives"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|alternative| {
            // Adding an access root helps only a verified outside-location
            // failure, not unknown targets or unsupported argument syntax.
            !alternative.contains("access_path_add")
                || explanation["requirement_checks"]
                    .as_array()
                    .is_some_and(|checks| {
                        checks.iter().any(|check| {
                            check["code"] == "location_outside"
                                && check["operation_ref"] == guidance["operation_ref"]
                                && check["rule_ref"] == guidance["rule_ref"]
                        })
                    })
        })
        .rev()
        .max_by_key(|alternative| shared(alternative))
}

/// The rule that denied the request and its reason, for a Deny that carries no
/// guidance or failed requirement of its own.
fn denying_rule(explanation: &Value) -> Option<String> {
    if explanation["decision"] != "deny" {
        return None;
    }
    let rule = explanation["matches"]
        .as_array()?
        .iter()
        .find(|m| m["applied"] == true && m["decision"] == "deny")?;
    let id = rule["rule_id"].as_str()?;
    let reason: String = rule["reason"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect();
    let reason = reason.trim().trim_end_matches('.');
    Some(if reason.is_empty() {
        format!("Rule `{id}` denies this request.")
    } else {
        format!("Rule `{id}` denies this request: {reason}.")
    })
}

/// Rule guidance or a failed requirement, without the denying rule fallback.
pub(crate) fn guidance_detail(explanation: &Value) -> Option<String> {
    detail_from(explanation, false)
}

pub(crate) fn detail(explanation: &Value) -> Option<String> {
    detail_from(explanation, true)
}

fn detail_from(explanation: &Value, rule_fallback: bool) -> Option<String> {
    let guidance = explanation["guidance"].as_array().and_then(|v| v.first());
    let message = guidance
        .and_then(|g| g["message"].as_str())
        .map(str::to_owned)
        .or_else(|| failure(explanation))
        .or_else(|| rule_fallback.then(|| denying_rule(explanation)).flatten());
    let alternative = guidance.and_then(|g| closest(explanation, g));
    let mut detail = message.unwrap_or_default();
    if let Some(alternative) = alternative {
        if !detail.is_empty() {
            detail.push(' ');
        }
        detail.push_str("Candidate: ");
        detail.push_str(alternative);
    }
    if detail.is_empty() {
        return None;
    }
    if !detail.ends_with(['.', '!', '?']) {
        detail.push('.');
    }
    bound(&mut detail, MAX_MESSAGE - RETRY.len());
    detail.push_str(RETRY);
    Some(detail)
}

/// The most bytes of one feedback message, as SDK receipt validation requires.
pub(crate) const MAX_MESSAGE: usize = 1024;
const RETRY: &str = " Re-evaluate any revised request.";
/// Redacting an already redacted message changes at most the word cut by
/// [`bound`], which then grows by no more than this marker.
const REDACTION: &str = "[REDACTED]";

/// Shortens `text` to at most `max` bytes on a character boundary, marking the cut.
fn bound(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max - "...".len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("...");
}

pub(crate) fn diagnostics(explanation: &Value) -> Value {
    let checks = explanation["requirement_checks"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let guidance = explanation["guidance"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let denied: Vec<_> = explanation["matches"].as_array().into_iter().flatten()
        .filter(|matched| matched["applied"] == true && matched["decision"] == "deny")
        .map(|matched| json!({"executable":matched["executable"],"rule_id":matched["rule_id"],"operation_ref":matched["operation_ref"]}))
        .take(4).collect();
    // Receipts are redacted after this, which can lengthen the message.
    let mut message = json!(detail(explanation));
    crate::evidence::redact(&mut message);
    if let Value::String(text) = &mut message {
        // Bound the body so the retry instruction survives the cut.
        if let Some(body) = text.strip_suffix(RETRY) {
            let mut body = body.to_owned();
            bound(&mut body, MAX_MESSAGE - REDACTION.len() - RETRY.len());
            body.push_str(RETRY);
            *text = body;
        } else {
            bound(text, MAX_MESSAGE - REDACTION.len());
        }
    }
    json!({"message":message,"denied_commands":denied.iter().take(3).collect::<Vec<_>>(),"requirement_checks":checks.iter().take(32).collect::<Vec<_>>(),"guidance":guidance.iter().take(3).collect::<Vec<_>>(),"truncated":checks.len()>32 || guidance.len()>3 || denied.len()>3})
}

/// Name denied command forms without echoing operands, paths or option values.
pub(crate) fn denial_detail(diagnostics: &Value) -> Option<String> {
    let identifier = |value: &Value, executable: bool| {
        value.as_str().and_then(|text| {
            let name = if executable {
                text.rsplit('/').next()?
            } else {
                text
            };
            (!name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b)))
            .then(|| name.to_owned())
        })
    };
    let denied: Vec<_> = diagnostics["denied_commands"]
        .as_array()?
        .iter()
        .take(3)
        .filter_map(|command| {
            Some(format!(
                "`{}` (rule `{}`)",
                identifier(&command["executable"], true)?,
                identifier(&command["rule_id"], false)?
            ))
        })
        .collect();
    (!denied.is_empty()).then(|| format!("Active policy denies these command forms: {}. Inspect the matching rules for supported arguments and requirements.", denied.join(", ")))
}

/// Analyzer guidance requires a revised request rather than host approval.
pub(crate) fn rewrite_detail(diagnostics: &Value) -> Option<&str> {
    let detail = diagnostics["message"].as_str()?;
    diagnostics["guidance"]
        .as_array()?
        .iter()
        .any(|item| item["source"] == "shield")
        .then_some(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_deny_names_its_rule_and_reason() {
        let mut explanation = json!({"decision":"deny","guidance":[],"requirement_checks":[],
            "matches":[{"source":"pack","rule_id":"npm-config","decision":"deny","applied":true,
                "reason":"Configuration changes require explicit operator control."}]});
        assert_eq!(
            detail(&explanation).unwrap(),
            "Rule `npm-config` denies this request: Configuration changes require explicit operator control. Re-evaluate any revised request."
        );
        assert_eq!(guidance_detail(&explanation), None);
        explanation["decision"] = json!("ask");
        assert_eq!(detail(&explanation), None);
    }

    #[test]
    fn long_multibyte_feedback_stays_within_the_receipt_message_bound() {
        let reason = "𝄞".repeat(400);
        let mut explanation = json!({"decision":"deny","guidance":[],"requirement_checks":[],
            "matches":[{"rule_id":"acme-deny","decision":"deny","applied":true,"reason":reason}]});
        let text = detail(&explanation).unwrap();
        assert!(text.len() <= MAX_MESSAGE, "{}", text.len());
        assert!(text.ends_with(RETRY), "{text}");
        explanation["guidance"] = json!([{"message":"é".repeat(700),
            "alternatives":[format!("acme {}", "--token ü ".repeat(80))]}]);
        let mut diagnostics = diagnostics(&explanation);
        // The receipt is redacted again before it is sent.
        crate::evidence::redact(&mut diagnostics);
        let message = diagnostics["message"].as_str().unwrap();
        assert!(message.len() <= MAX_MESSAGE, "{}", message.len());
        assert!(message.starts_with("éé"), "{message}");
        // Redaction lengthens the message; the re-bound keeps the retry instruction.
        assert!(message.ends_with(RETRY), "{message}");
    }

    #[test]
    fn folder_alternatives_require_a_matching_outside_failure() {
        let rule = json!({"scope":"pack","pack_selector":"acme/baseline","rule_id":"read-cat"});
        let mut explanation = json!({
            "guidance":[{"message":"Use approved paths.","operation_ref":"shell:0:0-10","rule_ref":rule,
                "alternatives":["Propose the outside folder with policy_propose (access_path_add)."]}],
            "requirement_checks":[{"code":"evidence_unavailable","status":"unknown","operation_ref":"shell:0:0-10","rule_ref":rule}]
        });
        for code in [
            "evidence_unavailable",
            "unknown_option",
            "location_excluded",
        ] {
            explanation["requirement_checks"][0]["code"] = json!(code);
            assert!(!detail(&explanation).unwrap().contains("access_path_add"));
        }
        explanation["requirement_checks"][0]["code"] = json!("location_outside");
        assert!(detail(&explanation).unwrap().contains("access_path_add"));
        explanation["requirement_checks"][0]["operation_ref"] = json!("shell:0:20-30");
        assert!(!detail(&explanation).unwrap().contains("access_path_add"));
        explanation["requirement_checks"][0]["operation_ref"] = json!("shell:0:0-10");
        explanation["requirement_checks"][0]["rule_ref"]["rule_id"] = json!("other-rule");
        assert!(!detail(&explanation).unwrap().contains("access_path_add"));
        explanation["guidance"][0]["alternatives"] = json!(["Use a literal file path."]);
        assert!(
            detail(&explanation)
                .unwrap()
                .contains("Use a literal file path.")
        );
    }

    #[test]
    fn catch_all_denials_do_not_suggest_folder_access_without_location_evidence() {
        let mut explanation = json!({"decision":"deny","guidance":[{
            "message":"Use supported options and approved paths.",
            "alternatives":["Propose with access_path_add."]}]});
        for checks in [Value::Null, json!([])] {
            explanation["requirement_checks"] = checks;
            assert!(!detail(&explanation).unwrap().contains("access_path_add"));
        }
    }

    #[test]
    fn denied_command_diagnostics_are_bounded_and_omit_operands() {
        let explanation = json!({"decision":"deny","matches":(0..5).map(|index| json!({
            "applied":true,"decision":"deny","rule_id":"read-form",
            "executable":"/usr/bin/tail","operation_ref":format!("shell:0:{index}"),
            "arguments":["--token=PRIVATE_VALUE","/private/report"]})).collect::<Vec<_>>()});
        let diagnostics = diagnostics(&explanation);
        assert_eq!(diagnostics["denied_commands"].as_array().unwrap().len(), 3);
        assert_eq!(diagnostics["truncated"], true);
        let message = denial_detail(&diagnostics).unwrap();
        assert!(message.contains("`tail` (rule `read-form`)"));
        assert!(!diagnostics.to_string().contains("PRIVATE_VALUE"));
        assert!(!message.contains("/usr/bin"));
        assert!(
            denial_detail(&json!({"denied_commands":[{"executable":"bad`name","rule_id":"read"}]}))
                .is_none()
        );
    }
}
