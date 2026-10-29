//! Adapter and runtime coverage for temporary and session resource observations.
use super::*;
use crate::enforcement::protocol::{self, NativeApproval, NativeContext};
use crate::location::WorkingFiles;

fn scoped_project(root: &Path) -> review::Context {
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","location-v1","access-paths-v1","requirements-v1"],
        "rules":[
            {"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"write","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"write"}}},"on_unmet":{"decision":"deny"}},
            {"id":"remove","decision":"allow","conditions":{"domain":"file","action":"remove"},"requirements":{"targets":{"action":"remove","all":{"within":{"root":"write"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"cat","decision":"allow","conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"printf","decision":"allow","conditions":{"domain":"argv","executable":"printf","arguments":{"kind":"any"}}},
            {"id":"explicit-deny","decision":"deny","conditions":{"domain":"argv","executable":"acme-denied","arguments":{"kind":"any"}}}
        ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    review::mcp::initialize_fixture(root, &bundle)
}

#[test]
fn working_files_default_temporary_access_reaches_both_native_adapters() {
    let environment = assert_fs::TempDir::new().unwrap();
    let root = environment.path().canonicalize().unwrap();
    let home = root.join("home");
    let scratch = root.join("scratch");
    let claude = home.join("claude-config");
    let codex = home.join("codex-config");
    for path in [&home, &scratch, &claude, &codex] {
        fs::create_dir(path).unwrap();
    }
    // Do not mutate process-wide environment while other Rust tests run.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agents::hook::tests::working_files::default_temporary_access_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", &home)
        .env("CLAUDE_CONFIG_DIR", &claude)
        .env("CODEX_HOME", &codex)
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
#[ignore = "run by the parent test with an isolated native environment"]
fn default_temporary_access_fixture() {
    let root = assert_fs::TempDir::new().unwrap();
    let context = scoped_project(root.path());
    let root = Path::new(&context.project_root);
    let temp = assert_fs::TempDir::new().unwrap();
    let file = temp.path().join("arbitrary-existing.output");
    fs::write(&file, "acme").unwrap();
    let state = fs::read(root.join(".shield/review/state.json")).unwrap();
    for agent in ["claude", "codex"] {
        for source in [
            format!("cat '{}'", file.display()),
            format!(
                "printf acme > '{}'",
                temp.path().join("new-unowned-name").display()
            ),
        ] {
            let value = call(root, agent, &source);
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
        let protected = temp.path().join(".shield/review/state.json");
        let value = call(
            root,
            agent,
            &format!("printf acme > '{}'", protected.display()),
        );
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], "deny",
            "{value}"
        );
    }
    assert_eq!(
        fs::read(root.join(".shield/review/state.json")).unwrap(),
        state
    );
    assert!(!temp.path().join("new-unowned-name").exists());
}

#[test]
fn working_files_resource_scope_covers_writes_but_not_read_only_or_foreign_paths() {
    let fixture = assert_fs::TempDir::new().unwrap();
    let parent = fixture.path().canonicalize().unwrap();
    let project = parent.join("project");
    let scratch = parent.join("scratch");
    let output = parent.join("session-output");
    let foreign = parent.join("foreign");
    for path in [&project, &scratch, &output, &foreign] {
        fs::create_dir(path).unwrap();
        fs::write(path.join("file"), "acme").unwrap();
    }
    let context = scoped_project(&project);
    let scope = WorkingFiles {
        read: vec![output.clone()],
        write: vec![scratch.clone()],
        home: None,
        protected: vec![],
        http_environment: false,
        sole_operation: false,
    };
    let evaluate = |operation: Value, scoped: bool| {
        protocol::evaluate_native(&project,
            &serde_json::to_vec(&json!({"protocol_version":1,"request_id":"working-files-acme","expected_project_id":context.project_id,"cwd":project,"operation":operation,"host":null,"approval_request_id":null})).unwrap(),
            &json!({}), NativeContext { integration:"claude", approval:NativeApproval::Prompt, context_complete:true, working_files:scoped.then_some(&scope) }, None, &|_| {}).0
    };
    let file = |action: &str, path: &Path| json!({"kind":"file","action":action,"path":path,"recursive":false});
    for action in ["read", "write", "remove"] {
        assert_eq!(
            evaluate(file(action, &scratch.join("file")), true)["decision"],
            "allow"
        );
        assert_eq!(
            evaluate(file(action, &scratch.join("file")), false)["decision"],
            "deny"
        );
        assert_eq!(
            evaluate(file(action, &foreign.join("file")), true)["decision"],
            "deny"
        );
    }
    assert_eq!(
        evaluate(file("write", &scratch.join("new/created.txt")), true)["decision"],
        "allow"
    );
    assert_eq!(
        evaluate(file("read", &output.join("file")), true)["decision"],
        "allow"
    );
    assert_eq!(
        evaluate(file("write", &output.join("file")), true)["decision"],
        "deny"
    );
    assert_eq!(evaluate(file("remove", &scratch), true)["decision"], "deny");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&foreign, scratch.join("escape")).unwrap();
        assert_eq!(
            evaluate(file("read", &scratch.join("escape/file")), true)["decision"],
            "deny"
        );
        assert_eq!(
            evaluate(file("write", &scratch.join("escape/new.txt")), true)["decision"],
            "deny"
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
            protected: vec![config.clone()],
            http_environment: false,
            home: None,
            sole_operation: false,
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
