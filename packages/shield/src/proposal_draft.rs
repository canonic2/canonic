//! Observed command evidence and non-command proposals for agent policy review.
use serde_json::{Value, json};
use std::path::Path;

/// Whether a matched pack rule's guidance for this operation asks for a project
/// rule proposal, as pack rules for project scripts do. Standalone requests
/// share a null operation reference. Shield's own generic advice has no rule
/// reference and does not count.
pub(crate) fn invited(guidance: &Value, operation_ref: &Value) -> bool {
    guidance
        .as_array()
        .into_iter()
        .flatten()
        .filter(|guidance| guidance["operation_ref"] == *operation_ref)
        .filter(|guidance| guidance["rule_ref"]["scope"] == "pack")
        .flat_map(|guidance| guidance["alternatives"].as_array().into_iter().flatten())
        .any(|alternative| {
            alternative
                .as_str()
                .is_some_and(|text| text.contains("policy_propose"))
        })
}

/// Internal exact-command representation for evidence,
/// limited to the project when the request was normalized to run there.
pub(crate) fn decisive_command(guidance: &Value) -> Option<Value> {
    if guidance["source"] != "decisive" {
        return None;
    }
    let request = &guidance["request"];
    let arguments: Option<Vec<&str>> = request["arguments"]
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect();
    let mut change = command(request["executable"].as_str()?, &arguments?)?;
    if request["cwd"] == "project" {
        change["rule"]["conditions"]["cwd"] = json!({"within":{"root":"project"}});
        let reason = change["rule"]["reason"].as_str().unwrap_or_default();
        let reason = format!(
            "{} in the project without asking.",
            reason.trim_end_matches(" without asking.")
        );
        change["rule"]["reason"] = json!(reason);
    }
    Some(change)
}

/// An internal rule shape for command evidence; never offered to agents as a draft.
pub(crate) fn command(executable: &str, arguments: &[&str]) -> Option<Value> {
    if executable.is_empty()
        || arguments.len() > 64
        || [executable]
            .iter()
            .chain(arguments)
            .any(|word| word.len() > 512 || word.contains('\0'))
    {
        return None;
    }
    let shown = std::iter::once(executable)
        .chain(arguments.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    Some(json!({"kind":"rule_add","rule":{
        "id":identifier("allow", &shown),
        "decision":"allow",
        "reason":format!("Allow `{}` without asking.", excerpt(&shown)),
        "conditions":{"domain":"argv","executable":executable,
            "arguments":{"kind":"exact","values":arguments}}}}))
}

/// Installing a registry pack whose rules decide this kind of command.
pub(crate) fn pack(reference: &str) -> Value {
    json!({"kind":"pack_add","reference":reference})
}

/// A rule allowing every web search. A query can carry project text, so the
/// reason says so for the user reviewing it.
pub(crate) fn web_search() -> Value {
    json!({"kind":"rule_add","rule":{
        "id":"allow-web-search",
        "decision":"allow",
        "reason":"Allow web searches without asking; queries can include project text.",
        "conditions":{"domain":"web","action":"search","query":{"regex":"(?s).*"}}}})
}

/// Loopback names a local development server answers on.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// A rule allowing fetches from this URL's scheme and host, and its explicit
/// port when it names one. A loopback URL allows shell downloads from every
/// local server instead.
pub(crate) fn web_fetch(url: &str) -> Option<Value> {
    fetch(url, local_fetch)
}

/// [`web_fetch`] for an agent host's own web tool; a loopback URL allows that
/// tool's fetches from every local server.
pub(crate) fn host_web_fetch(url: &str) -> Option<Value> {
    fetch(url, local_host_fetch)
}

fn fetch(url: &str, local: fn() -> Value) -> Option<Value> {
    let parsed = url::Url::parse(url).ok()?;
    let scheme = parsed.scheme();
    let host = parsed.host_str()?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    if LOOPBACK.contains(&host) {
        return Some(local());
    }
    let mut url = json!({"schemes":[scheme],"hosts":[host]});
    if let Some(port) = parsed.port() {
        url["ports"] = json!([port]);
    }
    Some(json!({"kind":"rule_add","rule":{
        "id":identifier("allow-fetch", host),
        "decision":"allow",
        "reason":format!("Allow fetching pages from {host} without asking."),
        "conditions":{"domain":"web","action":"fetch","url":url}}}))
}

/// GET/HEAD downloads from local servers, without permission to follow redirects.
fn local_fetch() -> Value {
    json!({"kind":"rule_add","rule":{
        "id":"allow-fetch-localhost",
        "decision":"allow",
        "reason":"Allow GET/HEAD requests to local servers on any port without following redirects.",
        "conditions":{"domain":"web","action":"fetch",
            "request_fields":{"/client":{"exact":"shell"},"/redirects":{"exact":"none"}},
            "url":{"schemes":["http","https"],"hosts":LOOPBACK}}}})
}

/// Agent web-tool fetches from local servers. Shell downloads keep their own
/// redirect-limited rule.
fn local_host_fetch() -> Value {
    json!({"kind":"rule_add","rule":{
        "id":"allow-agent-fetch-localhost",
        "decision":"allow",
        "reason":"Allow the agent's web tool to fetch pages from local servers on any port without asking.",
        "conditions":{"domain":"web","action":"fetch",
            "request_fields":{"/client":{"exact":"agent"}},
            "url":{"schemes":["http","https"],"hosts":LOOPBACK}}}})
}

/// An extra readable or writable folder outside the project.
pub(crate) fn folder(access: &str, path: &Path) -> Option<Value> {
    let path = path.to_str()?;
    (["read", "write"].contains(&access) && Path::new(path).is_absolute() && path.len() <= 4096)
        .then(|| json!({"kind":"access_path_add","access":access,"path":path}))
}

/// The tool arguments, linked to the history entry that prompted them. The
/// agent adds its own reason; the server derives the retry key.
pub(crate) fn arguments(change: &Value, entry: Option<&str>) -> Value {
    let mut value = json!({"change":change});
    if let Some(entry) = entry.filter(|entry| entry.len() <= 128) {
        value["source_entry_id"] = json!(entry);
    }
    value
}

/// The sentence an agent reads: what to call, with what, and what the user does.
pub(crate) fn offer(arguments: &Value) -> String {
    let entry = arguments["source_entry_id"]
        .as_str()
        .map_or_else(String::new, |entry| format!(" and source_entry_id {entry}"));
    let is_pack = arguments["change"]["kind"] == "pack_add";
    let check = if is_pack {
        " Review the proposal's Allow/Ask/Deny scope; use pack_info for requested detail."
    } else {
        ""
    };
    format!(
        "Call policy_propose with change {}{entry}; add agent_reason.{check} Copy the returned apply_arguments object unchanged into policy_apply for user approval.",
        arguments["change"]
    )
}

/// No executable policy is guessed from one command. The agent reviews its
/// effects and the documented grammar before authoring a reusable rule.
pub(crate) fn command_offer(entry: Option<&str>) -> String {
    let source = entry.map_or_else(String::new, |entry| {
        format!(" Link source_entry_id {entry}.")
    });
    format!(
        "After the task, offer one grouped review for eligible recurring requests or an explicit user request. Inspect scripts, lifecycle hooks, argument effects and relevant existing rules; skip unknown effects. Adapt the returned canonical authoring examples, preserving cwd and argument scope. Propose with policy_propose and link review_ids.{source} Copy apply_arguments unchanged to policy_apply for host confirmation. Decline or cancellation ends the offer."
    )
}

fn excerpt(text: &str) -> String {
    let mut short: String = text.chars().take(120).collect();
    if short.len() < text.len() {
        short.push('…');
    }
    short
}

/// A readable rule identity: a prefix, the request's words and a short digest so
/// different requests never share one.
fn identifier(prefix: &str, text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let digest = crate::policy::fingerprint(text.as_bytes());
    format!(
        "{prefix}-{}-{}",
        slug.trim_matches('-'),
        &digest[digest.len() - 8..]
    )
    .replace("--", "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluator(change: &Value) -> crate::configuration::Evaluator {
        use crate::configuration::{Candidate, Evaluator};
        use crate::policy::Scope;
        let document = crate::policy::rules::document(&[change["rule"].clone()]).unwrap();
        let candidate = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n")
            .unwrap()
            .capture(&[], Some(&document.to_string()))
            .unwrap();
        Evaluator::new(vec![(Scope::Project, candidate)]).unwrap()
    }

    /// A typed agent web-tool fetch, as native hooks submit it.
    fn host_fetch(evaluator: &crate::configuration::Evaluator, cwd: &str, url: &str) -> String {
        use crate::policy::normalized::{Context, Input};
        let input = Input::Web {
            action: "fetch".into(),
            query: None,
            url: Some(url.into()),
            method: Some("GET".into()),
            conditions: json!({"client":"agent"}),
            original_input: json!({"kind":"web","action":"fetch","url":url}),
        };
        let context = Context {
            cwd: cwd.into(),
            environment: None,
            working_files: None,
        };
        let request = crate::policy::Request {
            executable: "shield:web-fetch".into(),
            arguments: vec![url.into()],
            cwd: cwd.into(),
            project: cwd.into(),
        };
        let result = evaluator
            .evaluate_resource(&request, &input, &context, &|| Ok(()))
            .unwrap();
        serde_json::to_value(result.decision)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn loopback_host_fetch_drafts_cover_agent_web_tools_only() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().to_str().unwrap();
        let change = host_web_fetch("http://localhost:3000/health").unwrap();
        let evaluator = evaluator(&change);
        for (url, expected) in [
            ("http://localhost:3000/health", "allow"),
            ("http://127.0.0.1:8080/", "allow"),
            ("https://example.com/", "ask"),
        ] {
            assert_eq!(host_fetch(&evaluator, cwd, url), expected, "{url}");
        }
        // Shell downloads keep their redirect-limited draft.
        let result = crate::shell::analyze(
            "curl -q -L http://localhost:3000/health",
            crate::shell::Dialect::Bash,
            cwd,
            cwd,
            &evaluator,
        )
        .unwrap();
        assert_eq!(result["decision"], "ask", "{result}");
        // Non-loopback host drafts match the shell form.
        assert_eq!(
            host_web_fetch("https://docs.example.com/"),
            web_fetch("https://docs.example.com/")
        );
    }

    #[test]
    fn loopback_fetch_drafts_require_shell_clients_without_redirects() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root.path().to_str().unwrap();
        let evaluator = evaluator(&web_fetch("http://localhost:3000/health").unwrap());
        assert_eq!(
            host_fetch(&evaluator, cwd, "http://localhost:3000/health"),
            "ask"
        );
        for (source, expected) in [
            ("curl -q http://localhost:3000/health", "allow"),
            ("curl -q -I http://127.0.0.1:8080/health", "allow"),
            ("curl -q -L http://localhost:3000/health", "ask"),
            ("curl -q https://example.com/", "ask"),
            ("curl -q -X POST http://localhost:3000/health", "ask"),
            (
                "wget --no-hsts --no-netrc --max-redirect=0 -qO- http://localhost:3000/health",
                "allow",
            ),
            (
                "wget --no-hsts --no-netrc -qO- http://localhost:3000/health",
                "ask",
            ),
        ] {
            let result =
                crate::shell::analyze(source, crate::shell::Dialect::Bash, cwd, cwd, &evaluator)
                    .unwrap();
            assert_eq!(result["decision"], expected, "{source}: {result}");
        }
    }

    #[test]
    fn a_command_normalized_into_the_project_drafts_a_project_limited_rule() {
        let guidance = json!({"source":"decisive",
            "request":{"executable":"npm","arguments":["run","dev"],"cwd":"project"}});
        let change = decisive_command(&guidance).unwrap();
        let rule = &change["rule"];
        assert_eq!(
            rule["conditions"]["arguments"]["values"],
            json!(["run", "dev"])
        );
        assert_eq!(
            rule["conditions"]["cwd"],
            json!({"within":{"root":"project"}})
        );
        assert_eq!(
            rule["reason"],
            "Allow `npm run dev` in the project without asking."
        );
        let rule: crate::policy::normalized::Rule = serde_json::from_value(rule.clone()).unwrap();
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn drafts_are_valid_rules_with_stable_distinct_identities() {
        let npm = command("npm", &["install", "express@5"]).unwrap();
        let other = command("npm", &["install", "express@4"]).unwrap();
        assert_ne!(npm["rule"]["id"], other["rule"]["id"]);
        assert_eq!(npm, command("npm", &["install", "express@5"]).unwrap());
        let fetch = web_fetch("https://docs.example.com/guide?q=1").unwrap();
        assert_eq!(
            fetch["rule"]["conditions"]["url"]["hosts"],
            json!(["docs.example.com"])
        );
        // An explicit port stays part of the rule; a loopback URL covers
        // every local server, on any port.
        let ported = web_fetch("https://docs.example.com:8443/").unwrap();
        assert_eq!(ported["rule"]["conditions"]["url"]["ports"], json!([8443]));
        let local = web_fetch("http://localhost:3000/health").unwrap();
        assert_eq!(local, web_fetch("http://127.0.0.1:8080/").unwrap());
        assert_eq!(local["rule"]["conditions"]["url"].get("ports"), None);
        for change in [npm, web_search(), fetch, ported, local] {
            let rule: crate::policy::normalized::Rule =
                serde_json::from_value(change["rule"].clone()).unwrap();
            rule.validate().unwrap();
        }
        assert!(web_fetch("ftp://docs.example.com/").is_none());
        assert!(folder("read", Path::new("relative")).is_none());
        let args = arguments(
            &folder("read", Path::new("/acme/shared")).unwrap(),
            Some("request-acme"),
        );
        assert_eq!(args["source_entry_id"], "request-acme");
        assert!(args.get("idempotency_key").is_none());
    }
}
