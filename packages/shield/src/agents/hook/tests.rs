//! Real evaluator and retained project state through scoped native adapters.
use super::*;
use crate::{configuration::Bundle, review};
use std::fs;
mod mcp_connections;
#[cfg(unix)]
mod mcp_local_stdio;
mod mcp_scope_conflicts;
mod pack_suggestions;
mod working_files;

#[test]
fn native_denial_names_pipeline_commands_without_inventing_folder_access() {
    let (root, _) = fixture();
    fs::write(root.path().join("report.txt"), "Acme report").unwrap();
    for agent in ["claude", "codex"] {
        for source in [
            "head report.txt | head report.txt",
            "git reset --hard | head report.txt",
        ] {
            let value = call(root.path(), agent, source);
            let summary = value["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap();
            assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
            assert!(summary.contains("`head` (rule `head`)"), "{value}");
            if source.starts_with("git") {
                assert!(summary.contains("`git` (rule `deny`)"), "{value}");
            }
            let context = value["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            assert!(!context.contains("access_path_add"), "{value}");
            assert!(!summary.contains("report.txt"), "{value}");
            if source.starts_with("head") {
                assert_eq!(summary.matches("`head` (rule `head`)").count(), 2);
            }
        }
        assert_permitted(&call(root.path(), agent, "cat report.txt"), agent);
    }
    let value = tool(
        root.path(),
        "claude",
        "Monitor",
        &json!({"command":"head report.txt | head report.txt","description":"Acme output","timeout_ms":1000}),
    );
    let summary = value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(summary.contains("`head` (rule `head`)"), "{value}");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
}

fn fixture() -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"rules":[{"id":"allow","decision":"allow","reason":"Acme inspection","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}},{"id":"literal","decision":"allow","reason":"Acme literal","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}},{"id":"deny","decision":"deny","reason":"Acme destructive operation","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["reset","--hard"]}}},{"id":"ask","decision":"ask","reason":"Acme explicit approval","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["push"]}}},{"id":"read","decision":"allow","reason":"Acme text read","conditions":{"domain":"file","action":"read","sources":{"all":{"within":{"root":"project"}}}}},{"id":"write","decision":"allow","reason":"Acme project write","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"project"}}},"on_unmet":{"decision":"deny","message":"Write only inside the project."}},{"id":"cat","decision":"allow","reason":"Acme project read","conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny","message":"Read only inside the project."}},{"id":"head","decision":"deny","reason":"Acme catch-all read","conditions":{"domain":"argv","executable":"head","arguments":{"kind":"any"}},"guidance":{"message":"Use approved paths.","alternatives":["To use a folder outside the project, propose it with access_path_add."]}},{"id":"search","decision":"allow","reason":"Acme web search","conditions":{"domain":"web","action":"search","query":{"regex":"(?s).*"}}},{"id":"docs","decision":"allow","reason":"Acme docs","conditions":{"domain":"web","action":"fetch"},"requirements":{"url":{"schemes":["https"],"hosts":["docs.example.com"]}},"on_unmet":{"decision":"deny","message":"Fetch only https://docs.example.com pages."}}],"kind":"custom","capabilities":["argument-rules-v1","location-v1","regex-v1","requirements-v1","structured-rules-v1"]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    (root, context)
}

#[test]
fn native_pagers_keep_command_review_and_protected_path_guards() {
    let (root, _) = fixture();
    fs::write(root.path().join("report.txt"), "Acme report").unwrap();
    for (agent, expected) in [("claude", "ask"), ("codex", "deny")] {
        for source in [
            "less report.txt",
            "more report.txt",
            "LESSOPEN='|./pager-helper %s' less report.txt",
            "less report.txt | sed -n 1p",
        ] {
            let value = call(root.path(), agent, source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], expected,
                "{source}: {value}"
            );
        }
        for source in ["less .mcp.json", "more .shield/review/state.json"] {
            let value = call(root.path(), agent, source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{source}: {value}"
            );
        }
    }
}

#[test]
fn native_download_outputs_keep_protected_paths_blocked_in_attached_options() {
    let (root, _) = fixture();
    for agent in ["claude", "codex"] {
        for option in [
            "curl -fsSo",
            "curl -fsS -o ",
            "wget --no-hsts --no-netrc -qO",
            "wget --no-hsts --no-netrc -q -O ",
            "wget --no-hsts --no-netrc --output-document=",
        ] {
            let ordinary = format!("{option}download.txt https://docs.example.com/guide");
            assert_permitted(&unscoped_shell(root.path(), agent, &ordinary), agent);
            for path in [
                ".claude/settings.json",
                ".codex/hooks.json",
                ".mcp.json",
                ".shield/config.yml",
                ".shield/rules.yml",
                ".shield/review/state.json",
            ] {
                let source = format!("{option}{path} https://docs.example.com/guide");
                let value = unscoped_shell(root.path(), agent, &source);
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{agent}: {source}: {value}"
                );
                let reason = value["hookSpecificOutput"]["permissionDecisionReason"]
                    .as_str()
                    .unwrap();
                assert!(reason.contains("restricted"), "{source}: {value}");
            }
        }
    }
    assert!(!root.path().join("download.txt").exists());
}

#[test]
fn native_download_output_refusals_propose_the_writable_folder() {
    let (root, context) = fixture();
    let outside = assert_fs::TempDir::new().unwrap();
    let folder = outside.path().join("downloads");
    fs::create_dir(&folder).unwrap();
    let folder = folder.canonicalize().unwrap();
    let output = folder.join("catalog.json");
    for agent in ["claude", "codex"] {
        for option in [
            "curl -fsSo",
            "curl -fsS -o ",
            "wget --no-hsts --no-netrc -qO",
            "wget --no-hsts --no-netrc --output-document=",
        ] {
            let source = format!(
                "{option}'{}' https://docs.example.com/catalog.json",
                output.display()
            );
            let value = unscoped_shell(root.path(), agent, &source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{value}"
            );
            let arguments = propose_offered(&context, reason(&value));
            assert_eq!(
                arguments["change"],
                json!({"kind":"access_path_add","access":"write","path":folder}),
                "{source}: {value}"
            );
        }
    }
    assert!(!output.exists());
}

#[test]
fn native_downloads_to_the_null_device_need_only_fetch_permission() {
    let (root, _) = fixture();
    for agent in ["claude", "codex"] {
        for source in [
            "curl -fsS -o /dev/null https://docs.example.com/guide",
            "wget --no-hsts --no-netrc -qO/dev/null https://docs.example.com/guide",
            "wget --no-hsts --no-netrc --output-document=/dev/null https://docs.example.com/guide",
        ] {
            assert_permitted(&unscoped_shell(root.path(), agent, source), agent);
        }
    }
}

/// Stored record lines of the current history generation, oldest segment
/// first, without segment headers: the raw bytes Shield keeps on disk.
fn journal(root: &Path) -> String {
    let state: Value =
        serde_json::from_slice(&fs::read(root.join(".shield/review/state.json")).unwrap()).unwrap();
    let digest = state["history_generation"]
        .as_str()
        .unwrap()
        .trim_start_matches("journal-");
    let prefix = format!("history-{digest}.");
    let mut segments: Vec<_> = fs::read_dir(root.join(".shield/review"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|e| e == "jsonl")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    segments.sort();
    segments
        .iter()
        .flat_map(|path| {
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .skip(1)
                .map(|line| format!("{line}\n"))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn call(root: &Path, agent: &str, source: &str) -> Value {
    translate(root,agent,&serde_json::to_vec(&json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-call","session_id":"acme-session","cwd":root,"tool_input":{"command":source}})).unwrap())
}

/// Exercise outside-folder proposals with no integration resource observation.
/// Native adapter tests separately cover the default system temporary scope.
fn without_working_files(root: &Path, agent: &str, operation: &Value) -> Value {
    let adapter = crate::agents::find(agent).unwrap();
    let context = review::open(root).unwrap();
    let input = json!({"protocol_version":1,"request_id":"acme-unscoped","expected_project_id":context.project_id,"cwd":root,"operation":operation,"host":null,"approval_request_id":null});
    let (receipt, exit) = crate::enforcement::protocol::evaluate_native(
        root,
        &serde_json::to_vec(&input).unwrap(),
        &json!({}),
        crate::enforcement::protocol::NativeContext {
            integration: agent,
            approval: adapter.approval(Event::Before),
            context_complete: true,
            working_files: None,
        },
        None,
        &|_| {},
    );
    assert_eq!(exit, 0, "{receipt}");
    let reply = crate::agents::feedback::respond(
        root,
        adapter,
        Event::Before,
        &receipt,
        operation,
        "acme-session",
    )
    .unwrap();
    adapter.reply(Event::Before, &reply)
}

fn unscoped_shell(root: &Path, agent: &str, source: &str) -> Value {
    without_working_files(
        root,
        agent,
        &json!({"kind":"shell","source":source,"dialect":"posix"}),
    )
}

fn argv_fixture(executable: &str, decision: &str) -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"fixture","decision":decision,"conditions":{"domain":"argv","executable":executable,"arguments":{"kind":"any"}}}]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root.path(), &bundle);
    root
}

#[test]
fn heredoc_scaffolding_checks_destinations_without_an_extra_approval() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["argument-rules-v1","location-v1","requirements-v1","structured-rules-v1"],
        "rules":[
            {"id":"stdin-cat","decision":"allow","conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"exact","values":[]}}},
            {"id":"project-write","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"project"}}},"on_unmet":{"decision":"deny","message":"Write only inside the project."}},
            {"id":"deny-effect","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["write"]}}}
        ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root.path(), &bundle);
    for agent in ["claude", "codex"] {
        for source in [
            "cat > package.json <<'EOF'\n{\"private\":true}\nEOF\ncat > app.js <<'EOF'\nconst example = '$(acme write)';\nEOF\n",
            "cat > app.js <<< 'literal content'",
        ] {
            assert_permitted(&call(root.path(), agent, source), agent);
        }
        for source in [
            "cat > ../outside.js <<'EOF'\ndata\nEOF\n",
            "cat > .mcp.json <<'EOF'\n{}\nEOF\n",
            "cat > .shield/review/state.json <<'EOF'\n{}\nEOF\n",
            "cat > app.js <<EOF\n$(acme write)\nEOF\n",
        ] {
            let value = call(root.path(), agent, source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{source}: {value}"
            );
        }
    }
    assert!(!root.path().join("package.json").exists());
    assert!(!root.path().join("app.js").exists());
}

#[test]
fn output_text_is_not_a_protected_path_but_its_io_still_is() {
    let (root, _) = fixture();
    for agent in ["codex", "claude"] {
        crate::review::operator::integrations::configure_fixture(
            root.path(),
            crate::cli::public::IntegrationChoice::agent(agent),
            crate::review::integrations::AskFallback::Allow,
        );
        for source in [
            "printf '%s\\n' '.DS_Store' '.claude/' '.codex/' '.mcp.json' '.shield/' > .gitignore\n git init -b main\n git add .gitignore\n git commit -m \"Initial commit\"\n git status --short --branch",
            "echo .shield/ .mcp.json > .gitignore",
            "/usr/bin/printf '%s' .shield/review/state.json > notes.txt",
        ] {
            let value = call(root.path(), agent, source);
            if agent == "codex" {
                assert_permitted(&value, agent);
            } else {
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"], "ask",
                    "{source}: {value}"
                );
            }
            assert!(
                !reason(&value).starts_with("Shield: protected path."),
                "{agent}: {source}: {value}"
            );
        }
        for source in [
            "printf '%s' .shield/ > .shield/config.yml",
            "echo .shield/ > .mcp.json",
            "printf '%s' .shield/ < .shield/review/state.json > .gitignore",
            "printf '%s' \"$(cat .shield/review/state.json)\" > .gitignore",
            "echo .shield/ | tee .shield/config.yml",
            "echo .shield/ > .gitignore; cat .shield/review/state.json",
            ".shield/review/printf '%s' .shield/ > .gitignore",
        ] {
            let value = call(root.path(), agent, source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{agent}: {source}: {value}"
            );
            assert!(
                reason(&value).starts_with("Shield: protected path."),
                "{agent}: {source}: {value}"
            );
        }
    }
}

#[test]
fn git_permission_accepts_reported_cwd_and_keeps_policy_ask_and_deny() {
    for policy in ["allow", "ask", "deny"] {
        let root = argv_fixture("git", policy);
        let cwd = root.path().canonicalize().unwrap();
        let source = format!(
            "git -C '{}' status && git -C '{}' status --short",
            cwd.display(),
            cwd.display()
        );
        for command in [
            source,
            "git status".into(),
            "git init".into(),
            "git add acme.txt".into(),
            "git init && git add acme.txt && git commit -m 'Initial commit'".into(),
        ] {
            let host = json!({"hook_event_name":"PermissionRequest","tool_name":"Bash",
                "session_id":"acme-git-context","cwd":cwd,"tool_input":{"command":command,"description":"Acme explicit Git context"}});
            let value = translate_for(
                root.path(),
                "codex",
                Event::Permission,
                &serde_json::to_vec(&host).unwrap(),
            );
            let expected = if policy == "deny" {
                "deny"
            } else if policy == "allow" {
                "allow"
            } else {
                ""
            };
            assert_eq!(
                value["hookSpecificOutput"]["decision"]["behavior"]
                    .as_str()
                    .unwrap_or_default(),
                expected,
                "{policy} {command}: {value}"
            );
        }
    }
}

#[test]
fn ripgrep_lists_names_and_searches_bounded_trees_without_reading_protected_state() {
    for agent in ["codex", "claude"] {
        let root = argv_fixture("rg", "allow");
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/app.js"), "Acme").unwrap();
        fs::write(root.path().join(".ignore"), "!**/.shield/**\n").unwrap();
        for source in [
            "rg --files -g '*'",
            "rg --files --hidden -g '!.git' .",
            "rg --hidden --files -g 'AGENTS.md' -g '.gitignore'",
            "rg --no-ignore --hidden --files .",
            "rg Acme src",
            "rg -g '*.js' Acme src",
            "rg --no-config --files -g '*'",
            "rg --no-config Acme src",
            "rg --no-config -g '*.js' Acme src",
            "rg --no-config --no-ignore Acme .",
        ] {
            assert_permitted(&call(root.path(), agent, source), agent);
        }
        for source in [
            "rg Acme .",
            "rg --hidden Acme .",
            "rg --hidden Acme src",
            "rg --hidden --files-with-matches Acme .",
            "rg --files --hidden --pre sh src",
            "rg --files --hidden --follow .",
            "rg --pre sh Acme src",
            "rg --follow Acme src",
        ] {
            assert!(
                !reason(&call(root.path(), agent, source)).is_empty(),
                "{source}"
            );
        }
        let result = call(root.path(), agent, "rg --no-config Acme .");
        assert!(reason(&result).contains(".shield/review"), "{result}");
        assert!(
            reason(&result).contains("Search a narrower path"),
            "{result}"
        );
        // A pattern-only search after a pipe reads the pipe, not the project;
        // a path, closed input or no pipe still walks the directory.
        assert_permitted(&call(root.path(), agent, "rg --files | rg app"), agent);
        assert_permitted(
            &call(root.path(), agent, "rg --files | rg -i -m 3 app"),
            agent,
        );
        for source in [
            "rg --no-config Acme",
            "rg --files | rg --no-config Acme .",
            "rg --files | rg --no-config Acme <&-",
            "rg --files | { rg --no-config Acme; }",
        ] {
            assert!(
                reason(&call(root.path(), agent, source)).contains(".shield"),
                "{source}"
            );
        }
        fs::create_dir_all(root.path().join("src/nested/.shield/review")).unwrap();
        assert!(reason(&call(root.path(), agent, "rg --no-config Acme src")).contains(".shield"));
    }
}

#[test]
#[cfg(unix)]
fn fixed_text_read_syntax_never_classifies_edit_or_output_options_as_reads() {
    for agent in ["codex", "claude"] {
        for (executable, read, write) in [
            (
                "sed",
                "sed -n '1,20p' .shield/config.yml",
                "sed -i '' '1,20p' .shield/config.yml",
            ),
            (
                "awk",
                "awk '{print $1}' .shield/config.yml",
                "awk -f .shield/config.yml",
            ),
            (
                "sort",
                "sort .shield/config.yml",
                "sort -o .shield/config.yml .shield/config.yml",
            ),
            (
                "uniq",
                "uniq .shield/config.yml",
                "uniq .shield/config.yml .shield/rules.yml",
            ),
            (
                "cut",
                "cut -f1 .shield/config.yml",
                "cut --unmodeled .shield/config.yml",
            ),
        ] {
            let root = argv_fixture(executable, "allow");
            assert_permitted(&call(root.path(), agent, read), agent);
            assert!(
                reason(&call(root.path(), agent, write))
                    .contains("counts as a change to `.shield/config.yml`"),
                "{agent}: {write}"
            );
        }
    }
}

#[test]
fn uncertain_search_footprints_explain_narrowing_and_sensitive_reads_explain_continuing() {
    let root = argv_fixture("rg", "allow");
    fs::create_dir(root.path().join("src")).unwrap();
    for index in 0..1025 {
        fs::write(root.path().join(format!("src/{index}.txt")), "Acme").unwrap();
    }
    let result = call(root.path(), "codex", "rg --no-config Acme src");
    assert!(
        reason(&result).contains("Search a narrower path"),
        "{result}"
    );
    let mut deep = root.path().join("deep");
    fs::create_dir(&deep).unwrap();
    for _ in 0..32 {
        deep = deep.join("nested");
        fs::create_dir(&deep).unwrap();
    }
    let result = call(root.path(), "codex", "rg --no-config Acme deep");
    assert!(
        reason(&result).contains("Search a narrower path"),
        "{result}"
    );
    let root = argv_fixture("cat", "allow");
    let result = call(root.path(), "codex", "cat .mcp.json");
    assert_eq!(result["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(reason(&result).contains("terminal"), "{result}");
    assert!(!reason(&result).contains("policy_propose"), "{result}");
}

fn assert_permitted(value: &Value, agent: &str) {
    let output = &value["hookSpecificOutput"];
    if agent == "codex" {
        assert!(
            !output
                .as_object()
                .unwrap()
                .contains_key("permissionDecision"),
            "{value}"
        );
        assert!(
            output["additionalContext"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "{value}"
        );
        assert!(output.get("updatedInput").is_none(), "{value}");
    } else {
        assert_eq!(output["permissionDecision"], "allow", "{value}");
    }
}

#[test]
fn claude_ask_completions_preserve_the_prompt_without_claiming_operator_approval() {
    for source in ["git push", "cat .mcp.json", "npm install express@5"] {
        for (event, status) in [(Event::Completed, "unknown"), (Event::Failed, "failed")] {
            let (root, context) = fixture();
            fs::write(root.path().join(".mcp.json"), "{}").unwrap();
            let before = call(root.path(), "claude", source);
            assert_eq!(
                before["hookSpecificOutput"]["permissionDecision"], "ask",
                "{source}: {before}"
            );
            let name = if event == Event::Failed {
                "PostToolUseFailure"
            } else {
                "PostToolUse"
            };
            let post = json!({"hook_event_name":name, "tool_name":"Bash",
                "tool_use_id":"acme-call", "session_id":"acme-session", "cwd":root.path(),
                "tool_input":{"command":source}});
            translate_for(
                root.path(),
                "claude",
                event,
                &serde_json::to_vec(&post).unwrap(),
            );
            let response = review::mcp::call(
                &context,
                &review::mcp::Connection::new("claude-ask-history".into()),
                "history_list",
                &json!({}),
                &|| Ok(()),
            );
            let entries = response["data"]["entries"].as_array().unwrap();
            assert_eq!(entries.len(), 1, "{response}");
            let entry = &entries[0];
            assert_eq!(
                entry["observations"].as_array().unwrap().len(),
                2,
                "{response}"
            );
            assert_eq!(entry["disposition"], "approval_required");
            assert_eq!(entry["observations"][0]["details"]["host_decision"], "ask");
            assert_eq!(entry["approved"], false);
            assert_eq!(entry["outcome"]["status"], status);
            assert_eq!(entry["outcome"]["evidence"], "host-reported");
            assert!(entry["outcome"]["exit_code"].is_null());
        }
    }
}

#[test]
fn native_runtime_status_arguments_keep_ordinary_approval_without_rewrite() {
    let (root, _) = fixture();
    for source in [
        r#"echo "exit $?""#,
        r#"echo "exit ${?}""#,
        "echo $?",
        r#"acme read; echo "exit $?""#,
    ] {
        let reply = call(root.path(), "claude", source);
        assert_eq!(
            reply["hookSpecificOutput"]["permissionDecision"], "ask",
            "{reply}"
        );
        assert!(!reason(&reply).contains("rewrite"), "{reply}");
    }
    for source in [r#"echo "$? $unknown""#, r#"echo "$? $(acme)""#, "head $?"] {
        let reply = call(root.path(), "claude", source);
        assert_eq!(
            reply["hookSpecificOutput"]["permissionDecision"], "deny",
            "{reply}"
        );
    }
}

#[test]
fn native_rewrite_refusal_history_matches_the_delivered_block() {
    let (root, context) = fixture();
    let source = "pid=$(cat pidfile); kill $pid";
    let reply = call(root.path(), "claude", source);
    assert_eq!(reply["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(reason(&reply).contains("rewrite this request"), "{reply}");
    let response = review::mcp::call(
        &context,
        &review::mcp::Connection::new("rewrite-history".into()),
        "history_list",
        &json!({}),
        &|| Ok(()),
    );
    let entry = &response["data"]["entries"][0];
    assert_eq!(entry["disposition"], "blocked", "{response}");
    let decision = &entry["observations"][0]["details"];
    assert_eq!(decision["decision"], "ask");
    assert_eq!(decision["host_decision"], "deny");
    assert_eq!(decision["code"], "request-rewrite");
    assert_eq!(entry["outcome"], "unobserved");
    assert_eq!(entry["denied"], false); // A rewrite refusal is not a policy Deny.
}

#[test]
fn native_completion_history_joins_without_a_shield_receipt() {
    for agent in ["codex", "claude"] {
        let (root, context) = fixture();
        call(root.path(), agent, "git status");
        let post = json!({"hook_event_name":"PostToolUse", "tool_name":"Bash",
            "tool_use_id":"acme-call", "session_id":"acme-session", "cwd":root.path(),
            "tool_input":{"command":"git status"}});
        translate_for(
            root.path(),
            agent,
            Event::Completed,
            &serde_json::to_vec(&post).unwrap(),
        );
        let response = review::mcp::call(
            &context,
            &review::mcp::Connection::new("native-completion-history".into()),
            "history_list",
            &json!({}),
            &|| Ok(()),
        );
        let entries = response["data"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{response}");
        assert_eq!(
            entries[0]["observations"].as_array().unwrap().len(),
            2,
            "{agent}: {response}"
        );
        assert_eq!(entries[0]["outcome"]["status"], "unknown");
        assert_eq!(entries[0]["outcome"]["evidence"], "host-reported");
        assert!(entries[0]["outcome"]["exit_code"].is_null());
    }
}

#[test]
fn native_decisions_recover_missing_plans_while_completions_leave_them_missing() {
    for agent in ["codex", "claude"] {
        let (root, _) = fixture();
        let state_path = root.path().join(".shield/review/state.json");
        let status: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        let cache = &status["active"]["compiled"];
        let path = Path::new(cache["directory"].as_str().unwrap())
            .join("compiled")
            .join(cache["project_key"].as_str().unwrap())
            .join(format!("{}.bin", cache["plan_key"].as_str().unwrap()));
        fs::remove_file(&path).unwrap();
        let host = json!({"hook_event_name":"PostToolUse","tool_name":"Bash",
            "tool_use_id":"acme-cold-completion","session_id":"acme-session",
            "cwd":root.path(),"tool_input":{"command":"git status"}});
        translate_for(
            root.path(),
            agent,
            Event::Completed,
            &serde_json::to_vec(&host).unwrap(),
        );
        assert!(!path.exists(), "completion must not compile");
        assert_permitted(&call(root.path(), agent, "git status"), agent);
        assert!(path.exists());
        let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        assert_eq!(after["active"], status["active"]);
    }
}

#[test]
fn effective_external_cache_and_ancestors_are_protected_under_allow_fallback() {
    let (root, context) = fixture();
    review::operator::integrations::configure_fixture(
        root.path(),
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    let cache = crate::review::compiled_cache_root(root.path())
        .unwrap()
        .unwrap();
    for source in [
        format!("printf changed > {}/compiled/other.bin", cache.display()),
        format!("rm -rf {}", cache.display()),
        format!("mv {} /tmp/acme", cache.display()),
    ] {
        let value = call(root.path(), "codex", &source);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{value}"
        );
    }
    assert_eq!(
        Project::open(std::path::Path::new(&context.project_root))
            .unwrap()
            .snapshot()
            .unwrap()
            .identity()
            .project_id(),
        context.project_id
    );
}

#[test]
fn unrecoverable_compiled_plan_blocks_allow_fallback_with_a_short_repair_route() {
    let (root, context) = fixture();
    review::operator::integrations::configure_fixture(
        root.path(),
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    let value: Value =
        serde_json::from_slice(&fs::read(root.path().join(".shield/review/state.json")).unwrap())
            .unwrap();
    let artifact = &value["active"]["compiled"];
    let file = std::path::Path::new(artifact["directory"].as_str().unwrap())
        .join("compiled")
        .join(artifact["project_key"].as_str().unwrap())
        .join(format!("{}.bin", artifact["plan_key"].as_str().unwrap()));
    fs::remove_file(&file).unwrap();
    // A missing plan recovers even with an Allow fallback; a path occupied by
    // another object cannot be replaced and must still fail closed.
    assert_permitted(&call(root.path(), "codex", "echo acme"), "codex");
    fs::remove_file(&file).unwrap();
    fs::create_dir(file).unwrap();
    let value = call(root.path(), "codex", "echo acme");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("shield cache prepare"), "{reason}");
    assert!(
        reason.contains(&format!("--project '{}'", context.project_root)),
        "{reason}"
    );
    assert!(reason.len() < 200);
    let agent = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        agent.contains(&format!(
            "shield cache prepare --project '{}'",
            context.project_root
        )),
        "{agent}"
    );
}

#[test]
fn diagnostic_evaluation_records_core_decision_without_command_source() {
    use clap::Parser;
    let (root, context) = fixture();
    let logs = root.path().join("logs");
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.to_str().unwrap(),
        "status",
    ]);
    let log = crate::diagnostics::Log::open(&cli);
    let host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-call","session_id":"acme-session","cwd":root.path(),"tool_input":{"command":"git status"}});
    let response = translate_logged(
        Path::new(&context.project_root),
        "codex",
        &serde_json::to_vec(&host).unwrap(),
        &log,
    );
    assert_permitted(&response, "codex");
    let text = fs::read_to_string(log.path().unwrap()).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let evaluation = records
        .iter()
        .find(|record| record["event"] == "hook_evaluation")
        .unwrap();
    assert_eq!(evaluation["fields"]["decision"], "allow");
    assert_eq!(evaluation["fields"]["exit"], 0);
    assert!(evaluation["fields"]["policy_revision"].is_string());
    let stages: Vec<_> = records
        .iter()
        .filter(|record| record["event"] == "hook_stage")
        .map(|record| record["fields"]["stage"].as_str().unwrap())
        .collect();
    assert_eq!(
        stages,
        [
            "project_open_start",
            "project_open_ready",
            "registration_ready",
            "policy_evaluation_start",
            "snapshot_load_start",
            "snapshot_load_ready",
            "policy_evaluation_ready",
            "native_handoff_ready",
            "history_start",
            "history_ready",
        ]
    );
    assert!(records.windows(2).all(|pair| {
        pair[0]["elapsed_ms"].as_u64().unwrap() <= pair[1]["elapsed_ms"].as_u64().unwrap()
    }));
    assert!(!text.contains("git status"));
}

fn tool(root: &Path, agent: &str, name: &str, input: &Value) -> Value {
    translate(root,agent,&serde_json::to_vec(&json!({"hook_event_name":"PreToolUse","tool_name":name,"tool_use_id":"acme-tool","session_id":"acme-session","cwd":root,"tool_input":input})).unwrap())
}

fn reason(value: &Value) -> &str {
    if value["hookSpecificOutput"]["permissionDecision"] == "deny"
        && let Some(context) = value["hookSpecificOutput"]["additionalContext"].as_str()
    {
        return context;
    }
    value["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default()
}

fn permission(root: &Path, tool: &str, mut input: Value) -> Value {
    if tool == "Bash" && input.get("workdir").is_none() {
        input["workdir"] = json!(root);
    }
    translate_for(
        root,
        "codex",
        Event::Permission,
        &serde_json::to_vec(&json!({"hook_event_name":"PermissionRequest",
        "session_id":"acme-permission","cwd":root,"tool_name":tool,"tool_input":input}))
        .unwrap(),
    )
}

fn translate_for(root: &Path, agent: &str, event: Event, bytes: &[u8]) -> Value {
    translate_event(
        root,
        find(agent).unwrap(),
        bytes,
        event,
        &crate::diagnostics::Log::disabled(),
    )
}

#[test]
fn permission_with_missing_execution_context_follows_policy() {
    let (root, _) = fixture();
    for (command, expected) in [
        ("acme read", "allow"),
        ("git status", "allow"),
        ("git push", ""),
        ("git reset --hard", "deny"),
        ("cat .codex/hooks.json", "deny"),
    ] {
        let request = json!({"hook_event_name":"PermissionRequest","session_id":"Acme","cwd":root.path(),"tool_name":"Bash","tool_input":{"command":command}});
        let reply = translate_for(
            root.path(),
            "codex",
            Event::Permission,
            &serde_json::to_vec(&request).unwrap(),
        );
        assert_eq!(
            reply["hookSpecificOutput"]["decision"]["behavior"]
                .as_str()
                .unwrap_or_default(),
            expected,
            "{command}: {reply}"
        );
    }
}

#[test]
fn node_syntax_checks_read_source_and_keep_protected_paths_blocked() {
    let (root, _) = fixture();
    fs::write(
        root.path().join("acme.js"),
        "throw new Error('do not execute');",
    )
    .unwrap();
    for agent in ["codex", "claude"] {
        let source = format!(
            "'{}' --check acme.js",
            root.path().join("tools/node").display()
        );
        let reply = call(root.path(), agent, &source);
        assert_eq!(
            reply["hookSpecificOutput"]["permissionDecision"],
            if agent == "codex" { "deny" } else { "ask" },
            "{reply}"
        );
        for source in [
            "node --check acme.js",
            "node -c acme.js",
            "node --check .shield/config.yml",
        ] {
            assert_permitted(&call(root.path(), agent, source), agent);
        }
        for source in [
            "node --check .shield/review/state.json",
            "node --check .codex/hooks.json",
        ] {
            let reply = call(root.path(), agent, source);
            assert_eq!(
                reply["hookSpecificOutput"]["permissionDecision"],
                if agent == "codex" || source.contains("review") {
                    "deny"
                } else {
                    "ask"
                },
                "{reply}"
            );
        }
    }
    assert_eq!(
        permission(
            root.path(),
            "Bash",
            json!({"command":"node --check acme.js"})
        )["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );
}

#[test]
fn permission_request_uses_current_policy_and_never_the_ask_fallback() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let (root, context) = fixture();
    for fallback in [AskFallback::Block, AskFallback::Allow] {
        integrations::configure_fixture(root.path(), IntegrationChoice::agent("codex"), fallback);
        assert_eq!(
            permission(root.path(), "Bash", json!({"command":"git status"}))["hookSpecificOutput"]
                ["decision"]["behavior"],
            "allow"
        );
        assert_eq!(
            permission(root.path(), "Bash", json!({"command":"git push"})),
            json!({})
        );
        assert_eq!(
            permission(root.path(), "Bash", json!({"command":"git reset --hard"}))["hookSpecificOutput"]
                ["decision"]["behavior"],
            "deny"
        );
        assert_eq!(
            permission(root.path(), "Bash", json!({"command":"unmatched-acme"})),
            json!({})
        );
    }
    let events: Vec<Value> = journal(root.path())
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["event"].clone())
        .collect();
    let asks: Vec<_> = events
        .iter()
        .filter(|e| {
            e["provenance"]["host_event_name"] == "PermissionRequest"
                && e["details"]["decision"] == "ask"
        })
        .collect();
    assert!(!asks.is_empty(), "{events:?}");
    assert!(
        asks.iter()
            .all(|e| e["details"]["disposition"] == "approval_required"
                && e["details"]["ask_fallback"].is_null()
                && e["provenance"]["tool_use_id"].is_null())
    );
    assert_eq!(
        context.project_id,
        review::open(root.path()).unwrap().project_id
    );
}

#[test]
fn permission_request_blocks_invalid_unsupported_and_protected_requests() {
    let (root, _) = fixture();
    for (name, input) in [
        ("Bash", json!({"command":"git status","unexpected":true})),
        ("Bash", json!({"command":"git status","workdir":"relative"})),
        ("Bash", json!({"command":"shield policy review"})),
        ("Bash", json!({"command":"cat .codex/hooks.json"})),
        (
            "Bash",
            json!({"command":"printf changed > .shield/config.yml"}),
        ),
        ("mcp__shield__unknown", json!({})),
    ] {
        let value = permission(root.path(), name, input);
        assert_eq!(
            value["hookSpecificOutput"]["hookEventName"], "PermissionRequest",
            "{value}"
        );
        assert_eq!(
            value["hookSpecificOutput"]["decision"]["behavior"], "deny",
            "{value}"
        );
        assert!(
            value["hookSpecificOutput"]["decision"]["message"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        assert!(
            value["hookSpecificOutput"]
                .get("permissionDecision")
                .is_none()
        );
    }
}

#[test]
fn permission_batch_aggregation_keeps_ask_and_deny_before_encoding() {
    let (root, _) = fixture();
    let add = "*** Begin Patch\n*** Add File: acme.txt\n+Acme\n*** End Patch";
    assert_eq!(
        permission(root.path(), "apply_patch", json!({"command":add}))["hookSpecificOutput"]["decision"]
            ["behavior"],
        "allow"
    );
    let mixed =
        "*** Begin Patch\n*** Add File: acme.txt\n+Acme\n*** Delete File: other.txt\n*** End Patch";
    assert_eq!(
        permission(root.path(), "apply_patch", json!({"command":mixed})),
        json!({})
    );
    let denied = "*** Begin Patch\n*** Delete File: other.txt\n*** Add File: .shield/config.yml\n+tampered\n*** End Patch";
    assert_eq!(
        permission(root.path(), "apply_patch", json!({"command":denied}))["hookSpecificOutput"]["decision"]
            ["behavior"],
        "deny"
    );
}

#[test]
fn pinned_event_encodes_failures_without_trusting_the_input_event() {
    let (root, _) = fixture();
    let adapter = find("codex").unwrap();
    for bytes in [
        b"not JSON".as_slice(),
        br#"{"hook_event_name":"PostToolUse"}"#,
        br#"{"hook_event_name":"Acme"}"#,
    ] {
        let value = translate_event(
            root.path(),
            adapter,
            bytes,
            Event::Permission,
            &crate::diagnostics::Log::disabled(),
        );
        assert_eq!(
            value["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(value["hookSpecificOutput"]["decision"]["behavior"], "deny");
    }
    // A new evaluation fails closed after an earlier policy allowance.
    assert_eq!(
        permission(root.path(), "Bash", json!({"command":"git status"}))["hookSpecificOutput"]["decision"]
            ["behavior"],
        "allow"
    );
    fs::remove_file(root.path().join(".shield/review/state.json")).unwrap();
    assert_eq!(
        permission(root.path(), "Bash", json!({"command":"git status"}))["hookSpecificOutput"]["decision"]
            ["behavior"],
        "deny"
    );
}

#[test]
fn event_mismatch_denies_in_the_supplied_authorization_events_format() {
    let (root, _) = fixture();
    let deny = |agent: &str, pinned: Event, supplied: &str| {
        let host = json!({"hook_event_name":supplied,"tool_name":"Bash","session_id":"acme-session",
            "cwd":root.path(),"tool_input":{"command":"git status"}});
        translate_for(
            root.path(),
            agent,
            pinned,
            &serde_json::to_vec(&host).unwrap(),
        )
    };
    // Codex pinned to PermissionRequest answers a PreToolUse call in its format.
    let value = deny("codex", Event::Permission, "PreToolUse");
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        value["hookSpecificOutput"].get("decision").is_none(),
        "{value}"
    );
    // And the reverse.
    for pinned in [Event::Before, Event::Completed] {
        let value = deny("codex", pinned, "PermissionRequest");
        assert_eq!(
            value["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(value["hookSpecificOutput"]["decision"]["behavior"], "deny");
    }
    for pinned in [Event::Completed, Event::Failed] {
        let value = deny("claude", pinned, "PreToolUse");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    }
}

#[test]
fn permission_overflow_detects_names_after_the_limit_and_across_chunks() {
    let padding = "x".repeat(MAX_HOOK_INPUT);
    for suffix in [0, 65_529, 65_530, 65_535] {
        let bytes = format!(
            r#"{{"padding":"{padding}{}","hook_event_name":"PermissionRequest"}}"#,
            "y".repeat(suffix)
        );
        let (_, overflow) = read_input(&mut bytes.as_bytes());
        assert!(overflow.unwrap().names_pre);
    }
}

#[test]
fn claude_file_tools_follow_location_policy_and_protected_state() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let outside = assert_fs::TempDir::new().unwrap();
    let project = |name: &str| root.join(name).to_str().unwrap().to_owned();
    for (name, input, decision) in [
        (
            "Write",
            json!({"file_path":project("src/index.js"),"content":"Acme"}),
            "allow",
        ),
        (
            "Edit",
            json!({"file_path":project("acme.txt"),"old_string":"a","new_string":"b"}),
            "allow",
        ),
        (
            "MultiEdit",
            json!({"file_path":"relative.txt","edits":[]}),
            "allow",
        ),
        (
            "NotebookEdit",
            json!({"notebook_path":project("acme.ipynb"),"new_source":"x"}),
            "allow",
        ),
        (
            "Write",
            json!({"file_path":outside.path().join("x").to_str().unwrap(),"content":"Acme"}),
            "deny",
        ),
        (
            "Write",
            json!({"file_path":project(".shield/config.yml"),"content":"Acme"}),
            "deny",
        ),
        (
            "Edit",
            json!({"file_path":project(".claude/settings.local.json"),"old_string":"a","new_string":"b"}),
            "deny",
        ),
        // A content search of the project root reaches `.shield/review`, which
        // is blocked for reads; a narrower directory is searched by policy.
        ("Grep", json!({"pattern":"Acme"}), "deny"),
        (
            "Read",
            json!({"file_path":project("src/index.js")}),
            "allow",
        ),
        ("Read", json!({"file_path":"README.md"}), "allow"),
        (
            "Glob",
            json!({"pattern":"**/*.js","path":project("src")}),
            "allow",
        ),
    ] {
        let value = tool(root, "claude", name, &input);
        let output = &value["hookSpecificOutput"];
        assert_eq!(
            output["permissionDecision"], decision,
            "{name} {input}: {value}"
        );
        if input.to_string().contains(".shield") || input.to_string().contains(".claude") {
            assert!(
                reason(&value).starts_with("Shield: protected path."),
                "{value}"
            );
        }
    }
    // Writes are decided, never performed by the hook.
    assert!(!root.join("src/index.js").exists());
}

#[test]
fn native_session_tools_leave_the_decision_to_the_host() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for name in [
        "Agent",
        "AskUserQuestion",
        "CronCreate",
        "CronDelete",
        "CronList",
        "EndConversation",
        "EnterPlanMode",
        "ExitPlanMode",
        "ListAgents",
        "ListMcpResourcesTool",
        "ReportFindings",
        "ScheduleWakeup",
        "SendFeedback",
        "Skill",
        "SubagentHandback",
        "Task",
        "TaskCreate",
        "TaskGet",
        "TaskList",
        "TaskOutput",
        "TaskStop",
        "TaskUpdate",
        "TodoWrite",
        "ToolSearch",
        "WaitForMcpServers",
        "Workflow",
    ] {
        assert_eq!(tool(root, "claude", name, &json!({})), json!({}), "{name}");
    }
    for name in [
        "close_agent",
        "current_time",
        "followup_task",
        "get_context_remaining",
        "interrupt_agent",
        "list_agents",
        "list_available_plugins_to_install",
        "list_mcp_resource_templates",
        "list_mcp_resources",
        "new_context_window",
        "request_permissions",
        "request_user_input",
        "request_user_input_async",
        "resume_agent",
        "send_input",
        "send_message",
        "sleep",
        "spawn_agent",
        "tool_search",
        "update_plan",
        "wait_agent",
        "wait_for_environment",
    ] {
        assert_eq!(tool(root, "codex", name, &json!({})), json!({}), "{name}");
    }
    // Reading a running command's output or interrupting it passes; typed
    // text could run commands in an interactive program.
    for chars in ["", "\u{3}", "\u{4}"] {
        let input = json!({"session_id":7,"chars":chars});
        assert_eq!(
            tool(root, "codex", "write_stdin", &input),
            json!({}),
            "{chars:?}"
        );
    }
    let typed = tool(
        root,
        "codex",
        "write_stdin",
        &json!({"session_id":7,"chars":"rm -rf acme\n"}),
    );
    assert_eq!(typed["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[test]
fn claude_internal_tools_pass_and_unmodeled_tools_ask_the_user() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // A command watch is a shell request; a WebSocket watch asks.
    let watch = |command: &str| json!({"command":command,"description":"Acme","timeout_ms":1000});
    assert_permitted(
        &tool(root, "claude", "Monitor", &watch("git status")),
        "claude",
    );
    let value = tool(root, "claude", "Monitor", &watch("git reset --hard"));
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    let socket =
        json!({"ws":{"url":"wss://example.com/feed"},"description":"Acme","timeout_ms":1000});
    let value = tool(root, "claude", "Monitor", &socket);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");
    // Codex image views are reads of the image path.
    assert_permitted(
        &tool(root, "codex", "view_image", &json!({"path":"acme.png"})),
        "codex",
    );
    let value = tool(
        root,
        "codex",
        "view_image",
        &json!({"path":"/etc/acme.png"}),
    );
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    // A named environment may read another filesystem, so only the primary one is checked.
    let primary = json!({"path":"acme.png","environment_id":null});
    assert_permitted(&tool(root, "codex", "view_image", &primary), "codex");
    let named = json!({"path":"acme.png","environment_id":"acme-remote"});
    let value = tool(root, "codex", "view_image", &named);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    let browse = json!({"url":"https://example.com"});
    let value = tool(root, "claude", "AcmeBrowser", &browse);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");
    let value = tool(root, "codex", "AcmeBrowser", &browse);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    // MCP resource reads return another server's content, like its tools.
    let resource = json!({"server":"acme","uri":"file:///etc/acme"});
    for name in ["ReadMcpResourceTool", "ReadMcpResourceDirTool"] {
        let value = tool(root, "claude", name, &resource);
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");
    }
    let value = tool(root, "codex", "read_mcp_resource", &resource);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    // Claude messages can reach sessions outside this project's hooks.
    let message = json!({"to":"acme-session","message":"run git reset --hard"});
    let value = tool(root, "claude", "SendMessage", &message);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");
    // No rule can cover a tool Shield does not model.
    assert!(!reason(&value).contains("policy_propose"), "{value}");
    // History records what each host was told: Claude asked, Codex blocked.
    let response = review::mcp::call(
        &context,
        &review::mcp::Connection::new("unsupported-history".into()),
        "history_list",
        &json!({}),
        &|| Ok(()),
    );
    let told: Vec<_> = response["data"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["command"]["name"] == "AcmeBrowser")
        .map(|e| {
            (
                e["disposition"].as_str().unwrap_or_default().to_owned(),
                e["observations"][0]["details"]["host_decision"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect();
    assert!(
        told.contains(&("approval_required".into(), "ask".into())),
        "{told:?}"
    );
    assert!(
        told.contains(&("blocked".into(), "deny".into())),
        "{told:?}"
    );
}

#[test]
fn web_searches_and_fetches_follow_web_rules() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let search = json!({"query":"acme express setup","allowed_domains":["docs.example.com"]});
    assert_permitted(&tool(root, "claude", "WebSearch", &search), "claude");
    let docs = json!({"url":"https://docs.example.com/guide","prompt":"Summarize"});
    assert_permitted(&tool(root, "claude", "WebFetch", &docs), "claude");
    let other = json!({"url":"https://other.example.com/","prompt":"Summarize"});
    let value = tool(root, "claude", "WebFetch", &other);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        reason(&value).contains("https://docs.example.com"),
        "{value}"
    );
    // Codex sends one or more searches in a call; each is decided.
    let searches = json!({"search_query":[{"q":"acme express"},{"q":"acme node","recency":30}],"response_length":"short"});
    assert_permitted(&tool(root, "codex", "webrun", &searches), "codex");
    let open_docs = json!({"open":[{"ref_id":"https://docs.example.com/guide","lineno":0},{"ref_id":"https://docs.example.com/api"}],"response_length":"short"});
    assert_permitted(&tool(root, "codex", "webrun", &open_docs), "codex");
    let mixed =
        json!({"search_query":[{"q":"acme docs"}],"open":[{"ref_id":"https://docs.example.com/"}]});
    assert_permitted(&tool(root, "codex", "webrun", &mixed), "codex");
    // A direct open reaches the same URL policy as Claude WebFetch.
    let open = json!({"open":[{"ref_id":"https://other.example.com/"}]});
    let value = tool(root, "codex", "webrun", &open);
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    let message = reason(&value);
    assert!(
        message.contains("Fetch only https://docs.example.com"),
        "{message}"
    );
    for malformed in [
        json!({"search_query":[]}),
        json!({"search_query":[{"q":"acme","url":"https://other.example.com/"}]}),
    ] {
        let value = tool(root, "codex", "webrun", &malformed);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{malformed}"
        );
    }
    let value = tool(
        root,
        "claude",
        "WebFetch",
        &json!({"url":"https://docs.example.com/","method":"POST"}),
    );
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[test]
fn host_fetches_are_the_agent_client_for_web_rules() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"agent-docs","decision":"allow","conditions":{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["docs.example.com"]},"request_fields":{"/client":{"exact":"agent"}}}}
    ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let project = Path::new(&context.project_root);
    let docs = json!({"url":"https://docs.example.com/guide","prompt":"Summarize"});
    assert_permitted(&tool(project, "claude", "WebFetch", &docs), "claude");
    let open = json!({"open":[{"ref_id":"https://docs.example.com/guide"}]});
    assert_permitted(&tool(project, "codex", "webrun", &open), "codex");
    let value = unscoped_shell(
        project,
        "claude",
        "curl -fsS https://docs.example.com/guide",
    );
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "ask",
        "{value}"
    );
}

#[test]
fn codex_open_batches_preserve_policy_decisions_and_original_input() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"docs","decision":"allow","conditions":{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["docs.example.com"]}}},
        {"id":"deny","decision":"deny","reason":"Blocked Acme host","conditions":{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["blocked.example.com"]}}},
        {"id":"ask","decision":"ask","conditions":{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["review.example.com"]}}}
    ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let project = Path::new(&context.project_root);
    for fallback in [AskFallback::Block, AskFallback::Allow] {
        integrations::configure_fixture(project, IntegrationChoice::agent("codex"), fallback);
        for host in ["review.example.com", "unmatched.example.com"] {
            let input = json!({"open":[{"ref_id":"https://docs.example.com/"},{"ref_id":format!("https://{host}/")}]});
            let value = tool(project, "codex", "webrun", &input);
            if matches!(fallback, AskFallback::Allow) {
                assert_permitted(&value, "codex");
            } else {
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{value}"
                );
            }
            // PermissionRequest keeps Ask in the host's existing flow.
            assert_eq!(permission(project, "webrun", input), json!({}));
        }
        for input in [
            json!({"open":[{"ref_id":"https://docs.example.com/"},{"ref_id":"https://blocked.example.com/"}]}),
            json!({"search_query":[{"q":"acme"}],"open":[{"ref_id":"https://blocked.example.com/"},{"ref_id":"https://docs.example.com/"}]}),
        ] {
            let value = tool(project, "codex", "webrun", &input);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{value}"
            );
            assert!(
                reason(&value).contains("Shield: denied. Rule `deny` denies this request"),
                "{value}"
            );
            assert_eq!(
                permission(project, "webrun", input)["hookSpecificOutput"]["decision"]["behavior"],
                "deny"
            );
        }
    }
    let original = json!({"open":[{"ref_id":"https://docs.example.com/guide?a=1#part","lineno":12},{"ref_id":"https://docs.example.com/api"}],"response_length":"short"});
    assert_permitted(&tool(project, "codex", "webrun", &original), "codex");
    let response = review::mcp::call(
        &context,
        &review::mcp::Connection::new("web-history".into()),
        "history_list",
        &json!({}),
        &|| Ok(()),
    );
    let entries = response["data"]["entries"].as_array().unwrap();
    for target in [
        "https://docs.example.com/guide?a=1#part",
        "https://docs.example.com/api",
    ] {
        assert!(
            entries.iter().any(|e| e["command"]["url"] == target
                && e["evaluation"]["decision"] == "allow"
                && e["observations"][0]["details"]["original_input"] == original),
            "{response}"
        );
    }
    assert!(entries.iter().any(|e| e["evaluation"]["decision"] == "ask"
        && e["ask_fallback"] == "allow"
        && e["approved"] == false));
}

#[test]
fn codex_unmodeled_and_malformed_browsing_never_uses_allow_fallback() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let (_root, context) = fixture();
    let project = Path::new(&context.project_root);
    integrations::configure_fixture(
        project,
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    for input in [
        json!({"open":[{"ref_id":"turn0search0"}]}),
        json!({"open":[{"ref_id":"https://docs.example.com/"},{"ref_id":"turn1view0"}]}),
        json!({"search_query":[{"q":"acme"}],"click":[{"ref_id":"turn1view0","id":2}]}),
        json!({"open":[{"ref_id":"file:///etc/hosts"}]}),
        json!({"find":[{"ref_id":"https://docs.example.com/","pattern":"acme"}]}),
    ] {
        let value = tool(project, "codex", "webrun", &input);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{input}: {value}"
        );
        let message = reason(&value);
        assert!(
            message.contains("`webrun`") && message.contains("direct HTTP(S) URLs"),
            "{message}"
        );
        assert!(!message.contains("policy_propose"), "{message}");
    }
    for input in [
        json!({}),
        json!({"open":[]}),
        json!({"open":null}),
        json!({"open":[{}]}),
        json!({"open":[{"ref_id":42}]}),
        json!({"open":[{"ref_id":"https://"}]}),
        json!({"open":[{"ref_id":"https:docs.example.com"}]}),
        json!({"open":[{"ref_id":"https://docs.example.com/\n"}]}),
        json!({"open":[{"ref_id":"https://docs.example.com/","lineno":-1}]}),
        json!({"open":[{"ref_id":"https://docs.example.com/","method":"POST"}]}),
        json!({"open":[{"ref_id":"https://docs.example.com/"}],"response_length":"huge"}),
        json!({"search_query":[{"q":"acme","recency":"recent"}]}),
        json!({"search_query":[{"q":"acme","domains":[1]}]}),
        json!({"open":vec![json!({"ref_id":"https://docs.example.com/"});17]}),
        json!({"search_query":[{"q":"acme"}],"open":vec![json!({"ref_id":"https://docs.example.com/"});16]}),
    ] {
        let value = tool(project, "codex", "webrun", &input);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{input}: {value}"
        );
        assert!(reason(&value).contains("invalid request"), "{value}");
    }
}

#[test]
fn unmatched_web_requests_ask_and_codex_uses_its_fallback() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let project = Path::new(&context.project_root);
    let search = json!({"query":"acme"});
    let value = tool(project, "claude", "WebSearch", &search);
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "ask",
        "{value}"
    );
    let value = tool(
        project,
        "codex",
        "webrun",
        &json!({"search_query":[{"q":"acme"}]}),
    );
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "deny",
        "{value}"
    );
}

#[test]
fn timeout_wrapped_commands_are_decided_by_the_command_they_run() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let value = call(root, "claude", "timeout 5 git reset --hard");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    for source in ["timeout 5 git status", "PORT=3999 timeout 5 acme serve"] {
        let value = call(root, "claude", source);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "ask",
            "{source}: {value}"
        );
        assert!(
            !reason(&value).contains("cannot inspect"),
            "{source}: {value}"
        );
    }
}

#[test]
fn agents_use_mcp_tools_instead_of_the_operator_cli() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        let value = call(root, agent, "shield --help");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(reason(&value).contains("policy_propose"), "{value}");
        assert!(
            !reason(&value).starts_with("Shield: protected path."),
            "{value}"
        );
        // Interpreter launches ask plainly; rewriting cannot make them inspectable.
        let value = call(root, agent, "node acme.js");
        assert!(!reason(&value).contains("rewrite"), "{value}");
        let value = call(root, agent, "git reset --hard");
        assert!(reason(&value).contains("policy_propose"), "{value}");
    }
}

#[test]
fn agents_cannot_launch_renamed_mcp_transports_from_shell_tools() {
    let root = argv_fixture("acme-shield-copy", "allow");
    let original = root.path().join("original");
    let copy = root.path().join("acme-shield-copy");
    let link = root.path().join("acme-shield-link");
    std::fs::write(&original, "Acme executable identity fixture").unwrap();
    std::fs::copy(&original, &copy).unwrap();
    std::fs::hard_link(&original, &link).unwrap();
    for agent in ["claude", "codex"] {
        assert_permitted(&call(root.path(), agent, "acme-shield-copy read"), agent);
        for executable in [
            "acme-shield-copy",
            "./acme-shield-copy",
            "./acme-shield-link",
        ] {
            for arguments in [
                "mcp serve --agent codex",
                "--project . mcp serve --agent claude-code",
            ] {
                let value = call(root.path(), agent, &format!("{executable} {arguments}"));
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{value}"
                );
                assert!(reason(&value).contains("policy_propose"), "{value}");
            }
        }
    }
}

#[test]
fn uninspectable_source_returns_to_the_agent_to_rewrite() {
    let (root, context) = fixture();
    for agent in ["claude", "codex"] {
        let value = call(
            Path::new(&context.project_root),
            agent,
            "acme read \"$(touch side-effect)\"",
        );
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        let reason = reason(&value);
        assert!(
            reason.starts_with("Shield: rewrite this request"),
            "{reason}"
        );
        assert!(
            reason.contains("Candidate: Use literal arguments"),
            "{reason}"
        );
        // A command no rule covers names the reason in Claude's user prompt;
        // Codex's agent-facing refusal also explains how to propose a rule.
        let value = call(Path::new(&context.project_root), agent, "git add acme.txt");
        let expected = if agent == "claude" { "ask" } else { "deny" };
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], expected);
        let reason = self::reason(&value);
        assert!(reason.contains("approval required"), "{reason}");
        assert!(
            reason.contains("`git add acme.txt` matches no rule"),
            "{reason}"
        );
        assert_eq!(
            reason.contains("policy_propose"),
            agent == "codex",
            "{reason}"
        );
    }
    assert!(!root.path().join("side-effect").exists());
}

#[test]
fn background_pid_origin_reaches_approval_without_literal_pid_authorization() {
    let (root, _) = fixture();
    let source = "(PORT=3999 node src/server.js & pid=$!; for i in 1 2 3; do curl -s localhost:3999/health && break; sleep 0.3; done; kill \"$pid\")";
    for agent in ["claude", "codex"] {
        let reply = call(root.path(), agent, source);
        assert_eq!(
            reply["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" },
            "{agent}: {reply}"
        );
        let text = reason(&reply);
        assert!(text.contains("background job"), "{agent}: {reply}");
        assert!(
            text.contains("runtime ownership is unverified"),
            "{agent}: {reply}"
        );
        assert!(!text.contains("rewrite this request"), "{agent}: {reply}");
        assert!(
            !text.contains("\"executable\":\"kill\""),
            "partial kill argv must not become a proposed exact rule: {reply}"
        );
    }
    let reply = call(root.path(), "claude", "pid=$(cat pidfile); kill \"$pid\"");
    assert_eq!(reply["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(reason(&reply).contains("rewrite this request"));
}

#[test]
fn finite_loop_feedback_names_the_command_requiring_approval() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        for source in [
            "for attempt in 1 2 3; do node src/server.js && break; done",
            "for attempt in 1 2 3; do acme-unlisted && continue; done",
        ] {
            let value = call(root, agent, source);
            let text = reason(&value);
            let command = if source.contains("node") {
                "node src/server.js"
            } else {
                "acme-unlisted"
            };
            assert!(text.contains(command), "{agent}: {value}");
            assert!(!text.contains("rewrite this request"), "{agent}: {value}");
            assert!(!text.contains("`break`"), "{agent}: {value}");
            assert!(!text.contains("`continue`"), "{agent}: {value}");
        }
        let value = call(
            root,
            agent,
            "(PORT=3999 node src/server.js & pid=$!; for i in 1 2 3 4 5 6 7 8 9 10; do curl -s localhost:3999/health && break; sleep 0.3; done; echo; kill $pid)",
        );
        let text = reason(&value);
        assert!(text.contains("`kill $pid`"), "{agent}: {value}");
        assert!(!text.contains("`break`"), "{agent}: {value}");
    }
}

#[test]
fn native_policy_allow_ask_and_deny_reach_the_host_without_executing_source() {
    let (root, context) = fixture();
    for agent in ["claude", "codex"] {
        for (source, decision, code) in [
            ("acme read", "allow", "permits this request"),
            ("git status", "allow", "permits this request"),
            ("git reset --hard", "deny", "policy-deny"),
            ("git add acme.txt", "deny", "approval-required"),
            ("git status; git reset --hard", "deny", "policy-deny"),
            ("git status; touch side-effect", "deny", "approval-required"),
            (
                "git -c alias.status='!touch side-effect' status",
                "deny",
                "approval-required",
            ),
        ] {
            let value = call(Path::new(&context.project_root), agent, source);
            let expected = if agent == "claude" && code == "approval-required" {
                "ask"
            } else {
                decision
            };
            if decision == "allow" {
                assert_permitted(&value, agent);
            } else {
                assert_eq!(value["hookSpecificOutput"]["permissionDecision"], expected);
            }
            if agent == "codex" && code == "approval-required" {
                assert!(value.to_string().contains("Codex cannot prompt here"));
            }
            let reason = if code == "approval-required" {
                "approval required"
            } else if code == "policy-deny" {
                "Shield: denied. Rule `deny` denies this request"
            } else {
                code
            };
            if agent == "claude" && decision == "allow" {
                assert!(
                    value["hookSpecificOutput"]
                        .get("additionalContext")
                        .is_none()
                );
            } else {
                assert!(value.to_string().contains(reason), "{source}: {value}");
            }
            assert!(!root.path().join("side-effect").exists());
        }
        // Retaining vendor arguments must not reduce the native envelope capacity
        // by serializing the same source twice in the normalized envelope.
        let host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_use_id":"acme-large","session_id":"acme-session","cwd":context.project_root,
            "tool_input":{"command":format!("acme read{}", " ".repeat(3_000)),
                "description":" ".repeat(61_000)}});
        assert_permitted(
            &translate(
                Path::new(&context.project_root),
                agent,
                &serde_json::to_vec(&host).unwrap(),
            ),
            agent,
        );
        // Commands past the 4,096-byte argument bound reach their policy
        // decision, including JSON escaping; source over 64 KiB is refused.
        for long in [
            format!("acme read{}", " ".repeat(8_000)),
            format!("acme read # {}", "\"".repeat(34_000)),
        ] {
            assert_permitted(&call(Path::new(&context.project_root), agent, &long), agent);
        }
        let value = call(
            Path::new(&context.project_root),
            agent,
            &format!("acme read{}", " ".repeat(65_536)),
        );
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{agent}"
        );
    }
}

#[test]
fn native_bound_reads_allow_and_malformed_foreign_unknown_requests_block() {
    let (_fixture, context) = fixture();
    let root = Path::new(&context.project_root);
    let path = root.join("acme.txt");
    fs::write(&path, "ACME_PRIVATE_READ_OUTPUT").unwrap();
    let input = json!({"hook_event_name":"PreToolUse","tool_name":"Read","tool_use_id":"acme-read","session_id":"acme-session","cwd":root,"tool_input":{"file_path":path}});
    let result = translate(root, "claude", &serde_json::to_vec(&input).unwrap());
    assert_eq!(
        result["hookSpecificOutput"]["permissionDecision"], "allow",
        "{result}"
    );
    let mut extra = input.clone();
    extra["tool_input"]["approved"] = json!(true);
    let mut foreign = input.clone();
    foreign["cwd"] = json!("/");
    let mut unknown = input.clone();
    unknown["tool_name"] = json!("AcmeTool");
    for (mutated, decision) in [(extra, "deny"), (foreign, "deny"), (unknown, "ask")] {
        assert_eq!(
            translate(root, "claude", &serde_json::to_vec(&mutated).unwrap())["hookSpecificOutput"]
                ["permissionDecision"],
            decision
        );
    }
    for agent in ["claude", "codex"] {
        let value = tool(
            root,
            agent,
            "AcmeTool",
            &json!({"private":"ACME_PRIVATE_INPUT"}),
        );
        let message = reason(&value);
        assert!(message.contains("`AcmeTool`"), "{agent}: {message}");
        assert!(!message.contains("ACME_PRIVATE_INPUT"), "{message}");
        assert!(!message.contains("search_query"), "{message}");
        assert!(!message.contains("policy_propose"), "{message}");
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" },
            "{value}"
        );
    }
    for bytes in [
        b"invalid".as_slice(),
        br#"{"hook_event_name":"PreToolUse","hook_event_name":"PostToolUse"}"#,
    ] {
        assert_eq!(
            translate(root, "codex", bytes)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
    let state = fs::read_to_string(root.join(".shield/review/state.json")).unwrap();
    assert!(!state.contains("ACME_PRIVATE_READ_OUTPUT"));
}

#[test]
fn canonical_mcp_inspection_remains_usable_and_unknown_server_tools_block() {
    let (_fixture, context) = fixture();
    let root = Path::new(&context.project_root);
    let mut host = json!({"hook_event_name":"PreToolUse","tool_name":"mcp__shield__status","tool_use_id":"acme-status","session_id":"acme-session","cwd":root,"tool_input":{}});
    for tool in ["status", "policy_propose"] {
        host["tool_name"] = json!(format!("mcp__shield__{tool}"));
        let allowed = translate(root, "claude", &serde_json::to_vec(&host).unwrap());
        assert_permitted(&allowed, "claude");
        assert!(
            allowed["hookSpecificOutput"]
                .get("additionalContext")
                .is_none()
        );
        assert_permitted(
            &translate(root, "codex", &serde_json::to_vec(&host).unwrap()),
            "codex",
        );
        host["hook_event_name"] = json!("PermissionRequest");
        assert_eq!(
            translate_for(
                root,
                "codex",
                Event::Permission,
                &serde_json::to_vec(&host).unwrap()
            )["hookSpecificOutput"]["decision"]["behavior"],
            "allow"
        );
        host["hook_event_name"] = json!("PostToolUse");
        for agent in ["claude", "codex"] {
            assert_eq!(
                translate_for(
                    root,
                    agent,
                    Event::Completed,
                    &serde_json::to_vec(&host).unwrap()
                ),
                json!({})
            );
        }
        host["hook_event_name"] = json!("PreToolUse");
    }
    host["tool_name"] = json!("mcp__shield__policy_apply");
    for mode in ["default", "auto", "bypassPermissions"] {
        host["permission_mode"] = json!(mode);
        let bytes = serde_json::to_vec(&host).unwrap();
        assert_eq!(
            translate(root, "claude", &bytes)["hookSpecificOutput"]["permissionDecision"],
            "allow"
        );
        host["tool_name"] = json!("mcp__shield__status");
        assert_permitted(
            &translate(root, "claude", &serde_json::to_vec(&host).unwrap()),
            "claude",
        );
        host["tool_name"] = json!("mcp__shield__policy_apply");
    }
    host["permission_mode"] = json!("default");
    crate::review::operator::integrations::configure_fixture(
        root,
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    let bytes = serde_json::to_vec(&host).unwrap();
    assert_eq!(
        translate(root, "claude", &bytes)["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(translate(root, "codex", &bytes), json!({}));
    host["hook_event_name"] = json!("PermissionRequest");
    assert_eq!(
        translate_for(
            root,
            "codex",
            Event::Permission,
            &serde_json::to_vec(&host).unwrap()
        ),
        json!({})
    );
    host["hook_event_name"] = json!("PreToolUse");
    host["tool_name"] = json!("mcp__shield__approve");
    assert_eq!(
        translate(root, "codex", &serde_json::to_vec(&host).unwrap())["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    host["tool_name"] = json!("mcp__other__policy_propose");
    assert_eq!(
        translate(root, "codex", &serde_json::to_vec(&host).unwrap()),
        json!({})
    );
}

#[test]
fn unmodeled_completions_are_quiet_without_hiding_supported_history_failures() {
    let (root, _) = fixture();
    let before = journal(root.path());
    for adapter in super::super::ADAPTERS {
        for (name, event) in adapter
            .spec()
            .events
            .iter()
            .filter(|(_, event)| event.post())
        {
            for tool in ["mcp__acme__run", "AcmeTool"] {
                let host = json!({"hook_event_name":name,"tool_name":tool,"tool_use_id":"acme-unmodeled",
                    "session_id":"acme-session","cwd":root.path(),"tool_input":{},"tool_response":"ACME_PRIVATE_RESULT"});
                let value = translate_for(
                    root.path(),
                    adapter.spec().id,
                    *event,
                    &serde_json::to_vec(&host).unwrap(),
                );
                assert_eq!(
                    value,
                    json!({}),
                    "{} {name} {tool}: {value}",
                    adapter.spec().id
                );
            }
        }
    }
    assert_eq!(
        journal(root.path()),
        before,
        "Unsupported completions must not invent execution history"
    );

    fs::remove_file(root.path().join(".shield/review/state.json")).unwrap();
    for agent in ["claude", "codex"] {
        let host = json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"acme-supported",
            "session_id":"acme-session","cwd":root.path(),"tool_input":{"command":"git status"}});
        let value = translate_for(
            root.path(),
            agent,
            Event::Completed,
            &serde_json::to_vec(&host).unwrap(),
        );
        assert_eq!(
            value,
            json!({"systemMessage":"Shield completion history unavailable; outcome remains unknown."}),
            "{agent}: {value}"
        );
    }
}

#[test]
fn malformed_completion_input_is_quiet_unless_it_names_an_authorization_event() {
    let (root, _) = fixture();
    let before = journal(root.path());
    for adapter in super::super::ADAPTERS {
        for (name, event) in adapter
            .spec()
            .events
            .iter()
            .filter(|(_, event)| event.post())
        {
            let malformed = [
                b"not JSON".to_vec(),
                serde_json::to_vec(&json!({"hook_event_name":name})).unwrap(),
                serde_json::to_vec(&json!({"tool_name":"Bash"})).unwrap(),
            ];
            for bytes in malformed {
                let value = translate_for(root.path(), adapter.spec().id, *event, &bytes);
                assert_eq!(value, json!({}), "{} {name}: {value}", adapter.spec().id);
            }
            let pre = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-pre",
                "session_id":"acme-session","cwd":root.path(),"tool_input":{"command":"git status"}});
            let value = translate_for(
                root.path(),
                adapter.spec().id,
                *event,
                &serde_json::to_vec(&pre).unwrap(),
            );
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"],
                "deny",
                "{} {name}: {value}",
                adapter.spec().id
            );
            // Unparseable input, or parsed input with no recognized event, that
            // names an authorization event still blocks.
            for bytes in [
                br#"{"hook_event_name":"PreToolUse","hook_event_name":"PreToolUse","tool_name":"Bash"}"#.as_slice(),
                br#"{"hook_event_name":"PreToolUse","tool_name":"Ba"#,
                br#"{"hook_event_name":7,"tool_name":"Bash","note":"PreToolUse"}"#,
            ] {
                let value = translate_for(root.path(), adapter.spec().id, *event, bytes);
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"],
                    "deny",
                    "{} {name}: {value}",
                    adapter.spec().id
                );
            }
        }
    }
    assert_eq!(journal(root.path()), before);
}

#[test]
fn shell_history_redacts_named_secrets_without_retaining_analyzer_syntax_trees() {
    let (_fixture, context) = fixture();
    let root = Path::new(&context.project_root);
    call(root, "claude", "acme --token ACME_PRIVATE_TOKEN_VALUE");
    let history = journal(root);
    assert!(!history.contains("ACME_PRIVATE_TOKEN_VALUE"));
    assert!(!history.contains("source_units"));
}

#[test]
fn native_ask_fallback_is_scoped_truthful_and_never_permits_failures_or_protected_access() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let (root, context) = fixture();
    let (other, _) = fixture();
    integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    fs::create_dir_all(root.path().join(".claude/skills/acme")).unwrap();
    assert_permitted(
        &call(root.path(), "codex", "cat .claude/skills/acme/SKILL.md"),
        "codex",
    );
    assert_permitted(&call(root.path(), "codex", "git push"), "codex");
    assert_permitted(&call(root.path(), "codex", "git add acme.txt"), "codex");
    assert_eq!(
        call(other.path(), "codex", "git add acme.txt")["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(
        call(root.path(), "claude", "git add acme.txt")["hookSpecificOutput"]["permissionDecision"],
        "ask"
    );
    for source in [
        "git reset --hard",
        "git status; git reset --hard",
        "shield mcp setup --agent codex",
        "shield request approve request-1",
        "echo allow > .shield/review/state.json",
        "rm -rf .",
        "cd .codex; mv hooks.json backup.json",
        "rm -rf .codex",
        "rm -rf .claude",
    ] {
        assert_eq!(
            call(root.path(), "codex", source)["hookSpecificOutput"]["permissionDecision"],
            "deny",
            "{source}"
        );
    }
    let invalid = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"bad","session_id":"test","cwd":root.path(),"tool_input":{"command":"echo hi","ask_fallback":"allow"}});
    assert_eq!(
        translate(root.path(), "codex", &serde_json::to_vec(&invalid).unwrap())["hookSpecificOutput"]
            ["permissionDecision"],
        "deny"
    );
    let unknown = json!({"hook_event_name":"PreToolUse","tool_name":"Write","tool_use_id":"bad","session_id":"test","cwd":root.path(),"tool_input":{}});
    assert_eq!(
        translate(root.path(), "codex", &serde_json::to_vec(&unknown).unwrap())["hookSpecificOutput"]
            ["permissionDecision"],
        "deny"
    );
    let response = review::mcp::call(
        &context,
        &review::mcp::Connection::new("fallback-history".into()),
        "history_list",
        &json!({}),
        &|| Ok(()),
    );
    let history = &response["data"];
    assert!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["evaluation"]["decision"] == "ask"
                && e["disposition"] == "allowed"
                && e["ask_fallback"] == "allow"
                && e["observations"][0]["details"]["host_decision"] == "allow"
                && e["approved"] == false)
    );
    integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Block,
    );
    assert_eq!(
        call(root.path(), "codex", "git add acme.txt")["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    let path = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["operator"]["integration"]["ask_fallback"]["codex"] = json!("invalid");
    fs::write(path, state.to_string()).unwrap();
    assert_eq!(
        call(root.path(), "codex", "acme read")["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}

#[test]
fn native_paths_bind_cwd_and_distinguish_move_sources_from_directory_destinations() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let (root, _) = fixture();
    integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    for source in [
        "cd .codex && printf test > hooks.json",
        "cd .codex/skills; printf test > ../hooks.json",
        "cd .codex; printf test >& hooks.json",
        "mv .codex ..",
        "mv /tmp/hooks.json .codex",
        "cp /tmp/hooks.json .codex",
        "mv -t.codex /tmp/hooks.json",
        "rm -rf .",
    ] {
        assert_eq!(
            call(root.path(), "codex", source)["hookSpecificOutput"]["permissionDecision"],
            "deny",
            "{source}"
        );
    }
    for source in [
        "mv /tmp/acme.txt .",
        "mv -t . /tmp/acme.txt",
        "mv -- /tmp/acme.txt .",
        "cp /tmp/acme.txt .",
        "mv acme.txt renamed.txt",
        "cd src; echo hello",
        "printf hello > ordinary.txt",
        "cd src; echo hello >&2",
    ] {
        assert_permitted(&call(root.path(), "codex", source), "codex");
    }
    for (directory, file) in [(".codex", "hooks.json"), (".shield/review", "state.json")] {
        let cwd = root.path().join(directory);
        let host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"relative","session_id":"test","cwd":cwd,"tool_input":{"command":format!("printf test > {file}")}});
        assert_eq!(
            translate(root.path(), "codex", &serde_json::to_vec(&host).unwrap())["hookSpecificOutput"]
                ["permissionDecision"],
            "deny"
        );
    }
}

#[cfg(unix)]
#[test]
fn native_protected_paths_cannot_hide_behind_file_or_directory_symlinks() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    use std::os::unix::fs::symlink;
    let (root, _) = fixture();
    integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    symlink(
        root.path().join(".codex/hooks.json"),
        root.path().join("ordinary.txt"),
    )
    .unwrap();
    symlink(
        root.path().join(".shield/review"),
        root.path().join("ordinary-dir"),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("bin")).unwrap();
    fs::write(
        root.path().join("bin/shield"),
        "fixture executable identity",
    )
    .unwrap();
    symlink(
        root.path().join("bin/shield"),
        root.path().join("ordinary-cli"),
    )
    .unwrap();
    for source in [
        "printf test > ordinary.txt",
        "printf test > ordinary-dir/new-state.json",
        "./ordinary-cli approve request-1",
    ] {
        assert_eq!(
            call(root.path(), "codex", source)["hookSpecificOutput"]["permissionDecision"],
            "deny",
            "{source}"
        );
    }
    fs::write(root.path().join("acme.txt"), "ordinary file").unwrap();
    symlink(
        root.path().join("acme.txt"),
        root.path().join("ordinary-alias.txt"),
    )
    .unwrap();
    assert_permitted(
        &call(root.path(), "codex", "printf test > ordinary-alias.txt"),
        "codex",
    );
}

#[test]
fn native_shell_rules_match_complete_input_like_mcp_diagnostics() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    for source in ["acme read", "acme read; acme opaque"] {
        let root = assert_fs::TempDir::new().unwrap();
        let original = json!({"command":source,"description":"exact denied request"});
        let input = json!({"kind":"shell","source":source,"dialect":"posix","dialect_version":null,"original_input":original});
        let ctx = json!({"cwd":root.path().canonicalize().unwrap(),"environment":null});
        let rules = json!({"schema_version":1,"rules":[{"id":"literal","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}},{"id":"full-deny","decision":"deny","conditions":{"domain":"exact_request","input":input,"context":ctx,"executable":null},"reason":"Exact denial"}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
        let bundle = Bundle::capture(
            "schema_version: 1\npacks: []\nrules: custom.json\n",
            &[],
            Some(&rules),
        )
        .unwrap();
        let context = review::mcp::initialize_fixture(root.path(), &bundle);
        integrations::configure_fixture(
            root.path(),
            IntegrationChoice::agent("codex"),
            AskFallback::Allow,
        );
        let diagnostic = review::mcp::call(
            &context,
            &review::mcp::Connection::new("native-parity".into()),
            "policy_evaluate",
            &json!({"input":input,"context":ctx}),
            &|| Ok(()),
        );
        assert_eq!(diagnostic["data"]["decision"], "deny", "{diagnostic}");
        assert_eq!(diagnostic["data"]["explanation"]["decision"], "deny");
        let mut host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"exact","session_id":"test","cwd":root.path(),"tool_input":original});
        assert_eq!(
            translate(root.path(), "codex", &serde_json::to_vec(&host).unwrap())["hookSpecificOutput"]
                ["permissionDecision"],
            "deny"
        );
        host["tool_input"]["description"] = json!("different request");
        assert_permitted(
            &translate(root.path(), "codex", &serde_json::to_vec(&host).unwrap()),
            "codex",
        );
    }
}

#[test]
fn native_exact_allowances_bind_vendor_context_and_cannot_bypass_protection() {
    let root = assert_fs::TempDir::new().unwrap();
    let mut rows = Vec::new();
    for (id, source, expiry) in [
        ("exact", "acme opaque", None),
        ("expired", "acme expired", Some("2020-01-01T00:00:00Z")),
        ("protected", "printf test > .shield/review/state.json", None),
        ("nested-deny", "acme forbidden", None),
    ] {
        rows.push(json!({"id":id,"decision":"allow","conditions":{"domain":"exact_request","input":{"kind":"shell","source":source,"dialect":"bash","dialect_version":null,"original_input":{"command":source}},"context":{"cwd":root.path().canonicalize().unwrap(),"environment":null},"executable":null}}));
        if let Some(expiry) = expiry {
            rows.last_mut().unwrap()["expires_at"] = json!(expiry);
        }
    }
    rows.push(json!({"id":"deny-operation","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["forbidden"]}}}));
    let rules = json!({"schema_version":1,"rules":rows,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root.path(), &bundle);
    for (source, decision) in [
        ("acme opaque", "allow"),
        ("acme expired", "ask"),
        ("acme changed", "ask"),
        ("printf test > .shield/review/state.json", "deny"),
        ("acme forbidden", "deny"),
    ] {
        assert_eq!(
            call(root.path(), "claude", source)["hookSpecificOutput"]["permissionDecision"],
            decision,
            "{source}"
        );
    }
    let mut host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"changed","session_id":"test","cwd":root.path(),"tool_input":{"command":"acme opaque","description":"extra vendor field"}});
    assert_eq!(
        translate(root.path(), "claude", &serde_json::to_vec(&host).unwrap())["hookSpecificOutput"]
            ["permissionDecision"],
        "ask"
    );
    fs::create_dir(root.path().join("nested")).unwrap();
    host["tool_input"] = json!({"command":"acme opaque"});
    host["cwd"] = json!(root.path().join("nested"));
    assert_eq!(
        translate(root.path(), "claude", &serde_json::to_vec(&host).unwrap())["hookSpecificOutput"]
            ["permissionDecision"],
        "ask"
    );
}

#[test]
fn agents_read_and_record_policy_files_but_not_state_and_ask_for_agent_settings() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    let project = |name: &str| root.join(name).to_str().unwrap().to_owned();
    let blocked = |value: &Value| reason(value).starts_with("Shield: protected path.");
    for (name, input, decision) in [
        (
            "Read",
            json!({"file_path":project(".shield/config.yml")}),
            "allow",
        ),
        (
            "Read",
            json!({"file_path":project(".shield/review/state.json")}),
            "deny",
        ),
        (
            "Grep",
            json!({"pattern":"Acme","path":project(".shield")}),
            "deny",
        ),
        (
            "Write",
            json!({"file_path":project(".shield/rules.yml"),"content":"x"}),
            "deny",
        ),
        ("Read", json!({"file_path":project(".mcp.json")}), "ask"),
    ] {
        let value = tool(root, "claude", name, &input);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], decision,
            "{name} {input}: {value}"
        );
        assert_eq!(blocked(&value), decision == "deny", "{value}");
    }
    // Reads and Git records reach policy; writes and state stay blocked.
    for (source, protected) in [
        ("cat .shield/config.yml", false),
        ("git add .shield/rules.yml .shield/shield.lock", false),
        ("git add .shield", false),
        ("ls .shield", false),
        ("cat .shield/review/state.json", true),
        ("ls .shield/review", true),
        ("grep -r acme .shield", true),
        ("git checkout -- .shield/config.yml", true),
        ("git diff --output=.shield/config.yml", true),
        ("find .shield -delete", true),
        ("cat acme.txt > .shield/config.yml", true),
        ("sed -n 1p .shield/config.yml", false),
        // Compound commands keep each command's own access.
        (
            "echo --- ; git add .shield/rules.yml .shield/shield.lock",
            false,
        ),
        ("printf x > acme.txt && cat .shield/config.yml", false),
        ("echo x && cat acme.txt > .shield/config.yml", true),
        (
            "git add .shield/rules.yml; sed -i x .shield/rules.yml",
            true,
        ),
        ("cat .shield/review/state.json; echo", true),
    ] {
        let value = call(root, "claude", source);
        assert_eq!(blocked(&value), protected, "{source}: {value}");
    }
    let value = call(root, "claude", "git add .mcp.json");
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "ask",
        "{value}"
    );
    assert!(reason(&value).contains("credentials"), "{value}");
    let value = call(root, "codex", "cat .mcp.json");
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "deny",
        "{value}"
    );
    assert!(reason(&value).contains("credentials"), "{value}");
    // Committing .shield shares policy files, never review state or the cache.
    assert_eq!(
        fs::read_to_string(root.join(".shield/.gitignore")).unwrap(),
        "review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n"
    );
}

#[test]
fn claude_recursive_reads_check_the_tree_they_search_like_shell_ripgrep() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::write(root.join(".codex/config.toml"), "").unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/app.js"), "Acme").unwrap();
    let project = |name: &str| root.join(name).to_str().unwrap().to_owned();
    for (name, input, decision) in [
        // Agent settings below a searched folder ask, as reading them does.
        (
            "Grep",
            json!({"pattern":".","path":project(".codex")}),
            "ask",
        ),
        (
            "Grep",
            json!({"pattern":".","path":project("."),"glob":"{.shield,.shield/**}"}),
            "deny",
        ),
        (
            "Grep",
            json!({"pattern":"Acme","path":project("src"),"glob":"*.js","type":"js"}),
            "allow",
        ),
        // Listings read names, not contents.
        (
            "Glob",
            json!({"pattern":"**/*","path":project(".")}),
            "allow",
        ),
        // A selection the decided path does not bound cannot be verified.
        (
            "Glob",
            json!({"pattern":"/etc/*","path":project("src")}),
            "deny",
        ),
        (
            "Glob",
            json!({"pattern":"../**","path":project("src")}),
            "deny",
        ),
        (
            "Grep",
            json!({"pattern":"Acme","path":project("src"),"glob":"{*.js,../*}"}),
            "deny",
        ),
    ] {
        let value = tool(root, "claude", name, &input);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], decision,
            "{name} {input}: {value}"
        );
    }
}

#[test]
fn sed_edits_and_script_writes_are_decided_by_where_they_write() {
    let root = assert_fs::TempDir::new().unwrap();
    let within = |action: &str| json!({"action":action,"all":{"within":{"root":"project"}}});
    let sed = |id: &str, targets: Value| {
        json!({"id":id,"decision":"allow","conditions":{"domain":"argv","executable":"sed","arguments":{"kind":"any"}},
            "requirements":{"targets":targets},"on_unmet":{"decision":"deny","message":"Use sed inside the project."}})
    };
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["argument-rules-v1","location-v1","requirements-v1","structured-rules-v1"],
        "rules":[sed("sed-edit", json!([within("read"), within("write")]))]})
    .to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root.path(), &bundle);
    let root = root.path();
    fs::write(root.join("notes.txt"), "acme\n").unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    let copy = outside.path().join("copy.txt");
    for source in [
        "sed -i '' 's/acme/example/' notes.txt",
        "sed -i.bak 's/acme/example/' notes.txt",
        "sed -n 's/acme/example/w copy.txt' notes.txt",
    ] {
        let value = call(root, "claude", source);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "allow",
            "{source}: {value}"
        );
    }
    for source in [
        format!("sed -n 's/acme/example/w {}' notes.txt", copy.display()),
        format!("sed -i '' 's/acme/example/' {}", copy.display()),
    ] {
        let value = call(root, "claude", &source);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{source}: {value}"
        );
    }
    // Running a command from a script stays unmodeled.
    let value = call(root, "claude", "sed -n '1e id' notes.txt");
    assert_ne!(value["hookSpecificOutput"]["permissionDecision"], "allow");
    assert!(
        !root.join("copy.txt").exists(),
        "hooks decide; they do not run sed"
    );
}

#[test]
fn protected_blocks_name_the_first_path_and_what_the_command_does_to_it() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    let redact =
        "sed -E 's/(\"[^\"]*(key|token)[^\"]*\"\\s*:\\s*\")[^\"]+/\\1<redacted>/Ig' .mcp.json";
    // A substitution printed to standard output reads the file: Claude asks.
    let value = call(root, "claude", redact);
    assert_eq!(
        value["hookSpecificOutput"]["permissionDecision"], "ask",
        "{value}"
    );
    assert!(reason(&value).contains("credentials"), "{value}");
    // Review state comes first in the command, so it is the path named.
    let value = call(
        root,
        "claude",
        &format!("cat .shield/.gitignore .shield/review/.gitignore; echo ---; {redact}"),
    );
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    let text = reason(&value);
    assert!(
        text.starts_with(
            "Shield: protected path. `.shield/review/.gitignore` is protected review state"
        ),
        "{text}"
    );
    // Each protected group says what the command does and what to do instead.
    for (source, expected) in [
        ("sed -i 's/a/b/' .mcp.json", "can disable agent protection"),
        // A file the script writes is checked like a named argument.
        (
            "sed -n 's/a/b/w .mcp.json' acme.txt",
            "can disable agent protection",
        ),
        // Shared Claude settings can disable hooks, so they are protected too.
        (
            "cat acme.txt > .claude/settings.json",
            "can disable agent protection",
        ),
        ("cat acme.txt > .shield/config.yml", "policy_propose"),
        ("cat acme.txt > .shield/.gitignore", "out of Git"),
        ("touch .shield", "holds protected review state"),
        // Removals blocked as a whole name no single path.
        ("rm -rf .shield", "Never read or change .shield/review"),
    ] {
        let value = call(root, "claude", source);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{source}: {value}"
        );
        let text = reason(&value);
        assert!(
            text.starts_with("Shield: protected path."),
            "{source}: {text}"
        );
        assert!(text.contains(expected), "{source}: {text}");
        assert!(!text.contains(". this"), "{source}: {text}");
    }
    // Other project files under .claude, such as commands, follow policy.
    let value = call(root, "claude", "cat acme.txt > .claude/commands/acme.md");
    assert!(
        !reason(&value).starts_with("Shield: protected path."),
        "{value}"
    );
}

#[test]
fn operator_path_access_opens_settings_and_policy_files_but_never_review_state() {
    use crate::review::path_access::{self, Change, PathAccess, Read};
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    let project = |name: &str| root.join(name).to_str().unwrap().to_owned();
    let protected = |value: &Value| reason(value).starts_with("Shield: protected path.");
    let open = PathAccess {
        agent_settings_read: Read::Allow,
        agent_settings_change: Change::Allow,
        shield_config_change: Change::Allow,
    };
    path_access::configure_fixture(root, open);
    for (name, input, decision) in [
        ("Read", json!({"file_path":project(".mcp.json")}), "allow"),
        (
            "Edit",
            json!({"file_path":project(".claude/settings.local.json"),"old_string":"a","new_string":"b"}),
            "allow",
        ),
        (
            "Write",
            json!({"file_path":project(".shield/rules.yml"),"content":"x"}),
            "allow",
        ),
        (
            "Write",
            json!({"file_path":project(".shield/.gitignore"),"content":"x"}),
            "deny",
        ),
        (
            "Write",
            json!({"file_path":project(".shield/review/state.json"),"content":"x"}),
            "deny",
        ),
        (
            "Read",
            json!({"file_path":project(".shield/review/state.json")}),
            "deny",
        ),
    ] {
        let value = tool(root, "claude", name, &input);
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], decision,
            "{name} {input}: {value}"
        );
        assert_eq!(protected(&value), decision == "deny", "{value}");
    }
    for (source, locked) in [
        ("cat .mcp.json", false),
        ("git add .mcp.json .codex/hooks.json", false),
        ("cat acme.txt > .shield/config.yml", false),
        ("cat acme.txt > .codex/config.toml", false),
        ("rm .codex/hooks.json", false),
        ("rm -rf .claude", false),
        ("rm -rf .shield", true),
        ("mv .shield/review acme", true),
        ("cat acme.txt > .shield/.gitignore", true),
        ("cat .shield/review/state.json", true),
        ("grep -r acme .shield", true),
    ] {
        let value = call(root, "claude", source);
        assert_eq!(protected(&value), locked, "{source}: {value}");
    }
    let value = call(root, "codex", "cat .mcp.json");
    assert!(!reason(&value).contains("credentials"), "{value}");
    // The operator CLI stays out of reach whatever the access.
    let value = call(root, "claude", "shield protection set");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    // Each group opens separately.
    path_access::configure_fixture(
        root,
        PathAccess {
            agent_settings_read: Read::Allow,
            ..PathAccess::default()
        },
    );
    for (source, locked) in [
        ("cat .mcp.json", false),
        ("cat acme.txt > .mcp.json", true),
        ("rm -rf .claude", true),
        ("cat acme.txt > .shield/config.yml", true),
    ] {
        let value = call(root, "claude", source);
        assert_eq!(protected(&value), locked, "{source}: {value}");
    }
}

#[test]
fn invalid_path_access_blocks_every_native_request() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    crate::review::path_access::store_fixture(root, json!({"agent_settings_change":"always"}));
    for agent in ["claude", "codex"] {
        let value = call(root, agent, "git status");
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{agent}: {value}"
        );
    }
}

#[test]
fn approved_asks_tell_claude_and_suggest_a_rule_once_routine() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let event = |agent: &str, name: &str, id: &str, source: &str| {
        let host = json!({"hook_event_name":name,"tool_name":"Bash","tool_use_id":id,
            "session_id":"acme-session","cwd":root,"tool_input":{"command":source},
            "tool_response":{"stdout":""}});
        translate_for(
            root,
            agent,
            find(agent).unwrap().event(name).unwrap(),
            &serde_json::to_vec(&host).unwrap(),
        )
    };
    let note = |value: &Value| {
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .map(str::to_owned)
    };
    // The count stops at the routine threshold, so later approvals still offer it.
    for call in 1..=4 {
        let id = format!("acme-ask-{call}");
        let asked = event("claude", "PreToolUse", &id, "git add acme.txt");
        assert_eq!(asked["hookSpecificOutput"]["permissionDecision"], "ask");
        let text = note(&event("claude", "PostToolUse", &id, "git add acme.txt"))
            .expect("approved Ask note");
        assert!(
            text.contains("does not prove individual approval"),
            "{text}"
        );
        assert!(
            text.contains("`git add acme.txt` matches no rule"),
            "{text}"
        );
        assert!(text.contains("will ask again"), "{text}");
        assert!(text.contains("suggestion_list"), "{text}");
        assert!(!text.contains("Approved at least"), "{text}");
    }
    // Allowed calls, and Codex, which cannot prompt, get no note.
    event("claude", "PreToolUse", "acme-allow", "acme read");
    assert_eq!(
        event("claude", "PostToolUse", "acme-allow", "acme read"),
        json!({})
    );
    event("codex", "PreToolUse", "acme-codex", "git add acme.txt");
    assert_eq!(
        event("codex", "PostToolUse", "acme-codex", "git add acme.txt"),
        json!({})
    );
}

#[test]
fn protected_blocks_name_the_path_that_tripped_them() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let value = call(root, "claude", "cat acme.txt .shield/review/state.json");
    assert!(
        reason(&value).contains("`.shield/review/state.json` is protected review state"),
        "{value}"
    );
}

#[test]
fn codex_patches_decide_every_changed_file_and_refuse_unknown_patch_text() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let outside = assert_fs::TempDir::new().unwrap();
    let outside = outside.path().join("acme.txt");
    let outside = outside.to_str().unwrap();
    let patch = |body: &str| json!({"command":format!("*** Begin Patch\n{body}\n*** End Patch\n")});
    let decision = |value: &Value| {
        value["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap_or("permitted")
            .to_owned()
    };
    for (body, expected) in [
        ("*** Add File: .gitignore\n+node_modules/", "permitted"),
        (
            "*** Update File: src/app.js\n@@\n-old\n+new\n*** Add File: README.md\n+# Acme",
            "permitted",
        ),
        // One file outside approved folders blocks the whole patch.
        (
            &format!("*** Add File: README.md\n+# Acme\n*** Add File: {outside}\n+x"),
            "deny",
        ),
        ("*** Update File: .shield/config.yml\n@@\n-a\n+b", "deny"),
        (
            &format!("*** Update File: src/app.js\n*** Move to: {outside}\n@@\n-a\n+b"),
            "deny",
        ),
        // Deletion has no allow rule here, and Codex cannot prompt.
        ("*** Delete File: README.md", "deny"),
    ] {
        let value = tool(root, "codex", "apply_patch", &patch(body));
        assert_eq!(decision(&value), expected, "{body}: {value}");
    }
    for input in [
        json!({"command":"*** Add File: x\n+y"}),
        json!({"command":"*** Begin Patch\n*** Rename File: a\n*** End Patch"}),
        json!({"command":"*** Begin Patch\n*** Move to: b\n*** End Patch"}),
        patch("*** Add File: README.md\n+x"),
    ] {
        let mut input = input;
        if input["command"].as_str().unwrap().contains("README.md") {
            input["workdir"] = json!("/");
        }
        let value = tool(root, "codex", "apply_patch", &input);
        assert_eq!(decision(&value), "deny", "{input}: {value}");
    }
    assert_eq!(
        tool(root, "codex", "update_plan", &json!({"plan":[]})),
        json!({})
    );
    assert!(!root.join(".gitignore").exists());
}

#[test]
fn guarded_project_reads_preserve_location_evidence() {
    let (root, _) = fixture();
    for agent in ["codex", "claude"] {
        crate::review::operator::integrations::configure_fixture(
            root.path(),
            crate::cli::public::IntegrationChoice::agent(agent),
            crate::review::integrations::AskFallback::Allow,
        );
        for present in [false, true] {
            if present {
                fs::write(root.path().join("AGENTS.md"), "Acme instructions").unwrap();
            }
            for source in [
                "cat AGENTS.md",
                "if [ -f AGENTS.md ]; then cat AGENTS.md; fi",
                "ls -la && rg --files --hidden -g '!.git' && if [ -f AGENTS.md ]; then cat AGENTS.md; fi",
            ] {
                let value = call(root.path(), agent, source);
                assert_ne!(
                    value["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{agent} present={present} {source}: {value}"
                );
            }
        }
        for source in [
            "if [ -f ../AGENTS.md ]; then cat ../AGENTS.md; fi",
            "cd /; if [ -f AGENTS.md ]; then cat AGENTS.md; fi",
        ] {
            let value = call(root.path(), agent, source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{agent} {source}: {value}"
            );
        }
        fs::remove_file(root.path().join("AGENTS.md")).unwrap();
    }
}

#[test]
fn unmet_requirements_ask_for_a_revised_request_not_a_rule() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    let input = json!({"url":"https://other.example.com/","prompt":"Summarize"});
    let value = tool(root, "claude", "WebFetch", &input);
    let text = reason(&value);
    assert!(
        text.starts_with("Shield: blocked in this form; a revised request can pass."),
        "{text}"
    );
    assert!(
        text.contains("Fetch only https://docs.example.com"),
        "{text}"
    );
    assert!(!text.contains("policy_propose"), "{text}");
    // A rule's plain Deny still points to proposals.
    let value = call(root, "claude", "git reset --hard");
    assert!(reason(&value).contains("policy_propose"), "{value}");
}

/// Submits the proposal a refusal offers through the real MCP tool.
fn propose_offered(context: &review::Context, text: &str) -> Value {
    let start = text
        .find("policy_propose with change ")
        .unwrap_or_else(|| panic!("no proposal offered: {text}"))
        + "policy_propose with change ".len();
    let end = start + text[start..].find("; add agent_reason").unwrap();
    let (change, entry) = text[start..end]
        .split_once(" and source_entry_id ")
        .unwrap_or((&text[start..end], ""));
    let mut arguments = json!({"change":serde_json::from_str::<Value>(change).unwrap()});
    if !entry.is_empty() {
        arguments["source_entry_id"] = json!(entry);
    }
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    mcp.agent = Some("codex".into());
    mcp.connection_id = Some("acme".into());
    let proposed = review::mcp::call(
        &mcp,
        &review::mcp::Connection::new("acme".into()),
        "policy_propose",
        &arguments,
        &|| Ok(()),
    );
    assert!(
        proposed["data"]["proposal_id"].is_string(),
        "{arguments} -> {proposed}"
    );
    arguments
}

#[test]
fn refusals_offer_command_drafting_guidance_or_non_command_proposals() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // Unmatched commands direct the agent to learn the grammar and draft a
    // reusable rule, with no complete command allowance to copy.
    let value = call(root, "codex", "npm install acme@5");
    let guidance = reason(&value);
    assert!(guidance.contains("suggestion_list"), "{guidance}");
    assert!(guidance.contains("eligible recurring"), "{guidance}");
    assert!(guidance.contains("source_entry_id"), "{guidance}");
    assert!(!guidance.contains("with change {"), "{guidance}");
    // A path outside the project: the folder it needs.
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::write(outside.path().join("notes.txt"), "acme").unwrap();
    let file = outside.path().join("notes.txt");
    let value = unscoped_shell(root, "codex", &format!("cat '{}'", file.display()));
    let text = reason(&value);
    assert!(text.contains("is outside the folders"), "{text}");
    let arguments = propose_offered(&context, text);
    assert_eq!(arguments["change"]["kind"], "access_path_add");
    assert_eq!(arguments["change"]["access"], "read");
    assert_eq!(
        arguments["change"]["path"],
        json!(outside.path().canonicalize().unwrap())
    );
    let value = without_working_files(
        root,
        "claude",
        &json!({"kind":"file","action":"write","path":outside.path().join("new/out.txt"),"recursive":false}),
    );
    let arguments = propose_offered(&context, reason(&value));
    assert_eq!(arguments["change"]["access"], "write");
    // Unmatched web requests: a search rule, or a rule for the fetched host.
    let bare = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: custom.json\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let web = review::mcp::initialize_fixture(bare.path(), &bundle);
    let project = Path::new(&web.project_root);
    let value = tool(
        project,
        "codex",
        "webrun",
        &json!({"search_query":[{"q":"acme"}]}),
    );
    let arguments = propose_offered(&web, reason(&value));
    assert_eq!(arguments["change"]["rule"]["id"], "allow-web-search");
    // A shell download that asks for its fetch offers the same host rule.
    let value = unscoped_shell(
        project,
        "codex",
        "curl -fsS https://registry.example.com/catalog.json",
    );
    let arguments = propose_offered(&web, reason(&value));
    assert_eq!(
        arguments["change"]["rule"]["conditions"]["url"],
        json!({"schemes":["https"],"hosts":["registry.example.com"]})
    );
    // An explicit deny rule stays a refusal without a ready-made override.
    let value = call(root, "codex", "git reset --hard");
    assert!(!reason(&value).contains("policy_propose with"), "{value}");
}

#[test]
fn a_fallback_allow_in_a_session_defers_rule_offers_to_its_suggestions() {
    let (root, context) = fixture();
    review::operator::integrations::configure_fixture(
        root.path(),
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Allow,
    );
    let project = Path::new(&context.project_root);
    // However often it repeats, the agent finishes its task and then offers
    // every rule at once from suggestion_list, instead of one inline offer.
    for _ in 1..=3 {
        let value = call(project, "codex", "npm test");
        let text = value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(text.contains("Ask fallback allows"), "{text}");
        assert!(
            text.contains("suggestion_list with session_id \"acme-session\""),
            "{text}"
        );
        assert!(!text.contains("policy_propose with"), "{text}");
    }
}

#[test]
fn a_command_a_registry_pack_covers_proposes_that_pack() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    review::observations::retain_release(&context, "main", "canonic/git@0.4.1");
    for source in ["git push", "git add acme.txt"] {
        let text = reason(&call(root, "codex", source)).to_owned();
        let start = text.find("with change ").unwrap() + "with change ".len();
        let end = start + text[start..].find(" and source_entry_id").unwrap();
        let change: Value = serde_json::from_str(&text[start..end]).unwrap();
        assert_eq!(
            change,
            json!({"kind":"pack_add","reference":"main/canonic/git"}),
            "{text}"
        );
        assert!(text.contains("pack_info"), "{text}");
    }
    // Other commands get drafting guidance instead of a command rule payload.
    let text = reason(&call(root, "codex", "npm test")).to_owned();
    assert!(text.contains("suggestion_list"), "{text}");
    assert!(!text.contains(r#""kind":"rule_add""#), "{text}");
}

#[test]
fn large_completions_are_read_and_oversized_pre_tool_calls_stay_blocked() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // A Shield MCP response larger than the old 64 KiB limit completes normally.
    let post = json!({"hook_event_name":"PostToolUse","tool_name":"mcp__shield__pack_info",
        "tool_use_id":"acme-info","session_id":"acme-session","cwd":root,
        "tool_input":{"reference":"main/canonic/git"},
        "tool_response":{"content":[{"type":"text","text":"x".repeat(200_000)}]}});
    let bytes = serde_json::to_vec(&post).unwrap();
    let (read, oversized) = read_input(&mut bytes.as_slice());
    assert!(oversized.is_none());
    assert_eq!(
        translate_for(root, "codex", Event::Completed, &read.unwrap()),
        json!({})
    );
    // Past the limit, only a completion that never names PreToolUse is let go.
    let padding = "x".repeat(MAX_HOOK_INPUT);
    let completion = format!(r#"{{"hook_event_name":"PostToolUse","tool_response":"{padding}"}}"#);
    let (_, oversized) = read_input(&mut completion.as_bytes());
    assert!(!oversized.unwrap().names_pre);
    // The event name may sit anywhere, even split across reads after the limit.
    for pre in [
        format!(r#"{{"hook_event_name":"PreToolUse","tool_input":"{padding}"}}"#),
        format!(r#"{{"tool_input":"{padding}","hook_event_name":"PreToolUse"}}"#),
        format!(
            r#"{{"tool_input":"{padding}{}","hook_event_name":"PreToolUse"}}"#,
            "y".repeat(65_530)
        ),
    ] {
        let (_, oversized) = read_input(&mut pre.as_bytes());
        assert!(oversized.unwrap().names_pre);
    }
}

#[test]
fn outside_paths_are_named_and_broad_folders_are_never_proposed() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // In a compound command, the refused command's outside path is named.
    let value = call(root, "codex", "ls -la; cat /acme-missing.md");
    let text = reason(&value);
    assert!(text.contains("`/acme-missing.md` is outside"), "{text}");
    // `/`, and folders holding the project, are too broad to offer.
    assert!(!text.contains("policy_propose with"), "{text}");
    assert!(text.contains("do not propose it"), "{text}");
    let value = unscoped_shell(root, "codex", "cat ../acme.txt");
    let text = reason(&value);
    assert!(text.contains("is outside"), "{text}");
    assert!(!text.contains("policy_propose with"), "{text}");
    // A redirection's target is named like a command's argument.
    let scratch = assert_fs::TempDir::new().unwrap();
    let pid = scratch.path().join("acme.pid");
    for agent in ["claude", "codex"] {
        for source in [
            format!("echo x > {}", pid.display()),
            format!("(acme serve & echo $! > {}; )", pid.display()),
        ] {
            let value = unscoped_shell(root, agent, &source);
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{agent}: {source}: {value}"
            );
            let text = reason(&value);
            assert!(
                text.contains("acme.pid` is outside"),
                "{agent}: {source}: {text}"
            );
        }
    }
}

#[test]
fn a_denial_of_words_shield_cannot_resolve_asks_for_literal_paths() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        let value = call(root, agent, "cat \"$ACME_TARGET\"");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        let text = reason(&value);
        assert!(text.starts_with("Shield: rewrite this request"), "{text}");
        assert!(text.contains("`cat \"$ACME_TARGET\"`"), "{text}");
        assert!(text.contains("Use literal arguments"), "{text}");
        assert!(!text.contains("access_path_add"), "{text}");
    }
}

#[test]
fn a_catch_all_denial_of_an_outside_read_names_the_path() {
    let (_root, context) = fixture();
    let root = Path::new(&context.project_root);
    // Loop words resolve, so each read is decided and the outside one is named.
    let value = call(
        root,
        "codex",
        "for p in /AGENTS.md /acme/AGENTS.md; do head \"$p\"; done",
    );
    let text = reason(&value);
    assert!(text.contains("`/AGENTS.md` is outside"), "{text}");
    assert!(text.contains("do not propose it"), "{text}");
    assert!(!text.contains("access_path_add"), "{text}");
}

#[test]
fn codex_prefix_covered_asks_block_even_under_the_allow_fallback() {
    use crate::{
        cli::public::IntegrationChoice,
        review::{integrations::AskFallback, operator::integrations},
    };
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["agent-outputs-v1","argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"logs","decision":"allow","agents":{"codex":{"prefixes":[["acme","logs"]]}},
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"options","prefix":["logs"],"flags":[],"values":[],"operands":false}}}]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root.path(), &bundle);
    integrations::configure_fixture(
        root.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Allow,
    );
    assert_permitted(&call(root.path(), "codex", "acme logs"), "codex");
    // Policy asks about an undeclared option; Codex would run it unsandboxed.
    let covered = call(root.path(), "codex", "acme logs --debug");
    assert_eq!(
        covered["hookSpecificOutput"]["permissionDecision"], "deny",
        "{covered}"
    );
    // Codex skips its sandbox when every part of a chain matches a prefix.
    for chain in [
        "acme logs && acme logs --debug",
        "acme logs --debug | acme logs",
    ] {
        let covered = call(root.path(), "codex", chain);
        assert_eq!(
            covered["hookSpecificOutput"]["permissionDecision"], "deny",
            "{chain}: {covered}"
        );
    }
    // An Ask no Shield prefix covers still follows the configured fallback:
    // an uncovered part, a wrapper or an embedded script keeps Codex's sandbox.
    for sandboxed in [
        "acme other",
        "acme logs --debug && acme other",
        "nice acme logs --debug",
        "bash -c 'acme logs --debug'",
    ] {
        assert_permitted(&call(root.path(), "codex", sandboxed), "codex");
    }
    // The agent is told why the allowing fallback did not apply.
    assert!(
        covered.to_string().contains("outside its sandbox"),
        "{covered}"
    );

    // A prefix rules file other than the one this policy writes may cover any
    // command, so every Ask blocks until it is repaired.
    let path = root.path().join(".codex/rules/shield.rules");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "prefix_rule(pattern = [\"acme\"], decision = \"allow\")\n",
    )
    .unwrap();
    let drifted = call(root.path(), "codex", "acme other");
    assert_eq!(
        drifted["hookSpecificOutput"]["permissionDecision"], "deny",
        "{drifted}"
    );
    let prefixes = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .codex_prefixes()
        .to_vec();
    let codex = crate::agents::find("codex").unwrap();
    std::fs::write(&path, codex.prefix_rules(&prefixes).unwrap()).unwrap();
    assert_permitted(&call(root.path(), "codex", "acme other"), "codex");
}
