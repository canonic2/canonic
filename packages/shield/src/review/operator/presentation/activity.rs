//! Views of what Shield is doing: status and diagnostics, the compiled cache,
//! approval requests and decisions, and request history.
use super::{command_line, label, plural, short, time_ms};
use serde_json::Value;
use std::fmt::Write;

fn agent_name(agent: &Value) -> String {
    agent
        .as_str()
        .map_or_else(|| label(agent), |name| crate::agents::label(name).into())
}

/// `shield status` and, with `doctor`, `shield doctor`.
pub(super) fn status(value: &Value, doctor: bool) -> String {
    let coverage = &value["coverage"];
    let agents: Vec<_> = coverage["installed_integrations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let mut name = agent_name(&row["agent"]);
            if row["status"] != "installed" {
                name.push_str(" (needs repair)");
            } else if row["ask_fallback"] == "allow" {
                name.push_str(" (Ask fallback allows requests)");
            }
            name
        })
        .collect();
    let root = label(&value["project_root"]);
    let mut output = if agents.is_empty() {
        format!(
            "Shield policy is active in {root}, but no agent is connected. Connect one with shield mcp setup.\n"
        )
    } else {
        format!("Shield is active in {root} for {}.\n", agents.join(" and "))
    };
    let packs = value["installed_packs"]["packs"]
        .as_array()
        .map_or(0, Vec::len);
    let _ = writeln!(
        output,
        "Policy: {} · revision {}",
        plural(packs, "pack", "packs"),
        short(&value["active"]["policy_revision"])
    );
    let history = &value["history"];
    let _ = writeln!(
        output,
        "History: {}",
        if history["available"] == false {
            "unavailable".into()
        } else if history["enabled"] == true {
            format!("on, keeping up to {} entries", label(&history["limit"]))
        } else {
            "off".into()
        }
    );
    let access = &value["path_access"];
    let relaxed: Vec<_> = [
        ("agent_settings_read", "read agent settings without asking"),
        ("agent_settings_change", "change agent settings"),
        ("shield_config_change", "change Shield policy files"),
    ]
    .into_iter()
    .filter(|(key, _)| access[*key] == "allow")
    .map(|(_, text)| text)
    .collect();
    if !access["error"].is_null() {
        output.push_str(
            "Protected file access: invalid record; agents are blocked. Run shield protection set.\n",
        );
    } else if !relaxed.is_empty() {
        let _ = writeln!(output, "Agents may {}.", relaxed.join(", "));
    }
    if value["pending_change"] == true {
        output.push_str(
            "A proposed policy change is waiting: review it with shield policy review.\n",
        );
    }
    if let Some(count) = value["pending_approvals"].as_u64().filter(|n| *n > 0) {
        let _ = writeln!(
            output,
            "{} waiting: review with shield request list.",
            plural(
                usize::try_from(count).unwrap_or(usize::MAX),
                "action request is",
                "action requests are"
            )
        );
    }
    if value["repair_needed"] == true {
        let _ = writeln!(
            output,
            "Project files need repair: {}.",
            if doctor {
                format!("run {}", label(&value["repair_route"]))
            } else {
                "run shield doctor".into()
            }
        );
    }
    if doctor {
        output.push_str(&diagnostics(value, !agents.is_empty()));
    }
    output
}

/// The extra facts `shield doctor` reports.
fn diagnostics(value: &Value, connected: bool) -> String {
    let coverage = &value["coverage"];
    let mut output = String::new();
    if connected && coverage["live_host_verification"] == "unverified" {
        output.push_str("Host loading: unverified. Restart each agent, trust this project and approve Shield's hooks and MCP server.\n");
    }
    for diagnostic in coverage["diagnostics"].as_array().into_iter().flatten() {
        let _ = writeln!(output, "Problem: {}", label(diagnostic));
    }
    for check in value["checks"].as_array().into_iter().flatten() {
        let _ = writeln!(
            output,
            "Problem: {}; run {}.",
            label(&check["detail"]),
            label(&check["repair"])
        );
    }
    let cache = &value["compiled_cache"];
    if !cache.is_null() && cache["presence"] != "present" {
        let _ = writeln!(
            output,
            "Compiled policy: {}; run {}.",
            label(&cache["presence"]),
            label(&cache["repair_route"])
        );
    } else if !cache.is_null() && cache["compatibility"] == "rebuild-required" {
        let _ = writeln!(
            output,
            "Compiled policy: needs rebuilding for this Shield version and policy; run {}.",
            label(&cache["repair_route"])
        );
    }
    if coverage["operator_protection"].is_string() {
        let _ = writeln!(
            output,
            "Limits: {}.",
            label(&coverage["operator_protection"])
        );
    }
    output
}

/// `shield cache prepare`.
pub(super) fn cache(value: &Value) -> String {
    if value["prepared"] != true {
        return super::fallback(value);
    }
    format!(
        "Compiled policy is ready in {}.\n",
        label(&value["cache_directory"])
    )
}

/// What a request asks to do: a command it would run or a file it would read.
fn action(request: &Value) -> String {
    let inner = if request["request"].is_object() {
        &request["request"]
    } else {
        request
    };
    let read = request["type"] == "read"
        || request["request_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("read-request"));
    if read {
        let paths: Vec<_> = inner["arguments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(label)
            .collect();
        return format!("read {}", paths.join(" "));
    }
    if inner["executable"].is_string() {
        let mut text = format!("run {}", command_line(inner));
        if inner["cwd"].is_string() {
            let _ = write!(text, " in {}", label(&inner["cwd"]));
        }
        text
    } else if inner["path"].is_string() {
        format!("read {}", label(&inner["path"]))
    } else {
        "an action".into()
    }
}

/// `shield request list`.
pub(super) fn approvals(value: &Value) -> String {
    let rows = value["requests"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "No requests are waiting and no approvals are unused.\n".into();
    }
    let mut output = String::new();
    for row in &rows {
        let expires = if row["status"] == "approved" {
            &row["grant_expires_at_ms"]
        } else {
            &row["pending_expires_at_ms"]
        };
        let _ = writeln!(
            output,
            "{}  {} · {} until {}",
            label(&row["request_id"]),
            action(row),
            label(&row["status"]),
            time_ms(expires)
        );
    }
    output.push_str("Inspect one with shield request show <request>; decide with shield request approve or shield request deny.\n");
    output
}

/// Who made a request and through which surface.
fn requester(provenance: &Value) -> String {
    let agent = if provenance["agent"].is_null() {
        "an unidentified agent".into()
    } else {
        agent_name(&provenance["agent"])
    };
    format!("{agent} via {}", label(&provenance["surface"]))
}

/// `shield request show`.
pub(super) fn request(value: &Value) -> String {
    let id = label(&value["request_id"]);
    let (status, until) = if value["status"] == "approved" {
        ("approved and unused", &value["grant"]["expires_at_ms"])
    } else {
        (
            value["status"].as_str().unwrap_or("unknown"),
            &value["pending_expires_at_ms"],
        )
    };
    let mut output = format!(
        "Request {id} to {}\n{} · from {} · until {}\n",
        action(value),
        status,
        requester(&value["provenance"]),
        time_ms(until)
    );
    let provenance = &value["provenance"];
    let _ = write!(
        output,
        "Requested {} in {}",
        time_ms(&value["created_at_ms"]),
        label(&provenance["cwd"])
    );
    if provenance["session_id"].is_string() {
        let _ = write!(output, ", session {}", label(&provenance["session_id"]));
    }
    output.push('\n');
    let policy = &value["policy"];
    if policy["decision"].as_str().is_some_and(|d| !d.is_empty()) {
        let _ = write!(output, "Policy: {}", label(&policy["decision"]));
        if policy["reason"].as_str().is_some_and(|r| !r.is_empty()) {
            let _ = write!(output, " ({})", label(&policy["reason"]));
        }
        output.push('\n');
        for rule in policy["rules"].as_array().into_iter().flatten() {
            let _ = writeln!(
                output,
                "Rule {}: {}",
                label(&rule["rule_id"]),
                label(&rule["reason"])
            );
        }
    }
    if value["status"] == "pending" {
        let _ = writeln!(
            output,
            "Approve it with shield request approve {id}, or deny it with shield request deny {id}."
        );
    }
    output
}

/// The review before approving or denying a request.
pub(super) fn review(value: &Value) -> Option<String> {
    let approving = match value["operation"].as_str()? {
        "approve once" => true,
        "deny/revoke" => false,
        _ => return None,
    };
    let request = &value["request"];
    Some(if approving {
        format!(
            "Approve once: {}\n  Requested by {}. One use within five minutes, for exactly this request.\n",
            action(request),
            requester(&request["provenance"])
        )
    } else {
        format!(
            "Deny: {}\n  Requested by {}. A pending request is denied; an unused approval is revoked.\n",
            action(request),
            requester(&request["provenance"])
        )
    })
}

/// The receipt of an approval decision.
pub(super) fn decided(value: &Value) -> String {
    let id = label(&value["request_id"]);
    match value["status"].as_str() {
        Some("approved") => format!(
            "Approved {id} once; it can run until {}.\n",
            time_ms(&value["expires_at_ms"])
        ),
        Some("revoked") => format!("Revoked the unused approval for {id}.\n"),
        Some("denied") => format!("Denied {id}.\n"),
        _ => super::fallback(value),
    }
}

/// The shell source an agent submitted, kept even when the decision record was
/// cut short for size.
fn shell_source(entry: &Value) -> Option<&str> {
    let command = &entry["command"];
    (command["kind"] == "shell")
        .then(|| command["source"].as_str())
        .flatten()
}

/// What a history entry ran or asked to run, on one line: a multiline shell
/// command shows its first line.
fn request_summary(entry: &Value) -> String {
    let operation = &entry["command"];
    if let Some(source) = shell_source(entry) {
        let mut lines = source.trim().lines();
        let mut text = label(&Value::from(lines.next().unwrap_or_default()));
        if lines.next().is_some() {
            text.push_str(" …");
        }
        text
    } else if operation["kind"] == "file" && operation["path"].is_string() {
        let mut text = format!(
            "{} {}",
            label(&operation["action"]),
            label(&operation["path"])
        );
        if operation["recursive"] == true {
            text.push_str(" (recursive)");
        }
        text
    } else if operation["kind"] == "web" {
        match (operation["action"].as_str(), operation["url"].as_str()) {
            (Some("fetch"), Some(_)) => format!("fetch {}", label(&operation["url"])),
            _ => format!("web search {}", label(&operation["query"])),
        }
    } else if operation["kind"] == "unsupported" && operation["name"].is_string() {
        format!("{} (unsupported tool request)", label(&operation["name"]))
    } else if operation["request"].is_object() {
        command_line(&operation["request"])
    } else if operation.is_string() {
        label(operation)
    } else if entry["request"].is_object() {
        command_line(&entry["request"])
    } else {
        label(&operation["kind"])
    }
}

/// The policy decision, followed by the operator's when there was one.
fn decision(entry: &Value) -> String {
    let evaluation = &entry["evaluation"];
    let mut text = label(if evaluation["decision"].is_null() {
        &evaluation["evaluation"]["decision"]
    } else {
        &evaluation["decision"]
    });
    if entry["disposition"] == "blocked" {
        text.push_str(if entry["code"] == "request-rewrite" {
            " → blocked (rewrite required)"
        } else {
            " → blocked"
        });
    } else if entry["approved"] == true {
        text.push_str(" → approved");
    } else if entry["denied"] == true && text != "deny" {
        text.push_str(" → denied");
    }
    text
}

/// `shield history list`.
pub(super) fn history(value: &Value) -> String {
    let rows = value["entries"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "No matching request history.\n".into();
    }
    let mut output = String::new();
    for row in &rows {
        let _ = writeln!(
            output,
            "{}  {}  {}  {}",
            label(&row["entry_id"]),
            label(&row["requested_at"]),
            decision(row),
            request_summary(row)
        );
    }
    output.push_str("Show one with shield history show <entry>.\n");
    output
}

/// `shield history show`.
pub(super) fn entry(value: &Value) -> String {
    let provenance = &value["provenance"];
    let mut output = format!(
        "{} · {}\nRequested {} by {} in {}\n",
        request_summary(value),
        decision(value),
        label(&value["requested_at"]),
        requester(provenance),
        label(&provenance["cwd"])
    );
    if let Some(source) = shell_source(value).filter(|s| s.trim().contains('\n')) {
        output.push_str("Command:\n");
        for line in source.trim().lines() {
            let _ = writeln!(output, "  {}", label(&Value::from(line)));
        }
    }
    // Agent hook entries keep the explanation; native runs keep the evaluation.
    let evaluation = &value["evaluation"];
    let explanation = if evaluation["explanation"].is_object() {
        &evaluation["explanation"]
    } else if evaluation["evaluation"].is_object() {
        &evaluation["evaluation"]
    } else {
        evaluation
    };
    let decided = label(&explanation["decision"]);
    let matched = explanation["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["applied"] == true)
        .find(|m| m["decision"].is_null() || label(&m["decision"]) == decided)
        .or_else(|| {
            explanation["matches"]
                .as_array()?
                .iter()
                .find(|m| m["applied"] == true)
        });
    if let Some(rule) = matched {
        let _ = writeln!(
            output,
            "Rule {}: {}",
            label(&rule["rule_id"]),
            label(&rule["reason"])
        );
    }
    if let Some(detail) = crate::agent_message::guidance_detail(explanation) {
        let _ = writeln!(output, "Reason: {}", label(&Value::String(detail)));
    }
    let outcome = &value["outcome"];
    if outcome == "unobserved" {
        output.push_str(if value["disposition"] == "blocked" {
            "Outcome: did not run; Shield blocked it\n"
        } else {
            "Outcome: not observed\n"
        });
    } else if outcome.is_object() {
        // A host that reports a shell call as finished without its exit
        // status leaves it unknown.
        let status = if outcome["status"] == "unknown" && outcome["exit_code"].is_null() {
            "finished; the agent did not report an exit code".into()
        } else {
            label(&outcome["status"])
        };
        let _ = write!(output, "Outcome: {status}");
        if !outcome["exit_code"].is_null() {
            let _ = write!(output, ", exit {}", label(&outcome["exit_code"]));
        }
        if !outcome["signal"].is_null() {
            let _ = write!(output, ", signal {}", label(&outcome["signal"]));
        }
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_leads_with_protection_and_names_missing_agents() {
        let text = status(
            &json!({"project_root":"/srv/acme","active":{"policy_revision":"0123456789abcdef"},
                "installed_packs":{"packs":[{}]},"history":{"enabled":true,"limit":5000},"coverage":{"installed_integrations":[]}}),
            false,
        );
        assert_eq!(
            text,
            "Shield policy is active in /srv/acme, but no agent is connected. Connect one with shield mcp setup.\nPolicy: 1 pack · revision 0123456789ab\nHistory: on, keeping up to 5000 entries\n"
        );
    }

    #[test]
    fn status_points_pending_action_requests_to_approvals() {
        let mut value =
            json!({"project_root":"/srv/acme","coverage":{"installed_integrations":[]}});
        for (count, line) in [
            (
                1,
                "1 action request is waiting: review with shield request list.\n",
            ),
            (
                2,
                "2 action requests are waiting: review with shield request list.\n",
            ),
        ] {
            value["pending_approvals"] = json!(count);
            assert!(status(&value, false).contains(line), "{count}");
        }
        value["pending_approvals"] = json!(0);
        assert!(!status(&value, false).contains("shield request list"));
    }

    #[test]
    fn requests_and_history_describe_the_command_and_decision() {
        let request = json!({"request_id":"run-1","status":"pending","created_at_ms":0,"pending_expires_at_ms":600_000,
            "provenance":{"agent":"codex","surface":"hook","cwd":"/srv/acme","session_id":"acme-session"},
            "policy":{"decision":"ask","reason":"matched-rule","rules":[
                {"source":"project","rule_id":"acme-echo","reason":"Review Acme echoes."}]},
            "request":{"executable":"/bin/echo","arguments":["Acme"],"cwd":"/srv/acme"}});
        assert_eq!(
            self::request(&request),
            "Request run-1 to run /bin/echo Acme in /srv/acme\npending · from Codex via hook · until 1970-01-01T00:10:00Z\n\
             Requested 1970-01-01T00:00:00Z in /srv/acme, session acme-session\nPolicy: ask (matched-rule)\n\
             Rule acme-echo: Review Acme echoes.\nApprove it with shield request approve run-1, or deny it with shield request deny run-1.\n"
        );
        let entry = json!({"entry_id":"run-1","requested_at":"2026-01-01T00:00:00Z","approved":true,
            "evaluation":{"decision":"ask"},"command":{"request":{"executable":"/bin/echo","arguments":["Acme"]}}});
        assert_eq!(
            history(&json!({"entries":[entry]})),
            "run-1  2026-01-01T00:00:00Z  ask → approved  /bin/echo Acme\nShow one with shield history show <entry>.\n"
        );
    }

    #[test]
    fn shell_history_shows_submitted_source_and_unreported_exit() {
        // A decision record cut short for size keeps no request, only the source.
        let mut row = json!({"entry_id":"request-acme","requested_at":"2026-01-01T00:00:00Z",
            "evaluation":{"decision":"ask"},"request":null,"provenance":{"agent":"claude-code","surface":"hook"},
            "command":{"kind":"shell","dialect":"bash","source":"mkdir -p src\ncat > src/app.js <<'EOF'\nacme\nEOF\n"},
            "outcome":{"status":"unknown","exit_code":null}});
        assert_eq!(
            history(&json!({"entries":[row]})),
            "request-acme  2026-01-01T00:00:00Z  ask  mkdir -p src …\nShow one with shield history show <entry>.\n"
        );
        let shown = entry(&row);
        assert!(shown.starts_with("mkdir -p src … · ask\n"));
        assert!(
            shown.contains("Command:\n  mkdir -p src\n  cat > src/app.js <<'EOF'\n  acme\n  EOF\n")
        );
        assert!(shown.ends_with("Outcome: finished; the agent did not report an exit code\n"));
        row["command"]["source"] = json!("git status --short");
        row["outcome"] = json!({"status":"failed","exit_code":1});
        let shown = entry(&row);
        assert!(shown.starts_with("git status --short · ask\n"));
        assert!(!shown.contains("Command:"));
        assert!(shown.ends_with("Outcome: failed, exit 1\n"));
        row["disposition"] = json!("blocked");
        row["evaluation"]["decision"] = json!("deny");
        row["outcome"] = json!("unobserved");
        assert!(entry(&row).ends_with("Outcome: did not run; Shield blocked it\n"));
        row["disposition"] = json!("approval_required");
        assert!(entry(&row).ends_with("Outcome: not observed\n"));
    }

    #[test]
    fn hook_history_names_the_rule_that_decided() {
        let row = json!({"entry_id":"request-acme","requested_at":"2026-01-01T00:00:00Z",
            "disposition":"blocked","code":"policy-deny","outcome":"unobserved",
            "evaluation":{"decision":"deny","explanation":{"decision":"deny","guidance":[],"requirement_checks":[],
                "matches":[{"rule_id":"acme-read","decision":"allow","applied":true,"reason":"Reads are routine."},
                    {"rule_id":"acme-config","decision":"deny","applied":true,"reason":"Configuration changes need the user."}]}},
            "command":{"kind":"shell","source":"cat acme.txt && acme config set x"}});
        let shown = entry(&row);
        assert!(
            shown.contains("\nRule acme-config: Configuration changes need the user.\n"),
            "{shown}"
        );
        assert!(!shown.contains("Reason:"), "{shown}");
    }

    fn chain_history_fixture() -> (assert_fs::TempDir, crate::review::Context, Value) {
        use crate::{configuration, policy::normalized, review};
        use std::collections::BTreeMap;

        let rules = json!({"schema_version":1,"kind":"custom",
            "capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{
                "id":"acme-scripts","decision":"ask","reason":"Review Acme scripts.",
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]});
        let bundle = configuration::Bundle::capture(
            r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
            &[],
            Some(&rules.to_string()),
        )
        .unwrap();
        let candidates = BTreeMap::from([("project".into(), json!(bundle))]);
        let evaluator =
            configuration::Evaluator::new(review::snapshots(&candidates).unwrap()).unwrap();
        let root = assert_fs::TempDir::new().unwrap();
        review::initialize(root.path(), candidates).unwrap();
        let mut context = review::open(root.path()).unwrap();
        context.surface = "hook".into();
        context.source = "host".into();
        context.agent = Some("codex".into());
        context.session_id = Some("acme-chain".into());
        context.host_event_name = Some("PreToolUse".into());
        let input = normalized::Input::Shell {
            source: CHAIN.into(),
            dialect: "bash".into(),
            dialect_version: None,
            original_input: json!({}),
        };
        let evaluation = evaluator
            .evaluate_normalized(
                &input,
                &normalized::Context {
                    cwd: context.project_root.clone(),
                    environment: None,
                    working_files: None,
                },
                &context.project_root,
                None,
                "2026-01-01T00:00:00Z",
                &|| Ok(()),
            )
            .unwrap();
        (root, context, evaluation)
    }

    const CHAIN: &str = "acme one && acme two && acme three && acme four && acme five";

    #[test]
    fn recorded_shell_chains_keep_suggestions_and_rule_names_after_compaction() {
        use crate::review;

        let (_root, context, evaluation) = chain_history_fixture();
        // Exercise analysis-only reduction and the smaller evaluation format.
        for compact in [false, true] {
            review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
            let mut evaluation = evaluation.clone();
            evaluation["analysis_coverage"]["padding"] = json!("x".repeat(9000));
            if compact {
                evaluation["explanation"]["padding"] = json!("x".repeat(9000));
            }
            let receipt = review::observations::decision(
                &context,
                json!({"decision":"ask","operation":{"kind":"shell","source":CHAIN},
                    "evaluation":evaluation,"disposition":"approval_required"}),
            )
            .unwrap();
            let suggestions = review::mcp::call(
                &context,
                &review::mcp::Connection::new("acme-chain".into()),
                "suggestion_list",
                &json!({"session_id":"acme-chain"}),
                &|| Ok(()),
            );
            let rows = suggestions["data"]["requests"].as_array().unwrap();
            assert_eq!(rows.len(), 5, "{suggestions}");
            for (row, argument) in rows.iter().zip(["one", "two", "three", "four", "five"]) {
                assert_eq!(row["command"], format!("acme {argument}"));
                assert_eq!(row["request"]["arguments"], json!([argument]));
            }
            let row = review::history::inspect(
                &context,
                &review::history::Query {
                    entry: receipt["request_id"].as_str(),
                    denied: false,
                    approved: false,
                    agent: None,
                },
            )
            .unwrap();
            assert_eq!(row["truncated"], true);
            if compact {
                assert!(row["evaluation"].get("explanation").is_none());
            }
            let shown = entry(&row);
            assert!(
                shown.contains("Rule acme-scripts: Review Acme scripts.\n"),
                "{shown}"
            );
        }
    }

    #[test]
    fn file_history_names_the_action_and_path() {
        let row = json!({"entry_id":"request-acme","requested_at":"2026-01-01T00:00:00Z",
            "evaluation":{"decision":"allow"},
            "command":{"kind":"file","action":"write","path":"/srv/acme/app.js","recursive":false},
            "request":{"executable":"shield:file-write","arguments":["/srv/acme/app.js"]}});
        assert_eq!(
            history(&json!({"entries":[row]})),
            "request-acme  2026-01-01T00:00:00Z  allow  write /srv/acme/app.js\nShow one with shield history show <entry>.\n"
        );
        for (operation, shown) in [
            (
                json!({"kind":"web","action":"fetch","url":"https://docs.example.com/","query":null}),
                "fetch https://docs.example.com/",
            ),
            (
                json!({"kind":"web","action":"search","url":null,"query":"acme"}),
                "web search acme",
            ),
            (
                json!({"kind":"unsupported","name":"webrun"}),
                "webrun (unsupported tool request)",
            ),
        ] {
            assert_eq!(self::request_summary(&json!({"command":operation})), shown);
        }
    }

    #[test]
    fn history_distinguishes_rewrite_blocks_from_approval_requests() {
        let mut row = json!({"entry_id":"request-acme","requested_at":"2026-01-01T00:00:00Z",
            "evaluation":{"decision":"ask","explanation":{"guidance":[{
                "message":"The loop cannot be resolved before it runs.",
                "alternatives":["Use literal arguments and separate simple commands."]}]}},
            "disposition":"blocked","code":"request-rewrite",
            "command":"for i in $(seq 1 20); do acme; done"});
        let list = history(&json!({"entries":[row]}));
        assert!(list.contains("ask → blocked (rewrite required)"));
        let shown = entry(&row);
        assert!(shown.contains("ask → blocked (rewrite required)"));
        assert!(shown.contains("The loop cannot be resolved before it runs."));
        assert!(shown.contains("Use literal arguments and separate simple commands."));
        row["disposition"] = json!("approval_required");
        row["code"] = json!("approval-required");
        assert_eq!(decision(&row), "ask");
        row["disposition"] = json!("blocked");
        row["code"] = json!("approval-unavailable");
        assert_eq!(decision(&row), "ask → blocked");
    }
}
