//! Adapter and runtime coverage for temporary and session resource observations.
use super::*;
use crate::enforcement::protocol::{self, NativeApproval, NativeContext};
use crate::location::WorkingFiles;
use std::path::PathBuf;

#[cfg(unix)]
fn native_shell_at(context: &review::Context, cwd: &Path, agent: &str, command: &str) -> Value {
    let input = json!({"protocol_version":1,"request_id":"external-workdir-acme",
        "expected_project_id":context.project_id,"cwd":cwd,
        "operation":{"kind":"shell","source":command,"dialect":"posix"},
        "host":null,"approval_request_id":null});
    protocol::evaluate_native(
        Path::new(&context.project_root),
        &serde_json::to_vec(&input).unwrap(),
        &json!({}),
        NativeContext {
            integration: agent,
            approval: NativeApproval::Prompt,
            context_complete: true,
            working_files: None,
        },
        None,
        &|_| {},
    )
    .0
}

#[cfg(unix)]
#[test]
fn monorepo_cd_uses_normal_lookup_and_preserves_native_boundaries() {
    let (_fixture, context) = fixture();
    let root = Path::new(&context.project_root);
    fs::create_dir_all(root.join("packages/ui")).unwrap();
    fs::write(root.join("packages/ui/note.txt"), "Acme project note").unwrap();
    for adapter in crate::agents::ADAPTERS {
        let agent = adapter.spec().id;
        for source in [
            "cd packages/ui && cat note.txt",
            "cd ./packages/ui && cat note.txt",
        ] {
            let reply = shell_tool(root, agent, source);
            let decision = denial_decision(&reply);
            assert_ne!(decision, "deny", "{agent}: {source}: {reply}");
            assert_ne!(decision, "ask", "{agent}: {source}: {reply}");
        }
        // Directory handling must preserve explicit denies and source uncertainty.
        for source in [
            "cd packages/ui && git reset --hard",
            "CDPATH=/unverified/search-root; cd packages/ui && cat note.txt",
            "cd packages/ui && cat ../../../outside.txt",
        ] {
            let reply = shell_tool(root, agent, source);
            assert_eq!(
                denial_decision(&reply),
                "deny",
                "{agent}: {source}: {reply}"
            );
        }
    }
}

fn scoped_project(root: &Path) -> review::Context {
    scoped_project_with_config(root, "schema_version: 1\npacks: []\nrules: rules.yml\n")
}

fn scoped_project_with_config(root: &Path, config: &str) -> review::Context {
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","location-v1","access-paths-v1","requirements-v1"],
        "rules":[
            {"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"write","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"write"}}},"on_unmet":{"decision":"deny"}},
            {"id":"remove","decision":"allow","conditions":{"domain":"file","action":"remove"},"requirements":{"targets":{"action":"remove","all":{"within":{"root":"write"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"cat","decision":"allow","conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"rg","decision":"allow","conditions":{"domain":"argv","executable":"rg","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"printf","decision":"allow","conditions":{"domain":"argv","executable":"printf","arguments":{"kind":"any"}}},
            {"id":"project-cwd","decision":"allow","conditions":{"domain":"argv","executable":"acme-project-only","arguments":{"kind":"any"}},"requirements":{"cwd":{"within":{"root":"project"}}},"on_unmet":{"decision":"deny"}},
            {"id":"git-output","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"prefix","values":[{"one_of":["diff","show"]}]}},"requirements":{"targets":{"action":"write","all":{"within":{"root":"write"}}}},"on_unmet":{"decision":"ask"}},
            {"id":"explicit-deny","decision":"deny","conditions":{"domain":"argv","executable":"acme-denied","arguments":{"kind":"any"}}}
        ]}).to_string();
    let bundle = Bundle::capture(config, &[], Some(&rules)).unwrap();
    review::mcp::initialize_fixture(root, &bundle)
}

#[test]
fn folder_recovery_skips_existing_grants_and_names_the_unapproved_operand() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let parent = fixture.path().canonicalize().unwrap();
    let project = parent.join("project");
    let approved = parent.join("approved");
    let outside = parent.join("outside");
    for folder in [&project, &approved, &outside] {
        fs::create_dir(folder).unwrap();
        fs::write(folder.join("note.txt"), "Acme note").unwrap();
    }
    let rule = crate::policy::rules::document(&[
        json!({"id":"project-cat","decision":"allow",
            "conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"any"}},
            "requirements":{"targets":{"action":"read","all":{"within":{"root":"project"}}}},
            "on_unmet":{"decision":"deny","message":"Use approved paths.",
                "alternatives":["Propose folder access with access_path_add."]}}),
        json!({"id":"approved-head","decision":"allow",
            "conditions":{"domain":"argv","executable":"head","arguments":{"kind":"any"}},
            "requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},
            "on_unmet":{"decision":"deny"}}),
    ])
    .unwrap()
    .to_string();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml",
        "access_paths":{"write":[approved]}})
    .to_string();
    let bundle = Bundle::capture(&config, &[], Some(&rule)).unwrap();
    let context = review::mcp::initialize_fixture(&project, &bundle);
    let approve_path = approved.join("note.txt");
    let outside_path = outside.join("note.txt");
    let command = format!("cat '{}'", approve_path.display());
    let value = unscoped_shell(&project, "codex", &command);
    assert_eq!(denial_decision(&value), "deny", "{value}");
    let message = reason(&value);
    assert!(message.contains("project-cat"), "{message}");
    assert!(message.contains("rule's location requirement"), "{message}");
    assert!(!message.contains("access_path_add"), "{message}");
    assert!(
        !value.to_string().contains("outside the approved folders"),
        "{value}"
    );

    let both = format!(
        "head '{}' '{}'",
        approve_path.display(),
        outside_path.display()
    );
    let value = unscoped_shell(&project, "codex", &both);
    assert_eq!(denial_decision(&value), "deny", "{value}");
    let arguments = propose_offered(&context, reason(&value));
    assert_eq!(arguments["change"]["path"], json!(outside), "{arguments}");
    assert_eq!(arguments["change"]["access"], "read", "{arguments}");
}

#[test]
fn routine_patches_work_with_mixed_folder_and_file_grants() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let parent = fixture.path().canonicalize().unwrap();
    let root = parent.join("project");
    let checkout = parent.join("checkout");
    for folder in [&root, &checkout] {
        fs::create_dir(folder).unwrap();
    }
    let metadata = parent.join("release.json");
    fs::write(&metadata, "{}\n").unwrap();
    fs::write(root.join("ROADMAP.md"), "Acme roadmap\n").unwrap();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml",
        "access_paths":{"write":[checkout,metadata]}})
    .to_string();
    let rule = crate::policy::rules::document(&[json!({"id":"file-write","decision":"allow",
        "conditions":{"domain":"file","action":"write"},
        "requirements":{"targets":{"action":"write","all":{"within":{"root":"write"},
            "exclude":[{"root":"write","path":".git"},{"root":"write","path":".shield/cache"}],
            "match":{"regex":".+"}}}},"on_unmet":{"decision":"deny"}})])
    .unwrap()
    .to_string();
    let bundle = Bundle::capture(&config, &[], Some(&rule)).unwrap();
    review::mcp::initialize_fixture(&root, &bundle);
    let evaluate = |body: &str| {
        tool(
            &root,
            "codex",
            "apply_patch",
            &json!({
                "command": format!("*** Begin Patch\n{body}\n*** End Patch\n")
            }),
        )
    };
    let create = format!(
        "*** Add File: {}\n+Acme fixture",
        checkout.join("scripts/pack.mjs").display()
    );
    let update = format!(
        "*** Update File: {}\n@@\n-{{}}\n+{{\"acme\":true}}",
        metadata.display()
    );
    assert_permitted(&evaluate(&format!("{create}\n{update}")), "codex");
    assert_permitted(
        &evaluate(&format!("*** Add File: large.txt\n+{}", "x".repeat(13_000))),
        "codex",
    );
    assert_permitted(
        &evaluate("*** Update File: ROADMAP.md\n@@\n-Acme roadmap\n+Acme complete"),
        "codex",
    );
    for path in [
        checkout.join(".git/config"),
        root.join(".shield/config.yml"),
    ] {
        let value = evaluate(&format!(
            "{create}\n*** Add File: {}\n+Acme blocked",
            path.display()
        ));
        assert_eq!(denial_decision(&value), "deny", "{path:?}: {value}");
    }
}

#[cfg(unix)]
#[test]
fn patch_refusals_name_the_blocking_target_and_failed_verification() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file"), "Acme ordinary file").unwrap();
    // A dangling grant fails root verification, even for a valid project path.
    let grant = root.join("grant");
    std::os::unix::fs::symlink(root.join("absent"), &grant).unwrap();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml",
        "access_paths":{"write":[grant]}})
    .to_string();
    scoped_project_with_config(&project, &config);
    let patch = |body: &str| {
        tool(
            &project,
            "codex",
            "apply_patch",
            &json!({
                "command":format!("*** Begin Patch\n{body}\n*** End Patch")
            }),
        )
    };
    let target = patch("*** Add File: file/new.txt\n+Acme content");
    let grants = patch("*** Add File: new.txt\n+Acme content");
    for (value, path, cause) in [
        (target, "file/new.txt", "parent components are directories"),
        (grants, "new.txt", "configured access paths"),
    ] {
        assert_eq!(denial_decision(&value), "deny", "{value}");
        assert!(reason(&value).contains(&format!("`{path}`")), "{value}");
        assert!(reason(&value).contains(cause), "{value}");
        assert!(
            value["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains(cause),
            "{value}"
        );
        assert!(!value.to_string().contains("Acme content"), "{value}");
    }
}

#[cfg(unix)]
#[test]
fn approved_external_workdirs_keep_launch_policy_and_path_permissions() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let parent = fixture.path().canonicalize().unwrap();
    let root = parent.join("project");
    let read = parent.join("read");
    let write = parent.join("write");
    let foreign = parent.join("foreign");
    for path in [&root, &read, &write, &foreign] {
        fs::create_dir(path).unwrap();
        fs::write(path.join("note.txt"), "Acme note").unwrap();
    }
    let file = parent.join("single-file");
    fs::write(&file, "Acme file grant").unwrap();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml",
        "access_paths":{"read":[read,file],"write":[write]}})
    .to_string();
    let context = scoped_project_with_config(&root, &config);
    // A command's workdir must not select this other enrollment's policy.
    let empty = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    review::mcp::initialize_fixture(&write, &empty);
    let escape = read.join("escape");
    std::os::unix::fs::symlink(&foreign, &escape).unwrap();
    let missing = write.join("missing");

    let decide = |agent: &str, cwd: &Path, command: &str| {
        let mut host = native_host(
            &root,
            agent,
            "PreToolUse",
            "Bash",
            &json!({"command":command}),
        );
        if agent == "codex" {
            host["tool_input"]["workdir"] = json!(cwd);
        } else {
            host["cwd"] = json!(cwd);
        }
        translate(&root, agent, &serde_json::to_vec(&host).unwrap())
    };
    for agent in ["codex", "claude"] {
        assert_permitted(&decide(agent, &write, "cat note.txt"), agent);
        for command in [
            "acme-denied",
            "acme-project-only",
            "cat ../foreign/note.txt",
            "printf x > .shield/config.yml",
        ] {
            let reply = native_shell_at(&context, &write, agent, command);
            assert_eq!(reply["decision"], "deny", "{agent} {command}: {reply}");
        }
        assert_eq!(
            native_shell_at(&context, &write, agent, "printf x > note.txt")["decision"],
            "allow"
        );
        // The command rule allows printf without a cwd or write predicate.
        assert_eq!(
            native_shell_at(&context, &read, agent, "printf x")["code"],
            "invalid-context"
        );
        assert_eq!(
            native_shell_at(&context, &write, agent, "printf x")["decision"],
            "allow"
        );
        for cwd in [&read, &foreign, &escape, &missing, &file] {
            let reply = decide(agent, cwd, "cat note.txt");
            assert_eq!(denial_decision(&reply), "deny", "{agent} {cwd:?}: {reply}");
            assert!(reason(&reply).contains("working directory"), "{reply}");
        }
    }
    let approval = |cwd: &Path| {
        let reply = permission(
            &root,
            "Bash",
            json!({"command":"cat note.txt","workdir":cwd}),
        );
        reply["hookSpecificOutput"]["decision"]["behavior"].clone()
    };
    assert_eq!(approval(&write), "allow");

    fs::write(
        root.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    for agent in ["codex", "claude"] {
        let reply = decide(agent, &write, "cat note.txt");
        assert_eq!(denial_decision(&reply), "deny", "{agent}: {reply}");
        assert!(reason(&reply).contains("working directory"), "{reply}");
    }
    assert_eq!(approval(&write), "deny");
}

#[test]
fn working_files_default_temporary_and_skill_access_reaches_all_native_adapters() {
    let environment = assert_fs::TempDir::new().unwrap();
    let root = environment.path().canonicalize().unwrap();
    // Each child owns fresh home/configuration state, including its symlinks.
    for overrides in [true, false] {
        let home = root.join(if overrides {
            "overridden-home"
        } else {
            "default-home"
        });
        let scratch = if cfg!(windows) {
            home.join("AppData/Local/Temp")
        } else {
            root.join("scratch")
        };
        let claude = home.join("claude-config");
        let codex = home.join("codex-config");
        for path in [
            &home,
            &scratch,
            &claude,
            &codex,
            &home.join(".codex"),
            &home.join(".claude"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        // Do not mutate process-wide environment while other Rust tests run.
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "agents::hook::tests::working_files::default_temporary_access_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("TMPDIR", &scratch)
            .env("TMP", &scratch)
            .env("TEMP", &scratch);
        if overrides {
            command
                .env("CLAUDE_CONFIG_DIR", &claude)
                .env("CODEX_HOME", &codex)
                .env("GEMINI_CLI_HOME", scratch.join("gemini-home"));
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "overrides={overrides}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

#[test]
#[ignore = "run by the parent test with an isolated native environment"]
fn default_temporary_access_fixture() {
    let root = assert_fs::TempDir::new().unwrap();
    let context = scoped_project(root.path());
    let root = Path::new(&context.project_root);
    let temp = assert_fs::TempDir::new().unwrap();
    let file = temp.path().join("arbitrary-existing.output");
    let new_file = temp.path().join("new-unowned-name");
    fs::write(&file, "acme").unwrap();
    let state = fs::read(root.join(".shield/state/state.json")).unwrap();
    for agent in ["claude", "codex"] {
        let sources = [
            format!("cat '{}'", file.display()),
            format!(
                "printf acme > '{}'",
                temp.path().join("new-unowned-name").display()
            ),
        ];
        let input = |write: bool, path: &Path| {
            let (tool, input) = if write {
                (
                    "apply_patch",
                    json!({"command":format!("*** Begin Patch\n*** Add File: {}\n+Acme\n*** End Patch",path.display())}),
                )
            } else {
                ("view_image", json!({"path":path}))
            };
            let host = json!({"hook_event_name":"PreToolUse","tool_name":tool,"tool_use_id":"acme-native-file","tool_input":input,"cwd":root,"session_id":"acme-session"});
            translate_for(
                root,
                agent,
                Event::Before,
                &serde_json::to_vec(&host).unwrap(),
            )
        };
        for (write, source) in sources.iter().enumerate() {
            let value = if cfg!(windows) && agent == "codex" {
                input(write == 1, if write == 0 { &file } else { &new_file })
            } else {
                call(root, agent, source)
            };
            assert_ne!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{agent}: {value}"
            );
            assert_ne!(
                value["hookSpecificOutput"]["permissionDecision"], "ask",
                "{agent}: {value}"
            );
        }
        assert_eq!(
            call(root, agent, "acme-denied /tmp/acme")["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        for state in [".shield/state/state.json", ".shield/review/state.json"] {
            let protected = temp.path().join(state);
            let value = if cfg!(windows) && agent == "codex" {
                input(true, &protected)
            } else {
                call(
                    root,
                    agent,
                    &format!("printf acme > '{}'", protected.display()),
                )
            };
            assert_eq!(
                value["hookSpecificOutput"]["permissionDecision"], "deny",
                "{value}"
            );
        }
    }
    // Refusals store requests for in-session approval; nothing else changes.
    let without_requests = |bytes: &[u8]| {
        let mut state: Value = serde_json::from_slice(bytes).unwrap();
        let fields = state.as_object_mut().unwrap();
        fields.remove("hook_requests");
        fields.remove("sequence");
        state
    };
    assert_eq!(
        without_requests(&fs::read(root.join(".shield/state/state.json")).unwrap()),
        without_requests(&state)
    );
    assert!(!temp.path().join("new-unowned-name").exists());
    check_skill_access(root);
    check_claude_memory_access(root);
    check_codex_plugin_and_memory_access(root);
    check_google_skill_access(root);
    check_google_session_access(root);
}

fn check_google_session_access(project: &Path) {
    let session = "12345678-acme";
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let gemini = crate::agents::working_files::configuration("GEMINI_CLI_HOME", "")
        .unwrap()
        .join(".gemini");
    let temporary = gemini.join("tmp/acme");
    let agy = home.join(".gemini/antigravity-cli/brain").join(session);
    let transcript = temporary.join("chats/session-2026-11-05T00-00-12345678.jsonl");
    let agy_transcript = agy.join(".system_generated/logs/transcript_full.jsonl");
    for path in [
        temporary.join(session),
        temporary.join("chats"),
        temporary.join("other-session"),
        agy.join(".system_generated/logs"),
        agy.parent().unwrap().join("other-session"),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(temporary.join(".project_root"), project.to_str().unwrap()).unwrap();
    for path in [&transcript, &agy_transcript] {
        fs::write(path, "Acme transcript").unwrap();
    }
    for (agent, own, sibling) in [
        (
            "gemini",
            temporary.join(session),
            temporary.join("other-session"),
        ),
        (
            "agy",
            agy.clone(),
            agy.parent().unwrap().join("other-session"),
        ),
    ] {
        let file = own.join("output.txt");
        let foreign = sibling.join("output.txt");
        fs::write(&file, "Acme output").unwrap();
        fs::write(&foreign, "Other output").unwrap();
        let read = |path: &Path, write: bool, spoof: bool| {
            let host = if agent == "gemini" {
                json!({"hook_event_name":"BeforeTool","session_id":if spoof {"87654321-other"} else {session},"cwd":project,"transcript_path":transcript,"tool_name":if write {"write_file"} else {"read_file"},"tool_input":{"file_path":path,"content":"Acme"}})
            } else {
                json!({"conversationId":if spoof {"87654321-other"} else {session},"workspacePaths":[project],"stepIdx":2,"artifactDirectoryPath":agy,"transcriptPath":agy_transcript,"toolCall":{"name":if write {"write_file"} else {"read_file"},"args":{"Path":path,"Content":"Acme"}}})
            };
            if !spoof {
                let scope = find(agent).unwrap().working_files(&host, project);
                assert!(
                    scope.read.contains(&own),
                    "{agent}: missing session scope: {scope:?}"
                );
            }
            translate_for(
                project,
                agent,
                Event::Before,
                &serde_json::to_vec(&host).unwrap(),
            )
        };
        let allowed = if agent == "agy" {
            "auto_approve"
        } else {
            "allow"
        };
        let value = read(&file, false, false);
        assert_eq!(value["decision"], allowed, "{agent}: {value}");
        assert_ne!(read(&foreign, false, false)["decision"], allowed, "{agent}");
        assert_eq!(read(&file, true, false)["decision"], "deny", "{agent}");
        assert_ne!(read(&file, false, true)["decision"], allowed, "{agent}");
        #[cfg(unix)]
        {
            let link = own.join("foreign-link");
            std::os::unix::fs::symlink(&foreign, &link).unwrap();
            assert_ne!(read(&link, false, false)["decision"], allowed, "{agent}");
        }
    }
}

fn check_google_skill_access(project: &Path) {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let gemini_home = crate::agents::working_files::configuration("GEMINI_CLI_HOME", "").unwrap();
    let gemini = gemini_home.join(".gemini");
    let agy = home.join(".gemini");
    for config in [&gemini, &agy] {
        fs::create_dir_all(config).unwrap();
        fs::write(config.join("credentials.json"), "Acme fixture").unwrap();
    }
    let candidates = [
        ("gemini", gemini.join("skills")),
        ("gemini", gemini_home.join(".agents/skills")),
        ("gemini", project.join(".gemini/skills")),
        ("agy", agy.join("config/skills")),
        ("agy", agy.join("antigravity-cli/skills")),
        ("agy", agy.join("skills")),
        ("agy", project.join(".agents/skills")),
    ];
    for (agent, library) in candidates {
        let allowed = if agent == "agy" {
            "auto_approve"
        } else {
            "allow"
        };
        fs::create_dir_all(library.join("acme")).unwrap();
        let skill = library.join("acme/SKILL.md");
        fs::write(&skill, "Acme skill fixture").unwrap();
        let native = |path: &Path, write: bool| {
            let host = if agent == "gemini" {
                json!({"hook_event_name":"BeforeTool","session_id":"acme-session","cwd":project,
                    "tool_name":if write {"write_file"} else {"read_file"},"tool_input":{"file_path":path,"content":"Acme"}})
            } else {
                json!({"conversationId":"acme-session","workspacePaths":[project],"stepIdx":2,
                    "toolCall":{"name":if write {"write_to_file"} else {"read_file"},"args":if write {json!({"TargetFile":path,"CodeContent":"Acme"})} else {json!({"Path":path})}}})
            };
            translate_for(
                project,
                agent,
                Event::Before,
                &serde_json::to_vec(&host).unwrap(),
            )
        };
        let read = native(&skill, false);
        assert_eq!(read["decision"], allowed, "{agent} {library:?}: {read}");
        if !cfg!(windows) {
            for source in [
                format!("cat '{}'", skill.display()),
                format!("rg --files '{}'", library.display()),
            ] {
                let host = if agent == "gemini" {
                    json!({"hook_event_name":"BeforeTool","session_id":"acme-session","cwd":project,"tool_name":"run_shell_command","tool_input":{"command":source,"dir_path":project}})
                } else {
                    json!({"conversationId":"acme-session","workspacePaths":[project],"stepIdx":2,"toolCall":{"name":"run_command","args":{"CommandLine":source}}})
                };
                let value = translate_for(
                    project,
                    agent,
                    Event::Before,
                    &serde_json::to_vec(&host).unwrap(),
                );
                assert_eq!(value["decision"], allowed, "{agent} {library:?}: {value}");
            }
        }
        let write = native(&skill, true);
        assert_eq!(write["decision"], allowed, "{agent} {library:?}: {write}");
        let config = if agent == "gemini" { &gemini } else { &agy };
        assert_eq!(
            native(&config.join("credentials.json"), false)["decision"],
            if config.starts_with(std::env::temp_dir()) {
                "ask"
            } else {
                "deny"
            }
        );
        let foreign = crate::agents::working_files::configuration("CLAUDE_CONFIG_DIR", ".claude")
            .unwrap()
            .join("skills/acme/SKILL.md");
        assert_eq!(native(&foreign, false)["decision"], "deny");
    }
}

/// Runs only in the child with isolated HOME and agent configuration overrides.
fn check_skill_access(project: &Path) {
    let codex = crate::agents::working_files::configuration("CODEX_HOME", ".codex").unwrap();
    let claude =
        crate::agents::working_files::configuration("CLAUDE_CONFIG_DIR", ".claude").unwrap();
    let shared = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".agents/skills");
    for config in [&codex, &claude] {
        fs::write(config.join("credentials.json"), "Acme fixture").unwrap();
    }
    for (agent, config) in [("codex", &codex), ("claude", &claude)] {
        let path = config.join("skills/new/SKILL.md");
        let value = if agent == "codex" {
            tool(
                project,
                agent,
                "apply_patch",
                &json!({"command":format!("*** Begin Patch\n*** Add File: {}\n+Acme workflow\n*** End Patch",path.display())}),
            )
        } else {
            tool(
                project,
                agent,
                "Write",
                &json!({"file_path":path,"content":"Acme workflow"}),
            )
        };
        assert_permitted(&value, agent);
    }
    let roots = [
        codex.join("skills"),
        claude.join("skills"),
        shared.clone(),
        project.join(".codex/skills"),
        project.join(".claude/skills"),
        project.join(".agents/skills"),
    ];
    for (agent, config) in [("codex", &codex), ("claude", &claude)] {
        let scope = find(agent).unwrap().working_files(&Value::Null, project);
        assert!(!scope.read.contains(&config.join("skills")));
    }
    for root in &roots {
        fs::create_dir_all(root.join("acme")).unwrap();
        fs::write(root.join("acme/SKILL.md"), "Acme workflow").unwrap();
    }
    for (agent, own, foreign, local) in [
        ("codex", &codex, &claude, ".codex/skills"),
        ("claude", &claude, &codex, ".claude/skills"),
    ] {
        check_agent_skills(project, agent, own, foreign, local, &shared);
    }
}

/// Claude keeps this project's memory beside its sessions; it may be created
/// on first use, while other projects' memory and the configuration stay shut.
fn check_claude_memory_access(project: &Path) {
    let claude =
        crate::agents::working_files::configuration("CLAUDE_CONFIG_DIR", ".claude").unwrap();
    let key: String = project
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let memory = claude.join("projects").join(&key).join("memory");
    let other = claude.join("projects/-acme-other-project/memory");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("MEMORY.md"), "Other memory").unwrap();
    let write = |file: &Path| {
        tool(
            project,
            "claude",
            "Write",
            &json!({"file_path":file,"content":"Acme memory"}),
        )
    };
    let read = |file: &Path| tool(project, "claude", "Read", &json!({"file_path":file}));
    assert!(
        find("claude")
            .unwrap()
            .working_file_categories()
            .contains(&"memory")
    );
    // Neither the project folder nor the memory folder exists yet.
    assert!(!memory.parent().unwrap().exists());
    assert_permitted(&write(&memory.join("acme-note.md")), "claude");
    fs::create_dir_all(&memory).unwrap();
    fs::write(memory.join("MEMORY.md"), "Acme memory").unwrap();
    assert_permitted(&read(&memory.join("MEMORY.md")), "claude");
    assert_permitted(&write(&memory.join("MEMORY.md")), "claude");
    if !cfg!(windows) {
        let value = call(
            project,
            "claude",
            &format!("cat '{}'", memory.join("MEMORY.md").display()),
        );
        assert_permitted(&value, "claude");
    }
    for path in [
        other.join("MEMORY.md"),
        claude
            .join("projects")
            .join(&key)
            .join("acme-session.jsonl"),
        claude.join("credentials.json"),
    ] {
        let value = write(&path);
        assert!(!reason(&value).is_empty(), "{path:?}: {value}");
    }
    let value = read(&other.join("MEMORY.md"));
    assert!(!reason(&value).is_empty(), "{value}");
    #[cfg(unix)]
    {
        // A memory link cannot redirect the scope to another project's notes.
        fs::remove_dir_all(&memory).unwrap();
        std::os::unix::fs::symlink(&other, &memory).unwrap();
        let value = write(&memory.join("MEMORY.md"));
        assert!(!reason(&value).is_empty(), "{value}");
        fs::remove_file(&memory).unwrap();
    }
}

/// Codex reads its installed plugins' skills without changing them; the plugin
/// staging folder and its machine-wide memory store, shared by every project
/// and written by Codex itself, stay shut.
fn check_codex_plugin_and_memory_access(project: &Path) {
    let codex = crate::agents::working_files::configuration("CODEX_HOME", ".codex").unwrap();
    let categories = find("codex").unwrap().working_file_categories();
    assert!(categories.contains(&"runtime_assets") && !categories.contains(&"memory"));
    let plugin = codex.join("plugins/cache/acme-market/acme/1.0.0/skills/acme");
    let staging = codex.join("plugins/.remote-plugin-install-staging/acme");
    for folder in [&plugin, &staging] {
        fs::create_dir_all(folder).unwrap();
        fs::write(folder.join("SKILL.md"), "Acme plugin skill").unwrap();
    }
    let skill = plugin.join("SKILL.md");
    assert_permitted(&read_skill(project, "codex", &skill), "codex");
    if !cfg!(windows) {
        let value = call(project, "codex", &format!("cat '{}'", skill.display()));
        assert_permitted(&value, "codex");
    }
    for path in [skill.clone(), plugin.join("new.md")] {
        let value = write_skill(project, "codex", &path);
        assert!(!reason(&value).is_empty(), "{path:?}: {value}");
    }
    let value = read_skill(project, "codex", &staging.join("SKILL.md"));
    assert!(!reason(&value).is_empty(), "{value}");
    for store in ["memories", "memories_v2"] {
        let note = codex.join(store).join("MEMORY.md");
        fs::create_dir_all(note.parent().unwrap()).unwrap();
        fs::write(&note, "Other project's memory").unwrap();
        for value in [
            read_skill(project, "codex", &note),
            write_skill(project, "codex", &note),
        ] {
            assert!(!reason(&value).is_empty(), "{store}: {value}");
        }
    }
    // Claude gets neither Codex scope.
    let value = read_skill(project, "claude", &skill);
    assert!(!reason(&value).is_empty(), "{value}");
}

fn read_skill(project: &Path, agent: &str, file: &Path) -> Value {
    let (name, arguments) = if agent == "codex" {
        ("view_image", json!({"path":file}))
    } else {
        ("Read", json!({"file_path":file}))
    };
    tool(project, agent, name, &arguments)
}

fn write_skill(project: &Path, agent: &str, file: &Path) -> Value {
    if agent == "codex" {
        tool(
            project,
            agent,
            "apply_patch",
            &json!({"command":format!("*** Begin Patch\n*** Add File: {}\n+Acme workflow\n*** End Patch",file.display())}),
        )
    } else {
        tool(
            project,
            agent,
            "Write",
            &json!({"file_path":file,"content":"Acme workflow"}),
        )
    }
}

fn check_agent_skills(
    project: &Path,
    agent: &str,
    own: &Path,
    foreign: &Path,
    local: &str,
    shared: &Path,
) {
    assert!(
        find(agent)
            .unwrap()
            .working_file_categories()
            .contains(&"skills")
    );
    let read = |file: &Path| read_skill(project, agent, file);
    let write = |file: &Path| write_skill(project, agent, file);
    for folder in [own.join("skills"), project.join(local)] {
        let file = folder.join("acme/SKILL.md");
        assert_permitted(&read(&file), agent);
        assert_permitted(&write(&file), agent);
        assert_permitted(&write(&folder.join("new-skill/SKILL.md")), agent);
        for source in [
            format!("cat '{}'", file.display()),
            format!("rg --files --hidden '{}'", folder.display()),
            format!("rg -n Acme '{}'", folder.display()),
        ] {
            let value = call(project, agent, &source);
            if cfg!(windows) && agent == "codex" {
                assert_unsupported_shell(&value);
            } else {
                assert_permitted(&value, agent);
            }
        }
    }
    // Each adapter adds only its own external roots. Existing project and
    // explicit operator permissions continue to apply independently.
    for path in [
        foreign.join("skills/acme/SKILL.md"),
        own.join("credentials.json"),
    ] {
        let value = read(&path);
        assert!(!reason(&value).is_empty(), "{agent}: {value}");
    }
    let value = read(&shared.join("acme/SKILL.md"));
    if agent == "codex" {
        assert_permitted(&value, agent);
    } else {
        assert!(!reason(&value).is_empty(), "{value}");
    }
    for source in [
        format!("cat '{}'", shared.join("acme/SKILL.md").display()),
        format!("rg --files '{}'", shared.display()),
    ] {
        let value = call(project, agent, &source);
        if cfg!(windows) && agent == "codex" {
            assert_unsupported_shell(&value);
        } else if agent == "codex" {
            assert_permitted(&value, agent);
        } else {
            assert!(!reason(&value).is_empty(), "{value}");
        }
    }
    if !cfg!(windows) {
        let source = format!(
            "printf Acme > '{}'",
            own.join("skills/acme/SKILL.md").display()
        );
        assert_permitted(&call(project, agent, &source), agent);
    }
    if agent == "codex" {
        assert_permitted(&write(&shared.join("acme/SKILL.md")), agent);
    } else {
        assert_ne!(reason(&write(&shared.join("acme/SKILL.md"))), "");
    }
    assert!(
        !reason(&write(&own.join("credentials.json"))).is_empty(),
        "skill write access must not open configuration siblings"
    );
    for source in [
        format!(
            "rg --files '{}' '{}'",
            own.join("skills").display(),
            foreign.join("skills").display()
        ),
        format!("rg --files '{}'", own.display()),
    ] {
        assert!(
            !reason(&call(project, agent, &source)).is_empty(),
            "{agent}: {source}"
        );
    }
    #[cfg(unix)]
    {
        let escape = own.join("skills/acme/escape.md");
        std::os::unix::fs::symlink(own.join("credentials.json"), &escape).unwrap();
        assert_ne!(
            reason(&call(
                project,
                agent,
                &format!("cat '{}'", escape.display())
            )),
            ""
        );
        assert!(
            !reason(&write(&escape)).is_empty(),
            "skill symlink must not open credentials"
        );
    }
}

struct ScopedWorkingFiles {
    _directory: assert_fs::TempDir,
    project: PathBuf,
    scratch: PathBuf,
    output: PathBuf,
    foreign: PathBuf,
    project_id: String,
    scope: WorkingFiles,
}

impl ScopedWorkingFiles {
    fn new() -> Self {
        let directory = assert_fs::TempDir::new().unwrap();
        let parent = directory.path().canonicalize().unwrap();
        let project = parent.join("project");
        let scratch = parent.join("scratch");
        let output = parent.join("session-output");
        let foreign = parent.join("foreign");
        for path in [&project, &scratch, &output, &foreign] {
            fs::create_dir(path).unwrap();
            fs::write(path.join("file"), "acme").unwrap();
        }
        let project_id = scoped_project(&project).project_id;
        let scope = WorkingFiles {
            read: vec![output.clone()],
            write: vec![scratch.clone()],
            ..WorkingFiles::default()
        };
        Self {
            _directory: directory,
            project,
            scratch,
            output,
            foreign,
            project_id,
            scope,
        }
    }

    fn evaluate(&self, operation: &Value, scoped: bool) -> Value {
        let request = json!({
            "protocol_version": 1,
            "request_id": "working-files-acme",
            "expected_project_id": self.project_id,
            "cwd": self.project,
            "operation": operation,
            "host": null,
            "approval_request_id": null,
        });
        protocol::evaluate_native(
            &self.project,
            &serde_json::to_vec(&request).unwrap(),
            &json!({}),
            NativeContext {
                integration: "claude",
                approval: NativeApproval::Prompt,
                context_complete: true,
                working_files: scoped.then_some(&self.scope),
            },
            None,
            &|_| {},
        )
        .0
    }
}

#[test]
fn working_files_resource_scope_covers_writes_but_not_read_only_or_foreign_paths() {
    let fixture = ScopedWorkingFiles::new();
    let ScopedWorkingFiles {
        scratch,
        output,
        foreign,
        ..
    } = &fixture;
    let file = |action: &str, path: &Path| json!({"kind":"file","action":action,"path":path,"recursive":false});
    for action in ["read", "write", "remove"] {
        assert_eq!(
            fixture.evaluate(&file(action, &scratch.join("file")), true)["decision"],
            "allow"
        );
        assert_eq!(
            fixture.evaluate(&file(action, &scratch.join("file")), false)["decision"],
            "deny"
        );
        assert_eq!(
            fixture.evaluate(&file(action, &foreign.join("file")), true)["decision"],
            "deny"
        );
    }
    assert_eq!(
        fixture.evaluate(&file("write", &scratch.join("new/created.txt")), true)["decision"],
        "allow"
    );
    assert_eq!(
        fixture.evaluate(&file("read", &output.join("file")), true)["decision"],
        "allow"
    );
    assert_eq!(
        fixture.evaluate(&file("write", &output.join("file")), true)["decision"],
        "deny"
    );
    assert_eq!(
        fixture.evaluate(&file("remove", scratch), true)["decision"],
        "deny"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(foreign, scratch.join("escape")).unwrap();
        assert_eq!(
            fixture.evaluate(&file("read", &scratch.join("escape/file")), true)["decision"],
            "deny"
        );
        assert_eq!(
            fixture.evaluate(&file("write", &scratch.join("escape/new.txt")), true)["decision"],
            "deny"
        );
    }
}

#[test]
fn git_output_requires_writable_destinations_and_preserves_protected_paths() {
    let fixture = ScopedWorkingFiles::new();
    let ScopedWorkingFiles {
        project,
        scratch,
        foreign,
        ..
    } = &fixture;
    let git = |args: &str, scoped: bool| {
        fixture.evaluate(
            &json!({"kind":"shell","dialect":"bash","source":format!("git {args}")}),
            scoped,
        )
    };
    let patch = scratch.join("review.diff");
    let diff = format!("diff base head --output='{}'", patch.display());
    // Ordinary diff inspection needs no settings-read grant, while its output
    // destination must still be writable under the active path policy.
    let inspection = git(&diff, true);
    assert_eq!(inspection["decision"], "allow", "{inspection}");
    assert_eq!(git(&diff, false)["decision"], "ask");
    review::path_access::configure_fixture(
        project,
        review::path_access::PathAccess {
            agent_settings_read: review::path_access::Read::Allow,
            ..Default::default()
        },
    );
    for command in [diff, format!("show HEAD --output '{}'", patch.display())] {
        let scoped = git(&command, true);
        assert_eq!(scoped["decision"], "allow", "{command}: {scoped}");
        let unscoped = git(&command, false);
        assert_eq!(unscoped["decision"], "ask", "{command}: {unscoped}");
    }
    let foreign_file = foreign.join("file");
    let comparison = git(
        &format!(
            "diff --output='{}' file '{}'",
            patch.display(),
            foreign_file.display()
        ),
        true,
    );
    assert_eq!(comparison["decision"], "ask", "{comparison}");
    let line_prefix = git(
        &format!(
            "diff HEAD --line-prefix '--output=../label.diff' --output='{}'",
            patch.display()
        ),
        true,
    );
    assert_eq!(line_prefix["decision"], "allow", "{line_prefix}");
    assert_eq!(
        git(
            &format!(
                "diff --output='{}' --output='{}'",
                foreign_file.display(),
                patch.display()
            ),
            true,
        )["decision"],
        "ask"
    );
    // The native path guard blocks writes to Shield configuration.
    let protected_write = git("show HEAD --output=.shield/config.yml", true);
    assert_eq!(protected_write["decision"], "deny", "{protected_write}");
    assert_eq!(
        protected_write["code"], "protected-access",
        "{protected_write}"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(foreign, scratch.join("escape")).unwrap();
        assert_eq!(
            git(
                &format!(
                    "diff --output='{}'",
                    scratch.join("escape/new.txt").display()
                ),
                true,
            )["decision"],
            "ask"
        );
    }
}

#[test]
fn working_files_relocated_configuration_remains_protected_inside_temporary_roots() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let parent = fixture.path().canonicalize().unwrap();
    let project = parent.join("project");
    let scratch = parent.join("temporary");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&scratch).unwrap();
    let context = scoped_project(&project);
    for agent in ["claude", "codex"] {
        let config = scratch.join(format!("acme-{agent}")).join("configuration");
        let outputs = config.join("session/tool-results");
        fs::create_dir_all(&outputs).unwrap();
        let scope = WorkingFiles {
            write: vec![scratch.clone()],
            read: vec![outputs.clone()],
            skills: vec![],
            protected: vec![config.clone()],
            http_environment: false,
            home: None,
            sole_operation: false,
            filesystem_unknown: false,
        };
        let evaluate = |operation: Value| {
            protocol::evaluate_native(
                &project,
                &serde_json::to_vec(
                    &json!({"protocol_version":1,"request_id":"relocated-config-acme",
                    "expected_project_id":context.project_id,"cwd":project,"operation":operation,
                    "host":null,"approval_request_id":null}),
                )
                .unwrap(),
                &json!({}),
                NativeContext {
                    integration: agent,
                    approval: NativeApproval::Prompt,
                    context_complete: true,
                    working_files: Some(&scope),
                },
                None,
                &|_| {},
            )
            .0
        };
        let write =
            |path: &Path| json!({"kind":"file","action":"write","path":path,"recursive":false});
        for path in [
            config.join("settings.json"),
            config.join("config.toml"),
            outputs.join("result.json"),
        ] {
            let value = evaluate(write(&path));
            assert_eq!(value["decision"], "deny", "{agent}: {value}");
            assert_eq!(value["code"], "protected-access", "{agent}: {value}");
        }
        assert_eq!(
            evaluate(write(&scratch.join("ordinary.txt")))["decision"],
            "allow"
        );
        let read =
            |path: &Path| json!({"kind":"file","action":"read","path":path,"recursive":false});
        fs::write(outputs.join("result.json"), "acme").unwrap();
        assert_eq!(
            evaluate(read(&outputs.join("result.json")))["decision"],
            "allow"
        );
        fs::write(config.join("credentials.json"), "acme").unwrap();
        assert_eq!(
            evaluate(read(&config.join("credentials.json")))["decision"],
            "ask"
        );
        let remove = json!({"kind":"file","action":"remove","path":config,"recursive":true});
        assert_eq!(evaluate(remove)["decision"], "deny");
        let remove_parent =
            json!({"kind":"file","action":"remove","path":config.parent(),"recursive":true});
        assert_eq!(evaluate(remove_parent)["decision"], "deny");
        let shell = json!({"kind":"shell","source":format!("printf acme > '{}'",config.join("settings.json").display()),"dialect":"bash"});
        assert_eq!(evaluate(shell)["decision"], "deny");
        #[cfg(unix)]
        {
            let alias = scratch.join(format!("alias-{agent}"));
            std::os::unix::fs::symlink(&config, &alias).unwrap();
            assert_eq!(
                evaluate(write(&alias.join("settings.json")))["decision"],
                "deny"
            );
        }
    }
}
