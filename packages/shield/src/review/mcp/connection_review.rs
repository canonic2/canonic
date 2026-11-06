//! A human-only connection projection that preserves evidence of masked edits.
use super::Error;
use serde_json::{Value, json};

pub(super) fn projection(before: &Value, after: &Value) -> Result<Value, Error> {
    let settings = |candidate: &Value| -> Result<Value, Error> {
        let config = crate::configuration::layers::merged(candidate)?;
        Ok(json!({"project":config["mcp"],"local":config["local_mcp"],
            "resolved":candidate["lock"]["mcp"]}))
    };
    Ok(redacted_pair(&settings(before)?, &settings(after)?))
}

fn redacted_pair(before: &Value, after: &Value) -> Value {
    let mut left = before.clone();
    let mut right = after.clone();
    crate::evidence::redact(&mut left);
    crate::evidence::redact(&mut right);
    annotate(before, Some(after), &mut left, "removed", false);
    annotate(after, Some(before), &mut right, "added", false);
    json!({"before":left,"after":right})
}

/// Compare retained JSON slots, never hashes or redacted placeholders. A
/// collapsed sensitive object is one slot; safe text around inline masks stays.
fn annotate(
    original: &Value,
    other: Option<&Value>,
    masked: &mut Value,
    missing: &str,
    forced: bool,
) {
    if let Some(text) = masked.as_str()
        && text.contains("[REDACTED]")
        && (forced || original != masked)
    {
        let change = match other {
            None => missing,
            Some(other) if other == original => "unchanged",
            Some(_) => "changed",
        };
        *masked = json!(text.replace("[REDACTED]", &format!("[REDACTED - {change}]")));
        return;
    }
    match masked {
        Value::Object(fields) => {
            for (key, value) in fields {
                if let Some(original) = original.get(key) {
                    annotate(
                        original,
                        other.and_then(|v| v.get(key)),
                        value,
                        missing,
                        crate::evidence::sensitive_key(key),
                    );
                }
            }
        }
        Value::Array(values) => {
            let mut next_secret = false;
            for (index, value) in values.iter_mut().enumerate() {
                if let Some(original) = original.get(index) {
                    annotate(
                        original,
                        other.and_then(|v| v.get(index)),
                        value,
                        missing,
                        next_secret,
                    );
                    next_secret = !next_secret
                        && original.as_str().is_some_and(|s| {
                            s.starts_with("--")
                                && crate::evidence::sensitive_key(s.trim_start_matches('-'))
                        });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_only_changes_remain_visible_without_disclosing_values() {
        let before = json!({"env":{"ACME_TOKEN":"before-private", "UNCHANGED_SECRET":"same-private",
            "REMOVED_SECRET":"removed-private", "REGION":"us-east-1"},
            "headers":{"Authorization":"Bearer before-private"},
            "args":["--token","before-private"],"inline":"connector --api-key=before-private --verbose"});
        let mut after = before.clone();
        after["env"]["ACME_TOKEN"] = json!("after-private");
        after["headers"]["Authorization"] = json!("Bearer after-private");
        after["args"][1] = json!("after-private");
        after["inline"] = json!("connector --api-key=after-private --verbose");
        let rendered = redacted_pair(&before, &after);
        for side in ["before", "after"] {
            assert_eq!(rendered[side]["env"]["ACME_TOKEN"], "[REDACTED - changed]");
            assert_eq!(
                rendered[side]["headers"]["Authorization"],
                "[REDACTED - changed]"
            );
            assert_eq!(rendered[side]["args"][1], "[REDACTED - changed]");
            assert_eq!(
                rendered[side]["inline"],
                "connector --api-key=[REDACTED - changed] --verbose"
            );
            assert_eq!(
                rendered[side]["env"]["UNCHANGED_SECRET"],
                "[REDACTED - unchanged]"
            );
            assert_eq!(rendered[side]["env"]["REGION"], "us-east-1");
        }
        assert!(!rendered.to_string().contains("private"));
        assert_eq!(before["env"]["ACME_TOKEN"], "before-private");
        assert_eq!(after["env"]["ACME_TOKEN"], "after-private");
    }

    #[test]
    fn masked_additions_removals_and_whole_objects_have_distinct_labels() {
        let before = json!({"env":{"REMOVE_TOKEN":"old-private","SAME_TOKEN":"[REDACTED]"},
            "token":{"command":"first-connector"}});
        let after = json!({"env":{"ADD_TOKEN":"new-private","SAME_TOKEN":"[REDACTED]"},
            "token":{"command":"second-connector"}});
        let rendered = redacted_pair(&before, &after);
        assert_eq!(
            rendered["before"]["env"]["REMOVE_TOKEN"],
            "[REDACTED - removed]"
        );
        assert_eq!(rendered["after"]["env"]["ADD_TOKEN"], "[REDACTED - added]");
        assert_eq!(
            rendered["before"]["env"]["SAME_TOKEN"],
            "[REDACTED - unchanged]"
        );
        for side in ["before", "after"] {
            assert_eq!(rendered[side]["token"], "[REDACTED - changed]");
        }
        assert!(!rendered.to_string().contains("connector"));
        assert!(!rendered.to_string().contains("private"));
    }
}
