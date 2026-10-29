//! Argument scope in host confirmation messages, separate from literal witnesses.
use super::text;
use serde_json::Value;
use std::fmt::Write as _;

fn vector(value: &Value) -> String {
    let Some(values) = value.as_array().filter(|values| !values.is_empty()) else {
        return "none".into();
    };
    let mut shown = values
        .iter()
        .take(8)
        .map(|value| format!("‘{}’", text(value)))
        .collect::<Vec<_>>()
        .join(" then ");
    if values.len() > 8 {
        let _ = write!(
            shown,
            "; {} more argument selectors not shown",
            values.len() - 8
        );
    }
    shown
}

fn each(value: &Value) -> String {
    format!(
        "{}–{} arguments, each matching {}",
        value["min"].as_u64().unwrap_or(0),
        value["max"].as_u64().unwrap_or(256),
        text(&value["match"])
    )
}

pub(super) fn describe(value: &Value) -> String {
    let mut scope = match value["kind"].as_str() {
        None if value.is_null() => "any arguments".into(),
        Some("any") => "any arguments".into(),
        Some("exact") => format!(
            "exact arguments: {}; no additional arguments",
            vector(&value["values"])
        ),
        Some("prefix") => format!(
            "argument prefix: {}; any remaining arguments",
            vector(&value["values"])
        ),
        Some("sequence") => format!(
            "argument prefix: {}; {}",
            vector(&value["prefix"]),
            match value["rest"]["kind"].as_str() {
                Some("none") => "no remaining arguments".into(),
                Some("any") => "any remaining arguments".into(),
                Some("each") => format!("remaining {}", each(&value["rest"])),
                _ => "remaining argument constraints not summarized".into(),
            }
        ),
        Some("options") => format!(
            "argument prefix: {}; {}; operands: {}{}",
            vector(&value["prefix"]),
            if value["unknown_options"] == "skip" {
                "undeclared options are skipped and their values count as operands"
            } else {
                "only declared options"
            },
            match &value["operands"] {
                Value::Bool(true) => "any".into(),
                Value::Bool(false) => "none".into(),
                operands => each(operands),
            },
            options(value)
        ),
        Some("risk_tokens") => format!(
            "argument prefix: {}; at least one remaining token matching {}; also detects long options with attached values and short flags in bundles before -- (conservative risk detection)",
            vector(&value["prefix"]),
            text(&value["any_of"])
        ),
        _ => "argument constraints not summarized".into(),
    };
    if let Some(leading) = value.get("leading") {
        let _ = write!(
            scope,
            "; leading options before the prefix{}",
            options(leading)
        );
    }
    scope
}

fn options(value: &Value) -> String {
    let mut result = String::new();
    for (key, label) in [
        ("flags", "flags"),
        ("values", "options taking values"),
        ("optional_values", "options taking optional values"),
        ("require_all", "all required options"),
        ("require_any", "at least one required option"),
        ("forbid", "forbidden options"),
        ("parse", "option parsing"),
        ("short_clusters", "short flag bundles"),
        ("long_equals", "long option =value syntax"),
        ("short_attached", "attached short option values"),
        ("long_abbreviations", "abbreviated long options"),
    ] {
        if let Some(setting) = value.get(key) {
            let _ = write!(
                result,
                "; {label}: {}",
                if setting.as_array().is_some_and(Vec::is_empty) {
                    "none".into()
                } else {
                    text(setting)
                }
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn confirmation_distinguishes_all_argument_kinds_and_prefixes() {
        for (arguments, expected) in [
            (Value::Null, "any arguments"),
            (json!({"kind":"any"}), "any arguments"),
            (
                json!({"kind":"exact","values":["status", "a b"]}),
                "exact arguments: ‘status’ then ‘a b’; no additional arguments",
            ),
            (
                json!({"kind":"exact","values":[]}),
                "exact arguments: none; no additional arguments",
            ),
            (
                json!({"kind":"prefix","values":["status"]}),
                "argument prefix: ‘status’; any remaining arguments",
            ),
            (
                json!({"kind":"sequence","prefix":["status"],"rest":{"kind":"none"}}),
                "argument prefix: ‘status’; no remaining arguments",
            ),
            (
                json!({"kind":"sequence","prefix":[],"rest":{"kind":"any"}}),
                "argument prefix: none; any remaining arguments",
            ),
            (
                json!({"kind":"sequence","prefix":["show"],"rest":{"kind":"each","match":{"glob":"*.txt"},"min":1,"max":8}}),
                "argument prefix: ‘show’; remaining 1–8 arguments, each matching glob: *.txt",
            ),
        ] {
            assert_eq!(describe(&arguments), expected);
        }
        let risk =
            describe(&json!({"kind":"risk_tokens","prefix":["push"],"any_of":["--force","-f"]}));
        assert!(risk.contains(
            "argument prefix: ‘push’; at least one remaining token matching --force, -f"
        ));
        assert!(risk.contains("conservative risk detection"));
    }

    #[test]
    fn confirmation_options_include_operand_constraints_leading_and_requirements() {
        let scope = describe(&json!({"kind":"options","prefix":["inspect"],
            "leading":{"flags":["--verbose"],"values":[]},
            "flags":["-n"],"values":[{"name":"--format","match":{"one_of":["json","text"]},"repeat":"forbid"}],
            "optional_values":["--color"],"operands":{"match":{"glob":"*.txt"}},
            "require_all":["--format"],"require_any":["-n"],"forbid":["--color"],
            "parse":"stop_at_operand","short_clusters":false,"long_equals":false,"short_attached":false}));
        for expected in [
            "argument prefix: ‘inspect’",
            "only declared options",
            "operands: 0–256 arguments, each matching glob: *.txt",
            "flags: -n",
            "name: --format",
            "one_of: json, text",
            "repeat: forbid",
            "options taking optional values: --color",
            "all required options: --format",
            "at least one required option: -n",
            "forbidden options: --color",
            "option parsing: stop_at_operand",
            "short flag bundles: disabled",
            "long option =value syntax: disabled",
            "attached short option values: disabled",
            "leading options before the prefix; flags: --verbose",
        ] {
            assert!(scope.contains(expected), "missing {expected}: {scope}");
        }
        assert!(!scope.contains('{'));
        let scan = describe(&json!({"kind":"options","prefix":[],"flags":[],
            "values":[{"name":"--profile","match":{"not":{"one_of":["acme-prod"]}}}],
            "operands":true,"unknown_options":"skip","long_abbreviations":true}));
        for expected in [
            "undeclared options are skipped",
            "not: one_of: acme-prod",
            "abbreviated long options: enabled",
        ] {
            assert!(scan.contains(expected), "missing {expected}: {scan}");
        }
        assert!(!scan.contains("only declared options"));
        for (operands, expected) in [(true, "any"), (false, "none")] {
            assert!(describe(&json!({"kind":"options","prefix":[],"flags":[],"values":[],"operands":operands})).contains(&format!("operands: {expected}")));
        }
    }

    #[test]
    fn confirmation_long_prefix_keeps_suffix_scope_and_marks_omissions() {
        let scope = describe(&json!({"kind":"prefix","values":vec!["long".repeat(80); 12]}));
        assert!(scope.contains("4 more argument selectors not shown"));
        assert!(scope.ends_with("any remaining arguments"));
    }
}
