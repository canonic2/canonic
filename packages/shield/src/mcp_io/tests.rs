use super::*;
use serde_json::json;
use std::fs;

fn configured(root: &Path) {
    fs::create_dir_all(root.join(".shield/state")).expect("fixture review directory");
    fs::write(
        root.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .expect("fixture config");
}

#[test]
fn action_request_kinds_sdk_uses_only_the_registered_claude_form_channel() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let input = ReviewContext {
        project: root.path().into(),
        agent: Some("claude-sdk".into()),
        session_id: Some("acme-session".into()),
        tool_use_id: None,
    };
    let before = review_context(&input).unwrap();
    assert!(before.agent.is_none() && !before.confirmed_mcp_forms);
    crate::review::operator::integrations::configure_fixture(
        root.path(),
        crate::review::integrations::IntegrationChoice::agent("claude"),
        crate::review::integrations::AskFallback::Block,
    );
    let unknown = review_context(&input).unwrap();
    assert!(unknown.agent.is_none() && !unknown.confirmed_mcp_forms);
    let native = review_context(&ReviewContext {
        agent: Some("claude".into()),
        ..input
    })
    .unwrap();
    assert_eq!(native.agent.as_deref(), Some("claude"));
    assert_eq!(native.session_id.as_deref(), Some("acme-session"));
}

#[test]
fn serve_project_agrees_with_hooks_in_a_fresh_worktree() {
    let root = assert_fs::TempDir::new().unwrap();
    let parent = root.path().join("acme");
    configured(&parent);
    let parent = parent.canonicalize().unwrap();
    // A fresh worktree has the committed config but no ignored review state.
    let worktree = parent.join(".gate/workspaces/feature");
    fs::create_dir_all(worktree.join(".shield")).unwrap();
    fs::write(
        worktree.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .unwrap();
    let worktree = worktree.canonicalize().unwrap();
    // The inherited server entry names the parent; the server still serves
    // the worktree, from its launch folder or the agent's session folder.
    let served = serve_project(Some(&parent), &worktree, None).unwrap();
    assert_eq!(served, worktree);
    assert_eq!(
        serve_project(Some(&parent), &parent, Some(&worktree)).unwrap(),
        worktree
    );
    assert_eq!(session_project(&worktree).as_ref(), Some(&served));
    // The worktree's files are its policy: it opens with no enrollment step.
    let opened = review_context(&ReviewContext {
        project: served.clone(),
        agent: None,
        session_id: None,
        tool_use_id: None,
    })
    .unwrap();
    assert_eq!(Path::new(&opened.project_root), served);
    // Outside every project the explicit selection stands.
    assert_eq!(
        serve_project(Some(&parent), root.path(), None).unwrap(),
        parent
    );
}

#[test]
fn other_projects_cannot_replace_an_explicit_selection() {
    let root = assert_fs::TempDir::new().unwrap();
    let selected = root.path().join("selected");
    let current = root.path().join("current");
    configured(&selected);
    configured(&current);
    let selected = selected.canonicalize().unwrap();
    let current = current.canonicalize().unwrap();
    fs::write(
        current.join(".shield/state/state.json"),
        json!({"schema_version":1,"project_root":current}).to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
    let nested = current.join("nested");
    configured(&nested);
    assert_eq!(project_path(None, &nested).unwrap(), nested);
    // The nested config has no copied state naming selected; do not cross it.
    assert_eq!(project_path(Some(&selected), &nested).unwrap(), selected);
    assert_eq!(project_path(Some(&current), &nested).unwrap(), current);
    let pending = current.join("pending");
    fs::create_dir_all(pending.join(".shield/state")).unwrap();
    assert_eq!(project_path(None, &pending).unwrap(), pending);
    assert_eq!(project_path(Some(&current), &pending).unwrap(), current);
    // An unconfigured working directory leaves an explicit selection intact.
    assert_eq!(
        project_path(Some(&selected), root.path()).unwrap(),
        selected
    );
}

#[cfg(unix)]
#[test]
fn discovery_canonicalizes_cwd_without_changing_an_explicit_selection() {
    let root = assert_fs::TempDir::new().unwrap();
    let selected = root.path().join("selected");
    let current = root.path().join("current");
    configured(&selected);
    configured(&current);
    let selected = selected.canonicalize().unwrap();
    let current = current.canonicalize().unwrap();
    let alias = root.path().join("current-alias");
    std::os::unix::fs::symlink(&current, &alias).unwrap();
    fs::write(
        current.join(".shield/state/state.json"),
        json!({
            "schema_version":1,"project_root":current,
            "operator":{"bootstrap":{"launch_roots":[selected]}}
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&selected), &alias).unwrap(), selected);
    assert_eq!(project_path(None, &alias).unwrap(), current);
    fs::remove_file(current.join(".shield/config.yml")).unwrap();
    fs::remove_file(current.join(".shield/state/state.json")).unwrap();
    std::os::unix::fs::symlink("missing-config", current.join(".shield/config.yml")).unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
}
