//! Read-only calls preserve pending setup; mutating recovery retains human control.
use super::*;

fn state(project: &Project) -> Value {
    serde_json::from_slice(
        &std::fs::read(project.root.path().join(".shield/state/state.json"))
            .expect("checkpoint bytes"),
    )
    .expect("checkpoint document")
}

fn edit(project: &Project, conflict: bool) -> Value {
    let path = project.root.path().join(".mcp.json");
    let mut file: Value = serde_json::from_slice(&std::fs::read(&path).expect("shared MCP config"))
        .expect("MCP document");
    file["mcpServers"]["acme-unrelated"] = json!({"command":"acme-helper","args":["read"]});
    if conflict {
        file["mcpServers"]["shield"]["command"] = json!("acme-manual-shield");
    }
    std::fs::write(path, file.to_string()).expect("unrelated host change");
    file
}

fn data(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}
fn answer(form: &Value, action: &str) -> Value {
    json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}})
}

fn prepared() -> (Transport, Project) {
    let (server, project) = fixture(Some("claude"));
    let transport = Transport::start(server);
    for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    crate::review::mcp::interrupted_setup_fixture(&project.review, "claude");
    (transport, project)
}

#[test]
fn interrupted_setup_read_only_calls_preserve_files_and_pending_publication() {
    for conflict in [false, true] {
        let (transport, project) = prepared();
        let edited = edit(&project, conflict);
        let checkpoint = project.root.path().join(".shield/state/state.json");
        let before = std::fs::read(&checkpoint).unwrap();
        let pending = state(&project)["operator"]["integration_pending"].clone();
        assert!(pending.is_object());
        let files: std::collections::BTreeMap<_, _> = pending["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .chain([".mcp.json", ".claude/settings.local.json"])
            .map(|name| {
                let path = project.root.path().join(name);
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => panic!("fixture publication file: {error}"),
                };
                (path, bytes)
            })
            .collect();
        let catalog = catalog(Some("claude")).unwrap();
        for (tool, args) in [
            ("status", json!({})),
            ("doctor", json!({})),
            ("docs_list", json!({})),
            ("docs_search", json!({"query":"policy","limit":1})),
            ("docs_read", json!({"path":"policy-rules.md","limit":1})),
            ("pack_list", json!({})),
            (
                "policy_evaluate",
                json!({
                    "input":{"kind":"argv","executable":"acme","arguments":["read"],"original_input":{}},
                    "context":{"cwd":project.review.project_root,"environment":{}}
                }),
            ),
        ] {
            assert!(catalog.get(tool).unwrap().read_only(), "{tool}");
            let response = transport.call(json!(2), tool, &args);
            assert!(data(&response)["error"].is_null(), "{tool}: {response}");
            assert!(
                std::fs::read(&checkpoint).unwrap() == before,
                "{tool} changed the pending checkpoint"
            );
            for (path, bytes) in &files {
                match bytes {
                    Some(bytes) => assert_eq!(
                        std::fs::read(path).unwrap(),
                        *bytes,
                        "{tool}: {}",
                        path.display()
                    ),
                    None => assert!(!path.exists(), "{tool} published {}", path.display()),
                }
            }
        }
        if !conflict {
            let resumed = transport.call(json!(3), "mcp_setup", &json!({}));
            assert!(data(&resumed)["error"].is_null(), "{resumed}");
            assert!(
                resumed["method"].is_null(),
                "confirmed setup must not ask again"
            );
            assert!(state(&project)["operator"]["integration_pending"].is_null());
            let actual: Value = serde_json::from_slice(
                &std::fs::read(project.root.path().join(".mcp.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, edited);
            assert!(
                project
                    .root
                    .path()
                    .join(".claude/settings.local.json")
                    .is_file()
            );
        }
        transport.close().1.unwrap();
    }
}

#[test]
fn interrupted_setup_conflict_review_decline_and_cancel_stay_in_session() {
    let (transport, project) = prepared();
    let edited = edit(&project, true);
    let status = transport.call(json!(2), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(
        data(&status)["data"]["repair_route"],
        "mcp_setup",
        "{status}"
    );
    let before = state(&project);
    let preview = transport.call(json!(3), "mcp_setup_preview", &json!({"pending":"review"}));
    assert!(data(&preview)["error"].is_null(), "{preview}");
    assert!(
        !preview.to_string().contains("acme-manual-shield"),
        "public preview must not return protected file bodies"
    );
    assert_eq!(state(&project), before);
    let review = transport.call(json!(4), "mcp_setup", &json!({"pending":"review"}));
    assert_eq!(review["method"], "elicitation/create", "{review}");
    let declined = transport.request(&answer(&review, "decline"));
    assert!(!data(&declined)["error"].is_null(), "{declined}");
    assert_eq!(state(&project), before);
    let cancel = transport.call(json!(5), "mcp_setup", &json!({"pending":"cancel"}));
    assert_eq!(cancel["method"], "elicitation/create", "{cancel}");
    let cancelled = transport.request(&answer(&cancel, "accept"));
    assert!(data(&cancelled)["error"].is_null(), "{cancelled}");
    assert!(state(&project)["operator"]["integration_pending"].is_null());
    let actual: Value =
        serde_json::from_slice(&std::fs::read(project.root.path().join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(
        actual, edited,
        "state-only cancellation preserves every manual file"
    );
    transport.close().1.unwrap();
    let context = crate::mcp_io::review_context(&crate::cli::ReviewContext {
        project: project.root.path().into(),
        agent: Some("claude".into()),
        session_id: None,
        tool_use_id: None,
    })
    .unwrap();
    assert!(context.confirmed_mcp_forms);
    let transport = Transport::start(Server::for_review(context).unwrap());
    for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let setup = transport.call(
        json!(6),
        "mcp_setup",
        &json!({"agents":["claude"],"repair":true}),
    );
    assert_eq!(setup["method"], "elicitation/create", "{setup}");
    let installed = transport.request(&answer(&setup, "accept"));
    assert!(data(&installed)["error"].is_null(), "{installed}");
    transport.close().1.unwrap();
}

#[test]
fn interrupted_setup_review_acceptance_and_changed_during_confirmation_preserve_exact_scope() {
    let (transport, project) = prepared();
    edit(&project, true);
    let review = transport.call(json!(2), "mcp_setup", &json!({"pending":"review"}));
    assert_eq!(review["method"], "elicitation/create", "{review}");
    let mut changed = edit(&project, true);
    changed["mcpServers"]["shield"]["command"] = json!("acme-newer-manual-shield");
    std::fs::write(project.root.path().join(".mcp.json"), changed.to_string()).unwrap();
    let stale = transport.request(&answer(&review, "accept"));
    assert!(
        data(&stale)["error"].is_object() || data(&stale)["data"]["setup_error"].is_string(),
        "{stale}"
    );
    assert!(!state(&project)["operator"]["integration_pending"].is_null());
    let fresh = transport.call(json!(3), "mcp_setup", &json!({"pending":"review"}));
    assert_eq!(fresh["method"], "elicitation/create", "{fresh}");
    let applied = transport.request(&answer(&fresh, "accept"));
    assert!(data(&applied)["error"].is_null(), "{applied}");
    assert!(state(&project)["operator"]["integration_pending"].is_null());
    let actual: Value =
        serde_json::from_slice(&std::fs::read(project.root.path().join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(
        actual["mcpServers"]["acme-unrelated"],
        changed["mcpServers"]["acme-unrelated"]
    );
    assert_ne!(
        actual["mcpServers"]["shield"]["command"],
        "acme-manual-shield"
    );
    transport.close().1.unwrap();
}

#[test]
fn interrupted_setup_preview_does_not_resume_or_mutate_confirmed_publication() {
    let (transport, project) = prepared();
    edit(&project, false);
    let before = state(&project);
    let preview = transport.call(json!(2), "mcp_setup_preview", &json!({}));
    assert!(data(&preview)["error"].is_null(), "{preview}");
    assert_eq!(state(&project), before);
    assert!(
        !project
            .root
            .path()
            .join(".claude/settings.local.json")
            .exists()
    );
    transport.close().1.unwrap();
}
