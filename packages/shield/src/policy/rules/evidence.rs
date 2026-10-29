//! Bounded requirement facts, shared by ordinary rules and pack preconditions.
use super::{Error, Facts, Input, Provider, Value, argument_rules, json, selectors};

fn sensitive_pointer(pointer: &str) -> bool {
    pointer.split('/').skip(1).any(|segment| {
        crate::evidence::sensitive_key(&segment.replace("~1", "/").replace("~0", "~"))
    })
}

pub(super) fn collect(
    predicate: &Value,
    key: &str,
    facts: &Facts<'_>,
    patterns: &dyn Provider,
) -> Result<Option<Value>, Error> {
    match (key, facts.input) {
        ("arguments", Input::Argv { arguments, .. }) if facts.known_arguments => Ok(Some(
            argument_rules::matches(predicate, arguments, patterns)?.evidence(predicate),
        )),
        (
            key,
            Input::Mcp {
                arguments,
                connection,
                ..
            },
        ) if key == "fields" || key == "connection" => {
            let fields = predicate.as_object().ok_or(Error::Syntax)?;
            let mut rows = Vec::new();
            let actual = if key == "fields" {
                Some(arguments)
            } else {
                connection.as_ref()
            };
            for (pointer, expected) in fields {
                let code = actual.map_or(Ok(Some("evidence_unavailable")), |actual| {
                    selectors::fields_match(&json!({pointer:expected}), actual, patterns)
                })?;
                let mut row = json!({"pointer":pointer,"status":if code.is_some(){"failed"}else{"met"},"code":code.unwrap_or("requirement_met")});
                row["source"] = json!(if key == "fields" {
                    "request"
                } else {
                    "reviewed-connection"
                });
                if key == "connection"
                    && let Some(name) = pointer.strip_prefix("/effective/")
                    && let Some(origin) = connection
                        .as_ref()
                        .and_then(|c| c["evidence_sources"].get(name))
                {
                    row["source"] = origin["source"].clone();
                    row["evidence_pointer"] = origin["pointer"].clone();
                }
                if !sensitive_pointer(pointer)
                    && !row["evidence_pointer"]
                        .as_str()
                        .is_some_and(sensitive_pointer)
                {
                    let mut safe = expected.clone();
                    crate::evidence::redact(&mut safe);
                    if safe.to_string().len() <= 512 {
                        row["expected"] = safe;
                    }
                }
                rows.push(row);
            }
            rows.sort_by_key(|row| row["status"] == "met");
            rows.truncate(4);
            Ok(Some(json!({"fields":rows,"truncated":fields.len()>4})))
        }
        _ => Ok(None),
    }
}
