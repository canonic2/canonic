//! Native observations of one project's current session tool-result directory.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn root(host: &Value, project: &Path, config: &Path) -> Option<PathBuf> {
    let session = super::text(&host["session_id"], 128)?;
    if !session
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let project = project.canonicalize().ok()?;
    let key: String = project
        .to_str()?
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let config = config.canonicalize().ok()?;
    let directory = config.join("projects").join(key);
    let transcript = Path::new(host["transcript_path"].as_str()?);
    if transcript != directory.join(format!("{session}.jsonl")) {
        return None;
    }
    let expected = directory.join(session).join("tool-results");
    let actual = expected.canonicalize().ok()?;
    // Directory symlinks cannot make an unrelated folder into session artifacts.
    (actual == expected && actual.is_dir()).then_some(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn artifact_reads_bind_project_session_config_and_real_directory() {
        let temp = assert_fs::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        let config = project.join("claude-config");
        let key: String = project
            .to_str()
            .unwrap()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let directory = config.join("projects").join(key);
        let artifacts = directory.join("acme-session/tool-results");
        std::fs::create_dir_all(&artifacts).unwrap();
        let mut host = json!({"session_id":"acme-session","transcript_path":directory.join("acme-session.jsonl")});
        assert_eq!(root(&host, &project, &config), Some(artifacts));
        host["session_id"] = json!("other-session");
        assert_eq!(root(&host, &project, &config), None);
        host["session_id"] = json!("../acme-session");
        assert_eq!(root(&host, &project, &config), None);
        host["session_id"] = json!("acme-session");
        host["transcript_path"] = json!(project.join("forged.jsonl"));
        assert_eq!(root(&host, &project, &config), None);
    }

    #[test]
    fn jq_artifact_reads_follow_policy_and_do_not_open_sibling_sessions_or_writes() {
        use crate::{configuration::Bundle, enforcement::protocol, review};
        let temp = assert_fs::TempDir::new().unwrap();
        let parent = temp.path().canonicalize().unwrap();
        let project = parent.join("project");
        std::fs::create_dir(&project).unwrap();
        let key: String = project
            .to_str()
            .unwrap()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let config = parent.join("claude");
        let directory = config.join("projects").join(key);
        let artifacts = directory.join("acme-session/tool-results");
        let sibling = directory.join("other-session/tool-results");
        for path in [&artifacts, &sibling] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("result.json"), "{}").unwrap();
        }
        std::fs::write(config.join("credentials.json"), "{}").unwrap();
        let rules = json!({"schema_version":1,"kind":"custom","capabilities":["access-paths-v1","argument-rules-v1","location-v1","requirements-v1","structured-rules-v1"],"rules":[
            {"id":"head","decision":"allow","conditions":{"domain":"argv","executable":"head","arguments":{"kind":"exact","values":["-20"]}}},
            {"id":"cut","decision":"allow","conditions":{"domain":"argv","executable":"cut","arguments":{"kind":"exact","values":["-c1-600"]}}},
            {"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}},
            {"id":"write","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"write"}}},"on_unmet":{"decision":"deny"}}
        ]}).to_string();
        let bundle = Bundle::capture(
            "schema_version: 1\npacks: []\nrules: rules.yml\n",
            &[],
            Some(&rules),
        )
        .unwrap();
        let context = review::mcp::initialize_fixture(&project, &bundle);
        let host = json!({"session_id":"acme-session","transcript_path":directory.join("acme-session.jsonl")});
        let scope = crate::location::WorkingFiles {
            read: vec![root(&host, &project, &config).unwrap()],
            write: vec![],
            home: None,
            protected: vec![config.clone()],
            http_environment: false,
            sole_operation: false,
        };
        let evaluate = |operation: Value, scoped: bool| {
            let input = json!({"protocol_version":1,"request_id":"acme","expected_project_id":context.project_id,"cwd":project,
                "operation":operation,"host":null,"approval_request_id":null});
            protocol::evaluate_native(
                &project,
                &serde_json::to_vec(&input).unwrap(),
                &json!({}),
                protocol::NativeContext {
                    integration: "claude",
                    approval: protocol::NativeApproval::Prompt,
                    context_complete: true,
                    working_files: scoped.then_some(&scope),
                },
                None,
                &|_| {},
            )
            .0
        };
        let command = |path: &Path| json!({"kind":"shell","source":format!("jq -c '.data.rules[] | select((tostring|test(\"npm|script\";\"i\"))) | {{id, decision, pack: (.pack // .source // null), conditions}}' '{}' | head -20 | cut -c1-600",path.display()),"dialect":"bash"});
        assert_eq!(
            evaluate(command(&artifacts.join("result.json")), true)["decision"],
            "allow"
        );
        assert_eq!(
            evaluate(command(&artifacts.join("result.json")), false)["decision"],
            "deny"
        );
        for path in [sibling.join("result.json"), config.join("credentials.json")] {
            assert_eq!(evaluate(command(&path), true)["decision"], "deny");
        }
        let write = json!({"kind":"file","action":"write","path":artifacts.join("result.json"),"recursive":false});
        assert_eq!(evaluate(write, true)["decision"], "deny");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                config.join("credentials.json"),
                artifacts.join("escape.json"),
            )
            .unwrap();
            assert_eq!(
                evaluate(command(&artifacts.join("escape.json")), true)["decision"],
                "deny"
            );
        }
        assert!(
            serde_json::from_value::<crate::policy::normalized::Context>(
                json!({"cwd":project,"environment":null,"working_files":{"read":scope.read}})
            )
            .is_err()
        );
    }
}
