//! Native observations of one project's Claude folders: the current session's
//! output directory and the project's auto-memory directory.
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Claude's folder name for a project under `<config>/projects/`.
fn project_key(project: &Path) -> Option<String> {
    let project = project.canonicalize().ok()?;
    Some(
        project
            .to_str()?
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect(),
    )
}

/// The folder Claude keys auto memory by: the Git repository holding the
/// project, shared by its subfolders and linked worktrees, or else the project.
/// A layout this cannot resolve, such as a bare repository's worktree, has none.
fn repository_root(project: &Path) -> Option<PathBuf> {
    let project = project.canonicalize().ok()?;
    for folder in project.ancestors() {
        let entry = folder.join(".git");
        let metadata = match std::fs::symlink_metadata(&entry) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if metadata.is_dir() {
            return Some(folder.to_path_buf());
        }
        if !metadata.is_file() || metadata.len() > 4096 {
            return None;
        }
        let text = std::fs::read_to_string(&entry).ok()?;
        let git = folder.join(text.strip_prefix("gitdir:")?.trim());
        // A linked worktree's metadata names the main checkout's `.git`.
        let Ok(common) = std::fs::read_to_string(git.join("commondir")) else {
            return Some(folder.to_path_buf());
        };
        let common = git.join(common.trim()).canonicalize().ok()?;
        return (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf))?;
    }
    Some(project)
}

/// This project's auto-memory folder, which Claude creates on first use. Its
/// existing components must be real folders below the configuration folder.
/// Distinct paths that encode to one key share it, as they do in Claude.
pub(super) fn memory(project: &Path, config: &Path) -> Vec<PathBuf> {
    repository_root(project)
        .and_then(|root| project_key(&root))
        .map_or_else(Vec::new, |key| {
            crate::agents::working_files::skill_directories([(
                config.to_path_buf(),
                Path::new("projects").join(key).join("memory"),
            )])
        })
}

/// The current session's output folder (`tool-results`, `subagents`,
/// `workflows`), read-only. The transcript beside it is not included.
pub(super) fn root(host: &Value, project: &Path, config: &Path) -> Option<PathBuf> {
    let session = super::text(&host["session_id"], 128)?;
    if !session
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let config = config.canonicalize().ok()?;
    let directory = config.join("projects").join(project_key(project)?);
    let transcript = Path::new(host["transcript_path"].as_str()?);
    if transcript != directory.join(format!("{session}.jsonl")) {
        return None;
    }
    let expected = directory.join(session);
    let actual = expected.canonicalize().ok()?;
    // Directory symlinks cannot make an unrelated folder into session output.
    (actual == expected && actual.is_dir()).then_some(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn memory_follows_the_repository_across_subfolders_and_linked_worktrees() {
        let temp = assert_fs::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let config = root.join("claude");
        let main = root.join("acme");
        let linked = root.join("acme-feature");
        let plain = root.join("acme-plain");
        let bare = root.join("acme-bare.git");
        let bare_linked = root.join("acme-bare-feature");
        for path in [
            config.clone(),
            main.join(".git/worktrees/feature"),
            main.join("packages/web"),
            linked.clone(),
            plain.clone(),
            bare.join("worktrees/feature"),
            bare_linked.clone(),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        let gitdir = |worktree: &Path, metadata: &Path, common: &str| {
            std::fs::write(
                worktree.join(".git"),
                format!("gitdir: {}\n", metadata.display()),
            )
            .unwrap();
            std::fs::write(metadata.join("commondir"), common).unwrap();
        };
        gitdir(&linked, &main.join(".git/worktrees/feature"), "../..\n");
        gitdir(&bare_linked, &bare.join("worktrees/feature"), "../..\n");
        let key = |path: &Path| project_key(path).unwrap();
        let folder = |path: &Path| config.join("projects").join(key(path)).join("memory");
        for (project, owner) in [
            (&main, &main),
            (&main.join("packages/web"), &main),
            (&linked, &main),
            (&plain, &plain),
        ] {
            assert_eq!(memory(project, &config), vec![folder(owner)], "{project:?}");
        }
        assert_eq!(memory(&bare_linked, &config), Vec::<PathBuf>::new());
        // Claude's encoding maps these to one folder; Shield follows the host.
        let underscore = root.join("acme_plain");
        std::fs::create_dir(&underscore).unwrap();
        assert_eq!(memory(&underscore, &config), memory(&plain, &config));
    }

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
        assert_eq!(
            root(&host, &project, &config).as_deref(),
            artifacts.parent()
        );
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
        let workflow = directory.join("acme-session/subagents/workflows/wf_acme");
        for path in [&artifacts, &sibling, &workflow] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("result.json"), "{}").unwrap();
        }
        std::fs::write(config.join("credentials.json"), "{}").unwrap();
        std::fs::write(directory.join("acme-session.jsonl"), "{}").unwrap();
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
            skills: vec![],
            home: None,
            protected: vec![config.clone()],
            http_environment: false,
            node_options: false,
            sole_operation: false,
            filesystem_unknown: false,
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
                    event: crate::agents::Event::Before,
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
        // Tool results and subagent workflow journals are both session output.
        for path in [artifacts.join("result.json"), workflow.join("result.json")] {
            assert_eq!(evaluate(command(&path), true)["decision"], "allow");
            assert_eq!(evaluate(command(&path), false)["decision"], "deny");
            let write = json!({"kind":"file","action":"write","path":path,"recursive":false});
            assert_eq!(evaluate(write, true)["decision"], "deny");
        }
        for path in [
            sibling.join("result.json"),
            config.join("credentials.json"),
            directory.join("acme-session.jsonl"),
        ] {
            assert_eq!(evaluate(command(&path), true)["decision"], "deny");
        }
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
            serde_json::from_value::<crate::policy::resolution::normalized::Context>(
                json!({"cwd":project,"environment":null,"working_files":{"read":scope.read}})
            )
            .is_err()
        );
    }
}
