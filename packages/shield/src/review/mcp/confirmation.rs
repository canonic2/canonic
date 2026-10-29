//! User-facing confirmation from a validated retained proposal, never model prose.
mod arguments;
mod scope;
use crate::review::model::Proposal;
use serde_json::Value;

fn text(value: &Value) -> String {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Null => "not set".into(),
        Value::Bool(true) => "enabled".into(),
        Value::Bool(false) => "disabled".into(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| format!("{key}: {}", text(value)))
            .collect::<Vec<_>>()
            .join("; "),
    };
    let mut result: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(180)
        .collect();
    if result.len() < value.len() {
        // Shorten at a word boundary when one is close, so words stay whole.
        if let Some(end) = result.rfind(' ').filter(|end| *end >= 120) {
            result.truncate(end);
            result.truncate(result.trim_end_matches([',', ';', ':']).len());
        }
        result.push('…');
    }
    result
}

fn name(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("pack")
        .rsplit('/')
        .next()
        .unwrap_or("pack")
        .to_owned()
}

/// The complete form message: a question, the summary and what each answer does.
pub(super) fn message(proposal: &Proposal, args: &Value) -> String {
    format!(
        "Approve this Shield policy change?\n\n{}\nAccept applies this change to this project. Decline leaves its policy unchanged.",
        summary(proposal, args)
    )
}

/// One line naming the first changes, for a short tool-result preview.
pub(super) fn headline(changes: &Value) -> Option<String> {
    let changes = changes.as_array().filter(|changes| !changes.is_empty())?;
    let [change] = changes.as_slice() else {
        return Some(batch_line(changes));
    };
    let mut lines: Vec<_> = change_lines(change).into_iter().collect();
    lines.truncate(2);
    Some(lines.join(" "))
}

/// The plain-language change summary, without a question or answer labels, for
/// hosts whose own prompt supplies them.
pub(super) fn summary(proposal: &Proposal, args: &Value) -> String {
    let mut lines = Vec::new();
    let changes = args["changes"].as_array().map_or(&[][..], Vec::as_slice);
    // Several changes lead with one line naming them all, which is what a
    // host showing only the first line can display.
    if changes.len() > 1 {
        lines.push(batch_line(changes));
    }
    for change in changes.iter().take(6) {
        lines.extend(change_lines(change));
    }
    if changes.len() > 6 {
        lines.push(format!(
            "Plus {} additional changes in this proposal.",
            changes.len() - 6
        ));
    }
    // Permission descriptions come from the candidate's exact acquired pack bytes.
    for change in changes
        .iter()
        .filter(|c| c["kind"] == "pack" && c["change"] != "removed" && c["change"] != "disabled")
        .take(2)
    {
        let pack = proposal.candidate["packs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|source| crate::policy::parse::<Value>(source.as_str()?).ok())
            .find(|pack| pack["id"] == change["id"] && pack["version"] == change["version"]);
        if let Some(pack) = pack {
            lines.extend(pack_lines(
                &pack,
                change["profile"].as_str().unwrap_or("recommended"),
            ));
        }
    }
    let comparison = &args["review"]["comparison"];
    lines.push(String::new());
    if comparison["complete"] != true {
        // Name what was not compared, and whether the examples are real changes.
        let examples = !comparison["expansion"].is_null() || !comparison["restriction"].is_null();
        let unsupported = comparison["incomplete_reason"] == "unsupported-semantic-domain";
        // Examples without a change prove nothing, so no claim is made then.
        lines.push(match (unsupported, examples) {
            (true, true) => "Shield compared example commands only; file, web and pack precondition rules are not compared, so the changes below are examples, not a complete list.",
            (true, false) => "Shield could not compare this change: rules for files, web requests and pack preconditions are not compared.",
            (false, true) => "Shield could not compare every command, so the changes below are examples, not a complete list.",
            (false, false) => "Shield could not compare every command; the summary is not a guarantee for every command.",
        }.into());
    } else if comparison["relation"] == "equivalent" {
        lines.push("No command decision changes were found in the supported comparison.".into());
    }
    for (kind, label) in [
        ("expansion", "More permission"),
        ("restriction", "Less permission"),
    ] {
        let witness = &comparison[kind];
        if !witness.is_null() {
            lines.push(format!(
                "{label}: {} ({} → {}).",
                command(&witness["request"]),
                text(&witness["before"]),
                text(&witness["after"])
            ));
        }
    }
    if !comparison["codex_prefixes"].is_null() {
        lines.push("This also changes Codex command prefixes, which can let matching commands run outside its sandbox.".into());
    }
    for line in comparison["formatting"].as_array().into_iter().flatten() {
        lines.push(text(line));
    }
    lines.join("\n").trim_end().to_owned()
}

fn change_lines(change: &Value) -> Vec<String> {
    let local = if change["layer"] == "local" {
        " (local only)"
    } else {
        ""
    };
    let line = match change["kind"].as_str() {
        Some("pack") if change["change"] == "mode" || change["change"] == "role" => {
            let to = if change["change"] == "role" {
                format!("a {} selection", text(&change["role"]))
            } else {
                text(&change["mode"])
            };
            format!(
                "Change the {} rule pack to {to} (was {}){local}.",
                name(&change["id"]),
                text(&change["from"])
            )
        }
        Some("pack") => {
            let action = match change["change"].as_str() {
                Some("added") => "Add",
                Some("removed") => "Remove",
                Some("disabled") => "Disable",
                Some("profile") => "Change profile for",
                _ => "Update",
            };
            let from = if change["change"] == "profile" {
                format!(" (was {})", text(&change["from"]))
            } else if change["change"] == "updated" {
                format!(" (was version {})", text(&change["from"]))
            } else {
                String::new()
            };
            format!(
                "{action} the {} rule pack — version {}, {} profile{from}{local}.",
                name(&change["id"]),
                text(&change["version"]),
                text(&change["profile"])
            )
        }
        Some("rule") => return scope::rule_lines(change, local),
        Some("access_paths") => {
            let mut lines = Vec::new();
            for (access, label) in [
                ("read", "read files in"),
                ("write", "read and change files in"),
            ] {
                let old = change["before"][access]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                let new = change["after"][access]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                for (from, to, verb) in [
                    (new, old, "Allow agents to"),
                    (old, new, "Remove permission to"),
                ] {
                    let count = from.iter().filter(|path| !to.contains(path)).count();
                    for path in from.iter().filter(|path| !to.contains(path)).take(4) {
                        lines.push(format!("{verb} {label} {}{local}.", text(path)));
                    }
                    if count > 4 {
                        lines.push(format!("Plus {} more {access} folder changes.", count - 4));
                    }
                }
            }
            return lines;
        }
        Some("variable" | "override") => format!(
            "{}: {} {} for the {} rule pack{local} ({} → {}).{}",
            if change["kind"] == "variable" {
                "Setting"
            } else {
                "Rule override"
            },
            text(&change["name"]),
            text(&change["change"]),
            name(&change["id"]),
            text(&change["before"]),
            text(&change["after"]),
            if change["widens"] == true {
                " This broadens permission."
            } else {
                ""
            }
        ),
        _ => "Additional policy configuration changes are included.".into(),
    };
    vec![line]
}

/// "3 changes: allow `npm run dev`; add the node rule pack; …" in plain words.
fn batch_line(changes: &[Value]) -> String {
    let shown: Vec<_> = changes.iter().take(6).map(brief).collect();
    let more = if changes.len() > 6 { "; …" } else { "" };
    format!("{} changes: {}{more}.", changes.len(), shown.join("; "))
}

/// A few words naming one change. Rule names and reasons come from the agent,
/// so a rule is named by its decision and command.
fn brief(change: &Value) -> String {
    match change["kind"].as_str() {
        Some("pack") => {
            let action = match change["change"].as_str() {
                Some("added") => "add",
                Some("removed") => "remove",
                Some("disabled") => "disable",
                Some("profile") => "change the profile of",
                _ => "update",
            };
            format!("{action} the {} rule pack", name(&change["id"]))
        }
        Some("rule") => {
            let rule = if change["rule"].is_object() {
                &change["rule"]
            } else {
                &change["after"]
            };
            let conditions = &rule["conditions"];
            match (rule["decision"].as_str(), conditions["executable"].as_str()) {
                (Some(decision), Some(executable)) if change["change"] != "removed" => {
                    if let Some(command) = scope::closed_command(rule) {
                        return format!("{decision} {command}");
                    }
                    let arguments = &conditions["arguments"];
                    let mut words = vec![executable.to_owned()];
                    words.extend(
                        arguments["values"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .take(6)
                            .map(text),
                    );
                    let shown = text(&Value::String(words.join(" ")));
                    // Only exact arguments name the whole command; anything
                    // else must not read as every use of the executable.
                    match arguments["kind"].as_str() {
                        Some("exact") => format!("{decision} `{shown}`"),
                        Some("prefix") => format!("{decision} `{shown} …`"),
                        None | Some("any") => format!("{decision} any `{executable}`"),
                        Some(_) => format!("{decision} limited `{executable}` forms"),
                    }
                }
                _ => format!("{} rule", text(&change["change"])),
            }
        }
        Some("access_paths") => "change folder access".into(),
        _ => "change pack settings".into(),
    }
}

fn command(request: &Value) -> String {
    let mut words = vec![text(&request["executable"])];
    words.extend(
        request["arguments"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .map(text),
    );
    text(&Value::String(words.join(" ")))
}

fn pack_lines(pack: &Value, profile: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if pack["description"].is_string() {
        lines.push(text(&pack["description"]));
    }
    for (decision, label) in [
        ("allow", "Without asking"),
        ("ask", "Still asks"),
        ("deny", "Blocks"),
    ] {
        // Lead with the descriptions that cover the most rules, which name the
        // pack's main permissions; ties keep the pack author's order.
        let mut reasons: Vec<(String, usize)> = Vec::new();
        for reason in pack["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|rule| rule["decision"] == decision || rule["decision"][profile] == decision)
            .filter_map(|rule| rule["reason"].as_str())
        {
            let reason = text(&Value::String(reason.into()));
            match reasons.iter_mut().find(|(known, _)| *known == reason) {
                Some((_, count)) => *count += 1,
                None => reasons.push((reason, 1)),
            }
        }
        reasons.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        if !reasons.is_empty() {
            let shown = reasons
                .iter()
                .take(2)
                .map(|(reason, _)| reason.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(format!(
                "{label}: {shown}{}",
                if reasons.len() > 2 {
                    " Other rules also apply."
                } else {
                    ""
                }
            ));
        }
    }
    if !lines.is_empty() {
        lines.push("These outcomes apply only when the pack's conditions and requirements are met; other active rules still apply.".into());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn confirmation_leads_with_permission_scope_instead_of_rule_names() {
        let lines = change_lines(&json!({
            "kind":"rule", "change":"added", "layer":"local",
            "rule":{"id":"safe-git", "decision":"ask", "conditions":{
                "domain":"argv", "executable":"git", "arguments":{
                    "kind":"risk_tokens", "prefix":["push"], "any_of":["--force"]
                }
            }}
        }));
        assert!(lines[0].starts_with("added — ask for git (argument prefix: ‘push’"));
        assert!(lines[0].contains("at least one remaining token matching --force"));
        assert!(lines[0].ends_with(" (local only)."));
        assert!(lines.contains(&"Rule ID: safe-git (added).".into()));
        assert!(!lines[0].contains('{'));
    }

    #[test]
    fn confirmation_settings_and_command_selectors_are_plain_text() {
        let lines = change_lines(
            &json!({"kind":"variable","change":"updated","id":"canonic/node","name":"folders","before":["src", "test"],"after":["src", "test", "scripts"]}),
        );
        assert!(lines[0].contains("src, test → src, test, scripts"));
        let selector = command(&json!({"executable":{"any":["npm","pnpm"]}, "arguments":[]}));
        assert_eq!(selector, "any: npm, pnpm");
        assert!(!lines[0].contains('['));
        assert!(!selector.contains('{'));
    }

    #[test]
    fn confirmation_pack_profile_describes_permissions_without_internal_json() {
        let pack = json!({"rules":[
            {"decision":{"recommended":"allow"},"reason":"Install local dependencies, including lifecycle scripts."},
            {"decision":{"recommended":"ask"},"reason":"Project scripts require review."},
            {"decision":{"recommended":"deny"},"reason":"Global installation changes the host."}
        ]});
        let mut lines = change_lines(
            &json!({"kind":"pack","change":"added","id":"canonic/node","version":"0.1.0","profile":"recommended"}),
        );
        lines.extend(pack_lines(&pack, "recommended"));
        let message = lines.join("\n");
        assert!(
            message.starts_with("Add the node rule pack — version 0.1.0, recommended profile.")
        );
        assert!(
            message.contains(
                "Without asking: Install local dependencies, including lifecycle scripts."
            )
        );
        assert!(message.contains("Still asks: Project scripts require review."));
        assert!(message.contains("Blocks: Global installation changes the host."));
        assert!(!message.contains('{'));
    }

    #[test]
    fn confirmation_pack_lines_lead_with_the_most_covered_permissions() {
        let allow = |reason: &str| json!({"decision":{"recommended":"allow"},"reason":reason});
        let pack = json!({"rules":[
            allow("Create a default project manifest."),
            allow("Install local dependencies."),
            allow("Install local dependencies."),
            allow("Inspect dependency metadata."),
            allow("Inspect dependency metadata."),
            allow("Add tags.")
        ]});
        assert_eq!(
            pack_lines(&pack, "recommended")[0],
            "Without asking: Install local dependencies. Inspect dependency metadata. Other rules also apply."
        );
    }

    #[test]
    fn confirmation_shortens_long_text_at_a_word_boundary() {
        let long = format!("{} skipping hooks.", "Rewrite history safely, ".repeat(8));
        let shown = text(&json!(long));
        assert!(shown.ends_with("safely, Rewrite…"), "{shown}");
        assert!(shown.chars().count() <= 181);
        assert!(text(&json!("x".repeat(200))).ends_with("x…"));
    }

    #[test]
    fn confirmation_closed_npm_forms_show_names_directory_and_script_effects() {
        let rule = |prefix| {
            json!({"kind":"rule","change":"added","rule":{
                "id":"project-scripts","decision":"allow","conditions":{
                    "domain":"argv","executable":"npm","cwd":{"exact":{"root":"project"}},
                    "arguments":{"kind":"options","prefix":prefix,"flags":[],"values":[],"operands":false}
                }
            }})
        };
        let scripts = rule(json!(["run",{"one_of":["dev","start","test","lint","format"]}]));
        let test = rule(json!(["test"]));
        let headline = headline(&json!([scripts, test])).unwrap();
        for expected in [
            "allow `npm run (dev|start|test|lint|format)`",
            "allow `npm test`",
            "no extra arguments",
            "working directory: exactly project directory",
            "future script edits and lifecycle hooks inherit this permission",
        ] {
            assert!(headline.contains(expected), "{headline}");
        }
        let lines = change_lines(&scripts);
        assert!(lines[0].contains("`npm run (dev|start|test|lint|format)`"));
        assert!(lines[0].contains("exactly project directory"));
        assert!(lines[0].contains("lifecycle hooks"));
    }

    #[test]
    fn confirmation_closed_forms_do_not_hide_options_or_selector_constraints() {
        let base = json!({"decision":"allow","conditions":{
            "domain":"argv","executable":"acme","arguments":{
                "kind":"options","prefix":["a b",{"one_of":["x","y"]}],
                "flags":[],"values":[],"operands":false
            }
        }});
        assert_eq!(
            scope::closed_command(&base).unwrap(),
            "`acme 'a b' (x|y)`; no extra arguments"
        );
        for (key, value) in [
            ("flags", json!(["--watch"])),
            ("values", json!(["--output"])),
            ("operands", json!(true)),
            ("leading", json!({"flags":["--verbose"]})),
            ("require_any", json!(["--watch"])),
            ("prefix", json!([{"glob":"*"}])),
            ("prefix", json!(["x".repeat(200)])),
        ] {
            let mut rule = base.clone();
            rule["conditions"]["arguments"][key] = value;
            assert!(scope::closed_command(&rule).is_none(), "{rule}");
        }
        let removed = json!({"kind":"rule","change":"removed","rule":base});
        assert_eq!(brief(&removed), "removed rule");
    }

    #[test]
    fn headline_describes_one_change_and_counts_and_names_several() {
        let pack = json!({"kind":"pack","change":"added","id":"acme/runtime","version":"1.0.0","profile":"recommended"});
        assert_eq!(
            headline(&json!([pack])).unwrap(),
            "Add the runtime rule pack — version 1.0.0, recommended profile."
        );
        assert_eq!(
            headline(&json!([pack, pack, pack])).unwrap(),
            "3 changes: add the runtime rule pack; add the runtime rule pack; add the runtime rule pack."
        );
        assert_eq!(headline(&json!([])), None);
    }

    #[test]
    fn confirmation_names_folder_permissions_and_local_scope() {
        let lines = change_lines(
            &json!({"kind":"access_paths","layer":"local","before":{"read":["/srv/old"]},"after":{"write":["/srv/acme"]}}),
        );
        assert!(lines.contains(
            &"Allow agents to read and change files in /srv/acme (local only).".to_owned()
        ));
        assert!(
            lines.contains(&"Remove permission to read files in /srv/old (local only).".to_owned())
        );
    }
}
